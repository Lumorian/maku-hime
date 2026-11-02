//! 应用配置：XDG 配置目录下的 config.toml，原子写入，读坏回退默认值。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// 主题。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

/// TTS 后端。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TtsProvider {
    /// 自动探测系统里的 edge-tts / piper / espeak-ng / spd-say
    #[default]
    Auto,
    /// 交给浏览器 / WebView 的 speechSynthesis
    Webview,
    /// 调用配置里的命令行程序
    Command,
    /// 小米 MiMo TTS：请求云端生成 WAV，再用本机播放器播放
    Mimo,
    Off,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    /// 桌面端启动后是否自动打开浏览器面板
    pub open_browser: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 3939,
            open_browser: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub theme: Theme,
    pub font_size: u16,
    pub opacity: f32,
    /// 弹幕墙保留的最大条数（也是页面里的历史深度）
    pub max_rows: usize,
    pub show_avatar: bool,
    pub show_enter: bool,
    pub show_like: bool,
    /// 是否在墙上显示礼物 / 上舰
    pub show_gift: bool,
    /// 紧凑模式：行距更小
    pub compact: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            font_size: 13,
            opacity: 1.0,
            max_rows: 300,
            show_avatar: true,
            show_enter: false,
            show_like: false,
            show_gift: true,
            compact: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlayConfig {
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub font_size: u16,
    pub opacity: f32,
    pub click_through: bool,
    pub always_on_top: bool,
    pub theme: Theme,
    pub show_header: bool,
    /// 悬浮窗底部是否显示发弹幕输入条
    pub show_composer: bool,
    /// 悬浮窗锁定（穿透）状态，与 click_through 同步
    pub locked: bool,
}

impl OverlayConfig {
    /// 把窗口几何写回配置（拖动/缩放后调用），宽高做下限保护。
    pub fn apply_geometry(&mut self, x: i32, y: i32, width: u32, height: u32) {
        self.x = x;
        self.y = y;
        self.width = width.max(280);
        self.height = height.max(160);
    }
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            width: 380,
            height: 640,
            x: 40,
            y: 80,
            font_size: 13,
            opacity: 0.88,
            // 默认解锁：打开悬浮窗就能拖动、发弹幕；需要穿透时再点锁按钮
            click_through: false,
            always_on_top: true,
            theme: Theme::Dark,
            show_header: true,
            show_composer: true,
            locked: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DanmakuConfig {
    pub auto_reconnect: bool,
    pub reconnect_max_seconds: u64,
    pub heartbeat_seconds: u64,
    pub keep_history: bool,
    /// 每个房间在内存里保留的条数
    pub history_limit: usize,
    /// 房间状态（在线人数等）刷新间隔
    pub stats_interval_seconds: u64,
}

impl Default for DanmakuConfig {
    fn default() -> Self {
        Self {
            auto_reconnect: true,
            reconnect_max_seconds: 30,
            heartbeat_seconds: 30,
            keep_history: true,
            history_limit: 2000,
            stats_interval_seconds: 30,
        }
    }
}

/// 小米 MiMo 云端 TTS 配置。
///
/// 请求形态与 dmnotifier 的 tts 插件一致：OpenAI 兼容的 chat/completions，
/// 请求头用 `api-key`，返回的 WAV 以 base64 放在 choices[0].message.audio.data。
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MimoConfig {
    /// API Key；config.toml 按 0600 保存，不会出现在日志里
    pub api_key: String,
    /// base URL（默认官方地址），实际请求 {base_url}/chat/completions
    pub base_url: String,
    pub model: String,
    /// 预置音色：茉莉 / 冰糖 / 苏打 / 白桦 / Mia / Chloe / Milo / Dean / mimo_default
    pub voice: String,
}

impl Default for MimoConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.xiaomimimo.com/v1".into(),
            model: "mimo-v2.5-tts".into(),
            voice: "茉莉".into(),
        }
    }
}

impl std::fmt::Debug for MimoConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MimoConfig")
            .field(
                "api_key",
                &if self.api_key.is_empty() {
                    ""
                } else {
                    "<redacted>"
                },
            )
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("voice", &self.voice)
            .finish()
    }
}

impl MimoConfig {
    /// 生成实际请求的 chat/completions 地址。
    pub fn endpoint(&self) -> String {
        let base = self.base_url.trim().trim_end_matches('/');
        if base.ends_with("/chat/completions") {
            base.to_string()
        } else {
            format!("{base}/chat/completions")
        }
    }

    pub fn configured(&self) -> bool {
        !self.api_key.trim().is_empty()
    }

    /// base_url 是否可接受：必须是 https、不带账号密码、不指向本机或内网。
    ///
    /// 这里能挡住的是「把 api-key 明文发出去」和「拿本机当跳板打内网」；
    /// 至于用户自己把 base_url 改成某个公网代理，那是用户的决定（改配置本身
    /// 就是特权操作，接口令牌才是这条边界）。
    pub fn base_url_allowed(&self) -> bool {
        let raw = self.base_url.trim();
        if raw.is_empty() {
            return true;
        }
        let Ok(url) = url::Url::parse(raw) else {
            return false;
        };
        if url.scheme() != "https" {
            return false;
        }
        if !url.username().is_empty() || url.password().is_some() {
            return false;
        }
        let Some(host) = url
            .host_str()
            .map(|host| host.trim_matches(['[', ']']).to_ascii_lowercase())
        else {
            return false;
        };
        if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
            return false;
        }
        match host.parse::<std::net::IpAddr>() {
            Ok(ip) => crate::http::is_public_ip(&ip),
            Err(_) => true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TtsConfig {
    pub enabled: bool,
    pub provider: TtsProvider,
    /// provider = command 时的命令行，{text} 会被替换成播报内容
    pub command: Vec<String>,
    pub voice: String,
    pub rate: f32,
    pub volume: f32,
    /// 小米 MiMo 云端 TTS 配置（provider = mimo 时使用）
    pub mimo: MimoConfig,
    pub read_chat: bool,
    pub read_gift: bool,
    pub read_guard: bool,
    pub read_super_chat: bool,
    pub min_super_chat_price: f64,
    /// 单条播报的最大字数，超出截断
    pub max_chars: usize,
    /// 队列上限，满了直接丢弃新播报（避免弹幕洪水时堆积）
    pub max_queue: usize,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: TtsProvider::Auto,
            command: vec![],
            voice: String::new(),
            rate: 1.0,
            volume: 1.0,
            mimo: MimoConfig::default(),
            read_chat: true,
            read_gift: true,
            read_guard: true,
            read_super_chat: true,
            min_super_chat_price: 0.0,
            max_chars: 60,
            max_queue: 32,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SendConfig {
    /// 弹幕颜色，十进制 RGB，默认白色
    pub color: u32,
    pub font_size: u32,
    pub mode: u32,
    pub max_length: usize,
}

impl Default for SendConfig {
    fn default() -> Self {
        Self {
            color: 16_772_215,
            font_size: 25,
            mode: 1,
            max_length: 20,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub ui: UiConfig,
    pub overlay: OverlayConfig,
    pub danmaku: DanmakuConfig,
    pub tts: TtsConfig,
    pub send: SendConfig,
    /// 启动时自动订阅的房间号
    pub rooms: Vec<u64>,
}

impl AppConfig {
    pub fn config_path() -> PathBuf {
        config_dir().join("config.toml")
    }

    pub fn load() -> Self {
        Self::load_from(&Self::config_path())
    }

    pub fn load_from(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => match toml::from_str::<AppConfig>(&text) {
                Ok(config) => config.sanitized(),
                Err(err) => {
                    tracing::warn!(%err, path = %path.display(), "配置文件解析失败，使用默认配置");
                    AppConfig::default()
                }
            },
            Err(_) => AppConfig::default(),
        }
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::config_path())
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)?;
        let tmp = path.with_extension("toml.tmp");
        {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            use std::io::Write;
            let mut file = options.open(&tmp)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
        }
        std::fs::rename(&tmp, path)?;
        // 配置里可能保存了 TTS API Key；旧文件是 0644 时也要收紧
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    /// 对外的配置快照：抹掉 TTS API Key，只告诉调用方「配没配」。
    ///
    /// 接口 /api/config、/api/state 和 WS 首帧都走这里，避免密钥在局域网、
    /// OBS 浏览器源、以及任何能读到接口的进程里裸奔。
    pub fn to_public_value(&self) -> serde_json::Value {
        let mut value = serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}));
        if let Some(mimo) = value
            .get_mut("tts")
            .and_then(|tts| tts.get_mut("mimo"))
            .and_then(serde_json::Value::as_object_mut)
        {
            let has_key = mimo
                .get("api_key")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|key| !key.trim().is_empty());
            mimo.insert(
                "api_key".to_string(),
                serde_json::Value::String(String::new()),
            );
            mimo.insert("api_key_set".to_string(), serde_json::Value::Bool(has_key));
        }
        value
    }

    /// 把明显不合理的值收拢到可用范围，避免前端拿到 NaN / 0 宽高。
    pub fn sanitized(mut self) -> Self {
        self.server.port = if self.server.port == 0 {
            3939
        } else {
            self.server.port
        };
        self.ui.font_size = self.ui.font_size.clamp(10, 28);
        self.ui.opacity = clamp01(self.ui.opacity);
        self.ui.max_rows = self.ui.max_rows.clamp(50, 5000);
        self.overlay.width = self.overlay.width.clamp(280, 2000);
        self.overlay.height = self.overlay.height.clamp(160, 2000);
        self.overlay.font_size = self.overlay.font_size.clamp(10, 28);
        self.overlay.opacity = clamp01(self.overlay.opacity);
        self.danmaku.heartbeat_seconds = self.danmaku.heartbeat_seconds.clamp(10, 60);
        self.danmaku.stats_interval_seconds = self.danmaku.stats_interval_seconds.clamp(10, 300);
        self.danmaku.history_limit = self.danmaku.history_limit.clamp(200, 20_000);
        self.tts.max_chars = self.tts.max_chars.clamp(10, 200);
        self.tts.max_queue = self.tts.max_queue.clamp(1, 256);
        self.tts.mimo.api_key = self.tts.mimo.api_key.trim().to_string();
        self.tts.mimo.base_url = self
            .tts
            .mimo
            .base_url
            .trim()
            .trim_end_matches('/')
            .to_string();
        if self.tts.mimo.base_url.is_empty() {
            self.tts.mimo.base_url = MimoConfig::default().base_url;
        }
        if self.tts.mimo.model.trim().is_empty() {
            self.tts.mimo.model = MimoConfig::default().model;
        }
        if self.tts.mimo.voice.trim().is_empty() {
            self.tts.mimo.voice = MimoConfig::default().voice;
        }
        if !self.tts.mimo.base_url_allowed() {
            tracing::warn!(
                base_url = %self.tts.mimo.base_url,
                "MiMo base_url 不是公网 https 地址，已恢复默认值"
            );
            self.tts.mimo.base_url = MimoConfig::default().base_url;
        }
        // 命令行播报本身就是「配置即执行」，至少要挡住 sh -c 这类万能解释器：
        // 否则任何能写配置的人都能直接拿到任意代码执行。
        if self
            .tts
            .command
            .first()
            .is_some_and(|program| is_shell_like(program))
        {
            tracing::warn!(command = ?self.tts.command, "TTS 命令行指向 shell，已忽略");
            self.tts.command.clear();
        }
        self.send.max_length = self.send.max_length.clamp(1, 40);
        self
    }
}

/// 是不是 shell / 万能解释器（命令行播报不允许走它们）。
fn is_shell_like(program: &str) -> bool {
    let lowered = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .trim()
        .to_ascii_lowercase();
    let name = lowered.strip_suffix(".exe").unwrap_or(&lowered);
    matches!(
        name,
        "sh" | "bash"
            | "zsh"
            | "dash"
            | "ksh"
            | "fish"
            | "csh"
            | "tcsh"
            | "busybox"
            | "cmd"
            | "powershell"
            | "pwsh"
            | "env"
            | "xargs"
            | "eval"
            | "exec"
            | "nohup"
    )
}

fn clamp01(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.2, 1.0)
    } else {
        1.0
    }
}

/// XDG 配置目录：~/.config/maku-hime
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(crate::APP_NAME)
}

/// XDG 数据目录：~/.local/share/maku-hime（历史记录）
pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(crate::APP_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let config = AppConfig::default();
        assert_eq!(config.server.port, 3939);
        assert!(config.ui.opacity > 0.0 && config.ui.opacity <= 1.0);
    }

    #[test]
    fn roundtrips_through_toml() {
        let config = AppConfig {
            rooms: vec![689_422, 7_734_200],
            tts: TtsConfig {
                enabled: true,
                provider: TtsProvider::Command,
                command: vec!["spd-say".into(), "{text}".into()],
                ..Default::default()
            },
            ..Default::default()
        };
        let dir = std::env::temp_dir().join("makuhime-config-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        config.save_to(&path).unwrap();
        let loaded = AppConfig::load_from(&path);
        assert_eq!(loaded.rooms, vec![689_422, 7_734_200]);
        assert_eq!(loaded.tts.provider, TtsProvider::Command);
        assert_eq!(loaded.tts.command.len(), 2);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("[overlay]"));
        assert!(text.contains("click_through"));
    }

    #[test]
    fn falls_back_to_defaults_on_broken_file() {
        let dir = std::env::temp_dir().join("makuhime-config-broken");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, "this is not toml [").unwrap();
        let loaded = AppConfig::load_from(&path);
        assert_eq!(loaded.server.port, 3939);
    }

    #[test]
    fn applies_window_geometry_with_minimum_size() {
        let mut overlay = OverlayConfig::default();
        overlay.apply_geometry(120, 64, 500, 720);
        assert_eq!((overlay.x, overlay.y), (120, 64));
        assert_eq!((overlay.width, overlay.height), (500, 720));

        // 低于下限的尺寸会被夹住，避免保存成 0×0 的隐形窗口
        overlay.apply_geometry(-10, -20, 10, 10);
        assert_eq!((overlay.x, overlay.y), (-10, -20));
        assert_eq!((overlay.width, overlay.height), (280, 160));
    }

    #[test]
    fn sanitizes_out_of_range_values() {
        let mut config = AppConfig::default();
        config.ui.opacity = f32::NAN;
        config.overlay.width = 10;
        config.danmaku.heartbeat_seconds = 0;
        let config = config.sanitized();
        assert_eq!(config.ui.opacity, 1.0);
        assert_eq!(config.overlay.width, 280);
        assert_eq!(config.danmaku.heartbeat_seconds, 10);
    }

    #[test]
    fn mimo_config_roundtrips_and_hides_key_in_debug() {
        let dir = std::env::temp_dir().join("makuhime-config-mimo");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let config = AppConfig {
            tts: TtsConfig {
                provider: TtsProvider::Mimo,
                mimo: MimoConfig {
                    api_key: "sk-secret-value".into(),
                    voice: "冰糖".into(),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        };
        config.save_to(&path).unwrap();
        let loaded = AppConfig::load_from(&path);
        assert_eq!(loaded.tts.provider, TtsProvider::Mimo);
        assert_eq!(loaded.tts.mimo.api_key, "sk-secret-value");
        assert_eq!(loaded.tts.mimo.voice, "冰糖");
        let debug = format!("{loaded:?}");
        assert!(
            !debug.contains("sk-secret-value"),
            "Debug 输出了 API Key: {debug}"
        );
        assert!(debug.contains("<redacted>"));
    }

    #[test]
    fn mimo_base_url_and_voice_are_normalized() {
        let mut config = AppConfig::default();
        config.tts.mimo.base_url = " https://example.com/v1/// ".into();
        config.tts.mimo.voice = "   ".into();
        let config = config.sanitized();
        assert_eq!(
            config.tts.mimo.endpoint(),
            "https://example.com/v1/chat/completions"
        );
        assert_eq!(config.tts.mimo.voice, "茉莉");
        assert!(!config.tts.mimo.configured());
        assert_eq!(
            MimoConfig {
                base_url: "https://example.com/v1/chat/completions".into(),
                ..Default::default()
            }
            .endpoint(),
            "https://example.com/v1/chat/completions"
        );
    }

    #[cfg(unix)]
    #[test]
    fn config_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join("makuhime-config-mode");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        AppConfig::default().save_to(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "config.toml 可能含 API Key，权限必须是 0600");
    }
}
