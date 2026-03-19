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

    /// Customize how different request paths are routed. Allows for
    /// customizing caching behavior by route and which routes should be
    /// forwarded upstream.
    ///
    /// Routes are matched against URL paths using the crate
    /// [`path-tree`](https://docs.rs/path-tree/0.8.3/path_tree/).
    ///
    /// Each host defined in [`hosts`] can define its own route configuration
    /// too, which takes precedence over this setting.
    #[serde(default)]
    pub routes: HashMap<String, RouteConfig>,

    /// Cache behavior and how long to cache things for.
    ///
    /// The settings at the [`hosts`] level or the [`routes`] level take
    /// precedence over this setting.
    #[serde(default)]
    pub cache: CacheConfig,

    /// Configuration for where cached data should be stored.
    #[serde(default)]
    pub storage: StorageConfig,

    /// Control which metadata response headers to include, if any. By default,
    /// the server doesn't include any extra metadata headers.
    #[serde(default)]
    pub metadata: MetadataConfig,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HostConfig {
    /// The upstream server for this host.
    pub upstream: Option<UpstreamConfig>,

    /// Customize how different request paths are routed. Allows for
    /// customizing caching behavior by route and which routes should be
    /// forwarded upstream.
    ///
    /// Routes are matched against URL paths using the crate
    /// [`path-tree`](https://docs.rs/path-tree/0.8.3/path_tree/).
    pub routes: Option<HashMap<String, RouteConfig>>,

    /// Cache behavior and how long to cache things for.
    ///
    /// The setting within the host-level [`routes`] take precedence over
    /// this setting, if set.
    pub cache: Option<CacheConfig>,
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
    /// - Request paths are normalized by stripping extra leading `/`s (but
    ///   not trailing `/`s).
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

    /// Proxy requests by fetching objects from an S3 bucket.
    ///
    /// Only a few configuration options are provided here. Other options
    /// can be set using the standard AWS SDK convention, such as environment
    /// variables and the config file `~/.aws/config`. Most importantly,
    /// authentication is handled purely through the SDK.
    ///
    /// Here are some extra environment variables that you may want to set
    /// to customize the SDK behavior:
    ///
    /// - `$AWS_ACCESS_KEY_ID` / `$AWS_SECRET_KEY`
    /// - `AWS_REQUEST_CHECKSUM_CALCULATION=WHEN_REQUIRED`
    /// - `AWS_RESPONSE_CHECKSUM_CALCULATION=WHEN_REQUIRED`
    S3(UpstreamS3Config),

    /// Send proxied requests to multiple upstreams, trying one after the
    /// other. Returns the first successful response. If none succeeded,
    /// returns an error if any upstream failed, or "not found" otherwise.
    Fallthrough(UpstreamFallthroughConfig),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UpstreamHttpConfig {
    /// The upstream server URL. Incoming requests are joined with this URL
    /// to pick the upstream resource to retrieve.
    ///
    /// Determining the upstream URL follows the semantics of the method
    /// [`url::Url::join`]. For a subpath, the URL should end in a `/` to
    /// avoid the final path component from getting removed.
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
pub struct UpstreamS3Config {
    /// The S3 bucket to get objects from.
    pub bucket: String,

    /// An optional object prefix. Appended verbatim to each requested key.
    ///
    /// This should probably end with a trailing `/` if you want to serve
    /// a "directory" from S3!
    #[serde(default)]
    pub prefix: String,

    /// Which AWS profile to use for making requests.
    pub profile: Option<String>,

    /// The name of the AWS region of the bucket.
    pub region: Option<String>,

    /// A custom endpoint URL to use for the bucket. This is useful for using
    /// other S3-compatible object store providers.
    pub endpoint_url: Option<url::Url>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UpstreamFallthroughConfig {
    /// A sequence of other upstream configs to try, from first to last.
    pub upstreams: Vec<UpstreamConfig>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum RouteConfig {
    Disabled(DisabledRoute),

    Enabled {
        /// Cache behavior and how long to cache things for.
        #[serde(flatten)]
        cache: CacheConfig,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisabledRoute {
    /// Disable this route, and don't send it to the upstream server. Also
    /// helps distinguish metrics ("not found" versus "unrouted" metrics).
    Disabled,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnabledRouteConfig {
    Disabled,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CacheConfig {
    /// Set how long a cached resource is valid before it's considered stale
    /// and needs to be refetched. Set to `never` or `0` to avoid caching
    /// the resource, `forever` to keep the cached copy forever (or until
    /// it's evicted by other means), or to a number of seconds or duration
    /// value.
    #[serde(default = "CacheConfigMaxAge::forever")]
    pub max_age: CacheConfigMaxAge,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_age: CacheConfigMaxAge::forever(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum CacheConfigMaxAge {
    Seconds(i64),
    Duration(
        #[serde(serialize_with = "jiff::fmt::serde::duration::friendly::compact::required")]
        jiff::SignedDuration,
    ),
    Other(CacheConfigMaxAgeOther),
}

impl CacheConfigMaxAge {
    pub fn never() -> Self {
        Self::Other(CacheConfigMaxAgeOther::Never)
    }

    pub fn forever() -> Self {
        Self::Other(CacheConfigMaxAgeOther::Forever)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheConfigMaxAgeOther {
    Forever,
    Never,
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
    /// - A single cached resource exceeding the maximum disk capacity
    #[serde(default = "default_max_disk_capacity")]
    pub max_disk_capacity: bytesize::ByteSize,

    /// Minimum number of file descriptors that should be used for the cache.
    /// This influences the default value for [Self::max_cache_files].
    #[serde(default = "default_min_cache_files")]
    pub min_cache_files: u64,

    /// Maximum number of file descriptors that should be used to cache resources.
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

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct MetadataConfig {
    /// Enable the `Server3-Cache-Outcome` response header.
    #[serde(default)]
    pub cache_outcome: bool,

    /// Enable the `Server3-Expires-At` response header.
    #[serde(default)]
    pub expires_at: bool,

    /// Set a value for the `Server3-Node-Name` response header. This is
    /// useful in a distributed environment, so each machine can respond
    /// with a different node name.
    pub node_name: Option<String>,
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
