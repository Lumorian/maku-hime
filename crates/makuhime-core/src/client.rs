//! 弹幕 WebSocket 客户端：连接、认证、心跳、解包、断线重连。
//!
//! 参考实现 blivedm-go 有几个已知缺陷（重连只睡 3ms、认证失败静默、没有心跳看门狗），
//! 这里都做了修正：
//! - 指数退避重连（1s → 30s）；
//! - 认证回复 code != 0 视为失败并重连；
//! - 超过心跳周期 2.5 倍没收到任何帧就判定半死连接，主动重连。

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::mpsc::Sender;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_util::sync::CancellationToken;

use crate::config::DanmakuConfig;
use crate::error::{Error, Result};
use crate::http::{HttpClient, UA};
use crate::model::Event;
use crate::packet::{self, OP_AUTH_REPLY, OP_HEARTBEAT_REPLY, OP_MESSAGE};
use crate::parser;
use crate::room;

/// 客户端向引擎上报的东西。
#[derive(Debug, Clone)]
pub enum ClientEvent {
    /// 已连上并完成认证
    Connected { host: String },
    /// 连接断开（引擎据此更新 UI 状态）
    Disconnected { reason: String },
    /// 认证被拒绝
    AuthRejected { code: i64 },
    /// 一条归一化事件
    Message(Box<Event>),
    /// 心跳回复带回来的人气值
    Popularity(u64),
    /// 不影响主流程的告警
    Warning(String),
}

/// 单房间弹幕客户端。
pub struct DanmakuClient {
    room_id: u64,
    http: HttpClient,
    heartbeat: Duration,
    max_backoff: Duration,
    auto_reconnect: bool,
}

impl DanmakuClient {
    pub fn new(room_id: u64, http: HttpClient, config: &DanmakuConfig) -> Self {
        Self {
            room_id,
            http,
            heartbeat: Duration::from_secs(config.heartbeat_seconds.max(5)),
            max_backoff: Duration::from_secs(config.reconnect_max_seconds.max(5)),
            auto_reconnect: config.auto_reconnect,
        }
    }

    /// 常驻运行：内部自己重连，直到 cancel 被触发。
    ///
    /// 事件通道是有界的：消费端跟不上时通过 `send().await` 反压到 WebSocket 读取侧，
    /// 宁可让 TCP 缓冲堆积，也不在内存里无限排队。
    pub async fn run(self, tx: Sender<ClientEvent>, cancel: CancellationToken) {
        let mut attempt: u32 = 0;
        loop {
            if cancel.is_cancelled() {
                break;
            }
            let started = std::time::Instant::now();
            let result = self.connect_and_stream(&tx, &cancel).await;
            if cancel.is_cancelled() {
                break;
            }
            let reason = match &result {
                Ok(()) => "连接已关闭".to_string(),
                Err(err) => err.to_string(),
            };
            // 连上过一段时间才断开，重置退避，避免一次抖动就把退避拉满
            if started.elapsed() > Duration::from_secs(30) {
                attempt = 0;
            }
            if !emit(
                &tx,
                ClientEvent::Disconnected {
                    reason: reason.clone(),
                },
                &cancel,
            )
            .await
            {
                break;
            }
            if !self.auto_reconnect {
                break;
            }
            attempt = attempt.saturating_add(1);
            let backoff = backoff_for(attempt, self.max_backoff);
            tracing::warn!(
                room_id = self.room_id,
                attempt,
                ?backoff,
                reason,
                "弹幕连接断开，准备重连"
            );
            tokio::select! {
                _ = tokio::time::sleep(backoff) => {}
                _ = cancel.cancelled() => break,
            }
        }
    }

    async fn connect_and_stream(
        &self,
        tx: &Sender<ClientEvent>,
        cancel: &CancellationToken,
    ) -> Result<()> {
        let info = room::fetch_danmu_info(&self.http, self.room_id).await?;
        if info.token.is_empty() {
            emit(
                tx,
                ClientEvent::Warning(
                    "未取得进房 token，使用默认弹幕服务器（可能收不到部分消息）".into(),
                ),
                cancel,
            )
            .await;
        }
        let buvid = self
            .http
            .buvid3()
            .await
            .or_else(|| cookie_value(&self.http.cookie(), "buvid3"))
            .unwrap_or_default();

        let host = info
            .hosts
            .first()
            .cloned()
            .unwrap_or_else(|| room::DEFAULT_DANMU_HOST.to_string());
        let url = format!("wss://{host}/sub");

        let mut request = url
            .clone()
            .into_client_request()
            .map_err(|e| Error::other(format!("构造 WebSocket 请求失败: {e}")))?;
        let headers = request.headers_mut();
        headers.insert("User-Agent", UA.parse().unwrap());
        headers.insert("Origin", "https://live.bilibili.com".parse().unwrap());
        headers.insert(
            "Referer",
            format!("https://live.bilibili.com/{}", self.room_id)
                .parse()
                .unwrap(),
        );

        let (mut ws, _response) = tokio::select! {
            result = tokio_tungstenite::connect_async(request) => result?,
            _ = cancel.cancelled() => return Ok(()),
        };

        let auth = packet::auth(0, self.room_id, &buvid, &info.token)?;
        ws.send(Message::binary(auth)).await?;

        if !emit(tx, ClientEvent::Connected { host: host.clone() }, cancel).await {
            return Ok(());
        }
        tracing::info!(room_id = self.room_id, host, "弹幕连接已建立");

        let mut heartbeat = tokio::time::interval(self.heartbeat);
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let watchdog = self.heartbeat.mul_f32(2.5).max(Duration::from_secs(45));
        let mut last_frame = std::time::Instant::now();

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    let _ = ws.close(None).await;
                    return Ok(());
                }
                _ = heartbeat.tick() => {
                    if let Err(err) = ws.send(Message::binary(packet::heartbeat())).await {
                        return Err(Error::other(format!("心跳发送失败: {err}")));
                    }
                }
                _ = tokio::time::sleep(Duration::from_secs(5)) => {
                    if last_frame.elapsed() > watchdog {
                        return Err(Error::other(format!(
                            "{} 秒没有收到任何数据，判定连接已失效",
                            last_frame.elapsed().as_secs()
                        )));
                    }
                }
                message = ws.next() => {
                    let Some(message) = message else {
                        return Ok(());
                    };
                    let message = message?;
                    match message {
                        Message::Binary(bytes) => {
                            last_frame = std::time::Instant::now();
                            for frame in packet::decode(&bytes)? {
                                match frame.op {
                                    OP_HEARTBEAT_REPLY => {
                                        if let Some(popularity) = packet::parse_popularity(&frame) {
                                            emit(tx, ClientEvent::Popularity(popularity), cancel).await;
                                        }
                                    }
                                    OP_AUTH_REPLY => {
                                        let code = frame
                                            .json::<Value>()
                                            .ok()
                                            .and_then(|v| v.get("code").and_then(Value::as_i64))
                                            .unwrap_or(-1);
                                        if code != 0 {
                                            emit(tx, ClientEvent::AuthRejected { code }, cancel).await;
                                            return Err(Error::other(format!("认证被拒绝，code={code}")));
                                        }
                                        tracing::debug!(room_id = self.room_id, "进房认证通过");
                                    }
                                    OP_MESSAGE => {
                                        let Ok(value) = frame.json::<Value>() else { continue };
                                        let now = now_millis();
                                        if let Some(event) = parser::parse_event(&value, self.room_id, now)
                                            && !emit(tx, ClientEvent::Message(Box::new(event)), cancel)
                                                .await
                                        {
                                            return Ok(());
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Message::Close(frame) => {
                            let reason = frame
                                .map(|f| format!("{} {}", f.code, f.reason))
                                .unwrap_or_else(|| "服务端关闭连接".into());
                            return Err(Error::other(reason));
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

/// 发送事件；通道关闭或收到取消信号时返回 false。
async fn emit(tx: &Sender<ClientEvent>, event: ClientEvent, cancel: &CancellationToken) -> bool {
    tokio::select! {
        _ = cancel.cancelled() => false,
        result = tx.send(event) => result.is_ok(),
    }
}

fn backoff_for(attempt: u32, max: Duration) -> Duration {
    let seconds = 1u64 << attempt.min(5);
    Duration::from_secs(seconds.min(max.as_secs().max(1)))
}

fn cookie_value(cookie: &str, name: &str) -> Option<String> {
    cookie.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        if key.trim() == name {
            Some(value.trim().to_string())
        } else {
            None
        }
    })
}

pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_then_caps() {
        assert_eq!(
            backoff_for(1, Duration::from_secs(30)),
            Duration::from_secs(2)
        );
        assert_eq!(
            backoff_for(2, Duration::from_secs(30)),
            Duration::from_secs(4)
        );
        assert_eq!(
            backoff_for(9, Duration::from_secs(30)),
            Duration::from_secs(30)
        );
    }

    #[test]
    fn reads_cookie_values() {
        let cookie = "SESSDATA=abc; bili_jct=def; DedeUserID=42";
        assert_eq!(cookie_value(cookie, "bili_jct").as_deref(), Some("def"));
        assert_eq!(cookie_value(cookie, "missing"), None);
    }
}
