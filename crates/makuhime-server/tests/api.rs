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
