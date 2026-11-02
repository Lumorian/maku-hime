//! 带 UA / Cookie / WBI 签名的 HTTP 客户端。
//!
//! B 站直播的大部分接口都能匿名访问，但 getDanmuInfo 从 2024 年起要求 WBI 签名，
//! 否则返回 -352（风控）。签名算法见
//! <https://github.com/SocialSisterYi/bilibili-API-collect/blob/master/docs/misc/sign/wbi.md>。

use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use md5::{Digest, Md5};
use serde_json::Value;
use url::Url;

use crate::error::{Error, Result};

/// 伪装成桌面浏览器的 UA，B 站对默认 UA 会更容易触发风控。
pub const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// 直播姬（LiveHime）客户端的 UA。开播接口按客户端投白名单：
/// 浏览器 UA 会被 60034（系统维护，仅支持直播姬开播）挡掉。
pub const LIVEHIME_UA: &str =
    "LiveHime/7.63.0.10783 os/Windows pc_app/livehime build/10783 osVer/10.0_x86_64";

/// 是否允许走系统代理（默认**不**走）。
///
/// reqwest 默认会读 `http_proxy` / `https_proxy` / `all_proxy` 这些环境变量。
/// 对弹幕姬这种「只连 B 站和 TTS 服务」的本地工具，这是个坑：用户开着
/// Clash / v2rayN 之类的「系统代理」时，一换节点、一关代理，已经在跑的
/// 长连接和后续请求就被打断，弹幕也跟着断。
///
/// 所以所有对外客户端统一 `no_proxy()`（见 [`client_builder`]），直连 B 站。
/// 真的必须走代理（公司网络、海外线路）时，显式设 `MAKUHIME_SYSTEM_PROXY=1`
/// 打开，此时行为和以前一致：切代理会断，请自己挑个稳定的节点。
pub fn system_proxy_enabled() -> bool {
    proxy_enabled_from(std::env::var("MAKUHIME_SYSTEM_PROXY").ok().as_deref())
}

fn proxy_enabled_from(raw: Option<&str>) -> bool {
    matches!(
        raw.map(|value| value.trim().to_ascii_lowercase())
            .as_deref(),
        Some("1" | "true" | "yes" | "on")
    )
}

/// 全项目唯一的 ClientBuilder 入口：保证所有对外请求用同一套代理策略，
/// 以后新增客户端不会再漏掉 `no_proxy()`。
pub(crate) fn client_builder() -> reqwest::ClientBuilder {
    let builder = reqwest::Client::builder();
    if system_proxy_enabled() {
        builder
    } else {
        builder.no_proxy()
    }
}

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

/// multipart 里的一个文件字段。
pub struct UploadPart<'a> {
    pub field: &'a str,
    pub file_name: &'a str,
    pub mime: &'a str,
    pub bytes: &'a [u8],
}

#[derive(Clone)]
pub struct HttpClient {
    client: reqwest::Client,
    session: std::sync::Arc<Mutex<Session>>,
}

impl HttpClient {
    pub fn new() -> Result<Self> {
        // 不吃系统代理：见 client_builder()
        let client = client_builder()
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
            .header(reqwest::header::ACCEPT, "application/json, text/plain, */*");
        // 登录 Cookie 只发给 B 站的域名：url 里可能带着用户填写的第三方地址
        // （例如封面链接），无条件附带会把 SESSDATA 泄露给任意主机。
        if is_bilibili_host(url) {
            builder = builder
                .header(reqwest::header::ORIGIN, "https://live.bilibili.com")
                .header(reqwest::header::REFERER, "https://live.bilibili.com/");
            let cookie = self.cookie();
            if !cookie.trim().is_empty() {
                builder = builder.header(reqwest::header::COOKIE, cookie);
            }
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
        let status = response.status();
        let headers: Vec<String> = response
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok().map(str::to_string))
            .collect();
        let text = response.text().await?;
        if !status.is_success() {
            // B 站风控会直接返回 412 + HTML，这里给出原始片段而不是 serde 报错
            return Err(Error::other(format!("HTTP {status}: {}", snippet(&text))));
        }
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
        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            return Err(Error::other(format!("HTTP {status}: {}", snippet(&text))));
        }
        Ok(serde_json::from_str(&text)?)
    }

    /// GET 原始字节（下载封面用），超过 limit 直接失败。
    pub async fn get_bytes(&self, url: &str, limit: usize) -> Result<Vec<u8>> {
        let response = self.request(reqwest::Method::GET, url).send().await?;
        let status = response.status();
        if !status.is_success() {
            return Err(Error::other(format!("下载封面失败：HTTP {status}")));
        }
        if let Some(length) = response.content_length()
            && length as usize > limit
        {
            return Err(Error::other("封面图片太大，请先压缩再上传"));
        }
        let bytes = response.bytes().await?;
        if bytes.len() > limit {
            return Err(Error::other("封面图片太大，请先压缩再上传"));
        }
        Ok(bytes.to_vec())
    }

    /// GET 原始字节，**不携带任何登录凭据**，用于抓取用户填写的第三方地址
    /// （封面链接等）。重定向手动逐跳校验：首跳合法、后续跳到内网的写法也会被挡住。
    pub async fn get_bytes_public(
        &self,
        url: &str,
        limit: usize,
        max_redirects: usize,
    ) -> Result<Vec<u8>> {
        let mut current =
            Url::parse(url).map_err(|err| Error::other(format!("图片地址无法解析：{err}")))?;
        let mut hops = 0usize;
        loop {
            let pin = ensure_public_https(&current).await?;
            let client = build_public_client(pin.as_ref())?;
            let response = client
                .get(current.clone())
                .header(reqwest::header::ACCEPT, "image/*,*/*;q=0.8")
                .send()
                .await?;
            let status = response.status();
            if status.is_redirection() {
                if hops >= max_redirects {
                    return Err(Error::other("图片地址重定向次数过多"));
                }
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| Error::other("图片地址重定向缺少目标"))?
                    .to_string();
                drop(response);
                current = current
                    .join(&location)
                    .map_err(|err| Error::other(format!("图片地址重定向无效：{err}")))?;
                hops += 1;
                continue;
            }
            if !status.is_success() {
                return Err(Error::other(format!("下载图片失败：HTTP {status}")));
            }
            if let Some(length) = response.content_length()
                && length as usize > limit
            {
                return Err(Error::other("图片太大，请先压缩再上传"));
            }
            let bytes = response.bytes().await?;
            if bytes.len() > limit {
                return Err(Error::other("图片太大，请先压缩再上传"));
            }
            return Ok(bytes.to_vec());
        }
    }

    /// GET 并返回 JSON，UA 可指定（开播设置这类接口按客户端区分白名单）。
    pub async fn get_json_as(&self, url: &str, ua: &str) -> Result<Value> {
        let response = self
            .request(reqwest::Method::GET, url)
            .header(reqwest::header::USER_AGENT, ua)
            .send()
            .await?;
        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            return Err(Error::other(format!("HTTP {status}: {}", snippet(&text))));
        }
        Ok(serde_json::from_str(&text)?)
    }

    /// POST 表单，UA 可指定。
    pub async fn post_form_as(
        &self,
        url: &str,
        form: &[(String, String)],
        ua: &str,
    ) -> Result<Value> {
        let response = self
            .request(reqwest::Method::POST, url)
            .header(reqwest::header::USER_AGENT, ua)
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .form(form)
            .send()
            .await?;
        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            return Err(Error::other(format!("HTTP {status}: {}", snippet(&text))));
        }
        Ok(serde_json::from_str(&text)?)
    }

    /// POST multipart（直播封面上传用）。
    pub async fn post_multipart_as(
        &self,
        url: &str,
        fields: &[(&str, String)],
        file: UploadPart<'_>,
        ua: &str,
    ) -> Result<Value> {
        let mut form = reqwest::multipart::Form::new();
        for (key, value) in fields {
            form = form.text((*key).to_string(), value.clone());
        }
        let part = reqwest::multipart::Part::bytes(file.bytes.to_vec())
            .file_name(file.file_name.to_string())
            .mime_str(file.mime)?;
        form = form.part(file.field.to_string(), part);

        let response = self
            .request(reqwest::Method::POST, url)
            .header(reqwest::header::USER_AGENT, ua)
            .multipart(form)
            .send()
            .await?;
        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            return Err(Error::other(format!("HTTP {status}: {}", snippet(&text))));
        }
        Ok(serde_json::from_str(&text)?)
    }

    /// 用直播姬客户端身份 POST 表单。
    ///
    /// 开播接口只认官方客户端：浏览器 UA、没有 appkey Cookie 的请求都会被 60034 挡掉，
    /// 所以这里换 UA 并补上客户端 Cookie（appkey / device_platform / buvid3）。
    pub async fn post_form_as_livehime(
        &self,
        url: &str,
        form: &[(String, String)],
        cookies: &[String],
    ) -> Result<Value> {
        let mut cookie = self.cookie();
        for extra in cookies {
            if !cookie.trim().is_empty() {
                cookie.push_str("; ");
            }
            cookie.push_str(extra);
        }
        let mut builder = self
            .client
            .request(reqwest::Method::POST, url)
            .header(reqwest::header::USER_AGENT, LIVEHIME_UA)
            .header(reqwest::header::ACCEPT, "application/json")
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            );
        if !cookie.trim().is_empty() {
            builder = builder.header(reqwest::header::COOKIE, cookie);
        }
        let response = builder.form(form).send().await?;
        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            return Err(Error::other(format!("HTTP {status}: {}", snippet(&text))));
        }
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
            if let Some(keys) = &session.wbi
                && keys.fetched_at.elapsed() < Duration::from_secs(3600)
            {
                return Ok(keys.mixin.clone());
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

/// 只有 B 站自己的域名才允许携带登录 Cookie / 伪装 Origin。
fn is_bilibili_host(url: &str) -> bool {
    const SUFFIXES: [&str; 6] = [
        "bilibili.com",
        "biligame.com",
        "hdslb.com",
        "biliapi.net",
        "bilivideo.com",
        "b23.tv",
    ];
    let Ok(parsed) = Url::parse(url) else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    SUFFIXES.iter().any(|suffix| {
        host == *suffix
            || (host.ends_with(suffix) && host.as_bytes()[host.len() - suffix.len() - 1] == b'.')
    })
}

/// 公网地址判定：内网 / 环回 / 链路本地 / 组播 / 保留段一律拒绝。
pub fn is_public_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_unspecified()
                || v4.is_multicast()
                // 100.64.0.0/10 运营商级 NAT、192.0.0.0/24 等特殊用途段
                || (v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1]))
                || (v4.octets()[0] == 192 && v4.octets()[1] == 0 && v4.octets()[2] == 0)
                || v4.octets()[0] >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_public_ip(&IpAddr::V4(mapped));
            }
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                // fc00::/7 唯一本地地址、fe80::/10 链路本地
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
                || (first & 0xffc0) == 0xfec0)
        }
    }
}

/// 校验地址是「公网 https」。返回需要固定连接的地址（防 DNS rebinding
/// 在解析与建连之间被换掉）；IP 字面量则返回 None。
async fn ensure_public_https(url: &Url) -> Result<Option<(String, SocketAddr)>> {
    if url.scheme() != "https" {
        return Err(Error::other("图片地址必须是 https"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::other("图片地址不能包含用户名或密码"));
    }
    let host = url
        .host_str()
        .ok_or_else(|| Error::other("图片地址缺少主机名"))?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    let port = url.port_or_known_default().unwrap_or(443);
    if let Ok(ip) = host.parse::<IpAddr>() {
        return if is_public_ip(&ip) {
            Ok(None)
        } else {
            Err(Error::other("图片地址不能指向本机或内网"))
        };
    }
    let addrs = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|err| Error::other(format!("图片地址无法解析：{err}")))?;
    let mut pinned = None;
    for addr in addrs {
        if !is_public_ip(&addr.ip()) {
            return Err(Error::other("图片地址不能指向本机或内网"));
        }
        if pinned.is_none() {
            pinned = Some(addr);
        }
    }
    match pinned {
        Some(addr) => Ok(Some((host, addr))),
        None => Err(Error::other("图片地址无法解析")),
    }
}

fn build_public_client(pin: Option<&(String, SocketAddr)>) -> Result<reqwest::Client> {
    let mut builder = client_builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none());
    if let Some((host, addr)) = pin {
        builder = builder.resolve(host, *addr);
    }
    Ok(builder.build()?)
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
    fn proxy_switch_only_when_explicitly_enabled() {
        for raw in [
            None,
            Some(""),
            Some("0"),
            Some("false"),
            Some("no"),
            Some("off"),
        ] {
            assert!(!proxy_enabled_from(raw), "{raw:?} 不该打开系统代理");
        }
        for raw in [Some("1"), Some("true"), Some("YES"), Some(" on ")] {
            assert!(proxy_enabled_from(raw), "{raw:?} 应该打开系统代理");
        }
    }

    #[test]
    fn default_clients_do_not_read_system_proxy() {
        if std::env::var_os("MAKUHIME_SYSTEM_PROXY").is_some() {
            return; // 外面显式开了，这个默认值断言没有意义
        }
        assert!(!system_proxy_enabled());
        assert!(client_builder().build().is_ok());
    }

    #[test]
    fn cookie_only_goes_to_bilibili_hosts() {
        assert!(is_bilibili_host("https://api.live.bilibili.com/x"));
        assert!(is_bilibili_host("https://bilibili.com/x"));
        assert!(is_bilibili_host("https://i0.hdslb.com/bfs/a.png"));
        assert!(!is_bilibili_host("https://attacker.example/a.png"));
        assert!(!is_bilibili_host(
            "https://bilibili.com.attacker.example/a.png"
        ));
        assert!(!is_bilibili_host("https://127.0.0.1.nip.io:3939/a.png"));
        assert!(!is_bilibili_host("not a url"));
    }

    #[test]
    fn public_ip_check_rejects_internal_ranges() {
        use std::net::{Ipv4Addr, Ipv6Addr};
        for raw in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "224.0.0.1",
        ] {
            let ip: IpAddr = raw.parse().unwrap();
            assert!(!is_public_ip(&ip), "{raw} 不应被当成公网地址");
        }
        for raw in ["::1", "fd00::1", "fe80::1", "::ffff:127.0.0.1"] {
            let ip: IpAddr = raw.parse().unwrap();
            assert!(!is_public_ip(&ip), "{raw} 不应被当成公网地址");
        }
        assert!(is_public_ip(&IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1))));
        assert!(is_public_ip(&IpAddr::V6(Ipv6Addr::new(
            0x2400, 0, 0, 0, 0, 0, 0, 1
        ))));
    }

    #[test]
    fn file_stem_extracts_hash() {
        assert_eq!(
            file_stem("https://i0.hdslb.com/bfs/wbi/7cd084941338484aae1ad9425b84077c.png"),
            "7cd084941338484aae1ad9425b84077c"
        );
    }
}
