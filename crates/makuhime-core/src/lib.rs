//! maku-hime 核心：B 站直播弹幕协议、房间状态、发送、登录、历史与 TTS。
//!
//! 这个 crate 不依赖任何 GUI，既可以给 Tauri 壳用，也可以单独跑成本地服务。

pub mod client;
pub mod config;
pub mod engine;
pub mod error;
pub mod history;
pub mod http;
pub mod login;
pub mod model;
pub mod packet;
pub mod parser;
pub mod pb;
pub mod room;
pub mod sender;
pub mod tts;

pub use error::{Error, Result};
pub use model::{Event, LiveEvent};

/// 版本号，来自 Cargo.toml。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 产品名，出现在日志、User-Agent 与前端标题里。
pub const APP_NAME: &str = "maku-hime";
