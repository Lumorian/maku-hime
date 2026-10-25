//! 发送弹幕与登录凭据管理。
//!
//! 发送走官方 msg/send 表单接口，必须带登录 Cookie 与 csrf（即 cookie 里的 bili_jct）。
//! 凭据保存为 0600 的 credentials.json，只在本机使用。

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::now_millis;
use crate::config::{SendConfig, config_dir};
use crate::error::{Error, Result};
use crate::http::HttpClient;

/// B 站登录凭据（等价于登录态，务必不要外传）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Credentials {
    #[serde(default)]
    pub sessdata: String,
    #[serde(default)]
    pub bili_jct: String,
    #[serde(default)]
    pub dede_user_id: String,
    #[serde(default)]
    pub uname: String,
    #[serde(default)]
    pub face: String,
    #[serde(default)]
    pub saved_at: i64,
}

impl Credentials {
    pub fn path() -> PathBuf {
        config_dir().join("credentials.json")
    }

    pub fn is_valid(&self) -> bool {
        !self.sessdata.trim().is_empty() && !self.bili_jct.trim().is_empty()
    }

    /// 拼成 Cookie 请求头。
    pub fn cookie(&self) -> String {
        let mut parts = vec![
            format!("SESSDATA={}", self.sessdata),
            format!("bili_jct={}", self.bili_jct),
        ];
        if !self.dede_user_id.is_empty() {
            parts.push(format!("DedeUserID={}", self.dede_user_id));
        }
        parts.join("; ")
    }

    /// 从 Cookie 字符串里提取凭据。
    pub fn from_cookie(cookie: &str) -> Self {
        Self {
            sessdata: cookie_value(cookie, "SESSDATA").unwrap_or_default(),
            bili_jct: cookie_value(cookie, "bili_jct").unwrap_or_default(),
            dede_user_id: cookie_value(cookie, "DedeUserID").unwrap_or_default(),
            uname: String::new(),
            face: String::new(),
            saved_at: now_millis(),
        }
    }

    pub fn load() -> Option<Self> {
        let text = std::fs::read_to_string(Self::path()).ok()?;
        serde_json::from_str::<Credentials>(&text)
            .ok()
            .filter(Credentials::is_valid)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, text)?;
        #[cfg(unix)]
        {
            let mut perms = std::fs::metadata(&path)?.permissions();
            perms.set_mode(0o600);
            std::fs::set_permissions(&path, perms)?;
        }
        Ok(())
    }

    pub fn clear() -> Result<()> {
        let path = Self::path();
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
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

/// 发送前能本地发现的错误就不打网络请求。
pub fn validate(text: &str, config: &SendConfig) -> Result<()> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(Error::Send {
            code: -1,
            message: "弹幕内容不能为空".into(),
        });
    }
    let length = trimmed.chars().count();
    if length > config.max_length {
        return Err(Error::Send {
            code: -2,
            message: format!("弹幕最多 {} 个字，当前 {} 个字", config.max_length, length),
        });
    }
    Ok(())
}

/// 发送一条弹幕。
pub async fn send_danmaku(
    http: &HttpClient,
    credentials: &Credentials,
    room_id: u64,
    text: &str,
    config: &SendConfig,
) -> Result<()> {
    validate(text, config)?;
    if !credentials.is_valid() {
        return Err(Error::NotLoggedIn);
    }
    http.set_cookie(credentials.cookie());

    let form = vec![
        ("color", config.color.to_string()),
        ("fontsize", config.font_size.to_string()),
        ("mode", config.mode.to_string()),
        ("msg", text.trim().to_string()),
        ("roomid", room_id.to_string()),
        ("rnd", now_millis().to_string()),
        ("bubble", "0".to_string()),
        ("csrf", credentials.bili_jct.clone()),
        ("csrf_token", credentials.bili_jct.clone()),
    ];
    let value = http
        .post_form("https://api.live.bilibili.com/msg/send", &form)
        .await?;
    check_send_response(&value)
}

fn check_send_response(value: &Value) -> Result<()> {
    let code = value.get("code").and_then(Value::as_i64).unwrap_or(-1);
    if code == 0 {
        return Ok(());
    }
    let raw = value
        .get("message")
        .or_else(|| value.get("msg"))
        .and_then(Value::as_str)
        .unwrap_or("未知错误");
    Err(Error::Send {
        code,
        message: explain(code, raw),
    })
}

/// 把 B 站错误码翻译成能直接展示的中文。
pub fn explain(code: i64, raw: &str) -> String {
    let known = match code {
        -101 => "账号未登录或登录已过期，请重新扫码登录",
        -111 => "csrf 校验失败，请重新登录",
        -400 => "请求参数有问题，请检查房间号",
        -403 => "被 B 站风控拦截，稍后再试",
        10030 => "弹幕包含敏感词或已被拦截",
        10031 => "发送太频繁，请稍后再试",
        10032 => "弹幕长度超出限制",
        10024 => "你已被禁言",
        10026 => "账号存在风险，暂时不能发送",
        1003212 => "内容与上一条过于相似",
        _ => return format!("{raw}（code={code}）"),
    };
    known.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_credentials_from_cookie() {
        let creds = Credentials::from_cookie(
            "buvid3=x; SESSDATA=abc%2C123; bili_jct=deadbeef; DedeUserID=12345",
        );
        assert_eq!(creds.sessdata, "abc%2C123");
        assert_eq!(creds.bili_jct, "deadbeef");
        assert_eq!(creds.dede_user_id, "12345");
        assert!(creds.is_valid());
        assert_eq!(
            creds.cookie(),
            "SESSDATA=abc%2C123; bili_jct=deadbeef; DedeUserID=12345"
        );
    }

    #[test]
    fn empty_credentials_are_invalid() {
        assert!(!Credentials::default().is_valid());
        let partial = Credentials {
            sessdata: "x".into(),
            ..Default::default()
        };
        assert!(!partial.is_valid());
    }

    #[test]
    fn validates_length_and_emptiness() {
        let config = SendConfig::default();
        assert!(validate("你好", &config).is_ok());
        assert!(validate("   ", &config).is_err());
        assert!(validate(&"字".repeat(21), &config).is_err());
        assert!(validate(&"字".repeat(20), &config).is_ok());
    }

    #[test]
    fn explains_known_codes() {
        assert!(explain(-101, "x").contains("登录"));
        assert!(explain(10031, "x").contains("频繁"));
        assert_eq!(explain(9999, "服务异常"), "服务异常（code=9999）");
    }

    #[test]
    fn send_response_ok_and_error() {
        assert!(check_send_response(&serde_json::json!({"code": 0})).is_ok());
        let err = check_send_response(&serde_json::json!({"code": 10031, "message": "frequent"}))
            .unwrap_err();
        match err {
            Error::Send { code, message } => {
                assert_eq!(code, 10031);
                assert!(message.contains("频繁"));
            }
            other => panic!("错误类型不对: {other:?}"),
        }
    }
}
