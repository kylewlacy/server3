use std::sync::Arc;

use server3::upstream::{Upstream, UpstreamError, fallthrough::FallthroughUpstream};

use server3_test_support::{mockito_http_upstream, test_context, upstream_resource_to_string};

#[tokio::test]
async fn test_fallthrough_empty() {
    let _ctx = test_context();

    let fallthrough = FallthroughUpstream::new(vec![]);

    let resource = fallthrough.get("/hello-world").await.unwrap();
    assert!(resource.is_none());
}

#[tokio::test]
async fn test_fallthrough_passthrough_with_one() {
    let _ctx = test_context();

    let mut mock_server = mockito::Server::new_async().await;
    let upstream = mockito_http_upstream(&mock_server);

    let fallthrough = FallthroughUpstream::new(vec![Arc::new(upstream)]);

    let found_mock = mock_server
        .mock("GET", "/found")
        .with_body("found!")
        .create();
    let not_found_mock = mock_server
        .mock("GET", "/not-found")
        .with_status(404)
        .create();
    let error_mock = mock_server.mock("GET", "/error").with_status(500).create();

    let found = fallthrough.get("/found").await.unwrap().unwrap();
    assert_eq!(upstream_resource_to_string(found).await, "found!");

    let not_found = fallthrough.get("/not-found").await.unwrap();
    assert!(not_found.is_none());

    let error = fallthrough.get("/error").await;
    assert!(error.is_err());

    found_mock.assert_async().await;
    not_found_mock.assert_async().await;
    error_mock.assert_async().await;
}

#[tokio::test]
async fn test_fallthrough_returns_first() {
    let _ctx = test_context();

    let mut mock_server_a = mockito::Server::new_async().await;
    let upstream_a = mockito_http_upstream(&mock_server_a);

    let mut mock_server_b = mockito::Server::new_async().await;
    let upstream_b = mockito_http_upstream(&mock_server_b);

    let mut mock_server_c = mockito::Server::new_async().await;
    let upstream_c = mockito_http_upstream(&mock_server_c);

    let fallthrough = FallthroughUpstream::new(vec![
        Arc::new(upstream_a),
        Arc::new(upstream_b),
        Arc::new(upstream_c),
    ]);

    let a_mock = mock_server_a.mock("GET", "/foo").with_body("A").create();
    let b_mock = mock_server_b.mock("GET", "/foo").expect(0).create();
    let c_mock = mock_server_c.mock("GET", "/foo").expect(0).create();

    let resource = fallthrough.get("/foo").await.unwrap().unwrap();
    assert_eq!(upstream_resource_to_string(resource).await, "A");

    a_mock.assert_async().await;
    b_mock.assert_async().await;
    c_mock.assert_async().await;

    a_mock.remove_async().await;
    b_mock.remove_async().await;
    c_mock.remove_async().await;

    let a_mock = mock_server_a.mock("GET", "/bar").with_status(404).create();
    let b_mock = mock_server_b.mock("GET", "/bar").with_body("B").create();
    let c_mock = mock_server_c.mock("GET", "/bar").expect(0).create();

    let resource = fallthrough.get("/bar").await.unwrap().unwrap();
    assert_eq!(upstream_resource_to_string(resource).await, "B");

    a_mock.assert_async().await;
    b_mock.assert_async().await;
    c_mock.assert_async().await;

    a_mock.remove_async().await;
    b_mock.remove_async().await;
    c_mock.remove_async().await;

    let a_mock = mock_server_a.mock("GET", "/baz").with_status(404).create();
    let b_mock = mock_server_b.mock("GET", "/baz").with_status(404).create();
    let c_mock = mock_server_c.mock("GET", "/baz").with_status(404).create();

    let resource = fallthrough.get("/baz").await.unwrap();
    assert!(resource.is_none());

    a_mock.assert_async().await;
    b_mock.assert_async().await;
    c_mock.assert_async().await;

    a_mock.remove_async().await;
    b_mock.remove_async().await;
    c_mock.remove_async().await;
}

#[tokio::test]
async fn test_fallthrough_ignores_errors() {
    let _ctx = test_context();

    let mut mock_server_a = mockito::Server::new_async().await;
    let upstream_a = mockito_http_upstream(&mock_server_a);

    let mut mock_server_b = mockito::Server::new_async().await;
    let upstream_b = mockito_http_upstream(&mock_server_b);

    let mut mock_server_c = mockito::Server::new_async().await;
    let upstream_c = mockito_http_upstream(&mock_server_c);

    let fallthrough = FallthroughUpstream::new(vec![
        Arc::new(upstream_a),
        Arc::new(upstream_b),
        Arc::new(upstream_c),
    ]);

    let a_mock = mock_server_a.mock("GET", "/foo").with_status(404).create();
    let b_mock = mock_server_b.mock("GET", "/foo").with_status(500).create();
    let c_mock = mock_server_c.mock("GET", "/foo").with_body("C").create();

    let resource = fallthrough.get("/foo").await.unwrap().unwrap();
    assert_eq!(upstream_resource_to_string(resource).await, "C");

    a_mock.assert_async().await;
    b_mock.assert_async().await;
    c_mock.assert_async().await;

    a_mock.remove_async().await;
    b_mock.remove_async().await;
    c_mock.remove_async().await;
}

#[tokio::test]
async fn test_fallthrough_keeps_error_if_no_successes() {
    let _ctx = test_context();

    let mut mock_server_a = mockito::Server::new_async().await;
    let upstream_a = mockito_http_upstream(&mock_server_a);

    let mut mock_server_b = mockito::Server::new_async().await;
    let upstream_b = mockito_http_upstream(&mock_server_b);

    let mut mock_server_c = mockito::Server::new_async().await;
    let upstream_c = mockito_http_upstream(&mock_server_c);

    let fallthrough = FallthroughUpstream::new(vec![
        Arc::new(upstream_a),
        Arc::new(upstream_b),
        Arc::new(upstream_c),
    ]);

    let a_mock = mock_server_a.mock("GET", "/foo").with_status(404).create();
    let b_mock = mock_server_b.mock("GET", "/foo").with_status(500).create();
    let c_mock = mock_server_c.mock("GET", "/foo").with_status(503).create();

    let error = fallthrough.get("/foo").await.map(|_| ()).unwrap_err();
    let UpstreamError::Reqwest(error) = error else {
        panic!("expected reqwest error: {error:#?}");
    };

    // Since all requests failed, the error from the first upstream should
    // be preserved
    assert_eq!(error.status().unwrap().as_u16(), 500);

    a_mock.assert_async().await;
    b_mock.assert_async().await;
    c_mock.assert_async().await;

    a_mock.remove_async().await;
    b_mock.remove_async().await;
    c_mock.remove_async().await;
}
