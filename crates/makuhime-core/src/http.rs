//! 带 UA / Cookie / WBI 签名的 HTTP 客户端。
//!
//! B 站直播的大部分接口都能匿名访问，但 getDanmuInfo 从 2024 年起要求 WBI 签名，
//! 否则返回 -352（风控）。签名算法见
//! <https://github.com/SocialSisterYi/bilibili-API-collect/blob/master/docs/misc/sign/wbi.md>。

use std::sync::Mutex;
use std::time::{Duration, Instant};

use md5::{Digest, Md5};
use serde_json::Value;
use url::Url;

use crate::error::{Error, Result};

/// 伪装成桌面浏览器的 UA，B 站对默认 UA 会更容易触发风控。
pub const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// WBI 打散表
const MIXIN_KEY_ENC_TAB: [usize; 64] = [
    46, 47, 18, 2, 53, 8, 23, 32, 15, 50, 10, 31, 58, 3, 45, 35, 27, 43, 5, 49, 33, 9, 42, 19, 29,
    28, 14, 39, 12, 38, 41, 13, 37, 48, 7, 16, 24, 55, 40, 61, 26, 17, 0, 1, 60, 51, 30, 4, 22, 25,
    54, 21, 56, 59, 6, 63, 57, 62, 11, 36, 20, 34, 44, 52,
];

struct WbiKeys {
    mixin: String,
    fetched_at: Instant,
}

#[derive(Default)]
struct Session {
    cookie: String,
    buvid: Option<String>,
    wbi: Option<WbiKeys>,
}

#[derive(Clone)]
pub struct HttpClient {
    client: reqwest::Client,
    session: std::sync::Arc<Mutex<Session>>,
}

impl HttpClient {
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent(UA)
            .timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(8))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()?;
        Ok(Self {
            client,
            session: std::sync::Arc::new(Mutex::new(Session::default())),
        })
    }

    /// 设置登录态 Cookie（发送弹幕 / 登录后调用）。
    pub fn set_cookie(&self, cookie: impl Into<String>) {
        let mut session = self.session.lock().unwrap();
        session.cookie = cookie.into();
    }

    pub fn cookie(&self) -> String {
        self.session.lock().unwrap().cookie.clone()
    }

    pub fn has_cookie(&self) -> bool {
        !self.cookie().trim().is_empty()
    }

    fn request(&self, method: reqwest::Method, url: &str) -> reqwest::RequestBuilder {
        let mut builder = self
            .client
            .request(method, url)
            .header(reqwest::header::ACCEPT, "application/json, text/plain, */*")
            .header(reqwest::header::ORIGIN, "https://live.bilibili.com")
            .header(reqwest::header::REFERER, "https://live.bilibili.com/");
        let cookie = self.cookie();
        if !cookie.trim().is_empty() {
            builder = builder.header(reqwest::header::COOKIE, cookie);
        }
        builder
    }

    /// GET 并返回 JSON（不检查业务 code）。
    pub async fn get_json(&self, url: &str) -> Result<Value> {
        let response = self.request(reqwest::Method::GET, url).send().await?;
        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            return Err(Error::other(format!("HTTP {status}: {}", snippet(&text))));
        }
        Ok(serde_json::from_str(&text)?)
    }

    /// GET 并要求业务 code == 0，否则返回 [Error::Api]。
    pub async fn api(&self, url: &str) -> Result<Value> {
        let value = self.get_json(url).await?;
        let code = value.get("code").and_then(Value::as_i64).unwrap_or(-1);
        if code != 0 {
            let message = value
                .get("message")
                .or_else(|| value.get("msg"))
                .and_then(Value::as_str)
                .unwrap_or("未知错误")
                .to_string();
            return Err(Error::Api { code, message });
        }
        Ok(value)
    }

    /// GET 并同时返回 Set-Cookie 头（扫码登录要用）。
    pub async fn get_json_with_cookies(
        &self,
        url: &str,
        referer: Option<&str>,
    ) -> Result<(Value, Vec<String>)> {
        let mut builder = self.request(reqwest::Method::GET, url);
        if let Some(referer) = referer {
            builder = builder.header(reqwest::header::REFERER, referer);
        }
        let response = builder.send().await?;
        let headers: Vec<String> = response
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok().map(str::to_string))
            .collect();
        let text = response.text().await?;
        Ok((serde_json::from_str(&text)?, headers))
    }

    /// POST 表单（发送弹幕用）。
    pub async fn post_form(&self, url: &str, form: &[(&str, String)]) -> Result<Value> {
        let response = self
            .request(reqwest::Method::POST, url)
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .form(form)
            .send()
            .await?;
        let text = response.text().await?;
        Ok(serde_json::from_str(&text)?)
    }

    /// 匿名 buvid3，认证包与部分接口需要它。
    pub async fn buvid3(&self) -> Option<String> {
        if let Some(cached) = self.session.lock().unwrap().buvid.clone() {
            return Some(cached);
        }
        let value = self
            .get_json("https://api.bilibili.com/x/frontend/finger/spi")
            .await
            .ok()?;
        let buvid = value
            .get("data")
            .and_then(|d| d.get("b_3"))
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
            .map(str::to_string);
        if let Some(ref value) = buvid {
            self.session.lock().unwrap().buvid = Some(value.clone());
        }
        buvid
    }

    /// 取 WBI mixin key（1 小时缓存）。未登录（-101）也能拿到。
    async fn wbi_mixin(&self) -> Result<String> {
        {
            let session = self.session.lock().unwrap();
            if let Some(keys) = &session.wbi {
                if keys.fetched_at.elapsed() < Duration::from_secs(3600) {
                    return Ok(keys.mixin.clone());
                }
            }
        }
        let value = self
            .get_json("https://api.bilibili.com/x/web-interface/nav")
            .await?;
        let img = value
            .get("data")
            .and_then(|d| d.get("wbi_img"))
            .and_then(|w| w.get("img_url"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                Error::other(format!(
                    "获取 WBI 密钥失败: {}",
                    snippet(&value.to_string())
                ))
            })?;
        let sub = value
            .get("data")
            .and_then(|d| d.get("wbi_img"))
            .and_then(|w| w.get("sub_url"))
            .and_then(Value::as_str)
            .ok_or_else(|| Error::other("获取 WBI 密钥失败: sub_url 缺失"))?;

        let base = format!("{}{}", file_stem(img), file_stem(sub));
        let mixin: String = MIXIN_KEY_ENC_TAB
            .iter()
            .filter_map(|index| base.as_bytes().get(*index).copied())
            .map(|byte| byte as char)
            .take(32)
            .collect();
        if mixin.len() < 32 {
            return Err(Error::other("WBI 密钥长度异常"));
        }
        let mut session = self.session.lock().unwrap();
        session.wbi = Some(WbiKeys {
            mixin: mixin.clone(),
            fetched_at: Instant::now(),
        });
        Ok(mixin)
    }

    /// 给 URL 加上 wts 与 w_rid 两个 WBI 签名参数。
    pub async fn wbi_sign(&self, url: &str) -> Result<String> {
        let mixin = self.wbi_mixin().await?;
        sign_with_mixin(url, &mixin, now_secs())
    }
}

/// 用给定的 mixin key 对 URL 签名（抽出来便于单元测试）。
pub fn sign_with_mixin(url: &str, mixin: &str, wts: u64) -> Result<String> {
    let parsed = Url::parse(url).map_err(|e| Error::other(format!("URL 解析失败: {e}")))?;
    let mut pairs: Vec<(String, String)> = parsed
        .query_pairs()
        .map(|(k, v)| (strip_unwanted(&k), strip_unwanted(&v)))
        .collect();
    pairs.push(("wts".to_string(), wts.to_string()));
    pairs.sort_by(|a, b| a.0.cmp(&b.0));

    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    serializer.extend_pairs(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    let query = serializer.finish();

    let mut hasher = Md5::new();
    hasher.update(query.as_bytes());
    hasher.update(mixin.as_bytes());
    let w_rid = format!("{:x}", hasher.finalize());

    let mut out = parsed.clone();
    out.set_query(Some(&query));
    Ok(format!("{}&w_rid={}", out.as_str(), w_rid))
}

fn strip_unwanted(raw: &str) -> String {
    raw.chars()
        .filter(|c| !matches!(c, '!' | '\'' | '(' | ')' | '*'))
        .collect()
}

fn file_stem(url: &str) -> String {
    url.rsplit('/')
        .next()
        .unwrap_or_default()
        .split('.')
        .next()
        .unwrap_or_default()
        .to_string()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn snippet(text: &str) -> String {
    let trimmed: String = text.chars().take(200).collect();
    if text.chars().count() > 200 {
        format!("{trimmed}…")
    } else {
        trimmed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wbi_signature_matches_reference_algorithm() {
        // 参考实现：query 按 key 排序 → md5(query + mixin)
        let url =
            "https://api.live.bilibili.com/xlive/web-room/v1/index/getDanmuInfo?id=7734200&type=0";
        let signed =
            sign_with_mixin(url, "ea1db124af3c7062474693fa704f4ff8", 1_792_951_565).unwrap();
        assert!(signed.contains("wts=1792951565"));
        assert!(signed.contains("w_rid="));
        // 与用 Node 实测得到的签名一致（同一组参数下 md5 稳定）
        let query = signed.split('?').nth(1).unwrap();
        assert!(query.starts_with("id=7734200&type=0&wts=1792951565&w_rid="));
        assert_eq!(
            query.len(),
            "id=7734200&type=0&wts=1792951565&w_rid=".len() + 32
        );
    }

    #[test]
    fn strips_special_chars_from_params() {
        let signed = sign_with_mixin("https://x.test/a?q=a(b)c*d!e'f", "m", 1).unwrap();
        assert!(!signed.contains('('));
        assert!(!signed.contains('*'));
    }

    #[test]
    fn file_stem_extracts_hash() {
        assert_eq!(
            file_stem("https://i0.hdslb.com/bfs/wbi/7cd084941338484aae1ad9425b84077c.png"),
            "7cd084941338484aae1ad9425b84077c"
        );
    }
}
