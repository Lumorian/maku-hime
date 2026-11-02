//! 本地接口的访问令牌。
//!
//! 为什么需要它：这个服务能读你的弹幕、能用你保存的登录态发弹幕、还能改
//! TTS 命令并让本机执行。只靠 [crate::origin] 的 Host / Origin 校验是不够的
//! —— 浏览器实现差异、DNS rebinding 的变种、以及「本机上别的程序」都不在它的
//! 覆盖范围里。所以再加一层共享密钥：
//!
//! - 启动时生成 256 位随机令牌，写进 `config_dir()/api-token`（0600）；
//!   也可以用 `MAKUHIME_API_TOKEN` 环境变量指定（脚本 / CI 用）；
//! - 本服务发出的 HTML 会注入 `window.__MAKU_HIME_TOKEN__` 并下发
//!   `maku_token` Cookie，前端随请求带上 `X-Maku-Token` 头；
//! - WebSocket 不能自定义请求头，所以额外接受 `?token=` 查询参数；
//! - 比较用等长异或累加（常数时间），不泄漏「前缀对了几位」。
//!
//! 唯一免鉴权的接口是 `/api/health`：前端靠它探测端口，且它不返回任何
//! 敏感信息。静态资源也不需要令牌 —— 令牌本身就是随页面发出去的。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use axum::Json;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::SharedState;

/// 环境变量：手动指定令牌（跳过文件读写）。
pub const ENV_TOKEN: &str = "MAKUHIME_API_TOKEN";
/// 请求头名。
pub const HEADER: &str = "x-maku-token";
/// Cookie 名。
pub const COOKIE: &str = "maku_token";
/// 查询参数名（WebSocket 用）。
pub const QUERY: &str = "token";
/// 令牌文件（相对于配置目录）。
const TOKEN_FILE: &str = "api-token";
/// 太短的令牌当作没设置，防止手滑写成 "1"。
const MIN_LEN: usize = 16;

static TOKEN: OnceLock<String> = OnceLock::new();

/// 本次进程的访问令牌（首次调用时生成或读取）。
pub fn token() -> &'static str {
    TOKEN.get_or_init(load_or_create)
}

/// 令牌文件路径。
pub fn token_path() -> PathBuf {
    makuhime_core::config::config_dir().join(TOKEN_FILE)
}

/// 下发给浏览器的 Set-Cookie 值。
pub fn cookie_value_header() -> String {
    // 不带 Max-Age：会话 Cookie，重启后令牌换了也不会留存旧值
    format!("{COOKIE}={}; Path=/; SameSite=Strict; HttpOnly", token())
}

/// 中间件：没有有效令牌的 /api/* 请求一律 401。
pub async fn require(State(app): State<SharedState>, request: Request, next: Next) -> Response {
    if is_public_path(request.uri().path())
        || authenticated(&app.token, request.headers(), request.uri())
    {
        return next.run(request).await;
    }
    unauthorized()
}

/// 不需要令牌的路径：静态资源 + 健康检查。
///
/// 注意 `/api` 本身也算受保护路径（拼错的 /api/xxx 同样要令牌，
/// 免得以后新增路由时忘加校验）。
fn is_public_path(path: &str) -> bool {
    if path == "/api/health" {
        return true;
    }
    !(path == "/api" || path.starts_with("/api/"))
}

/// 请求是否带了正确令牌（头、Cookie、查询参数三选一）。
fn authenticated(expected: &str, headers: &HeaderMap, uri: &Uri) -> bool {
    let expected = expected.as_bytes();
    if let Some(value) = header_str(headers, HEADER)
        && ct_eq(value.trim().as_bytes(), expected)
    {
        return true;
    }
    if let Some(raw) = header_str(headers, header::COOKIE.as_str())
        && let Some(value) = cookie_of(raw, COOKIE)
        && ct_eq(value.as_bytes(), expected)
    {
        return true;
    }
    if let Some(query) = uri.query()
        && let Some(value) = query_of(query, QUERY)
        && ct_eq(value.as_bytes(), expected)
    {
        return true;
    }
    false
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn cookie_of<'a>(raw: &'a str, name: &str) -> Option<&'a str> {
    raw.split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

fn query_of<'a>(raw: &'a str, name: &str) -> Option<&'a str> {
    raw.split('&')
        .filter_map(|part| part.split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

/// 常数时间比较：长度不同直接失败，否则异或累加，不提前 return。
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "error": {
                "code": "unauthorized",
                "message": "缺少访问令牌：请通过本服务打开的页面访问，或用 MAKUHIME_API_TOKEN 指定令牌",
            }
        })),
    )
        .into_response()
}

fn load_or_create() -> String {
    if let Ok(value) = std::env::var(ENV_TOKEN) {
        let value = value.trim().to_string();
        if value.len() >= MIN_LEN {
            tracing::info!("使用 {ENV_TOKEN} 指定的访问令牌");
            return value;
        }
        tracing::warn!("{ENV_TOKEN} 少于 {MIN_LEN} 个字符，忽略并改为自动生成");
    }
    let path = token_path();
    if let Some(existing) = read_token(&path) {
        return existing;
    }
    let token = generate();
    match write_token(&path, &token) {
        Ok(()) => tracing::info!(path = %path.display(), "已生成访问令牌"),
        Err(err) => {
            tracing::warn!(%err, path = %path.display(), "访问令牌保存失败（本次运行仍然有效）")
        }
    }
    token
}

fn read_token(path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let token = raw.trim().to_string();
    (token.len() >= MIN_LEN).then_some(token)
}

fn write_token(path: &Path, token: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // 配置目录只给自己，别让同机其他用户翻到令牌
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
        }
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(token.as_bytes())?;
    file.write_all(b"\n")
}

/// 256 位随机令牌：两个 UUIDv4 的十六进制拼起来。
fn generate() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(*name, HeaderValue::from_str(value).unwrap());
        }
        map
    }

    fn uri(raw: &str) -> Uri {
        raw.parse().unwrap()
    }

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn health_and_static_assets_are_open() {
        assert!(is_public_path("/api/health"));
        assert!(is_public_path("/"));
        assert!(is_public_path("/obs"));
        assert!(is_public_path("/assets/index.js"));
        // 拼错的 /api 路径走静态 fallback 返回 JSON 404，但也得先过鉴权
        assert!(!is_public_path("/api"));
        assert!(!is_public_path("/api/state"));
        assert!(!is_public_path("/api/config"));
    }

    #[test]
    fn api_requires_the_exact_token() {
        assert!(!authenticated(TOKEN, &headers(&[]), &uri("/api/state")));
        assert!(!authenticated(
            TOKEN,
            &headers(&[(HEADER, "0123456789abcde")]),
            &uri("/api/state")
        ));
        assert!(!authenticated(
            TOKEN,
            &headers(&[(HEADER, "0123456789abcdef0123456789abcdee")]),
            &uri("/api/state")
        ));
        assert!(authenticated(
            TOKEN,
            &headers(&[(HEADER, TOKEN)]),
            &uri("/api/state")
        ));
    }

    #[test]
    fn cookie_and_query_token_work() {
        let cookie = format!("a=1; {COOKIE}={TOKEN}; b=2");
        assert!(authenticated(
            TOKEN,
            &headers(&[(header::COOKIE.as_str(), &cookie)]),
            &uri("/api/ws")
        ));
        let url = format!("/api/ws?room=1&{QUERY}={TOKEN}");
        assert!(authenticated(TOKEN, &headers(&[]), &uri(&url)));
        // 查询参数对不上就不行
        let url = "/api/ws?token=deadbeef";
        assert!(!authenticated(TOKEN, &headers(&[]), &uri(url)));
    }

    #[test]
    fn constant_time_compare_behaves() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abd"));
        assert!(!ct_eq(b"abc", b"abcd"));
        assert!(ct_eq(b"", b""));
    }

    #[test]
    fn generated_tokens_are_long_and_unique() {
        let a = generate();
        let b = generate();
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
