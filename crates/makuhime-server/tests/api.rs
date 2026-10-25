//! 路由级别的集成测试：不联网，只验证本地接口的形状与错误码。

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use makuhime_core::config::AppConfig;
use makuhime_core::engine::Engine;
use makuhime_server::{AppState, router};
use tower::ServiceExt;

async fn test_app() -> axum::Router {
    let mut config = AppConfig::default();
    config.rooms.clear();
    let engine = Engine::start(config).await.expect("引擎启动失败");
    let state = Arc::new(AppState {
        engine,
        web_dir: None,
        local_only: true,
    });
    router(state)
}

#[tokio::test]
async fn health_endpoint_reports_name_and_version() {
    let response = test_app()
        .await
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 8192).await.unwrap();
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("maku-hime"), "响应里应包含产品名: {text}");
    assert!(text.contains("version"), "响应里应包含版本号: {text}");
}

#[tokio::test]
async fn state_endpoint_returns_snapshot_with_config() {
    let response = test_app()
        .await
        .oneshot(
            Request::builder()
                .uri("/api/state")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(value["config"]["ui"]["font_size"].is_number());
    assert!(value["rooms"].is_array());
    assert_eq!(value["login"]["logged_in"], false);
}

#[tokio::test]
async fn sending_without_login_returns_401() {
    let response = test_app()
        .await
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/send")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"room_id": 1, "text": "你好"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = to_bytes(response.into_body(), 8192).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["error"]["code"], "not_logged_in");
}

fn request(method: &str, uri: &str, host: &str, origin: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", host);
    if let Some(origin) = origin {
        builder = builder.header("origin", origin);
    }
    builder.body(Body::empty()).unwrap()
}

#[tokio::test]
async fn state_endpoint_serves_local_origin() {
    let response = test_app()
        .await
        .oneshot(request(
            "GET",
            "/api/state",
            "127.0.0.1:3939",
            Some("http://127.0.0.1:3939"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn foreign_origin_gets_403_even_for_reads() {
    // 任意网页连本机接口（含 WebSocket 握手）必须被拒
    for uri in ["/api/state", "/api/ws", "/"] {
        let response = test_app()
            .await
            .oneshot(request(
                "GET",
                uri,
                "127.0.0.1:3939",
                Some("https://evil.example"),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{uri} 应该 403");
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["error"]["code"], "forbidden_origin");
    }
}

#[tokio::test]
async fn dns_rebinding_host_is_rejected() {
    let response = test_app()
        .await
        .oneshot(request(
            "GET",
            "/api/state",
            "evil.example:3939",
            Some("http://evil.example:3939"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn typo_in_api_path_returns_json_404_not_html() {
    // 有前端构建产物时，静态 fallback 不能把 /api/xxx 变成 index.html
    let dir = std::env::temp_dir().join("makuhime-spa-test");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), "<html>panel</html>").unwrap();

    let mut config = AppConfig::default();
    config.rooms.clear();
    let engine = Engine::start(config).await.unwrap();
    let state = Arc::new(AppState {
        engine,
        web_dir: Some(dir),
        local_only: true,
    });
    let response = router(state)
        .oneshot(request("GET", "/api/typo", "127.0.0.1:3939", None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = to_bytes(response.into_body(), 8192).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["error"]["code"], "not_found");
}

#[tokio::test]
async fn unknown_api_route_returns_404() {
    let response = test_app()
        .await
        .oneshot(
            Request::builder()
                .uri("/api/does-not-exist")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn history_of_unknown_room_is_empty() {
    let response = test_app()
        .await
        .oneshot(
            Request::builder()
                .uri("/api/history/123456?limit=10")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 8192).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value, serde_json::json!([]));
}
