use std::{sync::Arc, time::Duration};

use bstr::BStr;
use server3::{
    cache::{CacheEnabledRouteRule, CacheMaxAgeRule, CacheRouteRule, CacheRoutes},
    config::StorageConfig,
};

use crate::test_utils::{
    cache_config, cache_routes_forever, mockito_http_store, mockito_http_store_with_prefix,
    resource_content_type, resource_to_bytes, resource_to_string, test_context,
};

mod test_utils;

#[tokio::test]
async fn test_cache_resource() {
    let ctx = test_context();
    let now = std::time::Instant::now();
    let mut mock_server = mockito::Server::new_async().await;

    let upstream_store = mockito_http_store(&mock_server);
    let storage = server3::cache::CacheStorage::new(cache_config(&ctx)).unwrap();
    let cache = server3::cache::Cache::new(
        Arc::new(storage),
        "host".into(),
        cache_routes_forever(),
        upstream_store,
    );

    // Put a resource in the upstream server
    let key = "/foo/bar.txt";
    let resource_mock = mock_server
        .mock("GET", key)
        .with_body("resource data")
        .expect(1)
        .create();

    // Validate that we can get the resource via the cache
    let resource = cache.get(key, now).await.unwrap().unwrap();
    assert_eq!(resource_to_string(resource).await, "resource data");

    // Get the same resource again, but it should be cached and so shouldn't
    // trigger another upstream request.
    let resource = cache.get(key, now).await.unwrap().unwrap();
    assert_eq!(resource_to_string(resource).await, "resource data");

    resource_mock.assert_async().await;
}

#[tokio::test]
async fn test_cache_resource_subpath_handling() {
    let ctx = test_context();
    let now = std::time::Instant::now();
    let mut mock_server = mockito::Server::new_async().await;

    let upstream_store = mockito_http_store_with_prefix(&mock_server, "foo");
    let storage = server3::cache::CacheStorage::new(cache_config(&ctx)).unwrap();
    let cache = server3::cache::Cache::new(
        Arc::new(storage),
        "host".into(),
        cache_routes_forever(),
        upstream_store,
    );

    // Put some resources in the upstream server. The paths chosen ensure
    // that trailing slashes are considered significant.
    let foo_mock = mock_server
        .mock("GET", "/foo/")
        .with_body("foo")
        .expect(1)
        .create();
    let foo_bar_mock = mock_server
        .mock("GET", "/foo/bar")
        .with_body("bar")
        .expect(1)
        .create();
    let foo_bar_slash_mock = mock_server
        .mock("GET", "/foo/bar/")
        .with_body("bar/")
        .expect(1)
        .create();

    // Validate that we can get each resource via the cache

    let resource = cache.get("/", now).await.unwrap().unwrap();
    assert_eq!(resource_to_string(resource).await, "foo");

    let resource = cache.get("/bar", now).await.unwrap().unwrap();
    assert_eq!(resource_to_string(resource).await, "bar");

    let resource = cache.get("/bar/", now).await.unwrap().unwrap();
    assert_eq!(resource_to_string(resource).await, "bar/");

    foo_mock.assert_async().await;
    foo_bar_mock.assert_async().await;
    foo_bar_slash_mock.assert_async().await;
}

#[tokio::test]
async fn test_cache_resource_content_type() {
    let ctx = test_context();
    let now = std::time::Instant::now();
    let mut mock_server = mockito::Server::new_async().await;

    let upstream_store = mockito_http_store(&mock_server);
    let storage = server3::cache::CacheStorage::new(cache_config(&ctx)).unwrap();
    let cache = server3::cache::Cache::new(
        Arc::new(storage),
        "host".into(),
        cache_routes_forever(),
        upstream_store,
    );

    // Put some resourcess in the upstream store with various
    // Content-Type values
    let bar_none_mock = mock_server
        .mock("GET", "/foo/bar")
        .with_body("bar")
        .expect(1)
        .create();
    let bar_bin_mock = mock_server
        .mock("GET", "/foo/bar-bin")
        .with_header("Content-Type", "application/octet-stream")
        .with_body("bar.bin")
        .expect(1)
        .create();
    let bar_txt_mock = mock_server
        .mock("GET", "/foo/bar-txt")
        .with_header("Content-Type", "text/plain")
        .with_body("text")
        .expect(1)
        .create();
    let bar_html_mock = mock_server
        .mock("GET", "/foo/bar-html")
        .with_header("Content-Type", "text/html")
        .with_body("<html></html>")
        .expect(1)
        .create();
    let bar_json_mock = mock_server
        .mock("GET", "/foo/bar-json")
        .with_header("Content-Type", "application/json")
        .with_body("actually, this isn't valid json!")
        .expect(1)
        .create();
    let bar_custom_mock = mock_server
        .mock("GET", "/foo/bar-custom")
        .with_header("Content-Type", "application/x.custom")
        .with_body("custom")
        .expect(1)
        .create();
    let bar_invalid_mock = mock_server
        .mock("GET", "/foo/bar-invalid")
        .with_header("Content-Type", "not valid ascii 🤔")
        .with_body(b"binary...\xFF")
        .expect(1)
        .create();

    // Validate each of the resourcess has the right Content-Type. For good
    // measure, get each resource multiple times to make sure it's cached

    for _ in 0..5 {
        let resource = cache.get("/foo/bar", now).await.unwrap().unwrap();
        assert_eq!(resource_content_type(&resource), None);
        assert_eq!(resource_to_string(resource).await, "bar");

        let resource = cache.get("/foo/bar-bin", now).await.unwrap().unwrap();
        assert_eq!(
            resource_content_type(&resource).unwrap(),
            BStr::new("application/octet-stream")
        );
        assert_eq!(resource_to_string(resource).await, "bar.bin");

        let resource = cache.get("/foo/bar-txt", now).await.unwrap().unwrap();
        assert_eq!(
            resource_content_type(&resource).unwrap(),
            BStr::new("text/plain")
        );
        assert_eq!(resource_to_string(resource).await, "text");

        let resource = cache.get("/foo/bar-html", now).await.unwrap().unwrap();
        assert_eq!(
            resource_content_type(&resource).unwrap(),
            BStr::new("text/html")
        );
        assert_eq!(resource_to_string(resource).await, "<html></html>");

        let resource = cache.get("/foo/bar-json", now).await.unwrap().unwrap();
        assert_eq!(
            resource_content_type(&resource).unwrap(),
            BStr::new("application/json")
        );
        assert_eq!(
            resource_to_string(resource).await,
            "actually, this isn't valid json!"
        );

        let resource = cache.get("/foo/bar-custom", now).await.unwrap().unwrap();
        assert_eq!(
            resource_content_type(&resource).unwrap(),
            BStr::new("application/x.custom")
        );
        assert_eq!(resource_to_string(resource).await, "custom");

        let resource = cache.get("/foo/bar-invalid", now).await.unwrap().unwrap();
        assert_eq!(
            resource_content_type(&resource).unwrap(),
            BStr::new("not valid ascii 🤔")
        );
        assert_eq!(
            resource_to_bytes(resource).await,
            bstr::BStr::new(b"binary...\xFF")
        );
    }

    bar_none_mock.assert_async().await;
    bar_bin_mock.assert_async().await;
    bar_txt_mock.assert_async().await;
    bar_html_mock.assert_async().await;
    bar_json_mock.assert_async().await;
    bar_custom_mock.assert_async().await;
    bar_invalid_mock.assert_async().await;
}

#[tokio::test]
async fn test_cache_max_resources() {
    let ctx = test_context();
    let now = std::time::Instant::now();
    let mut mock_server = mockito::Server::new_async().await;

    let upstream_store = mockito_http_store(&mock_server);
    let storage = server3::cache::CacheStorage::new(StorageConfig {
        max_cache_files: Some(3),
        ..cache_config(&ctx)
    })
    .unwrap();
    let cache = server3::cache::Cache::new(
        Arc::new(storage),
        "host".into(),
        cache_routes_forever(),
        upstream_store,
    );

    // Add 3 resources to the upstream cache
    let resource_1_mock = mock_server
        .mock("GET", "/example-1.json")
        .with_body("example 1")
        .expect(1)
        .create();
    let resource_2_mock = mock_server
        .mock("GET", "/example-2.json")
        .with_body("example 2")
        .expect(1)
        .create();
    let resource_3_mock = mock_server
        .mock("GET", "/example-3.json")
        .with_body("example 3")
        .expect(1)
        .create();

    // Query each upstream resource 5 times
    for _ in 0..5 {
        let resource = cache.get("/example-1.json", now).await.unwrap().unwrap();
        assert_eq!(resource_to_string(resource).await, "example 1");

        let resource = cache.get("/example-2.json", now).await.unwrap().unwrap();
        assert_eq!(resource_to_string(resource).await, "example 2");

        let resource = cache.get("/example-3.json", now).await.unwrap().unwrap();
        assert_eq!(resource_to_string(resource).await, "example 3");
    }

    // At this point, resource 1 is the least-recently used, followed by
    // resource 2, then resource 3

    // Assert that each upstream project was fetched exactly once
    resource_1_mock.assert_async().await;
    resource_2_mock.assert_async().await;
    resource_3_mock.assert_async().await;

    // Now, remove the upstream mocks
    resource_1_mock.remove_async().await;
    resource_2_mock.remove_async().await;
    resource_3_mock.remove_async().await;
    drop(resource_1_mock);
    drop(resource_2_mock);
    drop(resource_3_mock);

    // Add resources 4 and 5 in the upstream, and add resource 1 with a
    // different body
    let resource_4_mock = mock_server
        .mock("GET", "/example-4.json")
        .with_body("example 4")
        .expect(1)
        .create();
    let resource_5_mock = mock_server
        .mock("GET", "/example-5.json")
        .with_body("example 5")
        .expect(1)
        .create();
    let resource_1_mock = mock_server
        .mock("GET", "/example-1.json")
        .with_body("example 1 new!")
        .expect(1)
        .create();

    // Fetch resource 4, which should evict resource 1 from the cache
    let project_source = cache.get("/example-4.json", now).await.unwrap().unwrap();
    assert_eq!(resource_to_string(project_source).await, "example 4");

    // Fetch resource 5, which should evict resource 2 from the cache
    let project_source = cache.get("/example-5.json", now).await.unwrap().unwrap();
    assert_eq!(resource_to_string(project_source).await, "example 5");

    // Fetch resource 1 again, which should evict resource 3 from the cache
    let project_source = cache.get("/example-1.json", now).await.unwrap().unwrap();
    assert_eq!(resource_to_string(project_source).await, "example 1 new!");

    // Resources 4 and 5 should've been fetched once, and resource 1 should've
    // been fetched again since it was evicted
    resource_4_mock.assert_async().await;
    resource_5_mock.assert_async().await;
    resource_1_mock.assert_async().await;
}

#[tokio::test]
async fn test_cache_max_disk_capacity() {
    let ctx = test_context();
    let now = std::time::Instant::now();
    let mut mock_server = mockito::Server::new_async().await;

    let upstream_store = mockito_http_store(&mock_server);
    let storage = server3::cache::CacheStorage::new(StorageConfig {
        max_disk_capacity: bytesize::ByteSize::b(499),
        ..cache_config(&ctx)
    })
    .unwrap();
    let cache = server3::cache::Cache::new(
        Arc::new(storage),
        "host".into(),
        cache_routes_forever(),
        upstream_store,
    );

    let bytes_1x100 = vec![1u8; 100];
    let bytes_2x100 = vec![2u8; 100];
    let bytes_3x100 = vec![3u8; 100];

    // Add three 1 KB resources to the upstream cache
    let resource_1_mock = mock_server
        .mock("GET", "/example-1.bin")
        .with_body(&bytes_1x100)
        .expect(1)
        .create();
    let resource_2_mock = mock_server
        .mock("GET", "/example-2.bin")
        .with_body(&bytes_2x100)
        .expect(1)
        .create();
    let resource_3_mock = mock_server
        .mock("GET", "/example-3.bin")
        .with_body(&bytes_3x100)
        .expect(1)
        .create();

    // Query each resource 5 times
    for _ in 0..5 {
        let resource_1 = cache.get("/example-1.bin", now).await.unwrap().unwrap();
        assert_eq!(resource_to_bytes(resource_1).await, bytes_1x100);

        let example_2 = cache.get("/example-2.bin", now).await.unwrap().unwrap();
        assert_eq!(resource_to_bytes(example_2).await, bytes_2x100);

        let example_3 = cache.get("/example-3.bin", now).await.unwrap().unwrap();
        assert_eq!(resource_to_bytes(example_3).await, bytes_3x100);
    }

    // At this point, resource 1 is the least-recently used, followed by
    // resource 2, then resource 3

    // Assert that each upstream resource was fetched exactly once
    resource_1_mock.assert_async().await;
    resource_2_mock.assert_async().await;
    resource_3_mock.assert_async().await;

    // Now, remove the upstream mocks
    resource_1_mock.remove_async().await;
    resource_2_mock.remove_async().await;
    resource_3_mock.remove_async().await;
    drop(resource_1_mock);
    drop(resource_2_mock);
    drop(resource_3_mock);

    // Add resources 4 and 5 upstream, and re-add resource 1 with
    // different contents
    let bytes_4x100 = vec![4u8; 100];
    let bytes_5x100 = vec![5u8; 100];
    let bytes_6x100 = vec![6u8; 100];
    let resource_4_mock = mock_server
        .mock("GET", "/example-4.bin")
        .with_body(&bytes_4x100)
        .expect(1)
        .create();
    let resource_5_mock = mock_server
        .mock("GET", "/example-5.bin")
        .with_body(&bytes_5x100)
        .expect(1)
        .create();
    let resource_1_mock = mock_server
        .mock("GET", "/example-1.bin")
        .with_body(&bytes_6x100)
        .expect(1)
        .create();

    // Fetch resource 4, which should evict resource 1 from the cache
    let resource_4 = cache.get("/example-4.bin", now).await.unwrap().unwrap();
    assert_eq!(resource_to_bytes(resource_4).await, bytes_4x100);

    // Fetch resource 5, which should evict resource 2 from the cache
    let resource_5 = cache.get("/example-5.bin", now).await.unwrap().unwrap();
    assert_eq!(resource_to_bytes(resource_5).await, bytes_5x100);

    // Fetch resource 1 again, which should evict resource 3
    // from the cache
    let resource_1 = cache.get("/example-1.bin", now).await.unwrap().unwrap();
    assert_eq!(resource_to_bytes(resource_1).await, bytes_6x100);

    // Resource 1 (again), 4, and 5 should've all been fetched
    // from upstream once
    resource_4_mock.assert_async().await;
    resource_5_mock.assert_async().await;
    resource_1_mock.assert_async().await;

    // Remove the upstream mocks
    resource_4_mock.remove_async().await;
    resource_5_mock.remove_async().await;
    resource_1_mock.remove_async().await;
    drop(resource_4_mock);
    drop(resource_5_mock);
    drop(resource_1_mock);

    // Add resource 6, which fully fills the cache on its own and so
    // evicts everything else
    let bytes_7x499 = vec![7u8; 499];
    let resource_6_mock = mock_server
        .mock("GET", "/example-6.bin")
        .with_body(&bytes_7x499)
        .expect(1)
        .create();

    // Fetch resource 6 five times
    for _ in 0..5 {
        let resource_6 = cache.get("/example-6.bin", now).await.unwrap().unwrap();
        assert_eq!(resource_to_bytes(resource_6).await, bytes_7x499);
    }

    // Make sure resource 6 was only fetched once
    resource_6_mock.assert_async().await;
}

#[tokio::test]
async fn test_cache_partition_by_host_key() {
    let ctx = test_context();
    let now = std::time::Instant::now();
    let mut mock_server = mockito::Server::new_async().await;

    let storage = server3::cache::CacheStorage::new(cache_config(&ctx)).unwrap();
    let storage = Arc::new(storage);

    let upstream_store_a = mockito_http_store_with_prefix(&mock_server, "a");
    let upstream_store_b1 = mockito_http_store_with_prefix(&mock_server, "b1");
    let upstream_store_b2 = mockito_http_store_with_prefix(&mock_server, "b2");

    // `cache_a` has a unique host key
    let cache_a = server3::cache::Cache::new(
        storage.clone(),
        "host-a".into(),
        cache_routes_forever(),
        upstream_store_a,
    );

    // `cache_b1` and `cache_b2` share the same host key, and so overlap in the cache
    let cache_b1 = server3::cache::Cache::new(
        storage.clone(),
        "host-b".into(),
        cache_routes_forever(),
        upstream_store_b1,
    );
    let cache_b2 = server3::cache::Cache::new(
        storage.clone(),
        "host-b".into(),
        cache_routes_forever(),
        upstream_store_b2,
    );

    // Put the same resource in the upstream for both a and b1
    let a_foo_mock = mock_server
        .mock("GET", "/a/foo.txt")
        .with_body("a foo")
        .expect(1)
        .create();
    let b1_foo_mock = mock_server
        .mock("GET", "/b1/foo.txt")
        .with_body("b1 foo")
        .expect(1)
        .create();
    let b1_bar_mock = mock_server
        .mock("GET", "/b1/bar.txt")
        .with_body("404 not found!")
        .with_status(404)
        .expect(1)
        .create();
    let b2_bar_mock = mock_server
        .mock("GET", "/b2/bar.txt")
        .with_body("b2 bar")
        .expect(1)
        .create();

    // Get foo.txt from a, which should fetch from the upstream
    let resource = cache_a.get("/foo.txt", now).await.unwrap().unwrap();
    assert_eq!(resource_to_string(resource).await, "a foo");

    // Get foo.txt from b1. Since it has a separate host key, it should
    // be distinct from foo.txt from a.
    let resource = cache_b1.get("/foo.txt", now).await.unwrap().unwrap();
    assert_eq!(resource_to_string(resource).await, "b1 foo");

    // Get foo.txt from b2. Since it has the same host key as b1, it should
    // re-use the cached result from b1.
    let resource = cache_b2.get("/foo.txt", now).await.unwrap().unwrap();
    assert_eq!(resource_to_string(resource).await, "b1 foo");

    // Try to get bar.txt from b1. Upstream returns a 404, so it should
    // not be returned.
    let resource = cache_b1.get("/bar.txt", now).await.unwrap();
    assert!(resource.is_none());

    // Get bar.txt from b2
    let resource = cache_b2.get("/bar.txt", now).await.unwrap().unwrap();
    assert_eq!(resource_to_string(resource).await, "b2 bar");

    a_foo_mock.assert_async().await;
    b1_foo_mock.assert_async().await;
    b1_bar_mock.assert_async().await;
    b2_bar_mock.assert_async().await;
}

#[tokio::test]
async fn test_cache_max_age() {
    let ctx = test_context();
    let now = std::time::Instant::now();
    let mut mock_server = mockito::Server::new_async().await;

    let upstream_store = mockito_http_store(&mock_server);
    let storage = server3::cache::CacheStorage::new(cache_config(&ctx)).unwrap();

    let mut routes = CacheRoutes::new(CacheRouteRule::Enabled(CacheEnabledRouteRule {
        max_age: CacheMaxAgeRule::CacheFor(Duration::from_secs(2)),
    }));
    routes.add_route(
        "/cache-for-3s",
        CacheRouteRule::Enabled(CacheEnabledRouteRule {
            max_age: CacheMaxAgeRule::CacheFor(Duration::from_secs(3)),
        }),
    );
    routes.add_route(
        "/cache-for-4s/*",
        CacheRouteRule::Enabled(CacheEnabledRouteRule {
            max_age: CacheMaxAgeRule::CacheFor(Duration::from_secs(4)),
        }),
    );
    routes.add_route(
        "/cache-forever/*",
        CacheRouteRule::Enabled(CacheEnabledRouteRule {
            max_age: CacheMaxAgeRule::CacheForever,
        }),
    );
    routes.add_route(
        "/cache-never/*",
        CacheRouteRule::Enabled(CacheEnabledRouteRule {
            max_age: CacheMaxAgeRule::CacheNever,
        }),
    );

    let cache = server3::cache::Cache::new(
        Arc::new(storage),
        "host".into(),
        Arc::new(routes),
        upstream_store,
    );

    {
        // "/cache-for-2s" should be cached for 2 seconds (the default
        // cache rule)

        let cache_for_2s_mock = mock_server
            .mock("GET", "/cache-for-2s")
            .with_body("A")
            .expect(1)
            .create();

        let resource = cache.get("/cache-for-2s", now).await.unwrap().unwrap();
        assert_eq!(resource_to_string(resource).await, "A");

        let resource = cache
            .get("/cache-for-2s", now + Duration::from_secs(1))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(resource_to_string(resource).await, "A");

        cache_for_2s_mock.assert_async().await;
        cache_for_2s_mock.remove_async().await;
        let cache_for_2s_mock = mock_server
            .mock("GET", "/cache-for-2s")
            .with_body("B")
            .expect(1)
            .create();

        let resource = cache
            .get("/cache-for-2s", now + Duration::from_secs(2))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(resource_to_string(resource).await, "B");

        let resource = cache
            .get("/cache-for-2s", now + Duration::from_secs(3))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(resource_to_string(resource).await, "B");

        cache_for_2s_mock.assert_async().await;
        cache_for_2s_mock.remove_async().await;
        let cache_for_2s_mock = mock_server
            .mock("GET", "/cache-for-2s")
            .with_status(404)
            .expect(1)
            .create();

        let resource = cache
            .get("/cache-for-2s", now + Duration::from_secs(4))
            .await
            .unwrap();
        assert!(resource.is_none());

        cache_for_2s_mock.assert_async().await;
        cache_for_2s_mock.remove_async().await;
    }

    {
        // "/cache-for-3s" should be cached for 3 seconds (exact path match)

        let cache_for_3s_mock = mock_server
            .mock("GET", "/cache-for-3s")
            .with_body("A")
            .expect(1)
            .create();

        let resource = cache.get("/cache-for-3s", now).await.unwrap().unwrap();
        assert_eq!(resource_to_string(resource).await, "A");

        let resource = cache
            .get("/cache-for-3s", now + Duration::from_secs(2))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(resource_to_string(resource).await, "A");

        cache_for_3s_mock.assert_async().await;
        cache_for_3s_mock.remove_async().await;
        let cache_for_3s_mock = mock_server
            .mock("GET", "/cache-for-3s")
            .with_body("B")
            .expect(1)
            .create();

        let resource = cache
            .get("/cache-for-3s", now + Duration::from_secs(3))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(resource_to_string(resource).await, "B");

        let resource = cache
            .get("/cache-for-3s", now + Duration::from_secs(5))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(resource_to_string(resource).await, "B");

        cache_for_3s_mock.assert_async().await;
        cache_for_3s_mock.remove_async().await;
        let cache_for_3s_mock = mock_server
            .mock("GET", "/cache-for-3s")
            .with_status(404)
            .expect(1)
            .create();

        let resource = cache
            .get("/cache-for-3s", now + Duration::from_secs(6))
            .await
            .unwrap();
        assert!(resource.is_none());

        cache_for_3s_mock.assert_async().await;
        cache_for_3s_mock.remove_async().await;
    }

    {
        // "/cache-for-3s" should be cached for 3 seconds (exact path match)

        let cache_for_3s_mock = mock_server
            .mock("GET", "/cache-for-3s")
            .with_body("A")
            .expect(1)
            .create();

        let resource = cache.get("/cache-for-3s", now).await.unwrap().unwrap();
        assert_eq!(resource_to_string(resource).await, "A");

        let resource = cache
            .get("/cache-for-3s", now + Duration::from_secs(2))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(resource_to_string(resource).await, "A");

        cache_for_3s_mock.assert_async().await;
        cache_for_3s_mock.remove_async().await;
        let cache_for_3s_mock = mock_server
            .mock("GET", "/cache-for-3s")
            .with_body("B")
            .expect(1)
            .create();

        let resource = cache
            .get("/cache-for-3s", now + Duration::from_secs(3))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(resource_to_string(resource).await, "B");

        let resource = cache
            .get("/cache-for-3s", now + Duration::from_secs(5))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(resource_to_string(resource).await, "B");

        cache_for_3s_mock.assert_async().await;
        cache_for_3s_mock.remove_async().await;
        let cache_for_3s_mock = mock_server
            .mock("GET", "/cache-for-3s")
            .with_status(404)
            .expect(1)
            .create();

        let resource = cache
            .get("/cache-for-3s", now + Duration::from_secs(6))
            .await
            .unwrap();
        assert!(resource.is_none());

        cache_for_3s_mock.assert_async().await;
        cache_for_3s_mock.remove_async().await;
    }

    {
        // "/cache-for-4s/foo" should be cached for 4 seconds (subpath match)

        let cache_for_4s_foo_mock = mock_server
            .mock("GET", "/cache-for-4s/foo")
            .with_body("A")
            .expect(1)
            .create();
        let cache_for_4s_bar_mock = mock_server
            .mock("GET", "/cache-for-4s/bar")
            .with_status(404)
            .expect(1)
            .create();

        let foo = cache.get("/cache-for-4s/foo", now).await.unwrap().unwrap();
        assert_eq!(resource_to_string(foo).await, "A");

        let bar = cache.get("/cache-for-4s/bar", now).await.unwrap();
        assert!(bar.is_none());

        let foo = cache
            .get("/cache-for-4s/foo", now + Duration::from_secs(3))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(resource_to_string(foo).await, "A");

        cache_for_4s_foo_mock.assert_async().await;
        cache_for_4s_foo_mock.remove_async().await;

        cache_for_4s_bar_mock.assert_async().await;
        cache_for_4s_bar_mock.remove_async().await;

        // "/cache-for-4s/bar" should be cached for 4 seconds too
        let cache_for_4s_foo_mock = mock_server
            .mock("GET", "/cache-for-4s/foo")
            .with_status(404)
            .expect(1)
            .create();
        let cache_for_4s_bar_mock = mock_server
            .mock("GET", "/cache-for-4s/bar")
            .with_body("C")
            .expect(1)
            .create();

        let foo = cache
            .get("/cache-for-4s/foo", now + Duration::from_secs(4))
            .await
            .unwrap();
        assert!(foo.is_none());

        let bar = cache
            .get("/cache-for-4s/bar", now + Duration::from_secs(4))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(resource_to_string(bar).await, "C");

        let bar = cache
            .get("/cache-for-4s/bar", now + Duration::from_secs(7))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(resource_to_string(bar).await, "C");

        cache_for_4s_foo_mock.assert_async().await;
        cache_for_4s_foo_mock.remove_async().await;

        cache_for_4s_bar_mock.assert_async().await;
        cache_for_4s_bar_mock.remove_async().await;
    }

    {
        // "/cache-forever/foo" should be cached permanently

        let cache_forever_foo_mock = mock_server
            .mock("GET", "/cache-forever/foo")
            .with_body("A")
            .expect(1)
            .create();

        let foo = cache.get("/cache-forever/foo", now).await.unwrap().unwrap();
        assert_eq!(resource_to_string(foo).await, "A");

        let foo = cache
            .get(
                "/cache-forever/foo",
                now + Duration::from_hours(365 * 24 * 100), // ~100 years
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(resource_to_string(foo).await, "A");

        cache_forever_foo_mock.assert_async().await;
        cache_forever_foo_mock.remove_async().await;
    }

    {
        // "/cache-never/foo" should never cache, and should always send a
        // request upstream

        let cache_never_foo_mock = mock_server
            .mock("GET", "/cache-never/foo")
            .with_body("A")
            .expect(1)
            .create();

        let foo = cache.get("/cache-never/foo", now).await.unwrap().unwrap();
        assert_eq!(resource_to_string(foo).await, "A");

        cache_never_foo_mock.assert_async().await;
        cache_never_foo_mock.remove_async().await;

        let cache_never_foo_mock = mock_server
            .mock("GET", "/cache-never/foo")
            .with_body("B")
            .expect(1)
            .create();

        let foo = cache.get("/cache-never/foo", now).await.unwrap().unwrap();
        assert_eq!(resource_to_string(foo).await, "B");

        cache_never_foo_mock.assert_async().await;
        cache_never_foo_mock.remove_async().await;

        let cache_never_foo_mock = mock_server
            .mock("GET", "/cache-never/foo")
            .with_body("C")
            .expect(1)
            .create();

        let foo = cache
            .get("/cache-never/foo", now + Duration::from_secs(1))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(resource_to_string(foo).await, "C");

        cache_never_foo_mock.assert_async().await;
        cache_never_foo_mock.remove_async().await;
    }
}

#[tokio::test]
async fn test_cache_match_path() {
    let ctx = test_context();
    let now = std::time::Instant::now();
    let mut mock_server = mockito::Server::new_async().await;

    let upstream_store = mockito_http_store(&mock_server);
    let storage = server3::cache::CacheStorage::new(cache_config(&ctx)).unwrap();

    let mut routes = CacheRoutes::new(CacheRouteRule::Enabled(CacheEnabledRouteRule {
        max_age: CacheMaxAgeRule::CacheNever,
    }));
    routes.add_route("/a", CacheRouteRule::Disabled);
    routes.add_route(
        "/a/foo",
        CacheRouteRule::Enabled(CacheEnabledRouteRule {
            max_age: CacheMaxAgeRule::CacheNever,
        }),
    );
    routes.add_route("/a/foo/bar", CacheRouteRule::Disabled);
    routes.add_route("/b", CacheRouteRule::Disabled);
    routes.add_route("/c/*", CacheRouteRule::Disabled);

    let cache = server3::cache::Cache::new(
        Arc::new(storage),
        "host".into(),
        Arc::new(routes),
        upstream_store,
    );

    {
        // "/whatever" should be enabled (default rule)

        let mock = mock_server.mock("GET", "/whatever").expect(1).create();

        let resource = cache.get("/whatever", now).await.unwrap();
        assert!(resource.is_some());

        mock.assert_async().await;
        mock.remove_async().await;
    }

    {
        // "/a" should be disabled (exact match)

        let mock = mock_server.mock("GET", "/a").expect(0).create();

        let resource = cache.get("/a", now).await.unwrap();
        assert!(resource.is_none());

        mock.assert_async().await;
        mock.remove_async().await;
    }

    {
        // "/a/whatever" should be enabled (default rule)

        let mock = mock_server.mock("GET", "/a/whatever").expect(1).create();

        let resource = cache.get("/a/whatever", now).await.unwrap();
        assert!(resource.is_some());

        mock.assert_async().await;
        mock.remove_async().await;
    }

    {
        // "/a/foo" should be enabled (exact match)

        let mock = mock_server.mock("GET", "/a/foo").expect(1).create();

        let resource = cache.get("/a/foo", now).await.unwrap();
        assert!(resource.is_some());

        mock.assert_async().await;
        mock.remove_async().await;
    }

    {
        // "/a/foo/bar" should be disabled (exact match)

        let mock = mock_server.mock("GET", "/a/foo/bar").expect(0).create();

        let resource = cache.get("/a/foo/bar", now).await.unwrap();
        assert!(resource.is_none());

        mock.assert_async().await;
        mock.remove_async().await;
    }

    {
        // "/a/foo/bar/whatever" should be enabled (default rule)

        let mock = mock_server
            .mock("GET", "/a/foo/bar/whatever")
            .expect(1)
            .create();

        let resource = cache.get("/a/foo/bar/whatever", now).await.unwrap();
        assert!(resource.is_some());

        mock.assert_async().await;
        mock.remove_async().await;
    }

    {
        // "/b" should be disabled (exact match)

        let mock = mock_server.mock("GET", "/b").expect(0).create();

        let resource = cache.get("/b", now).await.unwrap();
        assert!(resource.is_none());

        mock.assert_async().await;
        mock.remove_async().await;
    }

    {
        // "/b/foo" should be enabled (default rule)

        let mock = mock_server.mock("GET", "/b/foo").expect(1).create();

        let resource = cache.get("/b/foo", now).await.unwrap();
        assert!(resource.is_some());

        mock.assert_async().await;
        mock.remove_async().await;
    }

    {
        // "/c" should be enabled (default rule)

        let mock = mock_server.mock("GET", "/c").expect(1).create();

        let resource = cache.get("/c", now).await.unwrap();
        assert!(resource.is_some());

        mock.assert_async().await;
        mock.remove_async().await;
    }

    {
        // "/c/foo" should be disabled (wildcard match)

        let mock = mock_server.mock("GET", "/c/foo").expect(0).create();

        let resource = cache.get("/c/foo", now).await.unwrap();
        assert!(resource.is_none());

        mock.assert_async().await;
        mock.remove_async().await;
    }
}

#[tokio::test]
async fn test_cache_match_path_star() {
    let ctx = test_context();
    let now = std::time::Instant::now();
    let mock_server = mockito::Server::new_async().await;

    let upstream_store = mockito_http_store(&mock_server);
    let storage = server3::cache::CacheStorage::new(cache_config(&ctx)).unwrap();

    let mut routes = CacheRoutes::new(CacheRouteRule::Enabled(CacheEnabledRouteRule {
        max_age: CacheMaxAgeRule::CacheNever,
    }));
    routes.add_route("/*", CacheRouteRule::Disabled);

    let cache = server3::cache::Cache::new(
        Arc::new(storage),
        "host".into(),
        Arc::new(routes),
        upstream_store,
    );

    // "/*" should match all paths, and so the rule should always take
    // precedent over the default rule

    let resource = cache.get("/", now).await.unwrap();
    assert!(resource.is_none());

    let resource = cache.get("/foo", now).await.unwrap();
    assert!(resource.is_none());

    let resource = cache.get("/foo/bar", now).await.unwrap();
    assert!(resource.is_none());

    let resource = cache.get("/foo/bar/", now).await.unwrap();
    assert!(resource.is_none());
}
