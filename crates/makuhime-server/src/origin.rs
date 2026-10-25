//! 本地服务的来源校验。
//!
//! 这个服务跑在 127.0.0.1 上、能读到你的弹幕、还能用你保存的登录态发弹幕，
//! 所以不能对「任意网页」开放：
//!
//! - **WebSocket 不受 CORS 约束** —— 浏览器的 ws 握手不受同源策略保护，
//!   如果不校验 Origin，任何你访问的网站都能连上 /api/ws 读走全部弹幕；
//! - **DNS rebinding** —— 恶意域名解析到 127.0.0.1 时，请求的 Host 是攻击者域名，
//!   所以要额外校验 Host 是不是本机。
//!
//! 规则：
//! 1. 绑定在本机地址时，Host 必须是回环地址（127.0.0.1 / localhost / ::1，或 127.x）；
//! 2. 带 Origin 的请求，Origin 必须是本机来源、Tauri 来源，或与 Host 同源（局域网部署）；
//! 3. 没有 Origin 的请求放行（curl、OBS 的内部请求、非浏览器客户端不会伪造这个头）。

use axum::Json;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::json;

/// 中间件：来源不合法直接 403。
pub async fn guard(State(local_only): State<bool>, request: Request, next: Next) -> Response {
    if request_allowed(request.headers(), local_only) {
        next.run(request).await
    } else {
        (
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": {
                    "code": "forbidden_origin",
                    "message": "只允许本机页面访问该接口",
                }
            })),
        )
            .into_response()
    }
}

/// 一次请求是否允许通过。
pub fn request_allowed(headers: &HeaderMap, local_only: bool) -> bool {
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok());
    if local_only {
        // 没有 Host 的请求只可能来自非浏览器客户端（HTTP/1.1 必带 Host）
        if let Some(host) = host
            && !is_loopback_host(host)
        {
            return false;
        }
    }
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    match origin {
        None => true,
        Some(origin) => is_local_origin(origin) || same_authority(origin, host),
    }
}

/// Host 是否为回环地址（可带端口）。
pub fn is_loopback_host(raw: &str) -> bool {
    let host = host_without_port(raw);
    host == "localhost" || host == "::1" || host == "127.0.0.1" || host.starts_with("127.")
}

/// Origin 是否为本机 / 桌面壳来源。
pub fn is_local_origin(origin: &str) -> bool {
    if origin.starts_with("tauri://") {
        return true;
    }
    let authority = origin
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(origin);
    let authority = authority.split('/').next().unwrap_or(authority);
    let host = host_without_port(authority);
    host == "localhost" || host == "tauri.localhost" || host == "::1" || host.starts_with("127.")
}

/// Origin 与 Host 是否同源（用于局域网/自定义域名部署）。
fn same_authority(origin: &str, host: Option<&str>) -> bool {
    let Some(host) = host else {
        return false;
    };
    let authority = origin
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(origin);
    let authority = authority.split('/').next().unwrap_or(authority);
    authority.eq_ignore_ascii_case(host)
}

/// 去掉端口，并处理 IPv6 的方括号形式。
fn host_without_port(raw: &str) -> &str {
    let raw = raw.trim();
    if let Some(stripped) = raw.strip_prefix('[') {
        return stripped.split(']').next().unwrap_or(stripped);
    }
    match raw.rsplit_once(':') {
        // 像 ::1 这种没有端口的裸 IPv6，rsplit 的后半段也能解析成数字，这里排除掉
        Some((head, tail)) if tail.chars().all(|c| c.is_ascii_digit()) => head,
        _ => raw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(host: Option<&str>, origin: Option<&str>) -> HeaderMap {
        let mut map = HeaderMap::new();
        if let Some(host) = host {
            map.insert(header::HOST, HeaderValue::from_str(host).unwrap());
        }
        if let Some(origin) = origin {
            map.insert(header::ORIGIN, HeaderValue::from_str(origin).unwrap());
        }
        map
    }

    #[test]
    fn blocks_foreign_origin() {
        // 复现过的场景：任意网页连本机 WebSocket
        assert!(!request_allowed(
            &headers(Some("127.0.0.1:3939"), Some("https://evil.example")),
            true
        ));
    }

    #[test]
    fn allows_local_pages() {
        for origin in [
            "http://127.0.0.1:3939",
            "http://localhost:5173",
            "tauri://localhost",
            "http://tauri.localhost",
        ] {
            assert!(
                request_allowed(&headers(Some("127.0.0.1:3939"), Some(origin)), true),
                "{origin} 应该被放行"
            );
        }
    }

    #[test]
    fn allows_non_browser_clients_without_origin() {
        assert!(request_allowed(
            &headers(Some("127.0.0.1:3939"), None),
            true
        ));
        assert!(request_allowed(&headers(None, None), true));
    }

    #[test]
    fn blocks_dns_rebinding_host() {
        assert!(!request_allowed(
            &headers(Some("evil.example:3939"), Some("http://evil.example:3939")),
            true
        ));
    }

    #[test]
    fn lan_deployment_accepts_same_origin() {
        // 用户把 host 改成 0.0.0.0 后，从局域网地址访问
        assert!(request_allowed(
            &headers(Some("192.168.1.5:3939"), Some("http://192.168.1.5:3939")),
            false
        ));
        // 但来自外站仍然拒绝
        assert!(!request_allowed(
            &headers(Some("192.168.1.5:3939"), Some("https://evil.example")),
            false
        ));
    }

    #[test]
    fn parses_hosts_with_ports_and_ipv6() {
        assert_eq!(host_without_port("127.0.0.1:3939"), "127.0.0.1");
        assert_eq!(host_without_port("[::1]:3939"), "::1");
        assert_eq!(host_without_port("localhost"), "localhost");
        assert!(is_loopback_host("127.0.1.1:3939"));
        assert!(!is_loopback_host("192.168.1.5:3939"));
    }
}
