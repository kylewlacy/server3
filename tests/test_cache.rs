use server3::{
    config::CacheConfig,
    models::{BakeOutput, ProjectSource},
    store::Store as _,
};

use crate::test_utils::{
    body_to_bytes, body_to_string, cache_config, fake_hash_id, mockito_http_store, test_context,
};

mod test_utils;

#[tokio::test]
async fn test_cache_chunk() {
    let ctx = test_context();
    let mut mock_server = mockito::Server::new_async().await;

    let upstream_store = mockito_http_store(&mock_server);
    let cache = server3::store::cache::CacheStore::new(upstream_store, cache_config(&ctx)).unwrap();

    // Put a chunk in the upstream server
    let chunk_id = fake_hash_id(0x1234);
    let chunk_mock = mock_server
        .mock("GET", &*format!("/chunks/{chunk_id}.zst"))
        .with_body("chunk data")
        .expect(1)
        .create();

    // Validate that we can get the chunk via the cache
    let chunk = cache.get_chunk_zst(chunk_id).await.unwrap().unwrap();
    assert_eq!(body_to_string(chunk).await, "chunk data");

    // Get the same chunk again, but it should be cached and so shouldn't
    // trigger another upstream request.
    let chunk = cache.get_chunk_zst(chunk_id).await.unwrap().unwrap();
    assert_eq!(body_to_string(chunk).await, "chunk data");

    chunk_mock.assert_async().await;
}

#[tokio::test]
async fn test_cache_artifact() {
    let ctx = test_context();
    let mut mock_server = mockito::Server::new_async().await;

    let upstream_store = mockito_http_store(&mock_server);
    let cache = server3::store::cache::CacheStore::new(upstream_store, cache_config(&ctx)).unwrap();

    // Put an artifact in the upstream server
    let artifact_id = fake_hash_id(0x1111);
    let artifact_mock = mock_server
        .mock("GET", &*format!("/artifacts/{artifact_id}.bar.zst"))
        .with_body("fake artifact")
        .expect(1)
        .create();

    // Validate that we can get the artifact via the cache
    let artifact = cache
        .get_artifact_bar_zst(artifact_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(body_to_string(artifact).await, "fake artifact");

    // Get the same artifact again, but it should be cached and so shouldn't
    // trigger another upstream request.
    let artifact = cache
        .get_artifact_bar_zst(artifact_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(body_to_string(artifact).await, "fake artifact");

    artifact_mock.assert_async().await;
}

#[tokio::test]
async fn test_cache_project_source() {
    let ctx = test_context();
    let mut mock_server = mockito::Server::new_async().await;

    let upstream_store = mockito_http_store(&mock_server);
    let cache = server3::store::cache::CacheStore::new(upstream_store, cache_config(&ctx)).unwrap();

    // Put a project source in the upstream server
    let project_id = fake_hash_id(0x4321);
    let project_artifact_hash = fake_hash_id(0x9999);
    let project_source_str = format!(r#"{{"artifactHash":"{project_artifact_hash}"}}"#);
    let project_source_mock = mock_server
        .mock("GET", &*format!("/projects/{project_id}/source.json"))
        .with_body(project_source_str)
        .expect(1)
        .create();

    // Validate that we can get the project source via the cache
    let project_source = cache.get_project_source(project_id).await.unwrap().unwrap();
    assert_eq!(
        project_source,
        ProjectSource {
            artifact_hash: project_artifact_hash,
        },
    );

    // Get the same artifact again, but it should be cached and so shouldn't
    // trigger another upstream request.
    let project_source = cache.get_project_source(project_id).await.unwrap().unwrap();
    assert_eq!(
        project_source,
        ProjectSource {
            artifact_hash: project_artifact_hash,
        },
    );

    project_source_mock.assert_async().await;
}

#[tokio::test]
async fn test_cache_bake_output() {
    let ctx = test_context();
    let mut mock_server = mockito::Server::new_async().await;

    let upstream_store = mockito_http_store(&mock_server);
    let cache = server3::store::cache::CacheStore::new(upstream_store, cache_config(&ctx)).unwrap();

    // Put a project source in the upstream server
    let recipe_id = fake_hash_id(0x9876);
    let output_artifact_hash = fake_hash_id(0x8888);
    let bake_output_json_str = format!(r#"{{"outputHash":"{output_artifact_hash}"}}"#);
    let bake_output_mock = mock_server
        .mock("GET", &*format!("/bakes/{recipe_id}/output.json"))
        .with_body(bake_output_json_str)
        .expect(1)
        .create();

    // Validate that we can get the project source via the cache
    let bake_output = cache.get_bake_output(recipe_id).await.unwrap().unwrap();
    assert_eq!(
        bake_output,
        BakeOutput {
            output_hash: output_artifact_hash,
        },
    );

    // Get the same artifact again, but it should be cached and so shouldn't
    // trigger another upstream request.
    let bake_output = cache.get_bake_output(recipe_id).await.unwrap().unwrap();
    assert_eq!(
        bake_output,
        BakeOutput {
            output_hash: output_artifact_hash,
        },
    );

    bake_output_mock.assert_async().await;
}

#[tokio::test]
async fn test_cache_max_project_sources() {
    let ctx = test_context();
    let mut mock_server = mockito::Server::new_async().await;

    let upstream_store = mockito_http_store(&mock_server);
    let config = CacheConfig {
        max_project_sources: 3,
        ..cache_config(&ctx)
    };
    let cache = server3::store::cache::CacheStore::new(upstream_store, config).unwrap();

    let artifact_hash_1 = fake_hash_id(0x100);
    let artifact_hash_2 = fake_hash_id(0x2000);
    let artifact_hash_3 = fake_hash_id(0x3000);

    // Add 3 project sources to the upstream cache
    let project_source_1_mock = mock_server
        .mock(
            "GET",
            &*format!("/projects/{}/source.json", fake_hash_id(1)),
        )
        .with_body(format!(r#"{{"artifactHash":"{artifact_hash_1}"}}"#))
        .expect(1)
        .create();
    let project_source_2_mock = mock_server
        .mock(
            "GET",
            &*format!("/projects/{}/source.json", fake_hash_id(2)),
        )
        .with_body(format!(r#"{{"artifactHash":"{artifact_hash_2}"}}"#))
        .expect(1)
        .create();
    let project_source_3_mock = mock_server
        .mock(
            "GET",
            &*format!("/projects/{}/source.json", fake_hash_id(3)),
        )
        .with_body(format!(r#"{{"artifactHash":"{artifact_hash_3}"}}"#))
        .expect(1)
        .create();

    // Query each upstream project 5 times
    for _ in 0..5 {
        let project_source = cache
            .get_project_source(fake_hash_id(1))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            project_source,
            ProjectSource {
                artifact_hash: artifact_hash_1
            }
        );

        let project_source = cache
            .get_project_source(fake_hash_id(2))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            project_source,
            ProjectSource {
                artifact_hash: artifact_hash_2
            }
        );

        let project_source = cache
            .get_project_source(fake_hash_id(3))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            project_source,
            ProjectSource {
                artifact_hash: artifact_hash_3
            }
        );
    }

    // At this point, project 1 is the least-recently used, followed by
    // project 2, then project 3

    // Assert that each upstream project was fetched exactly once
    project_source_1_mock.assert_async().await;
    project_source_2_mock.assert_async().await;
    project_source_3_mock.assert_async().await;

    // Now, remove the upstream mocks
    project_source_1_mock.remove_async().await;
    project_source_2_mock.remove_async().await;
    project_source_3_mock.remove_async().await;
    drop(project_source_1_mock);
    drop(project_source_2_mock);
    drop(project_source_3_mock);

    let artifact_hash_1 = fake_hash_id(0x1001);
    let artifact_hash_2 = fake_hash_id(0x2001);
    let artifact_hash_3 = fake_hash_id(0x3001);

    // Add projects 4 and 5 upstream, and add project 1 with a different source
    let project_source_4_mock = mock_server
        .mock(
            "GET",
            &*format!("/projects/{}/source.json", fake_hash_id(4)),
        )
        .with_body(format!(r#"{{"artifactHash":"{artifact_hash_1}"}}"#))
        .expect(1)
        .create();
    let project_source_5_mock = mock_server
        .mock(
            "GET",
            &*format!("/projects/{}/source.json", fake_hash_id(5)),
        )
        .with_body(format!(r#"{{"artifactHash":"{artifact_hash_2}"}}"#))
        .expect(1)
        .create();
    let project_source_1_mock = mock_server
        .mock(
            "GET",
            &*format!("/projects/{}/source.json", fake_hash_id(1)),
        )
        .with_body(format!(r#"{{"artifactHash":"{artifact_hash_3}"}}"#))
        .expect(1)
        .create();

    // Fetch project 4, which should evict project 1 from the cache
    let project_source = cache
        .get_project_source(fake_hash_id(4))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        project_source,
        ProjectSource {
            artifact_hash: artifact_hash_1
        }
    );

    // Fetch project 5, which should evict project 2 from the cache
    let project_source = cache
        .get_project_source(fake_hash_id(5))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        project_source,
        ProjectSource {
            artifact_hash: artifact_hash_2
        }
    );

    // Fetch project 1 again, which should evict project 3 from the cache
    let project_source = cache
        .get_project_source(fake_hash_id(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        project_source,
        ProjectSource {
            artifact_hash: artifact_hash_3
        }
    );

    // Project 4 and 5 should've been fetched once, and project 1 should've
    // been fetched again since it was evicted
    project_source_4_mock.assert_async().await;
    project_source_5_mock.assert_async().await;
    project_source_1_mock.assert_async().await;
}

#[tokio::test]
async fn test_cache_max_bake_outputs() {
    let ctx = test_context();
    let mut mock_server = mockito::Server::new_async().await;

    let upstream_store = mockito_http_store(&mock_server);
    let config = CacheConfig {
        max_bake_outputs: 3,
        ..cache_config(&ctx)
    };
    let cache = server3::store::cache::CacheStore::new(upstream_store, config).unwrap();

    let artifact_hash_1 = fake_hash_id(0x100);
    let artifact_hash_2 = fake_hash_id(0x2000);
    let artifact_hash_3 = fake_hash_id(0x3000);

    // Add 3 bake outputs to the upstream cache
    let bake_output_1_mock = mock_server
        .mock("GET", &*format!("/bakes/{}/output.json", fake_hash_id(1)))
        .with_body(format!(r#"{{"outputHash":"{artifact_hash_1}"}}"#))
        .expect(1)
        .create();
    let bake_output_2_mock = mock_server
        .mock("GET", &*format!("/bakes/{}/output.json", fake_hash_id(2)))
        .with_body(format!(r#"{{"outputHash":"{artifact_hash_2}"}}"#))
        .expect(1)
        .create();
    let bake_output_3_mock = mock_server
        .mock("GET", &*format!("/bakes/{}/output.json", fake_hash_id(3)))
        .with_body(format!(r#"{{"outputHash":"{artifact_hash_3}"}}"#))
        .expect(1)
        .create();

    // Query each bake output 5 times
    for _ in 0..5 {
        let bake_output = cache
            .get_bake_output(fake_hash_id(1))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            bake_output,
            BakeOutput {
                output_hash: artifact_hash_1
            }
        );

        let bake_output = cache
            .get_bake_output(fake_hash_id(2))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            bake_output,
            BakeOutput {
                output_hash: artifact_hash_2
            }
        );

        let bake_output = cache
            .get_bake_output(fake_hash_id(3))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            bake_output,
            BakeOutput {
                output_hash: artifact_hash_3
            }
        );
    }

    // At this point, bake output 1 is the least-recently used, followed by
    // bake output 2, then bake output 3

    // Assert that each upstream bake output was fetched exactly once
    bake_output_1_mock.assert_async().await;
    bake_output_2_mock.assert_async().await;
    bake_output_3_mock.assert_async().await;

    // Now, remove the upstream mocks
    bake_output_1_mock.remove_async().await;
    bake_output_2_mock.remove_async().await;
    bake_output_3_mock.remove_async().await;
    drop(bake_output_1_mock);
    drop(bake_output_2_mock);
    drop(bake_output_3_mock);

    let artifact_hash_1 = fake_hash_id(0x1001);
    let artifact_hash_2 = fake_hash_id(0x2001);
    let artifact_hash_3 = fake_hash_id(0x3001);

    // Add bake outputs 4 and 5 upstream, and add bake output 1 with a
    // different output hash
    let bake_output_4_mock = mock_server
        .mock("GET", &*format!("/bakes/{}/output.json", fake_hash_id(4)))
        .with_body(format!(r#"{{"outputHash":"{artifact_hash_1}"}}"#))
        .expect(1)
        .create();
    let bake_output_5_mock = mock_server
        .mock("GET", &*format!("/bakes/{}/output.json", fake_hash_id(5)))
        .with_body(format!(r#"{{"outputHash":"{artifact_hash_2}"}}"#))
        .expect(1)
        .create();
    let bake_output_1_mock = mock_server
        .mock("GET", &*format!("/bakes/{}/output.json", fake_hash_id(1)))
        .with_body(format!(r#"{{"outputHash":"{artifact_hash_3}"}}"#))
        .expect(1)
        .create();

    // Fetch bake output 4, which should evict bake output 1 from the cache
    let bake_output = cache
        .get_bake_output(fake_hash_id(4))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        bake_output,
        BakeOutput {
            output_hash: artifact_hash_1
        }
    );

    // Fetch bake output 5, which should evict bake output 2 from the cache
    let bake_output = cache
        .get_bake_output(fake_hash_id(5))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        bake_output,
        BakeOutput {
            output_hash: artifact_hash_2
        }
    );

    // Fetch bake output 1 again, which should evict bake output 3
    // from the cache
    let bake_output = cache
        .get_bake_output(fake_hash_id(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        bake_output,
        BakeOutput {
            output_hash: artifact_hash_3
        }
    );

    // Bake outputs 4 and 5 should've been fetched once, and bake output 1
    // should've been fetched again since it was evicted
    bake_output_4_mock.assert_async().await;
    bake_output_5_mock.assert_async().await;
    bake_output_1_mock.assert_async().await;
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

    // Add a 1 KB artifact and two 1 KB chunks to the upstream cache
    let artifact_1_mock = mock_server
        .mock("GET", &*format!("/artifacts/{}.bar.zst", fake_hash_id(1)))
        .with_body(&bytes_1x100)
        .expect(1)
        .create();
    let chunk_1_mock = mock_server
        .mock("GET", &*format!("/chunks/{}.zst", fake_hash_id(1)))
        .with_body(&bytes_2x100)
        .expect(1)
        .create();
    let chunk_2_mock = mock_server
        .mock("GET", &*format!("/chunks/{}.zst", fake_hash_id(2)))
        .with_body(&bytes_3x100)
        .expect(1)
        .create();

    // Query the artifact and chunks 5 times each
    for _ in 0..5 {
        let artifact = cache
            .get_artifact_bar_zst(fake_hash_id(1))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(body_to_bytes(artifact).await, bytes_1x100);

        let chunk_1 = cache.get_chunk_zst(fake_hash_id(1)).await.unwrap().unwrap();
        assert_eq!(body_to_bytes(chunk_1).await, bytes_2x100);

        let chunk_2 = cache.get_chunk_zst(fake_hash_id(2)).await.unwrap().unwrap();
        assert_eq!(body_to_bytes(chunk_2).await, bytes_3x100);
    }

    // At this point, artifact 1 is the least-recently used, followed by
    // chunk 1, then chunk 2

    // Assert that each upstream artifact + chunk was fetched exactly once
    artifact_1_mock.assert_async().await;
    chunk_1_mock.assert_async().await;
    chunk_2_mock.assert_async().await;

    // Now, remove the upstream mocks
    artifact_1_mock.remove_async().await;
    chunk_1_mock.remove_async().await;
    chunk_2_mock.remove_async().await;
    drop(artifact_1_mock);
    drop(chunk_1_mock);
    drop(chunk_2_mock);

    // Add artifact 2 and chunk 3 upstream, and re-add artifact 1 with
    // different contents
    let bytes_4x100 = vec![4u8; 100];
    let bytes_5x100 = vec![5u8; 100];
    let bytes_6x100 = vec![6u8; 100];
    let artifact_2_mock = mock_server
        .mock("GET", &*format!("/artifacts/{}.bar.zst", fake_hash_id(2)))
        .with_body(&bytes_4x100)
        .expect(1)
        .create();
    let chunk_3_mock = mock_server
        .mock("GET", &*format!("/chunks/{}.zst", fake_hash_id(3)))
        .with_body(&bytes_5x100)
        .expect(1)
        .create();
    let artifact_1_mock = mock_server
        .mock("GET", &*format!("/artifacts/{}.bar.zst", fake_hash_id(1)))
        .with_body(&bytes_6x100)
        .expect(1)
        .create();

    // Fetch artifact 2, which should evict artifact 1 from the cache
    let artifact_2 = cache
        .get_artifact_bar_zst(fake_hash_id(2))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(body_to_bytes(artifact_2).await, bytes_4x100);

    // Fetch chunk 3, which should evict chunk 1 from the cache
    let chunk_3 = cache.get_chunk_zst(fake_hash_id(3)).await.unwrap().unwrap();
    assert_eq!(body_to_bytes(chunk_3).await, bytes_5x100);

    // Fetch artifact 1 again, which should evict chunk 2
    // from the cache
    let artifact_1 = cache
        .get_artifact_bar_zst(fake_hash_id(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(body_to_bytes(artifact_1).await, bytes_6x100);

    // Artifacts 1 (again) and 2, plus chunk 3 should've all been fetched
    // from upstream once
    artifact_2_mock.assert_async().await;
    chunk_3_mock.assert_async().await;
    artifact_1_mock.assert_async().await;

    // Remove the upstream mocks
    artifact_2_mock.remove_async().await;
    chunk_3_mock.remove_async().await;
    artifact_1_mock.remove_async().await;
    drop(artifact_2_mock);
    drop(chunk_3_mock);
    drop(artifact_1_mock);

    // Add artifact 3, which fully fills the cache on its own and so
    // evicts everything else
    let bytes_7x499 = vec![7u8; 499];
    let artifact_3_mock = mock_server
        .mock("GET", &*format!("/artifacts/{}.bar.zst", fake_hash_id(3)))
        .with_body(&bytes_7x499)
        .expect(1)
        .create();

    // Fetch artifact 3 five times
    for _ in 0..5 {
        let artifact_3 = cache
            .get_artifact_bar_zst(fake_hash_id(3))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(body_to_bytes(artifact_3).await, bytes_7x499);
    }

    // Make sure artifact 3 was only fetched once
    artifact_3_mock.assert_async().await;
}
