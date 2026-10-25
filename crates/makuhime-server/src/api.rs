//! REST + WebSocket 接口。
//!
//! 约定：成功返回 2xx + JSON；失败返回 {"error": {"code": "...", "message": "..."}}。

use std::convert::Infallible;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use makuhime_core::Error;
use makuhime_core::engine::{RoomState, ServerEvent, Snapshot};
use makuhime_core::history::ExportFormat;
use makuhime_core::login::{QrPoll, QrSession};
use makuhime_core::model::Event;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::SharedState;

pub fn routes() -> Router<SharedState> {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/state", get(state))
        .route("/api/ws", get(ws))
        .route("/api/rooms", post(add_room))
        .route("/api/rooms/{id}", delete(remove_room).get(get_room))
        .route("/api/rooms/{id}/refresh", post(refresh_room))
        .route("/api/send", post(send_danmaku))
        .route("/api/history/{id}", get(history).delete(clear_history))
        .route("/api/history/{id}/export", get(export_history))
        .route("/api/login/qrcode", get(qr_code))
        .route("/api/login/qrcode/poll", get(qr_poll))
        .route("/api/login/logout", post(logout))
        .route("/api/config", get(get_config).put(put_config))
        .route("/api/tts/test", post(tts_test))
}

// ---------------------------------------------------------------- 错误

pub struct ApiError {
    status: StatusCode,
    code: String,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, code: &str, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({ "error": { "code": self.code, "message": self.message } })),
        )
            .into_response()
    }
}

impl From<Error> for ApiError {
    fn from(err: Error) -> Self {
        let (status, code) = match &err {
            Error::NotLoggedIn => (StatusCode::UNAUTHORIZED, "not_logged_in"),
            Error::Send { .. } => (StatusCode::BAD_REQUEST, "send_failed"),
            Error::Api { .. } => (StatusCode::BAD_GATEWAY, "upstream_error"),
            Error::Http(_) => (StatusCode::BAD_GATEWAY, "network_error"),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        };
        ApiError::new(status, code, err.to_string())
    }
}

type ApiResult<T> = std::result::Result<Json<T>, ApiError>;

// ---------------------------------------------------------------- 基础

async fn health() -> Json<serde_json::Value> {
    Json(json!({
        "ok": true,
        "name": makuhime_core::APP_NAME,
        "version": makuhime_core::VERSION,
    }))
}

async fn state(State(app): State<SharedState>) -> Json<Snapshot> {
    Json(app.engine.snapshot().await)
}

// ---------------------------------------------------------------- WebSocket

async fn ws(ws: WebSocketUpgrade, State(app): State<SharedState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| ws_loop(socket, app))
}

async fn ws_loop(mut socket: WebSocket, app: SharedState) {
    let mut rx = app.engine.subscribe();
    let snapshot = app.engine.snapshot().await;
    if send_event(&mut socket, &ServerEvent::Snapshot(Box::new(snapshot)))
        .await
        .is_err()
    {
        return;
    }
    loop {
        tokio::select! {
            event = rx.recv() => {
                match event {
                    Ok(event) => {
                        if send_event(&mut socket, &event).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(skipped, "WebSocket 客户端消费过慢，跳过部分事件");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
        }
    }
}

async fn send_event(socket: &mut WebSocket, event: &ServerEvent) -> Result<(), axum::Error> {
    let text = match serde_json::to_string(event) {
        Ok(text) => text,
        Err(_) => return Ok(()),
    };
    socket.send(Message::Text(text.into())).await
}

// ---------------------------------------------------------------- 房间

async fn get_room(
    State(app): State<SharedState>,
    Path(id): Path<u64>,
) -> ApiResult<Option<RoomState>> {
    Ok(Json(app.engine.room_state(id).await))
}

async fn add_room(
    State(app): State<SharedState>,
    Json(body): Json<AddRoomRequest>,
) -> ApiResult<RoomState> {
    Ok(Json(app.engine.add_room(body.room_id).await?))
}

async fn remove_room(
    State(app): State<SharedState>,
    Path(id): Path<u64>,
) -> ApiResult<serde_json::Value> {
    let removed = app.engine.remove_room(id).await;
    Ok(Json(json!({ "removed": removed, "room_id": id })))
}

async fn refresh_room(State(app): State<SharedState>, Path(id): Path<u64>) -> ApiResult<RoomState> {
    Ok(Json(app.engine.refresh_room(id).await?))
}

#[derive(Debug, Deserialize)]
pub struct AddRoomRequest {
    pub room_id: u64,
}

// ---------------------------------------------------------------- 发送

#[derive(Debug, Deserialize)]
pub struct SendRequest {
    pub room_id: u64,
    pub text: String,
}

async fn send_danmaku(
    State(app): State<SharedState>,
    Json(body): Json<SendRequest>,
) -> ApiResult<serde_json::Value> {
    app.engine.send_danmaku(body.room_id, &body.text).await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------- 历史

#[derive(Debug, Deserialize)]
pub struct HistoryQuery {
    #[serde(default)]
    pub kinds: String,
    pub q: Option<String>,
    pub limit: Option<usize>,
}

async fn history(
    State(app): State<SharedState>,
    Path(id): Path<u64>,
    Query(query): Query<HistoryQuery>,
) -> ApiResult<Vec<Event>> {
    let kinds: Vec<String> = query
        .kinds
        .split(',')
        .map(str::trim)
        .filter(|kind| !kind.is_empty())
        .map(str::to_string)
        .collect();
    let limit = query.limit.unwrap_or(500).clamp(1, 5000);
    Ok(Json(
        app.engine
            .history(id, &kinds, query.q.as_deref(), limit)
            .await,
    ))
}

async fn clear_history(
    State(app): State<SharedState>,
    Path(id): Path<u64>,
) -> ApiResult<serde_json::Value> {
    app.engine.clear_history(id)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize)]
pub struct ExportQuery {
    pub format: Option<String>,
}

async fn export_history(
    State(app): State<SharedState>,
    Path(id): Path<u64>,
    Query(query): Query<ExportQuery>,
) -> Result<Response, ApiError> {
    let format = ExportFormat::parse(query.format.as_deref().unwrap_or("json"));
    let content = app.engine.export_history(id, format)?;
    let file_name = app.engine.export_file_name(id, format);
    let mut response = Response::new(content.into());
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_str(format.content_type()).unwrap(),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_str(&format!("attachment; filename=\"{file_name}\"")).unwrap(),
    );
    Ok(response)
}

// ---------------------------------------------------------------- 登录

async fn qr_code(State(app): State<SharedState>) -> ApiResult<QrSession> {
    Ok(Json(app.engine.start_qr_login().await?))
}

#[derive(Debug, Deserialize)]
pub struct QrPollQuery {
    pub key: String,
}

async fn qr_poll(
    State(app): State<SharedState>,
    Query(query): Query<QrPollQuery>,
) -> ApiResult<QrPoll> {
    Ok(Json(app.engine.poll_qr_login(&query.key).await?))
}

async fn logout(State(app): State<SharedState>) -> ApiResult<serde_json::Value> {
    app.engine.logout().await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------- 配置 / TTS

async fn get_config(State(app): State<SharedState>) -> Json<makuhime_core::config::AppConfig> {
    Json(app.engine.config().await)
}

async fn put_config(
    State(app): State<SharedState>,
    Json(body): Json<makuhime_core::config::AppConfig>,
) -> ApiResult<makuhime_core::config::AppConfig> {
    Ok(Json(app.engine.update_config(body).await?))
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TtsTestRequest {
    pub text: String,
}

async fn tts_test(
    State(app): State<SharedState>,
    Json(body): Json<TtsTestRequest>,
) -> ApiResult<serde_json::Value> {
    let backend = app.engine.speak_test(&body.text).await;
    Ok(Json(json!({ "ok": true, "backend": backend })))
}

/// 让 Infallible 的 handler 也能用 ? 的简写（保留给未来的扩展）。
#[allow(dead_code)]
fn infallible(error: Infallible) -> StatusCode {
    match error {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_error_maps_status_codes() {
        let err: ApiError = Error::NotLoggedIn.into();
        assert_eq!(err.status, StatusCode::UNAUTHORIZED);
        assert_eq!(err.code, "not_logged_in");

        let err: ApiError = Error::Send {
            code: 10031,
            message: "发送太频繁".into(),
        }
        .into();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert!(err.message.contains("频繁"));
    }
}
