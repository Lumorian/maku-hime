//! REST + WebSocket 接口。
//!
//! 约定：成功返回 2xx + JSON；失败返回 {"error": {"code": "...", "message": "..."}}。

use std::convert::Infallible;
use std::time::{Duration, Instant};

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
        .route("/api/live/start", post(start_live))
        .route("/api/live/stop", post(stop_live))
        .route(
            "/api/rooms/{id}/settings",
            get(room_settings).post(update_room_settings),
        )
        .route("/api/rooms/{id}/areas", get(room_areas))
        .route("/api/rooms/{id}/cover", post(upload_room_cover))
        .route("/api/rooms/{id}/emoticons", get(room_emoticons))
        .route("/api/send/emoticon", post(send_emoticon))
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
    /// B 站自己的错误码（-352、60034 之类）；本服务自身产生的错误为 None。
    /// 前端靠它区分具体失败原因，不必去匹配中文文案。
    upstream: Option<i64>,
}

impl ApiError {
    fn new(status: StatusCode, code: &str, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
            upstream: None,
        }
    }

    fn with_upstream(mut self, upstream: Option<i64>) -> Self {
        self.upstream = upstream;
        self
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "error": {
                    "code": self.code,
                    "message": self.message,
                    "upstream": self.upstream,
                }
            })),
        )
            .into_response()
    }
}

impl From<Error> for ApiError {
    fn from(err: Error) -> Self {
        let (status, code) = match &err {
            Error::NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
            Error::NotLoggedIn => (StatusCode::UNAUTHORIZED, "not_logged_in"),
            Error::Send { .. } => (StatusCode::BAD_REQUEST, "send_failed"),
            Error::Live { .. } => (StatusCode::BAD_REQUEST, "live_failed"),
            Error::Manage { .. } => (StatusCode::BAD_REQUEST, "manage_failed"),
            Error::Api { .. } => (StatusCode::BAD_GATEWAY, "upstream_error"),
            Error::Http(_) => (StatusCode::BAD_GATEWAY, "network_error"),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        };
        let upstream = match &err {
            Error::Live { code, .. }
            | Error::Manage { code, .. }
            | Error::Send { code, .. }
            | Error::Api { code, .. } => Some(*code),
            _ => None,
        };
        ApiError::new(status, code, err.to_string()).with_upstream(upstream)
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
                        // 丢了事件就补一份全量快照，否则前端数字会永久偏移
                        tracing::warn!(skipped, "WebSocket 客户端消费过慢，补发快照");
                        let snapshot = app.engine.snapshot().await;
                        if send_event(&mut socket, &ServerEvent::Snapshot(Box::new(snapshot)))
                            .await
                            .is_err()
                        {
                            break;
                        }
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

async fn get_room(State(app): State<SharedState>, Path(id): Path<u64>) -> ApiResult<RoomState> {
    app.engine.room_state(id).await.map(Json).ok_or_else(|| {
        ApiError::new(
            StatusCode::NOT_FOUND,
            "not_found",
            format!("房间 {id} 未被订阅"),
        )
    })
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

// ---------------------------------------------------------------- 开播 / 停播

#[derive(Debug, Deserialize)]
pub struct LiveRequest {
    pub room_id: u64,
}

async fn start_live(
    State(app): State<SharedState>,
    Json(body): Json<LiveRequest>,
) -> ApiResult<makuhime_core::live::StartOutcome> {
    Ok(Json(app.engine.start_live(body.room_id).await?))
}

async fn stop_live(
    State(app): State<SharedState>,
    Json(body): Json<LiveRequest>,
) -> ApiResult<serde_json::Value> {
    app.engine.stop_live(body.room_id).await?;
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------- 直播间设置

#[derive(Debug, Deserialize)]
pub struct RoomSettingsRequest {
    /// 只提交改过的字段；缺省表示不动
    pub title: Option<String>,
    pub area_id: Option<u64>,
    pub announcement: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CoverRequest {
    /// base64 或 data URL
    pub data: Option<String>,
    /// 图片链接（与 data 二选一）
    pub url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SendEmoticonRequest {
    pub room_id: u64,
    pub unique_name: String,
    pub text: String,
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub url: String,
    /// 表情原始尺寸，只用于本地回显，不影响发送
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
}

async fn room_settings(
    State(app): State<SharedState>,
    Path(id): Path<u64>,
) -> ApiResult<makuhime_core::manage::RoomSettings> {
    Ok(Json(app.engine.room_settings(id).await?))
}

async fn update_room_settings(
    State(app): State<SharedState>,
    Path(id): Path<u64>,
    Json(body): Json<RoomSettingsRequest>,
) -> ApiResult<makuhime_core::manage::RoomSettings> {
    Ok(Json(
        app.engine
            .update_room_settings(id, body.title, body.area_id, body.announcement)
            .await?,
    ))
}

async fn room_areas(
    State(app): State<SharedState>,
    Path(id): Path<u64>,
) -> ApiResult<makuhime_core::manage::AreaOptions> {
    Ok(Json(app.engine.room_areas(id).await?))
}

async fn upload_room_cover(
    State(app): State<SharedState>,
    Path(_id): Path<u64>,
    Json(body): Json<CoverRequest>,
) -> ApiResult<makuhime_core::manage::CoverOutcome> {
    let data = body
        .data
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let url = body.url.as_deref().map(str::trim).filter(|v| !v.is_empty());
    let outcome = match (data, url) {
        (Some(data), _) => app.engine.upload_room_cover_data(data).await?,
        (None, Some(url)) => app.engine.upload_room_cover_url(url).await?,
        (None, None) => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "bad_request",
                "请提供封面图片数据或图片地址",
            ));
        }
    };
    Ok(Json(outcome))
}

// ---------------------------------------------------------------- 表情

async fn room_emoticons(
    State(app): State<SharedState>,
    Path(id): Path<u64>,
) -> ApiResult<Vec<makuhime_core::emoticon::EmoticonPackage>> {
    Ok(Json(app.engine.emoticons(id).await?))
}

async fn send_emoticon(
    State(app): State<SharedState>,
    Json(body): Json<SendEmoticonRequest>,
) -> ApiResult<serde_json::Value> {
    let text = body.text.trim();
    // 只有「既没有文本、也没有表情标识」才是真的没内容可发；
    // 大表情本身就是靠标识发的，文本为空很正常。
    if text.is_empty() && body.unique_name.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "表情缺少可发送的内容",
        ));
    }
    app.engine
        .send_emoticon(
            body.room_id,
            body.unique_name.trim(),
            text,
            body.id,
            body.url.trim(),
            body.width,
            body.height,
        )
        .await?;
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
    // clear 会等写线程关闭文件句柄，属于阻塞操作，放到阻塞线程池
    let engine = app.engine.clone();
    tokio::task::spawn_blocking(move || engine.clear_history(id))
        .await
        .map_err(|err| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "clear_failed",
                err.to_string(),
            )
        })??;
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
    // 导出要读整份 JSONL 并拼字符串，属于阻塞 IO，放到阻塞线程池里做
    let engine = app.engine.clone();
    let content = tokio::task::spawn_blocking(move || engine.export_history(id, format))
        .await
        .map_err(|err| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "export_failed",
                err.to_string(),
            )
        })??;
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

/// 读配置。**必须**走 to_public_value()：原始配置里有 TTS 的 API Key，
/// 而这份响应会落到浏览器调试面板、扩展、以及任何能嗅到回环流量的东西手里。
async fn get_config(State(app): State<SharedState>) -> ApiResult<serde_json::Value> {
    Ok(Json(app.engine.config().await.to_public_value()))
}

/// 写配置。返回的同样是脱敏视图，前端靠 api_key_set 判断有没有存过 Key。
///
/// api_key 传空字符串表示「不修改」（设置页的密码框是空的，不能因此把 Key 抹掉）。
async fn put_config(
    State(app): State<SharedState>,
    Json(body): Json<makuhime_core::config::AppConfig>,
) -> ApiResult<serde_json::Value> {
    let updated = app.engine.update_config(body).await?;
    Ok(Json(updated.to_public_value()))
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TtsTestRequest {
    pub text: String,
}

/// 两次试听的最小间隔。合成要调付费接口（MiMo 按量计费），
/// 没有这个闸门的话一个失控的页面/脚本能在几秒内把额度烧光。
const TTS_TEST_INTERVAL: Duration = Duration::from_secs(2);
/// 试听文本上限：再长也不是「试听」了。
const TTS_TEST_MAX_CHARS: usize = 200;

async fn tts_test(
    State(app): State<SharedState>,
    Json(body): Json<TtsTestRequest>,
) -> ApiResult<serde_json::Value> {
    let text = body.text.trim();
    if text.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "tts_empty_text",
            "试听内容不能为空",
        ));
    }
    if text.chars().count() > TTS_TEST_MAX_CHARS {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "tts_text_too_long",
            format!("试听内容最多 {TTS_TEST_MAX_CHARS} 个字"),
        ));
    }
    {
        // 只做一次 compare-and-set，锁内没有 await
        let mut gate = app
            .tts_test_gate
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(last) = *gate {
            let elapsed = last.elapsed();
            if elapsed < TTS_TEST_INTERVAL {
                let wait = TTS_TEST_INTERVAL - elapsed;
                return Err(ApiError::new(
                    StatusCode::TOO_MANY_REQUESTS,
                    "tts_too_frequent",
                    format!("试听太频繁，请 {:.0} 秒后再试", wait.as_secs_f64().ceil()),
                ));
            }
        }
        *gate = Some(Instant::now());
    }
    let backend = app
        .engine
        .speak_test(text)
        .await
        .map_err(|message| ApiError::new(StatusCode::BAD_REQUEST, "tts_failed", message))?;
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
