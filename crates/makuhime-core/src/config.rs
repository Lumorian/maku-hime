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
    /// 悬浮窗锁定（穿透）状态，与 click_through 同步
    pub locked: bool,
}

impl OverlayConfig {
    /// 把窗口几何写回配置（拖动/缩放后调用），宽高做下限保护。
    pub fn apply_geometry(&mut self, x: i32, y: i32, width: u32, height: u32) {
        self.x = x;
        self.y = y;
        self.width = width.max(240);
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
            click_through: true,
            always_on_top: true,
            theme: Theme::Dark,
            show_header: true,
            locked: true,
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
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
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
        self.overlay.width = self.overlay.width.clamp(240, 2000);
        self.overlay.height = self.overlay.height.clamp(160, 2000);
        self.overlay.font_size = self.overlay.font_size.clamp(10, 28);
        self.overlay.opacity = clamp01(self.overlay.opacity);
        self.danmaku.heartbeat_seconds = self.danmaku.heartbeat_seconds.clamp(10, 60);
        self.danmaku.stats_interval_seconds = self.danmaku.stats_interval_seconds.clamp(10, 300);
        self.danmaku.history_limit = self.danmaku.history_limit.clamp(200, 20_000);
        self.tts.max_chars = self.tts.max_chars.clamp(10, 200);
        self.tts.max_queue = self.tts.max_queue.clamp(1, 256);
        self.send.max_length = self.send.max_length.clamp(1, 40);
        self
    }
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
        assert_eq!((overlay.width, overlay.height), (240, 160));
    }

    #[test]
    fn sanitizes_out_of_range_values() {
        let mut config = AppConfig::default();
        config.ui.opacity = f32::NAN;
        config.overlay.width = 10;
        config.danmaku.heartbeat_seconds = 0;
        let config = config.sanitized();
        assert_eq!(config.ui.opacity, 1.0);
        assert_eq!(config.overlay.width, 240);
        assert_eq!(config.danmaku.heartbeat_seconds, 10);
    }
}
