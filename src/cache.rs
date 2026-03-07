use std::{
    collections::{HashMap, HashSet},
    os::unix::fs::FileExt as _,
    path::PathBuf,
    sync::{Arc, atomic::AtomicU64},
};

use anyhow::Context as _;
use futures::TryStreamExt as _;
use lru::LruCache;
use tokio::sync::{Mutex, OnceCell};

use crate::{
    config::StorageConfig,
    upstream::{Upstream, UpstreamError, UpstreamResource, UpstreamResourceHeaders},
};

pub struct CacheStorage {
    cache_dir: PathBuf,
    capacity_pool: CacheCapacityPool,
    cached_resources: Mutex<LruCache<CachedResourceKey, Arc<OnceCell<Arc<CachedResource>>>>>,
}

impl CacheStorage {
    pub fn new(config: StorageConfig) -> anyhow::Result<Self> {
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

        metrics::gauge!("server3_cache_disk_max_bytes")
            .set(config.max_disk_capacity.as_u64() as f64);
        metrics::gauge!("server3_cache_disk_max_file_count").set(max_cache_files as f64);

        Ok(Self {
            capacity_pool: CacheCapacityPool::new(config.max_disk_capacity.as_u64()),
            cache_dir: config.dir,
            cached_resources: Mutex::new(LruCache::new(max_cache_files.try_into().unwrap())),
        })
    }
}

#[derive(Clone)]
pub struct Cache<S> {
    storage: Arc<CacheStorage>,
    host_key: Arc<str>,
    rules: Arc<CacheRoutes>,
    upstream: S,
    cache_eviction_count: metrics::Counter,
    cache_eviction_bytes: metrics::Counter,
    cache_disk_file_count: metrics::Gauge,
    cache_disk_bytes: metrics::Gauge,
    route_metrics: HashMap<Arc<str>, CacheRouteMetrics>,
    default_route_metrics: CacheRouteMetrics,
}

impl<S> Cache<S> {
    pub fn new(
        storage: Arc<CacheStorage>,
        host_key: Arc<str>,
        rules: Arc<CacheRoutes>,
        upstream: S,
    ) -> Self {
        let route_metrics = rules
            .route_paths
            .iter()
            .map(|path| {
                (
                    path.clone(),
                    CacheRouteMetrics::new(host_key.clone(), path.clone()),
                )
            })
            .collect();
        let default_route_metrics = CacheRouteMetrics::new(host_key.clone(), "DEFAULT".into());

        Self {
            storage,
            rules,
            upstream,
            route_metrics,
            default_route_metrics,
            cache_eviction_count: metrics::counter!("server3_cache_eviction_count", "host" => host_key.clone()),
            cache_eviction_bytes: metrics::counter!("server3_cache_eviction_bytes", "host" => host_key.clone()),
            cache_disk_file_count: metrics::gauge!("server3_cache_disk_file_count", "host" => host_key.clone()),
            cache_disk_bytes: metrics::gauge!("server3_cache_disk_bytes", "host" => host_key.clone()),
            host_key,
        }
    }
}

impl<S> Cache<S>
where
    S: Upstream + Send + Sync,
{
    pub async fn get(
        &self,
        path: &str,
        now: std::time::Instant,
    ) -> Result<Option<UpstreamResource>, UpstreamError> {
        let (rule, pattern) = self.rules.match_route(path);
        tracing::trace!(?path, pattern, ?rule, "looked up cache rule for route");

        let route_metrics = pattern.map_or_else(
            || &self.default_route_metrics,
            |pattern| &self.route_metrics[pattern],
        );
        route_metrics.request_count.increment(1);

        let cache_key = CachedResourceKey {
            host: self.host_key.clone(),
            path: path.to_string(),
        };
        let init_id = uuid::Uuid::new_v4();

        let max_age = match rule {
            CacheRouteRule::Enabled(CacheEnabledRouteRule { max_age }) => max_age,
            CacheRouteRule::Disabled => {
                route_metrics.unrouted_count.increment(1);
                return Ok(None);
            }
        };
        let new_resource_expires_after = match max_age {
            CacheMaxAgeRule::CacheForever => None,
            CacheMaxAgeRule::CacheFor(duration) => Some(now.checked_add(*duration).unwrap()),
            CacheMaxAgeRule::CacheNever => {
                route_metrics.never_count.increment(1);
                return self.upstream.get(path).await;
            }
        };

        let cell = {
            let mut cached_resources = self.storage.cached_resources.lock().await;

            let cell =
                cached_resources.get_or_insert_mut_ref(&cache_key, || Arc::new(OnceCell::new()));
            if cell.get().is_some_and(|resource| resource.is_expired(now)) {
                *cell = Arc::new(OnceCell::new());
            }

            cell.clone()
        };
        let result = cell
            .get_or_try_init(async || {
                let resource = match self.upstream.get(path).await {
                    Ok(Some(content)) => content,
                    Ok(None) => {
                        return Err(None);
                    }
                    Err(err) => {
                        return Err(Some(err));
                    }
                };
                let body = resource
                    .body
                    .into_data_stream()
                    .map_err(std::io::Error::other);
                let body = tokio_util::io::StreamReader::new(body);
                let file = create_cached_resource(
                    init_id,
                    new_resource_expires_after,
                    self,
                    resource.headers,
                    body,
                )
                .await;
                let file = match file {
                    Ok(file) => file,
                    Err(err) => {
                        return Err(Some(err));
                    }
                };
                Ok(Arc::new(file))
            })
            .await;

        let cached_resource = match result {
            Ok(cached_resource) => {
                if cached_resource.id == init_id {
                    // File ID matches the ID we just generated, so this was
                    // a cache miss that we've now filled in
                    route_metrics.miss_count.increment(1);
                    route_metrics.miss_bytes.increment(cached_resource.size);
                } else {
                    // File ID does not match our ID, so the file was already
                    // populated meaning this was a cache hit
                    route_metrics.hit_count.increment(1);
                    route_metrics.hit_bytes.increment(cached_resource.size);
                }
                cached_resource.clone()
            }
            Err(err) => {
                match err {
                    Some(err) => {
                        route_metrics.error_count.increment(1);
                        return Err(err);
                    }
                    None => {
                        route_metrics.not_found_count.increment(1);
                        return Ok(None);
                    }
                };
            }
        };
        let body = body_from_cached_resource(&cached_resource).await?;
        let resource = UpstreamResource {
            body,
            headers: cached_resource.headers.clone(),
        };
        Ok(Some(resource))
    }
}

#[derive(Debug)]
pub struct CacheRoutes {
    default_rule: CacheRouteRule,
    route_rules: path_tree::PathTree<(CacheRouteRule, Arc<str>)>,
    route_paths: HashSet<Arc<str>>,
}

impl CacheRoutes {
    pub fn from_config(
        cache_config: &crate::config::CacheConfig,
        routes_config: &HashMap<String, crate::config::RouteConfig>,
    ) -> Self {
        let mut routes = Self::new(CacheRouteRule::Enabled(cache_config.clone().into()));
        for (path, route) in routes_config {
            routes.add_route(path, route.clone().into());
        }
        routes
    }

    pub fn new(default_rule: CacheRouteRule) -> Self {
        Self {
            default_rule,
            route_rules: path_tree::PathTree::new(),
            route_paths: HashSet::new(),
        }
    }

    pub fn add_route(&mut self, path_pattern: &str, rule: CacheRouteRule) {
        let path_pattern = Arc::<str>::from(path_pattern);
        let _ = self
            .route_rules
            .insert(&path_pattern, (rule, path_pattern.clone()));
        self.route_paths.insert(path_pattern);
    }

    fn match_route(&self, path: &str) -> (&CacheRouteRule, Option<&str>) {
        if let Some(((rule, pattern), _)) = self.route_rules.find(path) {
            (rule, Some(pattern))
        } else {
            (&self.default_rule, None)
        }
    }
}

#[derive(Debug, Clone)]
struct CacheRouteMetrics {
    request_count: metrics::Counter,
    miss_count: metrics::Counter,
    miss_bytes: metrics::Counter,
    hit_count: metrics::Counter,
    hit_bytes: metrics::Counter,
    never_count: metrics::Counter,
    error_count: metrics::Counter,
    not_found_count: metrics::Counter,
    unrouted_count: metrics::Counter,
}

impl CacheRouteMetrics {
    fn new(host_key: Arc<str>, path_pattern: Arc<str>) -> Self {
        Self {
            request_count: metrics::counter!("server3_cache_request_count", "host" => host_key.clone(), "path" => path_pattern.clone()),
            miss_count: metrics::counter!("server3_cache_miss_count", "host" => host_key.clone(), "path" => path_pattern.clone()),
            miss_bytes: metrics::counter!("server3_cache_miss_bytes", "host" => host_key.clone(), "path" => path_pattern.clone()),
            hit_count: metrics::counter!("server3_cache_hit_count", "host" => host_key.clone(), "path" => path_pattern.clone()),
            hit_bytes: metrics::counter!("server3_cache_hit_bytes", "host" => host_key.clone(), "path" => path_pattern.clone()),
            never_count: metrics::counter!("server3_cache_never_count", "host" => host_key.clone(), "path" => path_pattern.clone()),
            error_count: metrics::counter!("server3_cache_error_count", "host" => host_key.clone(), "path" => path_pattern.clone()),
            not_found_count: metrics::counter!("server3_cache_not_found_count", "host" => host_key.clone(), "path" => path_pattern.clone()),
            unrouted_count: metrics::counter!("server3_cache_unrouted_count", "host" => host_key.clone(), "path" => path_pattern.clone()),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum CacheRouteRule {
    Enabled(CacheEnabledRouteRule),
    Disabled,
}

impl From<crate::config::RouteConfig> for CacheRouteRule {
    fn from(value: crate::config::RouteConfig) -> Self {
        match value {
            crate::config::RouteConfig::Disabled(crate::config::DisabledRoute::Disabled) => {
                Self::Disabled
            }
            crate::config::RouteConfig::Enabled { cache } => Self::Enabled(cache.into()),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CacheEnabledRouteRule {
    pub max_age: CacheMaxAgeRule,
}

impl From<crate::config::CacheConfig> for CacheEnabledRouteRule {
    fn from(value: crate::config::CacheConfig) -> Self {
        let crate::config::CacheConfig { max_age } = value;
        Self {
            max_age: max_age.into(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum CacheMaxAgeRule {
    CacheForever,
    CacheFor(std::time::Duration),
    CacheNever,
}

impl From<crate::config::CacheConfigMaxAge> for CacheMaxAgeRule {
    fn from(value: crate::config::CacheConfigMaxAge) -> Self {
        match value {
            crate::config::CacheConfigMaxAge::Seconds(seconds) => {
                if seconds > 0 {
                    Self::CacheFor(std::time::Duration::from_secs(
                        seconds.try_into().expect("duration overflowed"),
                    ))
                } else {
                    Self::CacheNever
                }
            }
            crate::config::CacheConfigMaxAge::Duration(duration) => {
                if duration.is_positive() {
                    Self::CacheFor(duration.try_into().expect("duration overflowed"))
                } else {
                    Self::CacheNever
                }
            }
            crate::config::CacheConfigMaxAge::Other(
                crate::config::CacheConfigMaxAgeOther::Forever,
            ) => Self::CacheForever,
            crate::config::CacheConfigMaxAge::Other(
                crate::config::CacheConfigMaxAgeOther::Never,
            ) => Self::CacheNever,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct CachedResourceKey {
    host: Arc<str>,
    path: String,
}

async fn create_cached_resource<S>(
    id: uuid::Uuid,
    expires_at: Option<std::time::Instant>,
    cache: &Cache<S>,
    headers: UpstreamResourceHeaders,
    mut body: impl tokio::io::AsyncBufRead + Unpin,
) -> Result<CachedResource, UpstreamError> {
    let cache_dir = cache.storage.cache_dir.clone();
    let mut file = tokio::task::spawn_blocking(move || {
        let file = tempfile::tempfile_in(&*cache_dir)?;
        std::io::Result::Ok(tokio::fs::File::from_std(file))
    })
    .await
    .unwrap()?;

    let size = tokio::io::copy(&mut body, &mut file).await?;

    let reservation = {
        let mut cached_resources_lock = None;
        loop {
            // Try and reserve enough space from the pool for the file
            let reservation = cache.storage.capacity_pool.reserve(size);
            if let Some(reservation) = reservation {
                // ...okay, we reserved the space
                break reservation;
            }

            // We couldn't reserve enough space, so make some space by
            // clearing the cache
            let cached_resources = match cached_resources_lock.as_mut() {
                None => cached_resources_lock.insert(cache.storage.cached_resources.lock().await),
                Some(cached_resources) => cached_resources,
            };

            let evicted = cached_resources.pop_lru();
            let Some((_evicted_key, evicted_file)) = evicted else {
                // Cache is empty but there still wasn't enough space last
                // we checked. Well, we already wrote the file, so reserve
                // as much space as we can from the pool and continue onward

                let reservation = cache.storage.capacity_pool.reserve_up_to(size);

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

    cache.cache_disk_file_count.increment(1);
    cache.cache_disk_bytes.increment(size as f64);

    Ok(CachedResource {
        id,
        expires_at,
        headers,
        file,
        size,
        cache_eviction_count: cache.cache_eviction_count.clone(),
        cache_eviction_bytes: cache.cache_eviction_bytes.clone(),
        cache_disk_file_count: cache.cache_disk_file_count.clone(),
        cache_disk_bytes: cache.cache_disk_bytes.clone(),
        _reservation: reservation,
    })
}

async fn body_from_cached_resource(
    cache_file: &CachedResource,
) -> tokio::io::Result<axum::body::Body> {
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

struct CachedResource {
    id: uuid::Uuid,
    headers: UpstreamResourceHeaders,
    file: tokio::fs::File,
    size: u64,
    expires_at: Option<std::time::Instant>,
    cache_eviction_count: metrics::Counter,
    cache_eviction_bytes: metrics::Counter,
    cache_disk_file_count: metrics::Gauge,
    cache_disk_bytes: metrics::Gauge,
    _reservation: CacheCapacityReservation,
}

impl CachedResource {
    pub fn is_expired(&self, now: std::time::Instant) -> bool {
        self.expires_at.is_some_and(|expires_at| expires_at <= now)
    }
}

impl Drop for CachedResource {
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
