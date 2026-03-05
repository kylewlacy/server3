use std::{collections::HashMap, path::PathBuf};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    /// The address the reverse proxy server listens on.
    #[serde(default = "default_bind_address")]
    pub bind_address: String,

    /// The address the metrics for the reverse proxy server listens on.
    #[serde(default = "default_bind_metrics_address")]
    pub bind_metrics_address: String,

    /// Per-host configuration. This can be to server different configuration
    /// or select different upstream servers based on the HTTP `Host` header.
    ///
    /// When configured, an incoming request will only match if the `Host`
    /// header is an exact match (ignoring the port number). Any requests
    /// that don't match will either proxy to the top-level [`upstream`]
    /// configuration, or otherwise will return a "not found"
    /// response.
    #[serde(default)]
    pub hosts: HashMap<String, HostConfig>,

    /// The source server to proxy requests to.
    ///
    /// Each host defined in [`hosts`] can define its own upstream server too,
    /// which takes precedence over this setting.
    pub upstream: Option<UpstreamConfig>,

    /// Configuration for where cached data should be stored.
    #[serde(default)]
    pub storage: StorageConfig,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HostConfig {
    /// The upstream server for this host.
    pub upstream: Option<UpstreamConfig>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum UpstreamConfig {
    /// Send proxied requests to an HTTP / HTTPS server.
    ///
    /// The defaults are not meant for acting as a generic HTTP
    /// reverse proxy! This is more tuned for RESTful servers and
    /// object stores like S3. A few notable defaults:
    ///
    /// - Only `GET` requests are proxied.
    /// - Request paths are normalized by stripping extra leading or
    ///   trailing `/`s.
    /// - Query strings and request headers are not forwarded.
    /// - Upstream `Cache-Control` headers are ignored. The configuration
    ///   alone decides if and how long cache entries are persisted.
    /// - Upstream 4xx or 5xx errors are treated as errors, and are
    ///   not returned or cached.
    /// - 3xx responses are followed while caching! The upstream response
    ///   is treated as if the server responded with the final resolved
    ///   response, and will be cached as its own resource.
    /// - Only some response headers (e.g. `Content-Type`) are cached and
    ///   returned.
    Http(UpstreamHttpConfig),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UpstreamHttpConfig {
    /// The upstream server URL.
    pub url: url::Url,

    /// The total time before the upstream request times out. Defaults to
    /// no timeout.
    #[serde(default, with = "humantime_serde::option")]
    pub http_timeout: Option<std::time::Duration>,

    /// The time to wait for a read operation before the upstream request
    /// times out, resetting each time data is read. Defaults to no timeout.
    #[serde(default, with = "humantime_serde::option")]
    pub http_read_timeout: Option<std::time::Duration>,

    /// The time to wait for a connection before the upstream request
    /// times out. Defaults to no timeout.
    #[serde(default, with = "humantime_serde::option")]
    pub http_connect_timeout: Option<std::time::Duration>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StorageConfig {
    /// Directory used to store temporary cache files. Defaults to the
    /// system's temporary directory, e.g. `/tmp`.
    ///
    /// Only temporary files are stored in this directory, and cached data
    /// is not reused across server restarts.
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

impl Default for StorageConfig {
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
