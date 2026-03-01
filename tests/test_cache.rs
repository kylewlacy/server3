use server3::{config::CacheConfig, store::Store as _};

use crate::test_utils::{
    body_to_bytes, body_to_string, cache_config, mockito_http_store, test_context,
};

mod test_utils;

#[tokio::test]
async fn test_cache_object() {
    let ctx = test_context();
    let mut mock_server = mockito::Server::new_async().await;

    let upstream_store = mockito_http_store(&mock_server);
    let cache = server3::store::cache::CacheStore::new(upstream_store, cache_config(&ctx)).unwrap();

    // Put an object in the upstream server
    let key = "/foo/bar.txt";
    let object_mock = mock_server
        .mock("GET", key)
        .with_body("object data")
        .expect(1)
        .create();

    // Validate that we can get the object via the cache
    let object = cache.get_object(key).await.unwrap().unwrap();
    assert_eq!(body_to_string(object).await, "object data");

    // Get the same object again, but it should be cached and so shouldn't
    // trigger another upstream request.
    let object = cache.get_object(key).await.unwrap().unwrap();
    assert_eq!(body_to_string(object).await, "object data");

    object_mock.assert_async().await;
}

#[tokio::test]
async fn test_cache_max_disk_capacity() {
    let ctx = test_context();
    let mut mock_server = mockito::Server::new_async().await;

    let upstream_store = mockito_http_store(&mock_server);
    let config = CacheConfig {
        max_disk_capacity: bytesize::ByteSize::b(499),
        ..cache_config(&ctx)
    };
    let cache = server3::store::cache::CacheStore::new(upstream_store, config).unwrap();

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
        let object_1 = cache.get_object("example-1.bin").await.unwrap().unwrap();
        assert_eq!(body_to_bytes(object_1).await, bytes_1x100);

        let example_2 = cache.get_object("example-2.bin").await.unwrap().unwrap();
        assert_eq!(body_to_bytes(example_2).await, bytes_2x100);

        let example_3 = cache.get_object("example-3.bin").await.unwrap().unwrap();
        assert_eq!(body_to_bytes(example_3).await, bytes_3x100);
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
    let object_4 = cache.get_object("example-4.bin").await.unwrap().unwrap();
    assert_eq!(body_to_bytes(object_4).await, bytes_4x100);

    // Fetch object 5, which should evict object 2 from the cache
    let object_5 = cache.get_object("example-5.bin").await.unwrap().unwrap();
    assert_eq!(body_to_bytes(object_5).await, bytes_5x100);

    // Fetch object 1 again, which should evict object 3
    // from the cache
    let object_1 = cache.get_object("example-1.bin").await.unwrap().unwrap();
    assert_eq!(body_to_bytes(object_1).await, bytes_6x100);

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
        let object_6 = cache.get_object("example-6.bin").await.unwrap().unwrap();
        assert_eq!(body_to_bytes(object_6).await, bytes_7x499);
    }

    // Make sure object 6 was only fetched once
    object_6_mock.assert_async().await;
}
