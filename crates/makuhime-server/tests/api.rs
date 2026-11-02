//! 路由级别的集成测试：不联网，只验证本地接口的形状、错误码与访问控制。

use std::sync::{Arc, Mutex};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use makuhime_core::config::AppConfig;
use makuhime_core::engine::Engine;
use makuhime_server::{AppState, router};
use tower::ServiceExt;

/// 测试用访问令牌（长度要过 auth 的最小长度检查其实无所谓，这里是我们自己塞进去的）。
const TOKEN: &str = "test-token-0123456789abcdef";

fn state_of(engine: Engine, web_dir: Option<std::path::PathBuf>) -> Arc<AppState> {
    Arc::new(AppState {
        engine,
        web_dir,
        local_only: true,
        port: 3939,
        token: TOKEN.to_string(),
        tts_test_gate: Mutex::new(None),
    })
}

async fn test_app() -> axum::Router {
    app_with_config(AppConfig::default(), None).await
}

async fn app_with_config(
    mut config: AppConfig,
    web_dir: Option<std::path::PathBuf>,
) -> axum::Router {
    config.rooms.clear();
    let engine = Engine::start_with_credentials(config, None)
        .await
        .expect("引擎启动失败");
    router(state_of(engine, web_dir))
}

/// 带令牌的请求（本机来源）。
fn request(method: &str, uri: &str, host: &str, origin: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", host)
        .header("x-maku-token", TOKEN);
    if let Some(origin) = origin {
        builder = builder.header("origin", origin);
    }
    builder.body(Body::empty()).unwrap()
}

/// 不带令牌的请求。
fn anon(method: &str, uri: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:3939")
        .body(Body::empty())
        .unwrap()
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
async fn api_without_token_is_rejected() {
    // 同机其它进程 / 没有令牌的页面一律 401
    for uri in ["/api/state", "/api/config", "/api/history/1", "/api/ws"] {
        let response = test_app().await.oneshot(anon("GET", uri)).await.unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{uri} 应该 401"
        );
        let body = to_bytes(response.into_body(), 8192).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["error"]["code"], "unauthorized");
    }
}

#[tokio::test]
async fn token_can_come_from_query_for_websocket() {
    // WebSocket 握手带不了自定义头，只能用 ?token=
    let response = test_app()
        .await
        .oneshot(
            Request::builder()
                .uri(format!("/api/state?token={TOKEN}"))
                .header("host", "127.0.0.1:3939")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn state_endpoint_returns_snapshot_with_config() {
    let response = test_app()
        .await
        .oneshot(request("GET", "/api/state", "127.0.0.1:3939", None))
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
async fn config_never_returns_the_mimo_api_key() {
    // /api/state 会广播给所有 WebSocket 客户端，/api/config 会落到浏览器里：
    // 两处都不能出现明文 API Key
    let mut config = AppConfig::default();
    config.tts.mimo.api_key = "sk-secret-value".into();
    let app = app_with_config(config, None).await;

    let response = app
        .clone()
        .oneshot(request("GET", "/api/config", "127.0.0.1:3939", None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    let text = String::from_utf8_lossy(&body);
    assert!(
        !text.contains("sk-secret-value"),
        "响应里泄露了 API Key: {text}"
    );
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["tts"]["mimo"]["api_key"], "");
    assert_eq!(value["tts"]["mimo"]["api_key_set"], true);

    let response = app
        .oneshot(request("GET", "/api/state", "127.0.0.1:3939", None))
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    let text = String::from_utf8_lossy(&body);
    assert!(
        !text.contains("sk-secret-value"),
        "快照里泄露了 API Key: {text}"
    );
}

#[tokio::test]
async fn sending_without_login_returns_401() {
    let response = test_app()
        .await
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/send")
                .header("host", "127.0.0.1:3939")
                .header("x-maku-token", TOKEN)
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
    // 任意网页连本机接口（含 WebSocket 握手）必须被拒，而且是在令牌校验之前被拒
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
    for host in ["evil.example:3939", "127.evil.example:3939"] {
        let response = test_app()
            .await
            .oneshot(request(
                "GET",
                "/api/state",
                host,
                Some("http://evil.example"),
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{host} 应该 403");
    }
}

#[tokio::test]
async fn typo_in_api_path_returns_json_404_not_html() {
    // 有前端构建产物时，静态 fallback 不能把 /api/xxx 变成 index.html
    let dir = std::env::temp_dir().join("makuhime-spa-test");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), "<html><head></head>panel</html>").unwrap();

    let mut config = AppConfig::default();
    config.rooms.clear();
    let engine = Engine::start_with_credentials(config, None).await.unwrap();
    let response = router(state_of(engine, Some(dir)))
        .oneshot(request("GET", "/api/typo", "127.0.0.1:3939", None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = to_bytes(response.into_body(), 8192).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["error"]["code"], "not_found");
}

#[tokio::test]
async fn index_page_carries_token_and_session_cookie() {
    let dir = std::env::temp_dir().join("makuhime-index-test");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("index.html"), "<html><head></head>panel</html>").unwrap();

    let mut config = AppConfig::default();
    config.rooms.clear();
    let engine = Engine::start_with_credentials(config, None).await.unwrap();
    let response = router(state_of(engine, Some(dir)))
        .oneshot(request("GET", "/", "127.0.0.1:3939", None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response
        .headers()
        .get("set-cookie")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(
        cookie.starts_with("maku_token="),
        "缺少会话 Cookie: {cookie}"
    );
    assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"));
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .and_then(|v| v.to_str().ok()),
        Some("no-store")
    );
    let body = to_bytes(response.into_body(), 65536).await.unwrap();
    let text = String::from_utf8_lossy(&body);
    assert!(
        text.contains("__MAKU_HIME_PORT__"),
        "页面里应注入端口: {text}"
    );
    assert!(
        text.contains("__MAKU_HIME_TOKEN__"),
        "页面里应注入令牌: {text}"
    );
}

#[tokio::test]
async fn unknown_api_route_returns_404() {
    let response = test_app()
        .await
        .oneshot(request(
            "GET",
            "/api/does-not-exist",
            "127.0.0.1:3939",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn history_of_unknown_room_is_empty() {
    let response = test_app()
        .await
        .oneshot(request(
            "GET",
            "/api/history/123456?limit=10",
            "127.0.0.1:3939",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 8192).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(value, serde_json::json!([]));
}

#[tokio::test]
async fn tts_test_rejects_empty_and_oversized_text() {
    let app = test_app().await;
    for text in ["", "   "] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/tts/test")
                    .header("host", "127.0.0.1:3939")
                    .header("x-maku-token", TOKEN)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::json!({ "text": text }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    let long = "字".repeat(201);
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/tts/test")
                .header("host", "127.0.0.1:3939")
                .header("x-maku-token", TOKEN)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::json!({ "text": long }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
