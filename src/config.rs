use std::path::PathBuf;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    /// The address the cache server listens on.
    #[serde(default = "default_bind_address")]
    pub bind_address: String,

    /// The address the metrics for the cache server listens on.
    #[serde(default = "default_bind_metrics_address")]
    pub bind_metrics_address: String,

    /// The upstream cache store this cache server sits in front of.
    pub upstream: UpstreamConfig,

    /// Configuration for the caching behavior.
    #[serde(default)]
    pub cache: CacheConfig,
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
    #[serde(default = "default_max_disk_capacity")]
    pub max_disk_capacity: bytesize::ByteSize,

    /// Minimum number of file descriptors that should be used for the cache.
    ///
    /// The _maximum_ number of cache file descriptors is derived from the
    /// process's rlimits and `min_non_cache_files`, so setting this value
    /// ensures a lower bound. If the soft rlimit is too low, we'll try to raise
    /// the soft rlimit, or we'll fail with an error.
    #[serde(default = "default_min_cache_files")]
    pub min_cache_files: u64,

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
