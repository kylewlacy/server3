use std::{collections::HashMap, path::PathBuf};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    /// The address the cache server listens on.
    #[serde(default = "default_bind_address")]
    pub bind_address: String,

    /// The address the metrics for the cache server listens on.
    #[serde(default = "default_bind_metrics_address")]
    pub bind_metrics_address: String,

    /// Per-host configuration. The config is selected based on the `Host` HTTP
    /// header. The hostname must match exactly (minus the port number).
    #[serde(default)]
    pub hosts: HashMap<String, HostConfig>,

    /// The upstream store to cache. If multiple hosts are configured, this
    /// is used as the default upstream for any request.
    pub upstream: Option<UpstreamConfig>,

    /// Configuration for the caching behavior.
    #[serde(default)]
    pub cache: CacheConfig,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HostConfig {
    /// The upstream store to cache for this host.
    pub upstream: Option<UpstreamConfig>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UpstreamConfig {
    /// The upstream cache URL.
    pub url: url::Url,

    /// For HTTP(S) upstreams, the total time before the upstream request
    /// times out. Defaults to no timeout.
    #[serde(default, with = "humantime_serde::option")]
    pub http_timeout: Option<std::time::Duration>,

    /// For HTTP(S) upstreams, the time to wait for a read operation before
    /// the upstream request times out, resetting each time data is read.
    /// Defaults to no timeout.
    #[serde(default, with = "humantime_serde::option")]
    pub http_read_timeout: Option<std::time::Duration>,

    /// For HTTP(S) upstreams, the time to wait for a connection before the
    /// upstream request times out. Defaults to no timeout.
    #[serde(default, with = "humantime_serde::option")]
    pub http_connect_timeout: Option<std::time::Duration>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CacheConfig {
    /// Directory used to store temporary cache files (files are unlinked after
    /// creation, so this is not used for persistence). Defaults to the system's
    /// temporary directory, e.g. `/tmp`.
    #[serde(default = "default_cache_dir")]
    pub dir: PathBuf,

    /// Max size of all temporary cache files, in bytes.
    ///
    /// Measures logical size rather than physical size (e.g. does not account
    /// for per-file overhead). The actual space used on disk may use more than
    /// this size for several reasons, such as:
    ///
    /// - Not accounting for physical file size
    /// - A single cached object exceeding the maximum disk capacity
    #[serde(default = "default_max_disk_capacity")]
    pub max_disk_capacity: bytesize::ByteSize,

    /// Minimum number of file descriptors that should be used for the cache.
    /// This influences the default value for [Self::max_cache_files].
    #[serde(default = "default_min_cache_files")]
    pub min_cache_files: u64,

    /// Maximum number of file descriptors that should be used to cache objects.
    ///
    /// When not set, the maximum number of cache files is set based on the
    /// process's `NOFILE` rlimit, but it reserves enough space for
    /// [`Self::min_non_cached_files`] and enforces [`Self::min_cache_files`]
    /// as a lower bound.
    ///
    /// Whether set by default or set explicitly, we'll attempt to raise the
    /// process's soft `NOFILE` rlimit if needed (or otherwise we'll fail with
    /// a hard error).
    pub max_cache_files: Option<u64>,

    /// Minimum number of file descriptors that should _not_ be used for the
    /// cache (for TCP sockets, etc.).
    #[serde(default = "default_min_non_cache_files")]
    pub min_non_cache_files: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            dir: default_cache_dir(),
            max_disk_capacity: default_max_disk_capacity(),
            max_cache_files: None,
            min_cache_files: default_min_cache_files(),
            min_non_cache_files: default_min_non_cache_files(),
        }
    }
}

fn default_bind_address() -> String {
    "0.0.0.0:3000".to_string()
}

fn default_bind_metrics_address() -> String {
    "0.0.0.0:3001".to_string()
}

fn default_cache_dir() -> PathBuf {
    std::env::temp_dir()
}

fn default_max_disk_capacity() -> bytesize::ByteSize {
    bytesize::ByteSize::gb(1)
}

fn default_min_cache_files() -> u64 {
    500
}

fn default_min_non_cache_files() -> u64 {
    500
}
