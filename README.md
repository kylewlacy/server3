# server3

A simple HTTP reverse proxy server and cache, designed (mainly) for caching objects from S3 or other object stores.

## Behavior

> [!IMPORTANT]
> server3 is meant mainly for serving mostly-static assets from S3 buckets! There are some nasty defaults and assumptions that make it pretty unpleasant for other use cases.

- Requests are proxied to an upstream source (e.g. an HTTP(S) URL or S3 bucket).
- Requested resources are cached with temp files on disk. After more than the maximum size has been written (default: 1 GiB) or when too many files are open, old cache files are evicted.
    - **Note**: The actual disk usage can exceed the configured maximum! It's recommended you over-allocate to account for this.
    - The server will attempt to raise the process's [`NOFILE` rlimit](https://www.man7.org/linux/man-pages/man2/getrlimit.2.html) at startup to ensure it can hold at least 500 cache files, and will fail otherwise. This limit can be configured.
- Cache files never expire by default. Expiration can be configured globally, by host, or by route.
    - The `Cache-Control` settings from the upstream server are ignored.
- Individual routes can be enabled or disabled. This can help reduce garbage requests to the upstream server, or can help with filtering metrics.
- Query params and request headers are not forwarded to the upstream server. Most response headers are neither cached nor included in the response (with the exception of `Content-Type`).
- Only successful (2xx) responses are considered valid. 3xx redirects are followed by the server instead of being returned. 4xx and 5xx errors are treated as errors by the proxy layer, and a generic error message is returned as a response.

## Configuration

Uses [Figment](https://github.com/SergioBenitez/Figment) for configuration, with configuration files written in [Styx](https://styx.bearcove.eu/). Here's a minimal configuration file:

```styx
// config.styx

upstream {
    type http
    url https://example.com/
}
```

## Usage

Run with:

```sh-session
$ server3 serve -c config.styx
```

The server will listen on `http://0.0.0.0:3000` (by default) and proxy requests to the configured upstream server. Check [`config.example.styx`](./config.example.styx) or [the `config` module](./src/config.rs) for more details on supported configuration options.


## Metrics

The server listens on `https://0.0.0.0:3001/metrics` (by default) and serves Prometheus-compatible metrics. A quick rundown of some of the metrics:

- `server3_cache_hit_count`: Count of requests served from already-cached content.
- `server3_cache_hit_bytes`: Sum of bytes served from already-cached content.
- `server3_cache_miss_count`: Count of requests served of uncached content that was then cached.
- `server3_cache_miss_bytes`: Sum of bytes served of uncached content that was then cached.
- `server3_cache_never_count`: Count of requests served from the upstream server, but where the config disables caching.
- `server3_cache_unrouted_count`: Count of requests that weren't sent upstream based on the config routing rules.
- `server3_cache_not_found_count`: Count of requests where the resource couldn't be found in the upstream server (i.e. 404's).
- `server3_cache_error_count`: Count of requests that failed because we couldn't get a (successful) response from the upstream server.
- `server3_cache_eviction_count`: Count of resources evicted from the cache (e.g. due to exceeding the configured storage space).
- `server3_cache_eviction_bytes`: Sum of file sizes of resources evicted from the cache.
- `server3_cache_disk_file_count`: Gauge tracking the current number of files stored in the cache.
- `server3_cache_disk_file_bytes`: Gauge tracking the sum of bytes stored in the cache.
- `server3_cache_disk_max_file_count`: Gauge tracking the maximum files that can be used in the cache. (Useful to calculate the % of files relative to the max allowed)
- `server3_cache_disk_max_bytes`: Gauge tracking the maximum space that can be used in the cache. (Useful to calculate the % of space used relative to the max allowed)

## Other projects

- [git-pages](https://codeberg.org/git-pages/git-pages): Static web server, with custom storage backed by S3 or the local filesystem, plus auth for publishing new versions of a site.
    - Strongly recommended if you need to serve a website instead of general object storage.
    - See also [Grebedoc](https://grebedoc.dev/) for a publicly-hosted version.
- [Vinyl Cache](https://vinyl-cache.org/) (formerly Varnish Cache): High performance, highly configurable HTTP cache server.
    - With a module, it can also be configured as a cache for S3 or other services that use AWS SigV4-- see the post ["Using Varnish Cache as a Secured AWS S3 Gateway"](https://info.varnish-software.com/blog/using-varnish-cache-secured-aws-s3-gateway).
- [Caddy](https://caddyserver.com/): Powerful, flexible HTTPS server with built-in support for certificate renewal via ACME.
    - Works great as a reverse proxy in general, and can even be used to handle HTTPS for server3! Or, to customize response headers, enable HTTP basic auth, etc.
