//! B 站扫码登录：生成二维码 → 轮询状态 → 落地 SESSDATA / bili_jct。
//!
//! 只有发弹幕需要登录，看弹幕完全匿名。凭据仅保存在本机
//! （~/.config/maku-hime/credentials.json，权限 0600）。

use serde::Serialize;
use serde_json::Value;

use crate::error::{Error, Result};
use crate::http::HttpClient;
use crate::sender::Credentials;

const PASSPORT_REFERER: &str = "https://www.bilibili.com/";

/// 扫码会话：前端用 url 生成二维码，用 qrcode_key 轮询。
#[derive(Debug, Clone, Serialize)]
pub struct QrSession {
    pub url: String,
    pub qrcode_key: String,
}

/// 轮询结果。
#[derive(Debug, Clone, Serialize)]
pub struct QrPoll {
    /// pending / scanned / expired / success / error
    pub status: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<Profile>,
}

/// 登录用户信息（可公开展示）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct Profile {
    pub uid: u64,
    pub uname: String,
    pub face: String,
}

pub async fn qr_generate(http: &HttpClient) -> Result<QrSession> {
    let url =
        "https://passport.bilibili.com/x/passport-login/web/qrcode/generate?source=main-fe-header";
    let (value, _) = http
        .get_json_with_cookies(url, Some(PASSPORT_REFERER))
        .await?;
    if value.get("code").and_then(Value::as_i64) != Some(0) {
        return Err(Error::Api {
            code: value.get("code").and_then(Value::as_i64).unwrap_or(-1),
            message: value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("生成二维码失败")
                .to_string(),
        });
    }
    let data = value.get("data").unwrap_or(&Value::Null);
    Ok(QrSession {
        url: data
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        qrcode_key: data
            .get("qrcode_key")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

pub async fn qr_poll(http: &HttpClient, qrcode_key: &str) -> Result<QrPoll> {
    let url = format!(
        "https://passport.bilibili.com/x/passport-login/web/qrcode/poll?qrcode_key={}&source=main-fe-header",
        urlencoding::encode(qrcode_key)
    );
    let (value, cookies) = http
        .get_json_with_cookies(&url, Some(PASSPORT_REFERER))
        .await?;
    let data = value.get("data").cloned().unwrap_or(Value::Null);
    let code = data.get("code").and_then(Value::as_i64).unwrap_or(-1);
    let message = data
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    match code {
        86101 => Ok(QrPoll {
            status: "pending".into(),
            message: "等待扫码".into(),
            profile: None,
        }),
        86090 => Ok(QrPoll {
            status: "scanned".into(),
            message: "已扫码，请在手机上确认".into(),
            profile: None,
        }),
        86038 => Ok(QrPoll {
            status: "expired".into(),
            message: "二维码已过期，请刷新".into(),
            profile: None,
        }),
        0 => {
            let mut credentials = credentials_from_cookies(&cookies);
            if !credentials.is_valid() {
                // 部分环境下 Cookie 会出现在 crossDomain 跳转地址的 query 里。
                // 注意那串参数用 & 分隔，不能直接丢给按分号切分的 from_cookie。
                if let Some(redirect) = data.get("url").and_then(Value::as_str) {
                    credentials = credentials_from_redirect(redirect);
                }
            }
            if !credentials.is_valid() {
                return Ok(QrPoll {
                    status: "error".into(),
                    message: "登录成功但没有拿到 Cookie，请重试".into(),
                    profile: None,
                });
            }
            http.set_cookie(credentials.cookie());
            let profile = fetch_profile(http).await.unwrap_or_default();
            credentials.uname = profile.uname.clone();
            credentials.face = profile.face.clone();
            if credentials.dede_user_id.is_empty() {
                credentials.dede_user_id = profile.uid.to_string();
            }
            if let Err(err) = credentials.save() {
                tracing::warn!(%err, "保存登录凭据失败");
            }
            Ok(QrPoll {
                status: "success".into(),
                message: "登录成功".into(),
                profile: Some(profile),
            })
        }
        other => Ok(QrPoll {
            status: "error".into(),
            message: if message.is_empty() {
                format!("登录失败（code={other}）")
            } else {
                message
            },
            profile: None,
        }),
    }
}

/// 用当前 Cookie 查 nav，拿到昵称 / 头像 / mid。
pub async fn fetch_profile(http: &HttpClient) -> Result<Profile> {
    let value = http
        .get_json_with_cookies("https://api.bilibili.com/x/web-interface/nav", None)
        .await?
        .0;
    if value.get("code").and_then(Value::as_i64) != Some(0) {
        return Err(Error::NotLoggedIn);
    }
    let data = value.get("data").unwrap_or(&Value::Null);
    if !data
        .get("isLogin")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err(Error::NotLoggedIn);
    }
    Ok(Profile {
        uid: data.get("mid").and_then(Value::as_u64).unwrap_or(0),
        uname: data
            .get("uname")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        face: data
            .get("face")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

/// 从 crossDomain 跳转地址的 query 里提取凭据。
/// 形如：https://passport.biligame.com/crossDomain?DedeUserID=1&DedeUserID__ckMd5=x&SESSDATA=y&bili_jct=z
fn credentials_from_redirect(url: &str) -> Credentials {
    let query = url.split_once('?').map(|(_, query)| query).unwrap_or(url);
    let query = query.split('#').next().unwrap_or(query);
    let mut credentials = Credentials::default();
    for pair in query.split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        match key {
            "SESSDATA" => credentials.sessdata = value.to_string(),
            "bili_jct" => credentials.bili_jct = value.to_string(),
            "DedeUserID" => credentials.dede_user_id = value.to_string(),
            _ => {}
        }
    }
    credentials.saved_at = crate::client::now_millis();
    credentials
}

fn credentials_from_cookies(cookies: &[String]) -> Credentials {
    let joined = cookies
        .iter()
        .map(|cookie| {
            // Set-Cookie: SESSDATA=xxx; Path=/; Domain=.bilibili.com
            cookie
                .split(';')
                .next()
                .unwrap_or_default()
                .trim()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("; ");
    Credentials::from_cookie(&joined)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_credentials_from_cross_domain_redirect() {
        let url = "https://passport.biligame.com/crossDomain?DedeUserID=12345&DedeUserID__ckMd5=abc&SESSDATA=xyz%2C1799%2Cdef&bili_jct=deadbeef&gourl=https%3A%2F%2Flive.bilibili.com";
        let creds = credentials_from_redirect(url);
        assert_eq!(creds.sessdata, "xyz%2C1799%2Cdef");
        assert_eq!(creds.bili_jct, "deadbeef");
        assert_eq!(creds.dede_user_id, "12345");
        assert!(creds.is_valid());
    }

    #[test]
    fn parses_credentials_from_set_cookie_headers() {
        let headers = vec![
            "SESSDATA=abc%2C123%2Cxyz; Path=/; Domain=.bilibili.com; HttpOnly".to_string(),
            "bili_jct=deadbeefcafe; Path=/; Domain=.bilibili.com".to_string(),
            "DedeUserID=12345; Path=/; Domain=.bilibili.com".to_string(),
        ];
        let creds = credentials_from_cookies(&headers);
        assert_eq!(creds.sessdata, "abc%2C123%2Cxyz");
        assert_eq!(creds.bili_jct, "deadbeefcafe");
        assert_eq!(creds.dede_user_id, "12345");
        assert!(creds.is_valid());
    }
}
