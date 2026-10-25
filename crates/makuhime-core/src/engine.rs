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
    // 事件本身较大（带昵称/头像/勋章），装箱避免整个枚举按最大变体分配
    Event(Box<Event>),
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
    /// 刚发出、等待 B 站回推的弹幕文本，用于去掉重复回显
    pending_echoes: std::sync::Mutex<HashMap<u64, VecDeque<(String, i64)>>>,
    /// 本地回显 id 的自增序号
    echo_seq: std::sync::atomic::AtomicU64,
    /// 串行化「改配置 + 落盘」，避免多个入口互相覆盖
    config_io: tokio::sync::Mutex<()>,
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
                pending_echoes: std::sync::Mutex::new(HashMap::new()),
                echo_seq: std::sync::atomic::AtomicU64::new(1),
                config_io: tokio::sync::Mutex::new(()),
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

        // 检查与插入必须在同一把写锁里，否则并发调用会各 spawn 一套任务
        let state = {
            let mut rooms = self.inner.rooms.write().await;
            if let Some(existing) = rooms.get(&room_id) {
                return Ok(existing.clone());
            }
            let state = RoomState::new(info);
            rooms.insert(room_id, state.clone());
            state
        };

        self.spawn_room_tasks(room_id).await;
        self.mutate_config(|cfg| {
            if !cfg.rooms.contains(&room_id) {
                cfg.rooms.push(room_id);
            }
        })
        .await;
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
        // 退订后把房间相关的内存状态一并清掉，避免反复加退房间时线性增长
        self.inner.seen.lock().await.remove(&room_id);
        self.inner.last_info_refresh.lock().await.remove(&room_id);
        self.inner.pending_echoes.lock().unwrap().remove(&room_id);
        self.inner.history.forget_room(room_id);

        if existed {
            self.mutate_config(|cfg| cfg.rooms.retain(|id| *id != room_id))
                .await;
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
                return Err(Error::NotFound("该房间未被订阅".into()));
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
        // 期间可能被 remove_room 删掉，这里不再 expect
        self.room_state(room_id)
            .await
            .ok_or_else(|| Error::NotFound("该房间已被取消订阅".into()))
    }

    pub async fn room_state(&self, room_id: u64) -> Option<RoomState> {
        self.inner.rooms.read().await.get(&room_id).cloned()
    }

    async fn spawn_room_tasks(&self, room_id: u64) {
        let cancel = CancellationToken::new();
        {
            let mut cancels = self.inner.cancels.lock().await;
            // 若已有旧任务（重复订阅/竞态），先取消，避免同一房间跑两套连接
            if let Some(previous) = cancels.insert(room_id, cancel.clone()) {
                previous.cancel();
            }
        }

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

    /// 收到一条外部事件：先做「自己发的弹幕回推」去重，再走正常发布流程。
    async fn handle_event(&self, event: Event) {
        if self.is_echo_of_own_message(&event).await {
            return;
        }
        self.publish_event(event).await;
    }

    /// B 站会把我们刚发的弹幕再推回来一条；本地已经回显过，这条要丢掉，
    /// 否则墙上会出现两条一模一样的。
    async fn is_echo_of_own_message(&self, event: &Event) -> bool {
        let LiveEvent::Chat { user, text, .. } = &event.body else {
            return false;
        };
        let our_uid = self.inner.login.read().await.uid;
        if our_uid == 0 || user.uid != our_uid {
            return false;
        }
        let mut pending = self.inner.pending_echoes.lock().unwrap();
        let Some(queue) = pending.get_mut(&event.room_id) else {
            return false;
        };
        take_pending_echo(queue, text, now_millis())
    }

    async fn publish_event(&self, event: Event) {
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
                LiveEvent::RoomUpdate {
                    fans: Some(fans), ..
                } => state.info.attention = *fans,
                _ => {}
            }
        })
        .await;

        if self.inner.config.read().await.danmaku.keep_history {
            self.inner.history.append(&event);
        }
        self.maybe_speak(&event).await;
        let _ = self.inner.events.send(ServerEvent::Event(Box::new(event)));

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
        if need_info && let Ok(info) = room::fetch_room_info(&self.inner.http, room_id).await {
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

        // 本地先回显一条；B 站随后推回的同一条由 is_echo_of_own_message 丢弃。
        // id 用自增序号，避免同一毫秒连发两条时撞 id 被 mark_seen 吞掉。
        let seq = self
            .inner
            .echo_seq
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.remember_echo(room_id, text.trim());
        let event = Event {
            id: format!("local-{}-{}", now_millis(), seq),
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
        self.publish_event(event).await;
        Ok(())
    }

    /// 记下「刚发出去、等待确认」的弹幕，供回推去重用。
    fn remember_echo(&self, room_id: u64, text: &str) {
        let mut pending = self.inner.pending_echoes.lock().unwrap();
        let queue = pending.entry(room_id).or_default();
        let now = now_millis();
        queue.retain(|(_, ts)| now - ts < ECHO_TTL_MS);
        queue.push_back((text.to_string(), now));
        if queue.len() > 16 {
            queue.pop_front();
        }
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
        // 别让 HTTP 客户端继续带着旧 SESSDATA 去请求 B 站
        self.inner.http.set_cookie("");
        *self.inner.credentials.write().await = None;
        *self.inner.login.write().await = LoginState::default();
        self.broadcast_snapshot().await;
        Ok(())
    }

    // ---------------------------------------------------------------- 配置

    pub async fn update_config(&self, config: AppConfig) -> Result<AppConfig> {
        let config = config.sanitized();
        let saved = self.mutate_config(|current| *current = config).await;
        self.broadcast_snapshot().await;
        Ok(saved)
    }

    /// 串行化「修改配置 + 落盘」。所有改配置的入口都走这里，
    /// 否则 add_room / remove_room / update_config 并发时会互相覆盖。
    async fn mutate_config(&self, mutate: impl FnOnce(&mut AppConfig)) -> AppConfig {
        let _io = self.inner.config_io.lock().await;
        let snapshot = {
            let mut config = self.inner.config.write().await;
            mutate(&mut config);
            config.clone()
        };
        if let Err(err) = snapshot.save() {
            tracing::warn!(%err, "保存配置失败");
        }
        snapshot
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

/// 本地回显与 B 站回推之间的匹配窗口
const ECHO_TTL_MS: i64 = 20_000;

/// 在待确认队列里找一条与 text 匹配的回显并消费掉。
fn take_pending_echo(queue: &mut VecDeque<(String, i64)>, text: &str, now: i64) -> bool {
    queue.retain(|(_, ts)| now - ts < ECHO_TTL_MS);
    let target = text.trim();
    if let Some(index) = queue.iter().position(|(item, _)| item == target) {
        queue.remove(index);
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_state_status_text_follows_live_status() {
        let live = RoomInfo {
            live_status: 1,
            ..Default::default()
        };
        assert_eq!(RoomState::new(live).status, "直播中");
        assert_eq!(RoomState::new(RoomInfo::default()).status, "未开播");
    }

    #[test]
    fn echo_queue_matches_exactly_once() {
        let mut queue = VecDeque::new();
        queue.push_back(("你好".to_string(), 1_000));
        assert!(take_pending_echo(&mut queue, " 你好 ", 1_100));
        assert!(queue.is_empty(), "匹配过的回显应被消费掉");
        assert!(!take_pending_echo(&mut queue, "你好", 1_200));
    }

    #[test]
    fn echo_queue_expires_old_entries() {
        let mut queue = VecDeque::new();
        queue.push_back(("过期了".to_string(), 1_000));
        assert!(!take_pending_echo(
            &mut queue,
            "过期了",
            1_000 + ECHO_TTL_MS + 1
        ));
        assert!(queue.is_empty(), "过期条目应被清理");
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
