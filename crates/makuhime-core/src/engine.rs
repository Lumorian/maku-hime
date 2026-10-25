//! 引擎：把「房间订阅 + 弹幕客户端 + 房间状态 + 历史 + TTS + 登录 + 发送」
//! 组装成一个可被 HTTP/WS 面板、Tauri 壳、OBS 页面共同消费的对象。
//!
//! Engine 是 Clone 的（内部 Arc），可以随意 clone 进后台任务。

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::sync::{Mutex, RwLock, broadcast, mpsc};
use tokio_util::sync::CancellationToken;

use crate::client::{ClientEvent, DanmakuClient, now_millis};
use crate::config::{self, AppConfig};
use crate::error::{Error, Result};
use crate::history::{ExportFormat, HistoryStore};
use crate::http::HttpClient;
use crate::login::{self, Profile, QrPoll, QrSession};
use crate::model::{Event, LiveEvent, User};
use crate::room::{self, RoomInfo};
use crate::sender::{self, Credentials};
use crate::tts::{self, TtsHandle, TtsJob};

/// 一个房间的实时状态。
#[derive(Debug, Clone, Serialize)]
pub struct RoomState {
    #[serde(flatten)]
    pub info: RoomInfo,
    /// 高能榜在线人数
    pub online: Option<u64>,
    /// 官方缩写的在线人数文本（如 13万+）
    pub online_text: String,
    /// 弹幕 WebSocket 是否已连上
    pub connected: bool,
    /// 状态说明（未开播 / 连接中断原因等）
    pub status: String,
    pub message_count: u64,
    pub last_event_at: Option<i64>,
    pub error: Option<String>,
}

impl RoomState {
    fn new(info: RoomInfo) -> Self {
        let status = info.status_text().to_string();
        Self {
            info,
            online: None,
            online_text: String::new(),
            connected: false,
            status,
            message_count: 0,
            last_event_at: None,
            error: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct LoginState {
    pub logged_in: bool,
    pub uid: u64,
    pub uname: String,
    pub face: String,
}

/// 给前端的全量状态。
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub version: String,
    pub server_url: String,
    pub started_at: i64,
    pub rooms: Vec<RoomState>,
    pub login: LoginState,
    pub config: AppConfig,
    pub tts_backend: String,
    pub tts_dropped: u64,
    pub history_dir: String,
}

/// 广播给所有前端的消息。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ServerEvent {
    Event(Event),
    Snapshot(Box<Snapshot>),
    Speak(TtsJob),
    Notice { level: String, message: String },
}

struct SeenIds {
    set: HashSet<String>,
    order: VecDeque<String>,
}

pub struct Engine {
    inner: Arc<Inner>,
}

struct Inner {
    http: HttpClient,
    config: Arc<RwLock<AppConfig>>,
    rooms: RwLock<BTreeMap<u64, RoomState>>,
    cancels: Mutex<HashMap<u64, CancellationToken>>,
    events: broadcast::Sender<ServerEvent>,
    history: Arc<HistoryStore>,
    tts: TtsHandle,
    login: RwLock<LoginState>,
    credentials: RwLock<Option<Credentials>>,
    seen: Mutex<HashMap<u64, SeenIds>>,
    last_info_refresh: Mutex<HashMap<u64, i64>>,
    started_at: i64,
    server_url: std::sync::Mutex<String>,
}

impl Clone for Engine {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl Engine {
    /// 载入配置、启动 TTS 与自动订阅。
    pub async fn start(config: AppConfig) -> Result<Self> {
        let config = config.sanitized();
        let http = HttpClient::new()?;

        let credentials = Credentials::load();
        let mut login = LoginState::default();
        if let Some(creds) = &credentials {
            http.set_cookie(creds.cookie());
            login = LoginState {
                logged_in: true,
                uid: creds.dede_user_id.parse().unwrap_or(0),
                uname: creds.uname.clone(),
                face: creds.face.clone(),
            };
        }

        let history = Arc::new(HistoryStore::new(
            config::data_dir().join("history"),
            config.danmaku.history_limit,
        ));
        let config = Arc::new(RwLock::new(config));
        let (events, _) = broadcast::channel(2048);

        let (tts_out_tx, mut tts_out_rx) = mpsc::unbounded_channel::<TtsJob>();
        let tts = tts::spawn(config.clone(), tts_out_tx);
        let tts_events = events.clone();
        tokio::spawn(async move {
            while let Some(job) = tts_out_rx.recv().await {
                let _ = tts_events.send(ServerEvent::Speak(job));
            }
        });

        let engine = Self {
            inner: Arc::new(Inner {
                http,
                config,
                rooms: RwLock::new(BTreeMap::new()),
                cancels: Mutex::new(HashMap::new()),
                events,
                history,
                tts,
                login: RwLock::new(login),
                credentials: RwLock::new(credentials),
                seen: Mutex::new(HashMap::new()),
                last_info_refresh: Mutex::new(HashMap::new()),
                started_at: now_millis(),
                server_url: std::sync::Mutex::new(String::new()),
            }),
        };

        if let Err(err) = engine.refresh_login().await {
            tracing::warn!(%err, "刷新登录信息失败");
        }

        let auto_rooms = engine.inner.config.read().await.rooms.clone();
        for rid in auto_rooms {
            if let Err(err) = engine.add_room(rid).await {
                tracing::warn!(%err, rid, "自动订阅房间失败");
            }
        }
        Ok(engine)
    }

    pub fn set_server_url(&self, url: impl Into<String>) {
        *self.inner.server_url.lock().unwrap() = url.into();
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ServerEvent> {
        self.inner.events.subscribe()
    }

    pub async fn config(&self) -> AppConfig {
        self.inner.config.read().await.clone()
    }

    pub async fn snapshot(&self) -> Snapshot {
        let config = self.inner.config.read().await.clone();
        let rooms = {
            let rooms = self.inner.rooms.read().await;
            rooms.values().cloned().collect::<Vec<_>>()
        };
        let login = self.inner.login.read().await.clone();
        // 先取出来，别让 std 的 MutexGuard 活到后面的 await 上（否则 future 不是 Send）
        let server_url = {
            let guard = self.inner.server_url.lock().unwrap();
            guard.clone()
        };
        let tts_backend = self.inner.tts.backend().await;
        Snapshot {
            version: crate::VERSION.to_string(),
            server_url,
            started_at: self.inner.started_at,
            rooms,
            login,
            config,
            tts_backend,
            tts_dropped: self.inner.tts.dropped(),
            history_dir: self.inner.history.dir().display().to_string(),
        }
    }

    pub async fn broadcast_snapshot(&self) {
        let snapshot = self.snapshot().await;
        let _ = self
            .inner
            .events
            .send(ServerEvent::Snapshot(Box::new(snapshot)));
    }

    pub fn notice(&self, level: &str, message: impl Into<String>) {
        let _ = self.inner.events.send(ServerEvent::Notice {
            level: level.to_string(),
            message: message.into(),
        });
    }

    // ---------------------------------------------------------------- 房间

    pub async fn add_room(&self, rid: u64) -> Result<RoomState> {
        let info = room::fetch_room_info(&self.inner.http, rid).await?;
        let room_id = info.room_id;

        {
            let rooms = self.inner.rooms.read().await;
            if let Some(existing) = rooms.get(&room_id) {
                return Ok(existing.clone());
            }
        }

        let state = RoomState::new(info);
        self.inner
            .rooms
            .write()
            .await
            .insert(room_id, state.clone());
        self.spawn_room_tasks(room_id).await;
        {
            let mut cfg = self.inner.config.write().await;
            if !cfg.rooms.contains(&room_id) {
                cfg.rooms.push(room_id);
                if let Err(err) = cfg.save() {
                    tracing::warn!(%err, "保存订阅房间失败");
                }
            }
        }
        self.broadcast_snapshot().await;
        // 立刻拉一次在线人数，别等 30s
        self.refresh_stats(room_id).await;
        Ok(state)
    }

    pub async fn remove_room(&self, room_id: u64) -> bool {
        let existed = self.inner.rooms.write().await.remove(&room_id).is_some();
        if let Some(cancel) = self.inner.cancels.lock().await.remove(&room_id) {
            cancel.cancel();
        }
        {
            let mut cfg = self.inner.config.write().await;
            cfg.rooms.retain(|id| *id != room_id);
            if let Err(err) = cfg.save() {
                tracing::warn!(%err, "保存订阅房间失败");
            }
        }
        if existed {
            self.broadcast_snapshot().await;
        }
        existed
    }

    /// 主动刷新房间信息（标题 / 分区 / 开播状态 / 粉丝数）。
    pub async fn refresh_room(&self, room_id: u64) -> Result<RoomState> {
        let info = room::fetch_room_info(&self.inner.http, room_id).await?;
        {
            let mut rooms = self.inner.rooms.write().await;
            let Some(state) = rooms.get_mut(&room_id) else {
                return Err(Error::other("该房间未被订阅"));
            };
            state.info = info;
            state.status = state.info.status_text().to_string();
        }
        self.inner
            .last_info_refresh
            .lock()
            .await
            .insert(room_id, now_millis());
        self.refresh_stats(room_id).await;
        self.broadcast_snapshot().await;
        Ok(self.room_state(room_id).await.expect("刚插入"))
    }

    pub async fn room_state(&self, room_id: u64) -> Option<RoomState> {
        self.inner.rooms.read().await.get(&room_id).cloned()
    }

    async fn spawn_room_tasks(&self, room_id: u64) {
        let cancel = CancellationToken::new();
        self.inner
            .cancels
            .lock()
            .await
            .insert(room_id, cancel.clone());

        let danmaku_config = self.inner.config.read().await.danmaku.clone();
        let client = DanmakuClient::new(room_id, self.inner.http.clone(), &danmaku_config);
        let (tx, mut rx) = mpsc::unbounded_channel::<ClientEvent>();

        let client_cancel = cancel.clone();
        tokio::spawn(async move {
            client.run(tx, client_cancel).await;
        });

        let engine = self.clone();
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                engine.handle_client_event(room_id, event).await;
            }
        });

        let stats_engine = self.clone();
        let stats_cancel = cancel.clone();
        tokio::spawn(async move {
            stats_engine.stats_loop(room_id, stats_cancel).await;
        });
    }

    async fn handle_client_event(&self, room_id: u64, event: ClientEvent) {
        match event {
            ClientEvent::Connected { host } => {
                tracing::info!(room_id, host, "弹幕已连接");
                self.update_room(room_id, |state| {
                    state.connected = true;
                    state.error = None;
                    state.status = if state.info.is_live() {
                        "直播中".into()
                    } else {
                        "已连接（直播间未开播）".into()
                    };
                })
                .await;
                self.broadcast_snapshot().await;
            }
            ClientEvent::Disconnected { reason } => {
                self.update_room(room_id, |state| {
                    state.connected = false;
                    state.error = Some(reason.clone());
                    state.status = "连接中断，重连中".into();
                })
                .await;
                self.broadcast_snapshot().await;
            }
            ClientEvent::AuthRejected { code } => {
                tracing::warn!(room_id, code, "弹幕认证被拒绝");
                self.notice(
                    "warn",
                    format!("房间 {room_id} 认证失败（code={code}），将自动重试"),
                );
            }
            ClientEvent::Popularity(value) => {
                self.update_room(room_id, |state| {
                    state.info.popularity = value;
                })
                .await;
            }
            ClientEvent::Warning(message) => {
                tracing::warn!(room_id, message, "弹幕客户端告警");
            }
            ClientEvent::Message(event) => {
                self.handle_event(*event).await;
            }
        }
    }

    async fn update_room(&self, room_id: u64, f: impl FnOnce(&mut RoomState)) {
        let mut rooms = self.inner.rooms.write().await;
        if let Some(state) = rooms.get_mut(&room_id) {
            f(state);
        }
    }

    async fn handle_event(&self, event: Event) {
        let room_id = event.room_id;
        if !self.mark_seen(room_id, &event.id).await {
            return;
        }

        let live_status_change = match &event.body {
            LiveEvent::LiveStatus { live_status } => Some(*live_status),
            _ => None,
        };

        self.update_room(room_id, |state| {
            state.message_count += 1;
            state.last_event_at = Some(event.ts);
            match &event.body {
                LiveEvent::Popularity { value } => state.info.popularity = *value,
                LiveEvent::OnlineRank { count, text } => {
                    state.online = Some(*count);
                    state.online_text = text.clone();
                }
                LiveEvent::LiveStatus { live_status } => {
                    state.info.live_status = *live_status;
                    state.status = state.info.status_text().to_string();
                }
                LiveEvent::RoomUpdate { fans, .. } => {
                    if let Some(fans) = fans {
                        state.info.attention = *fans;
                    }
                }
                _ => {}
            }
        })
        .await;

        if self.inner.config.read().await.danmaku.keep_history {
            self.inner.history.append(&event);
        }
        self.maybe_speak(&event).await;
        let _ = self.inner.events.send(ServerEvent::Event(event));

        if live_status_change.is_some() {
            self.refresh_stats(room_id).await;
            self.broadcast_snapshot().await;
        }
    }

    async fn mark_seen(&self, room_id: u64, id: &str) -> bool {
        let mut seen = self.inner.seen.lock().await;
        let entry = seen.entry(room_id).or_insert_with(|| SeenIds {
            set: HashSet::new(),
            order: VecDeque::new(),
        });
        if !entry.set.insert(id.to_string()) {
            return false;
        }
        entry.order.push_back(id.to_string());
        while entry.order.len() > 4000 {
            if let Some(old) = entry.order.pop_front() {
                entry.set.remove(&old);
            }
        }
        true
    }

    async fn maybe_speak(&self, event: &Event) {
        let tts_config = self.inner.config.read().await.tts.clone();
        if !tts_config.enabled {
            return;
        }
        let allowed = match &event.body {
            LiveEvent::Chat { .. } => tts_config.read_chat,
            LiveEvent::Gift { .. } => tts_config.read_gift,
            LiveEvent::Guard { .. } => tts_config.read_guard,
            LiveEvent::SuperChat { price, .. } => {
                tts_config.read_super_chat && *price >= tts_config.min_super_chat_price
            }
            _ => false,
        };
        if !allowed {
            return;
        }
        let Some(text) = event.body.tts_text() else {
            return;
        };
        let text = tts::truncate(&tts::sanitize(&text), tts_config.max_chars);
        if !text.trim().is_empty() {
            self.inner.tts.speak(event.kind(), text);
        }
    }

    async fn stats_loop(&self, room_id: u64, cancel: CancellationToken) {
        let interval = self
            .inner
            .config
            .read()
            .await
            .danmaku
            .stats_interval_seconds
            .max(10);
        let mut ticker = tokio::time::interval(Duration::from_secs(interval));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = ticker.tick() => {}
            }
            if self.refresh_stats(room_id).await {
                self.broadcast_snapshot().await;
            }
        }
    }

    async fn refresh_stats(&self, room_id: u64) -> bool {
        let Some(uid) = self.room_state(room_id).await.map(|state| state.info.uid) else {
            return false;
        };
        let mut changed = false;

        if let Ok((count, text)) = room::fetch_online(&self.inner.http, room_id, uid).await {
            let mut rooms = self.inner.rooms.write().await;
            if let Some(state) = rooms.get_mut(&room_id) {
                if state.online != count || state.online_text != text {
                    changed = true;
                }
                state.online = count;
                state.online_text = text;
            }
        }

        let need_info = {
            let map = self.inner.last_info_refresh.lock().await;
            map.get(&room_id)
                .map(|ts| now_millis() - ts > 300_000)
                .unwrap_or(true)
        };
        if need_info {
            if let Ok(info) = room::fetch_room_info(&self.inner.http, room_id).await {
                let mut rooms = self.inner.rooms.write().await;
                if let Some(state) = rooms.get_mut(&room_id) {
                    if state.info != info {
                        changed = true;
                    }
                    state.info = info;
                    state.status = state.info.status_text().to_string();
                }
                drop(rooms);
                self.inner
                    .last_info_refresh
                    .lock()
                    .await
                    .insert(room_id, now_millis());
            }
        }
        changed
    }

    // ---------------------------------------------------------------- 发送

    pub async fn send_danmaku(&self, room_id: u64, text: &str) -> Result<()> {
        let credentials = self
            .inner
            .credentials
            .read()
            .await
            .clone()
            .filter(Credentials::is_valid)
            .ok_or(Error::NotLoggedIn)?;
        let send_config = self.inner.config.read().await.send.clone();
        sender::send_danmaku(&self.inner.http, &credentials, room_id, text, &send_config).await?;

        // 本地回显：B 站会把这条弹幕推回 WS，前端按 id 去重
        let event = Event {
            id: format!("local-{}", now_millis()),
            ts: now_millis(),
            room_id,
            body: LiveEvent::Chat {
                user: User {
                    uid: credentials.dede_user_id.parse().unwrap_or(0),
                    name: credentials.uname.clone(),
                    face: credentials.face.clone(),
                    ..Default::default()
                },
                text: text.trim().to_string(),
                emoticon: None,
            },
        };
        self.handle_event(event).await;
        Ok(())
    }

    // ---------------------------------------------------------------- 登录

    pub async fn login_state(&self) -> LoginState {
        self.inner.login.read().await.clone()
    }

    pub async fn start_qr_login(&self) -> Result<QrSession> {
        login::qr_generate(&self.inner.http).await
    }

    pub async fn poll_qr_login(&self, key: &str) -> Result<QrPoll> {
        let poll = login::qr_poll(&self.inner.http, key).await?;
        if poll.status == "success" {
            if let Err(err) = self.refresh_login().await {
                tracing::warn!(%err, "刷新登录信息失败");
            }
            self.broadcast_snapshot().await;
        }
        Ok(poll)
    }

    pub async fn refresh_login(&self) -> Result<()> {
        let credentials = self.inner.credentials.read().await.clone();
        let Some(mut credentials) = credentials else {
            return Ok(());
        };
        if !credentials.is_valid() {
            return Ok(());
        }
        self.inner.http.set_cookie(credentials.cookie());
        let profile: Profile = login::fetch_profile(&self.inner.http)
            .await
            .unwrap_or_default();
        if profile.uid > 0 {
            credentials.uname = profile.uname.clone();
            credentials.face = profile.face.clone();
            credentials.dede_user_id = profile.uid.to_string();
            *self.inner.login.write().await = LoginState {
                logged_in: true,
                uid: profile.uid,
                uname: profile.uname,
                face: profile.face,
            };
            *self.inner.credentials.write().await = Some(credentials);
        }
        Ok(())
    }

    pub async fn logout(&self) -> Result<()> {
        Credentials::clear()?;
        *self.inner.credentials.write().await = None;
        *self.inner.login.write().await = LoginState::default();
        self.broadcast_snapshot().await;
        Ok(())
    }

    // ---------------------------------------------------------------- 配置

    pub async fn update_config(&self, config: AppConfig) -> Result<AppConfig> {
        let config = config.sanitized();
        config.save()?;
        *self.inner.config.write().await = config.clone();
        self.broadcast_snapshot().await;
        Ok(config)
    }

    // ---------------------------------------------------------------- 历史

    pub async fn history(
        &self,
        room_id: u64,
        kinds: &[String],
        keyword: Option<&str>,
        limit: usize,
    ) -> Vec<Event> {
        self.inner.history.query(room_id, kinds, keyword, limit)
    }

    pub fn export_history(&self, room_id: u64, format: ExportFormat) -> Result<String> {
        self.inner.history.export(room_id, format)
    }

    pub fn export_file_name(&self, room_id: u64, format: ExportFormat) -> String {
        self.inner.history.export_file_name(room_id, format)
    }

    pub fn clear_history(&self, room_id: u64) -> Result<()> {
        self.inner.history.clear(room_id)
    }

    pub fn history_size(&self, room_id: u64) -> u64 {
        self.inner.history.file_size(room_id)
    }

    // ---------------------------------------------------------------- TTS

    pub async fn speak_test(&self, text: &str) -> String {
        let text = tts::truncate(&tts::sanitize(text), 100);
        self.inner.tts.speak("test", text);
        self.inner.tts.backend().await
    }

    pub fn tts_dropped(&self) -> u64 {
        self.inner.tts.dropped()
    }

    pub async fn shutdown(&self) {
        let cancels = self.inner.cancels.lock().await;
        for cancel in cancels.values() {
            cancel.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_state_status_text_follows_live_status() {
        let mut info = RoomInfo::default();
        info.live_status = 1;
        let state = RoomState::new(info.clone());
        assert_eq!(state.status, "直播中");
        info.live_status = 0;
        assert_eq!(RoomState::new(info).status, "未开播");
    }

    #[test]
    fn snapshot_serializes_to_flat_json() {
        let state = RoomState::new(RoomInfo {
            room_id: 12345,
            uname: "测试主播".into(),
            ..Default::default()
        });
        let text = serde_json::to_string(&state).unwrap();
        assert!(text.contains("\"room_id\":12345"));
        assert!(text.contains("\"online\":null"));
    }
}
