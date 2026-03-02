use std::{
    os::unix::fs::FileExt as _,
    path::PathBuf,
    sync::{Arc, atomic::AtomicU64},
};

use anyhow::Context as _;
use futures::TryStreamExt as _;
use lru::LruCache;
use tokio::sync::{Mutex, OnceCell};

use crate::{
    config::CacheConfig,
    store::{Store, StoreError, StoreObject, StoreObjectHeaders},
};

pub struct CacheStorage {
    cache_dir: PathBuf,
    capacity_pool: CacheCapacityPool,
    cached_objects: Mutex<LruCache<CacheObjectKey, Arc<OnceCell<Arc<CacheFile>>>>>,
}

impl CacheStorage {
    pub fn new(config: CacheConfig) -> anyhow::Result<Self> {
        let (original_soft_limit, hard_limit) =
            rlimit::getrlimit(rlimit::Resource::NOFILE).context("failed to get rlimit")?;

        let min_soft_limit = if let Some(max_cache_files) = config.max_cache_files {
            max_cache_files
                .checked_add(config.min_non_cache_files)
                .unwrap()
        } else {
            config
                .min_non_cache_files
                .checked_add(config.min_cache_files.max(1))
                .unwrap()
        };

        anyhow::ensure!(
            min_soft_limit <= hard_limit,
            "NOFILE hard rlimit {hard_limit} is less than the minimum limit of {min_soft_limit} in the config"
        );

        let soft_limit = if original_soft_limit < min_soft_limit {
            tracing::info!(
                original_soft_limit,
                new_soft_limit = min_soft_limit,
                hard_limit,
                min_non_cache_files = config.min_non_cache_files,
                min_cache_files = config.min_cache_files,
                "increasing NOFILE soft rlimit"
            );
            rlimit::increase_nofile_limit(min_soft_limit)
                .context("failed to increase NOFILE rlimit")?
        } else {
            original_soft_limit
        };

        let max_cache_files = config
            .max_cache_files
            .unwrap_or_else(|| soft_limit.checked_sub(config.min_non_cache_files).unwrap());
        let max_cache_files: usize = max_cache_files.try_into().unwrap();

        tracing::info!(
            cache_dir = %config.dir.display(),
            max_disk_capacity_bytes = config.max_disk_capacity.as_u64(),
            max_cache_files,
            min_non_cache_files = config.min_non_cache_files,
            min_cache_files = config.min_cache_files,
            "creating cache storage",
        );

        metrics::gauge!("cache_disk_max_bytes").set(config.max_disk_capacity.as_u64() as f64);
        metrics::gauge!("cache_disk_max_file_count").set(max_cache_files as f64);

        Ok(Self {
            capacity_pool: CacheCapacityPool::new(config.max_disk_capacity.as_u64()),
            cache_dir: config.dir,
            cached_objects: Mutex::new(LruCache::new(max_cache_files.try_into().unwrap())),
        })
    }
}

pub struct CacheStore<S> {
    storage: Arc<CacheStorage>,
    host_key: Arc<str>,
    upstream_store: S,
    cache_miss_count: metrics::Counter,
    cache_miss_bytes: metrics::Counter,
    cache_hit_count: metrics::Counter,
    cache_hit_bytes: metrics::Counter,
    cache_error_count: metrics::Counter,
    cache_not_found_count: metrics::Counter,
    cache_eviction_count: metrics::Counter,
    cache_eviction_bytes: metrics::Counter,
    cache_disk_file_count: metrics::Gauge,
    cache_disk_bytes: metrics::Gauge,
}

impl<S> CacheStore<S> {
    pub fn new(
        storage: Arc<CacheStorage>,
        host_key: Arc<str>,
        upstream_store: S,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            storage,
            upstream_store,
            cache_miss_count: metrics::counter!("cache_miss_count", "host" => host_key.clone()),
            cache_miss_bytes: metrics::counter!("cache_miss_bytes", "host" => host_key.clone()),
            cache_hit_count: metrics::counter!("cache_hit_count", "host" => host_key.clone()),
            cache_hit_bytes: metrics::counter!("cache_hit_bytes", "host" => host_key.clone()),
            cache_error_count: metrics::counter!("cache_error_count", "host" => host_key.clone()),
            cache_not_found_count: metrics::counter!("cache_not_found_count", "host" => host_key.clone()),
            cache_eviction_count: metrics::counter!("cache_eviction_count", "host" => host_key.clone()),
            cache_eviction_bytes: metrics::counter!("cache_eviction_bytes", "host" => host_key.clone()),
            cache_disk_file_count: metrics::gauge!("cache_disk_file_count", "host" => host_key.clone()),
            cache_disk_bytes: metrics::gauge!("cache_disk_bytes", "host" => host_key.clone()),
            host_key,
        })
    }
}

#[async_trait::async_trait]
impl<S> Store for CacheStore<S>
where
    S: Store + Send + Sync,
{
    async fn get_object(&self, key: &str) -> Result<Option<StoreObject>, StoreError> {
        let cache_key = CacheObjectKey {
            host: self.host_key.clone(),
            key: key.to_string(),
        };
        let init_id = uuid::Uuid::new_v4();

        let cell = {
            let mut cached_objects = self.storage.cached_objects.lock().await;
            cached_objects
                .get_or_insert(cache_key.clone(), || Arc::new(OnceCell::new()))
                .clone()
        };
        let result = cell
            .get_or_try_init(async || {
                let object = match self.upstream_store.get_object(key).await {
                    Ok(Some(content)) => content,
                    Ok(None) => {
                        return Err(None);
                    }
                    Err(err) => {
                        return Err(Some(err));
                    }
                };
                let content = object
                    .body
                    .into_data_stream()
                    .map_err(std::io::Error::other);
                let content = tokio_util::io::StreamReader::new(content);
                let file = create_cache_file(init_id, self, object.headers, content).await;
                let file = match file {
                    Ok(file) => file,
                    Err(err) => {
                        return Err(Some(err));
                    }
                };
                Ok(Arc::new(file))
            })
            .await;

        let cache_file = match result {
            Ok(cache_file) => {
                if cache_file.id == init_id {
                    // File ID matches the ID we just generated, so this was
                    // a cache miss that we've now filled in
                    self.cache_miss_count.increment(1);
                    self.cache_miss_bytes.increment(cache_file.size);
                } else {
                    // File ID does not match our ID, so the file was already
                    // populated meaning this was a cache hit
                    self.cache_hit_count.increment(1);
                    self.cache_hit_bytes.increment(cache_file.size);
                }
                cache_file.clone()
            }
            Err(err) => {
                match err {
                    Some(err) => {
                        self.cache_error_count.increment(1);
                        return Err(err);
                    }
                    None => {
                        self.cache_not_found_count.increment(1);
                        return Ok(None);
                    }
                };
            }
        };
        let body = body_from_cache_file(&cache_file).await?;
        let object = StoreObject {
            body,
            headers: cache_file.headers.clone(),
        };
        Ok(Some(object))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct CacheObjectKey {
    host: Arc<str>,
    key: String,
}

async fn create_cache_file<S>(
    id: uuid::Uuid,
    store: &CacheStore<S>,
    headers: StoreObjectHeaders,
    mut data: impl tokio::io::AsyncBufRead + Unpin,
) -> Result<CacheFile, StoreError> {
    let cache_dir = store.storage.cache_dir.clone();
    let mut file = tokio::task::spawn_blocking(move || {
        let file = tempfile::tempfile_in(&*cache_dir)?;
        std::io::Result::Ok(tokio::fs::File::from_std(file))
    })
    .await
    .unwrap()?;

    let size = tokio::io::copy(&mut data, &mut file).await?;

    let reservation = {
        let mut cached_objects_lock = None;
        loop {
            // Try and reserve enough space from the pool for the file
            let reservation = store.storage.capacity_pool.reserve(size);
            if let Some(reservation) = reservation {
                // ...okay, we reserved the space
                break reservation;
            }

            // We couldn't reserve enough space, so make some space by
            // clearing the cache
            let cached_objects = match cached_objects_lock.as_mut() {
                None => cached_objects_lock.insert(store.storage.cached_objects.lock().await),
                Some(cached_objects) => cached_objects,
            };

            let evicted = cached_objects.pop_lru();
            let Some((_evicted_key, evicted_file)) = evicted else {
                // Cache is empty but there still wasn't enough space last
                // we checked. Well, we already wrote the file, so reserve
                // as much space as we can from the pool and continue onward

                let reservation = store.storage.capacity_pool.reserve_up_to(size);

                if reservation.reserved < size {
                    tracing::warn!(
                        size,
                        reserved = reservation.reserved,
                        "nothing left to evict from cache but failed to reserve enough space for file, resource may be too big for cache or there might be a lot of requests in flight?"
                    );
                }

                break reservation;
            };

            if let Some(evicted_file) = evicted_file.get() {
                // Update the counters for the file we just evicted rather
                // than the current store's counters. That way, we update the
                // metrics with the proper host key
                evicted_file.cache_eviction_count.increment(1);
                evicted_file
                    .cache_eviction_bytes
                    .increment(evicted_file.size);
            }

            // We just evicted something from the cache, so we're ready to
            // try again
        }
    };

    store.cache_disk_file_count.increment(1);
    store.cache_disk_bytes.increment(size as f64);

    Ok(CacheFile {
        id,
        headers,
        file,
        size,
        cache_eviction_count: store.cache_eviction_count.clone(),
        cache_eviction_bytes: store.cache_eviction_bytes.clone(),
        cache_disk_file_count: store.cache_disk_file_count.clone(),
        cache_disk_bytes: store.cache_disk_bytes.clone(),
        _reservation: reservation,
    })
}

async fn body_from_cache_file(cache_file: &CacheFile) -> tokio::io::Result<axum::body::Body> {
    let (body_tx, body_rx) = tokio::sync::mpsc::channel(1);
    let file = cache_file.file.try_clone().await?.into_std().await;

    tokio::task::spawn_blocking(move || {
        let mut offset: u64 = 0;
        let mut buf = [0; 16_384];
        loop {
            let result = file.read_at(&mut buf, offset);
            let send_result = match result {
                Ok(0) => {
                    break;
                }
                Ok(len) => {
                    let len_u64: u64 = len.try_into().unwrap();

                    let bytes = bytes::Bytes::copy_from_slice(&buf[..len]);
                    offset += len_u64;

                    body_tx.blocking_send(Ok(bytes))
                }
                Err(error) => body_tx.blocking_send(Err(error)),
            };
            if send_result.is_err() {
                break;
            }
        }
    });

    let body_stream = tokio_stream::wrappers::ReceiverStream::new(body_rx);

    let size_hint = http_body::SizeHint::with_exact(cache_file.size);
    let body = crate::response::BodyWithSize::new(crate::response::StreamBody::new(body_stream))
        .with_size_hint(Some(size_hint));
    Ok(axum::body::Body::new(body))
}

struct CacheFile {
    id: uuid::Uuid,
    headers: StoreObjectHeaders,
    file: tokio::fs::File,
    size: u64,
    cache_eviction_count: metrics::Counter,
    cache_eviction_bytes: metrics::Counter,
    cache_disk_file_count: metrics::Gauge,
    cache_disk_bytes: metrics::Gauge,
    _reservation: CacheCapacityReservation,
}

impl Drop for CacheFile {
    fn drop(&mut self) {
        self.cache_disk_file_count.decrement(1);
        self.cache_disk_bytes.decrement(self.size as f64);
    }
}

#[derive(Debug, Clone)]
#[repr(transparent)]
struct CacheCapacityPool {
    available_capacity: Arc<AtomicU64>,
}

impl CacheCapacityPool {
    fn new(capacity: u64) -> Self {
        Self {
            available_capacity: Arc::new(AtomicU64::new(capacity)),
        }
    }

    fn reserve(&self, size: u64) -> Option<CacheCapacityReservation> {
        if size == 0 {
            return Some(CacheCapacityReservation {
                pool: self.clone(),
                reserved: 0,
            });
        }

        loop {
            let available_capacity = self
                .available_capacity
                .load(std::sync::atomic::Ordering::Acquire);
            let remaining_capacity = available_capacity.checked_sub(size)?;

            let result = self.available_capacity.compare_exchange(
                available_capacity,
                remaining_capacity,
                std::sync::atomic::Ordering::Release,
                std::sync::atomic::Ordering::Relaxed,
            );

            if result.is_ok() {
                return Some(CacheCapacityReservation {
                    pool: self.clone(),
                    reserved: size,
                });
            }
        }
    }

    fn reserve_up_to(&self, size: u64) -> CacheCapacityReservation {
        if size == 0 {
            return CacheCapacityReservation {
                pool: self.clone(),
                reserved: size,
            };
        }

        loop {
            let available_capacity = self
                .available_capacity
                .load(std::sync::atomic::Ordering::Acquire);
            let remaining_capacity = available_capacity.saturating_sub(size);

            let result = self.available_capacity.compare_exchange(
                available_capacity,
                remaining_capacity,
                std::sync::atomic::Ordering::Release,
                std::sync::atomic::Ordering::Relaxed,
            );

            if result.is_ok() {
                let reserved = available_capacity.checked_sub(remaining_capacity).unwrap();
                return CacheCapacityReservation {
                    pool: self.clone(),
                    reserved,
                };
            }
        }
    }
}

#[derive(Debug)]
struct CacheCapacityReservation {
    pool: CacheCapacityPool,
    reserved: u64,
}

impl CacheCapacityReservation {
    fn return_to_pool(&mut self) {
        if self.reserved == 0 {
            return;
        }

        loop {
            let current_capacity = self
                .pool
                .available_capacity
                .load(std::sync::atomic::Ordering::Acquire);
            let new_capacity = current_capacity
                .checked_add(self.reserved)
                .expect("disk pool capacity overflowed");

            let result = self.pool.available_capacity.compare_exchange(
                current_capacity,
                new_capacity,
                std::sync::atomic::Ordering::Release,
                std::sync::atomic::Ordering::Relaxed,
            );

            if result.is_ok() {
                self.reserved = 0;
                return;
            }
        }
    }
}

impl Drop for CacheCapacityReservation {
    fn drop(&mut self) {
        self.return_to_pool();
    }
}
