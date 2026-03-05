use std::{sync::Arc, time::Duration};

use bstr::BStr;
use server3::{
    cache::{CacheEnabledRouteRule, CacheMaxAgeRule, CacheRouteRule, CacheRouteRules},
    config::StorageConfig,
};

use crate::test_utils::{
    cache_config, cache_routes_forever, mockito_http_store, mockito_http_store_with_prefix,
    object_content_type, object_to_bytes, object_to_string, test_context,
};

mod test_utils;

#[tokio::test]
async fn test_cache_object() {
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

    // Put an object in the upstream server
    let key = "/foo/bar.txt";
    let object_mock = mock_server
        .mock("GET", key)
        .with_body("object data")
        .expect(1)
        .create();

    // Validate that we can get the object via the cache
    let object = cache.get(key, now).await.unwrap().unwrap();
    assert_eq!(object_to_string(object).await, "object data");

    // Get the same object again, but it should be cached and so shouldn't
    // trigger another upstream request.
    let object = cache.get(key, now).await.unwrap().unwrap();
    assert_eq!(object_to_string(object).await, "object data");

    object_mock.assert_async().await;
}

#[tokio::test]
async fn test_cache_object_content_type() {
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

    // Put some objects in the upstream store with various
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

    // Validate each of the objects has the right Content-Type. For good
    // measure, get each object multiple times to make sure it's cached

    for _ in 0..5 {
        let object = cache.get("/foo/bar", now).await.unwrap().unwrap();
        assert_eq!(object_content_type(&object), None);
        assert_eq!(object_to_string(object).await, "bar");

        let object = cache.get("/foo/bar-bin", now).await.unwrap().unwrap();
        assert_eq!(
            object_content_type(&object).unwrap(),
            BStr::new("application/octet-stream")
        );
        assert_eq!(object_to_string(object).await, "bar.bin");

        let object = cache.get("/foo/bar-txt", now).await.unwrap().unwrap();
        assert_eq!(
            object_content_type(&object).unwrap(),
            BStr::new("text/plain")
        );
        assert_eq!(object_to_string(object).await, "text");

        let object = cache.get("/foo/bar-html", now).await.unwrap().unwrap();
        assert_eq!(
            object_content_type(&object).unwrap(),
            BStr::new("text/html")
        );
        assert_eq!(object_to_string(object).await, "<html></html>");

        let object = cache.get("/foo/bar-json", now).await.unwrap().unwrap();
        assert_eq!(
            object_content_type(&object).unwrap(),
            BStr::new("application/json")
        );
        assert_eq!(
            object_to_string(object).await,
            "actually, this isn't valid json!"
        );

        let object = cache.get("/foo/bar-custom", now).await.unwrap().unwrap();
        assert_eq!(
            object_content_type(&object).unwrap(),
            BStr::new("application/x.custom")
        );
        assert_eq!(object_to_string(object).await, "custom");

        let object = cache.get("/foo/bar-invalid", now).await.unwrap().unwrap();
        assert_eq!(
            object_content_type(&object).unwrap(),
            BStr::new("not valid ascii 🤔")
        );
        assert_eq!(
            object_to_bytes(object).await,
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
async fn test_cache_max_objects() {
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

    // Add 3 objects to the upstream cache
    let object_1_mock = mock_server
        .mock("GET", "/example-1.json")
        .with_body("example 1")
        .expect(1)
        .create();
    let object_2_mock = mock_server
        .mock("GET", "/example-2.json")
        .with_body("example 2")
        .expect(1)
        .create();
    let object_3_mock = mock_server
        .mock("GET", "/example-3.json")
        .with_body("example 3")
        .expect(1)
        .create();

    // Query each upstream object 5 times
    for _ in 0..5 {
        let object = cache.get("example-1.json", now).await.unwrap().unwrap();
        assert_eq!(object_to_string(object).await, "example 1");

        let object = cache.get("example-2.json", now).await.unwrap().unwrap();
        assert_eq!(object_to_string(object).await, "example 2");

        let object = cache.get("example-3.json", now).await.unwrap().unwrap();
        assert_eq!(object_to_string(object).await, "example 3");
    }

    // At this point, object 1 is the least-recently used, followed by
    // object 2, then object 3

    // Assert that each upstream project was fetched exactly once
    object_1_mock.assert_async().await;
    object_2_mock.assert_async().await;
    object_3_mock.assert_async().await;

    // Now, remove the upstream mocks
    object_1_mock.remove_async().await;
    object_2_mock.remove_async().await;
    object_3_mock.remove_async().await;
    drop(object_1_mock);
    drop(object_2_mock);
    drop(object_3_mock);

    // Add object 4 and 5 upstream, and add object 1 with a different body
    let object_4_mock = mock_server
        .mock("GET", "/example-4.json")
        .with_body("example 4")
        .expect(1)
        .create();
    let object_5_mock = mock_server
        .mock("GET", "/example-5.json")
        .with_body("example 5")
        .expect(1)
        .create();
    let object_1_mock = mock_server
        .mock("GET", "/example-1.json")
        .with_body("example 1 new!")
        .expect(1)
        .create();

    // Fetch object 4, which should evict object 1 from the cache
    let project_source = cache.get("example-4.json", now).await.unwrap().unwrap();
    assert_eq!(object_to_string(project_source).await, "example 4");

    // Fetch object 5, which should evict object 2 from the cache
    let project_source = cache.get("example-5.json", now).await.unwrap().unwrap();
    assert_eq!(object_to_string(project_source).await, "example 5");

    // Fetch object 1 again, which should evict object 3 from the cache
    let project_source = cache.get("example-1.json", now).await.unwrap().unwrap();
    assert_eq!(object_to_string(project_source).await, "example 1 new!");

    // Objects 4 and 5 should've been fetched once, and object 1 should've
    // been fetched again since it was evicted
    object_4_mock.assert_async().await;
    object_5_mock.assert_async().await;
    object_1_mock.assert_async().await;
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

    // Add three 1 KB objects to the upstream cache
    let object_1_mock = mock_server
        .mock("GET", "/example-1.bin")
        .with_body(&bytes_1x100)
        .expect(1)
        .create();
    let object_2_mock = mock_server
        .mock("GET", "/example-2.bin")
        .with_body(&bytes_2x100)
        .expect(1)
        .create();
    let object_3_mock = mock_server
        .mock("GET", "/example-3.bin")
        .with_body(&bytes_3x100)
        .expect(1)
        .create();

    // Query each object 5 times
    for _ in 0..5 {
        let object_1 = cache.get("example-1.bin", now).await.unwrap().unwrap();
        assert_eq!(object_to_bytes(object_1).await, bytes_1x100);

        let example_2 = cache.get("example-2.bin", now).await.unwrap().unwrap();
        assert_eq!(object_to_bytes(example_2).await, bytes_2x100);

        let example_3 = cache.get("example-3.bin", now).await.unwrap().unwrap();
        assert_eq!(object_to_bytes(example_3).await, bytes_3x100);
    }

    // At this point, object 1 is the least-recently used, followed by
    // object 2, then object 3

    // Assert that each upstream object was fetched exactly once
    object_1_mock.assert_async().await;
    object_2_mock.assert_async().await;
    object_3_mock.assert_async().await;

    // Now, remove the upstream mocks
    object_1_mock.remove_async().await;
    object_2_mock.remove_async().await;
    object_3_mock.remove_async().await;
    drop(object_1_mock);
    drop(object_2_mock);
    drop(object_3_mock);

    // Add objects 4 and 5 upstream, and re-add object 1 with
    // different contents
    let bytes_4x100 = vec![4u8; 100];
    let bytes_5x100 = vec![5u8; 100];
    let bytes_6x100 = vec![6u8; 100];
    let object_4_mock = mock_server
        .mock("GET", "/example-4.bin")
        .with_body(&bytes_4x100)
        .expect(1)
        .create();
    let object_5_mock = mock_server
        .mock("GET", "/example-5.bin")
        .with_body(&bytes_5x100)
        .expect(1)
        .create();
    let object_1_mock = mock_server
        .mock("GET", "/example-1.bin")
        .with_body(&bytes_6x100)
        .expect(1)
        .create();

    // Fetch object 4, which should evict object 1 from the cache
    let object_4 = cache.get("example-4.bin", now).await.unwrap().unwrap();
    assert_eq!(object_to_bytes(object_4).await, bytes_4x100);

    // Fetch object 5, which should evict object 2 from the cache
    let object_5 = cache.get("example-5.bin", now).await.unwrap().unwrap();
    assert_eq!(object_to_bytes(object_5).await, bytes_5x100);

    // Fetch object 1 again, which should evict object 3
    // from the cache
    let object_1 = cache.get("example-1.bin", now).await.unwrap().unwrap();
    assert_eq!(object_to_bytes(object_1).await, bytes_6x100);

    // Object 1 (again), 4, and 5 should've all been fetched
    // from upstream once
    object_4_mock.assert_async().await;
    object_5_mock.assert_async().await;
    object_1_mock.assert_async().await;

    // Remove the upstream mocks
    object_4_mock.remove_async().await;
    object_5_mock.remove_async().await;
    object_1_mock.remove_async().await;
    drop(object_4_mock);
    drop(object_5_mock);
    drop(object_1_mock);

    // Add object 6, which fully fills the cache on its own and so
    // evicts everything else
    let bytes_7x499 = vec![7u8; 499];
    let object_6_mock = mock_server
        .mock("GET", "/example-6.bin")
        .with_body(&bytes_7x499)
        .expect(1)
        .create();

    // Fetch object 6 five times
    for _ in 0..5 {
        let object_6 = cache.get("example-6.bin", now).await.unwrap().unwrap();
        assert_eq!(object_to_bytes(object_6).await, bytes_7x499);
    }

    // Make sure object 6 was only fetched once
    object_6_mock.assert_async().await;
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

    // Put the same object in the upstream for both a and b1
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
    let object = cache_a.get("foo.txt", now).await.unwrap().unwrap();
    assert_eq!(object_to_string(object).await, "a foo");

    // Get foo.txt from b1. Since it has a separate host key, it should
    // be distinct from foo.txt from a.
    let object = cache_b1.get("foo.txt", now).await.unwrap().unwrap();
    assert_eq!(object_to_string(object).await, "b1 foo");

    // Get foo.txt from b2. Since it has the same host key as b1, it should
    // re-use the cached result from b1.
    let object = cache_b2.get("foo.txt", now).await.unwrap().unwrap();
    assert_eq!(object_to_string(object).await, "b1 foo");

    // Try to get bar.txt from b1. Upstream returns a 404, so it should
    // not be returned.
    let object = cache_b1.get("bar.txt", now).await.unwrap();
    assert!(object.is_none());

    // Get bar.txt from b2
    let object = cache_b2.get("bar.txt", now).await.unwrap().unwrap();
    assert_eq!(object_to_string(object).await, "b2 bar");

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

    let mut routes = CacheRouteRules::new(CacheRouteRule::Enabled(CacheEnabledRouteRule {
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

        let object = cache.get("cache-for-2s", now).await.unwrap().unwrap();
        assert_eq!(object_to_string(object).await, "A");

        let object = cache
            .get("cache-for-2s", now + Duration::from_secs(1))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(object_to_string(object).await, "A");

        cache_for_2s_mock.assert_async().await;
        cache_for_2s_mock.remove_async().await;
        let cache_for_2s_mock = mock_server
            .mock("GET", "/cache-for-2s")
            .with_body("B")
            .expect(1)
            .create();

        let object = cache
            .get("cache-for-2s", now + Duration::from_secs(2))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(object_to_string(object).await, "B");

        let object = cache
            .get("cache-for-2s", now + Duration::from_secs(3))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(object_to_string(object).await, "B");

        cache_for_2s_mock.assert_async().await;
        cache_for_2s_mock.remove_async().await;
        let cache_for_2s_mock = mock_server
            .mock("GET", "/cache-for-2s")
            .with_status(404)
            .expect(1)
            .create();

        let object = cache
            .get("cache-for-2s", now + Duration::from_secs(4))
            .await
            .unwrap();
        assert!(object.is_none());

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

        let object = cache.get("cache-for-3s", now).await.unwrap().unwrap();
        assert_eq!(object_to_string(object).await, "A");

        let object = cache
            .get("cache-for-3s", now + Duration::from_secs(2))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(object_to_string(object).await, "A");

        cache_for_3s_mock.assert_async().await;
        cache_for_3s_mock.remove_async().await;
        let cache_for_3s_mock = mock_server
            .mock("GET", "/cache-for-3s")
            .with_body("B")
            .expect(1)
            .create();

        let object = cache
            .get("cache-for-3s", now + Duration::from_secs(3))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(object_to_string(object).await, "B");

        let object = cache
            .get("cache-for-3s", now + Duration::from_secs(5))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(object_to_string(object).await, "B");

        cache_for_3s_mock.assert_async().await;
        cache_for_3s_mock.remove_async().await;
        let cache_for_3s_mock = mock_server
            .mock("GET", "/cache-for-3s")
            .with_status(404)
            .expect(1)
            .create();

        let object = cache
            .get("cache-for-3s", now + Duration::from_secs(6))
            .await
            .unwrap();
        assert!(object.is_none());

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

        let object = cache.get("cache-for-3s", now).await.unwrap().unwrap();
        assert_eq!(object_to_string(object).await, "A");

        let object = cache
            .get("cache-for-3s", now + Duration::from_secs(2))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(object_to_string(object).await, "A");

        cache_for_3s_mock.assert_async().await;
        cache_for_3s_mock.remove_async().await;
        let cache_for_3s_mock = mock_server
            .mock("GET", "/cache-for-3s")
            .with_body("B")
            .expect(1)
            .create();

        let object = cache
            .get("cache-for-3s", now + Duration::from_secs(3))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(object_to_string(object).await, "B");

        let object = cache
            .get("cache-for-3s", now + Duration::from_secs(5))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(object_to_string(object).await, "B");

        cache_for_3s_mock.assert_async().await;
        cache_for_3s_mock.remove_async().await;
        let cache_for_3s_mock = mock_server
            .mock("GET", "/cache-for-3s")
            .with_status(404)
            .expect(1)
            .create();

        let object = cache
            .get("cache-for-3s", now + Duration::from_secs(6))
            .await
            .unwrap();
        assert!(object.is_none());

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

        let foo = cache.get("cache-for-4s/foo", now).await.unwrap().unwrap();
        assert_eq!(object_to_string(foo).await, "A");

        let bar = cache.get("cache-for-4s/bar", now).await.unwrap();
        assert!(bar.is_none());

        let foo = cache
            .get("cache-for-4s/foo", now + Duration::from_secs(3))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(object_to_string(foo).await, "A");

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
            .get("cache-for-4s/foo", now + Duration::from_secs(4))
            .await
            .unwrap();
        assert!(foo.is_none());

        let bar = cache
            .get("cache-for-4s/bar", now + Duration::from_secs(4))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(object_to_string(bar).await, "C");

        let bar = cache
            .get("cache-for-4s/bar", now + Duration::from_secs(7))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(object_to_string(bar).await, "C");

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

        let foo = cache.get("cache-forever/foo", now).await.unwrap().unwrap();
        assert_eq!(object_to_string(foo).await, "A");

        let foo = cache
            .get(
                "cache-forever/foo",
                now + Duration::from_hours(365 * 24 * 100), // ~100 years
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(object_to_string(foo).await, "A");

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

        let foo = cache.get("cache-never/foo", now).await.unwrap().unwrap();
        assert_eq!(object_to_string(foo).await, "A");

        cache_never_foo_mock.assert_async().await;
        cache_never_foo_mock.remove_async().await;

        let cache_never_foo_mock = mock_server
            .mock("GET", "/cache-never/foo")
            .with_body("B")
            .expect(1)
            .create();

        let foo = cache.get("cache-never/foo", now).await.unwrap().unwrap();
        assert_eq!(object_to_string(foo).await, "B");

        cache_never_foo_mock.assert_async().await;
        cache_never_foo_mock.remove_async().await;

        let cache_never_foo_mock = mock_server
            .mock("GET", "/cache-never/foo")
            .with_body("C")
            .expect(1)
            .create();

        let foo = cache
            .get("cache-never/foo", now + Duration::from_secs(1))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(object_to_string(foo).await, "C");

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

    let mut routes = CacheRouteRules::new(CacheRouteRule::Enabled(CacheEnabledRouteRule {
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

        let resource = cache.get("whatever", now).await.unwrap();
        assert!(resource.is_some());

        mock.assert_async().await;
        mock.remove_async().await;
    }

    {
        // "/a" should be disabled (exact match)

        let mock = mock_server.mock("GET", "/a").expect(0).create();

        let resource = cache.get("a", now).await.unwrap();
        assert!(resource.is_none());

        mock.assert_async().await;
        mock.remove_async().await;
    }

    {
        // "/a/whatever" should be enabled (default rule)

        let mock = mock_server.mock("GET", "/a/whatever").expect(1).create();

        let resource = cache.get("a/whatever", now).await.unwrap();
        assert!(resource.is_some());

        mock.assert_async().await;
        mock.remove_async().await;
    }

    {
        // "/a/foo" should be enabled (exact match)

        let mock = mock_server.mock("GET", "/a/foo").expect(1).create();

        let resource = cache.get("a/foo", now).await.unwrap();
        assert!(resource.is_some());

        mock.assert_async().await;
        mock.remove_async().await;
    }

    {
        // "/a/foo/bar" should be disabled (exact match)

        let mock = mock_server.mock("GET", "/a/foo/bar").expect(0).create();

        let resource = cache.get("a/foo/bar", now).await.unwrap();
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

        let resource = cache.get("a/foo/bar/whatever", now).await.unwrap();
        assert!(resource.is_some());

        mock.assert_async().await;
        mock.remove_async().await;
    }

    {
        // "/b" should be disabled (exact match)

        let mock = mock_server.mock("GET", "/b").expect(0).create();

        let resource = cache.get("b", now).await.unwrap();
        assert!(resource.is_none());

        mock.assert_async().await;
        mock.remove_async().await;
    }

    {
        // "/b/foo" should be enabled (default rule)

        let mock = mock_server.mock("GET", "/b/foo").expect(1).create();

        let resource = cache.get("b/foo", now).await.unwrap();
        assert!(resource.is_some());

        mock.assert_async().await;
        mock.remove_async().await;
    }

    {
        // "/c" should be enabled (default rule)

        let mock = mock_server.mock("GET", "/c").expect(1).create();

        let resource = cache.get("c", now).await.unwrap();
        assert!(resource.is_some());

        mock.assert_async().await;
        mock.remove_async().await;
    }

    {
        // "/c/foo" should be disabled (wildcard match)

        let mock = mock_server.mock("GET", "/c/foo").expect(0).create();

        let resource = cache.get("c/foo", now).await.unwrap();
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

    let mut routes = CacheRouteRules::new(CacheRouteRule::Enabled(CacheEnabledRouteRule {
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

    let resource = cache.get("", now).await.unwrap();
    assert!(resource.is_none());

    let resource = cache.get("foo", now).await.unwrap();
    assert!(resource.is_none());

    let resource = cache.get("foo/bar", now).await.unwrap();
    assert!(resource.is_none());
}
