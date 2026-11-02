//! 统一错误类型。

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("网络请求失败: {0}")]
    Http(#[from] reqwest::Error),

    #[error("WebSocket 错误: {0}")]
    Ws(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON 解析失败: {0}")]
    Json(#[from] serde_json::Error),

    #[error("配置写入失败: {0}")]
    Toml(#[from] toml::ser::Error),

    #[error("配置解析失败: {0}")]
    TomlParse(#[from] toml::de::Error),

    #[error("数据包格式错误: {0}")]
    Packet(String),

    #[error("B 站接口返回错误 [{code}]: {message}")]
    Api { code: i64, message: String },

    #[error("未登录：B 站账号凭据缺失或已过期")]
    NotLoggedIn,

    #[error("{0}")]
    NotFound(String),

    #[error("发送弹幕失败 [{code}]: {message}")]
    Send { code: i64, message: String },

    /// 开播 / 停播失败；message 已经翻译成可直接展示的中文
    #[error("{message}")]
    Live { code: i64, message: String },

    /// 直播间设置（标题/分区/封面/公告）失败；message 已翻译成中文
    #[error("{message}")]
    Manage { code: i64, message: String },

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn other(msg: impl Into<String>) -> Self {
        Error::Other(msg.into())
    }
}
