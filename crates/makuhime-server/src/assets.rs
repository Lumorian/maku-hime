//! 静态资源：Vite 构建出来的 dist/ 目录。
//!
//! 查找顺序：环境变量 MAKUHIME_WEB_DIR → ./dist → 可执行文件旁的 dist →
//! 开发时的 ../dist → 打包后的 resources/dist。都找不到就返回一个说明页。

use std::path::{Path, PathBuf};

use axum::Router;
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use tower_http::services::ServeDir;

use crate::SharedState;

/// 找到前端构建产物目录。
pub fn resolve_web_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("MAKUHIME_WEB_DIR") {
        let dir = PathBuf::from(dir);
        if dir.join("index.html").is_file() {
            // 这个变量指向的目录里的任意文件都会被当作前端资源发出去
            // （包括 .js，等于在页面同源里执行），正式包里只应作为调试手段。
            if !cfg!(debug_assertions) {
                tracing::warn!(
                    dir = %dir.display(),
                    "MAKUHIME_WEB_DIR 在正式构建中被使用：该目录下的所有文件都会                     以页面同源的身份对外提供，请确认目录内容可信"
                );
            }
            return Some(dir);
        }
    }
    let mut candidates = vec![PathBuf::from("dist")];
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        candidates.push(dir.join("dist"));
        candidates.push(dir.join("../dist"));
        candidates.push(dir.join("resources/dist"));
        candidates.push(dir.join("../Resources/dist"));
    }
    candidates
        .into_iter()
        .find(|dir| dir.join("index.html").is_file())
}

pub fn routes(state: SharedState) -> Router<SharedState> {
    let Some(dir) = state.web_dir.clone() else {
        return Router::new().fallback(get(missing_web_dir));
    };

    let index = dir.join("index.html");
    let obs = if dir.join("obs.html").is_file() {
        dir.join("obs.html")
    } else {
        index.clone()
    };

    let index_route = index.clone();
    let obs_route = obs.clone();
    let port = state.port;

    Router::new()
        .route(
            "/",
            get(move || {
                let path = index_route.clone();
                async move { serve_html(&path, port).await }
            }),
        )
        .route(
            "/obs",
            get(move || {
                let path = obs_route.clone();
                async move { serve_html(&path, port).await }
            }),
        )
        .route(
            "/overlay",
            get({
                let obs = obs.clone();
                move || {
                    let path = obs.clone();
                    async move { serve_html(&path, port).await }
                }
            }),
        )
        .nest_service("/assets", ServeDir::new(dir.join("assets")))
        .fallback({
            let dir = dir.clone();
            let index = index.clone();
            move |uri: Uri| {
                let dir = dir.clone();
                let index = index.clone();
                async move { spa_fallback(dir, index, uri, port).await }
            }
        })
}

/// SPA fallback：静态文件优先，其次 index.html。
/// 但拼错的 /api/xxx 必须返回 JSON 404 —— 否则前端会把 HTML 当成功响应。
async fn spa_fallback(dir: PathBuf, index: PathBuf, uri: Uri, port: u16) -> Response {
    let path = uri.path();
    if path.starts_with("/api/") {
        return (
            StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({
                "error": { "code": "not_found", "message": "接口不存在" }
            })),
        )
            .into_response();
    }
    let relative = path.trim_start_matches('/');
    if relative.split('/').any(|segment| segment == "..") {
        return (StatusCode::NOT_FOUND, "非法路径").into_response();
    }
    if !relative.is_empty() {
        let file = dir.join(relative);
        if file.is_file() {
            return serve_file(&file).await;
        }
    }
    serve_html(&index, port).await
}

/// 本服务自己发出的页面用的 CSP。
///
/// 注意 script-src 必须带 'unsafe-inline'（引导脚本是内联的），
/// 真正防 XSS 的活由「React 默认转义 + 远端颜色值白名单」承担。
const PAGE_CSP: &str = "default-src 'self'; \
script-src 'self' 'unsafe-inline'; \
style-src 'self' 'unsafe-inline'; \
img-src 'self' data: blob: https: http://127.0.0.1:*; \
media-src 'self' blob: data:; \
connect-src 'self' ws: http:; \
font-src 'self' data:; \
object-src 'none'; base-uri 'none'; form-action 'none'";

/// 页面注入实际端口和访问令牌。
///
/// - 端口：多实例并存时，前端只能跟自己这台服务讲话；
/// - 令牌：页面由本服务发出，所以能拿到令牌；别的网站的页面拿不到（跨源读不到响应体），
///   于是即便它猜到了端口也调不动 /api。
///
/// 同时下发一个 SameSite=Strict 的会话 Cookie，让 `<img src="/api/...">` 这种
/// 没法自定义请求头的同源请求（封面、表情包、导出下载）也能通过校验。
async fn serve_html(path: &Path, port: u16) -> Response {
    let Ok(bytes) = tokio::fs::read(path).await else {
        return (StatusCode::NOT_FOUND, "文件不存在").into_response();
    };
    let body = inject_port(bytes, port);
    let mut response = body.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    // 页面里带着令牌，任何一层缓存都不该留下它
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Ok(cookie) = HeaderValue::from_str(&crate::auth::cookie_value_header()) {
        headers.insert(header::SET_COOKIE, cookie);
    }
    // 页面里那段注入的引导脚本是内联的，所以 script-src 必须放开 'unsafe-inline'；
    // 其余一律收紧，挡住被注入的远程脚本、插件与嵌套框架。
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(PAGE_CSP),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}

fn inject_port(bytes: Vec<u8>, port: u16) -> Vec<u8> {
    let Ok(text) = String::from_utf8(bytes) else {
        return Vec::new();
    };
    // 令牌用 serde_json 序列化后再拼进脚本，避免引号/反斜杠破坏语句
    let token = serde_json::Value::String(crate::auth::token().to_string()).to_string();
    let script = format!(
        "<script>window.__MAKU_HIME_PORT__={port};window.__MAKU_HIME_TOKEN__={token}</script>"
    );
    let Some(index) = text.find("<head>") else {
        return text.into_bytes();
    };
    let mut out = String::with_capacity(text.len() + script.len());
    out.push_str(&text[..index + "<head>".len()]);
    out.push_str(&script);
    out.push_str(&text[index + "<head>".len()..]);
    out.into_bytes()
}

async fn serve_file(path: &Path) -> Response {
    match tokio::fs::read(path).await {
        Ok(bytes) => {
            let mime = mime_for(path);
            ([(header::CONTENT_TYPE, mime)], bytes).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "文件不存在").into_response(),
    }
}

fn mime_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
    {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "json" => "application/json; charset=utf-8",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

async fn missing_web_dir(uri: Uri) -> Response {
    if uri.path().starts_with("/api/") {
        return (StatusCode::NOT_FOUND, "接口不存在").into_response();
    }
    Html(
        r#"<!doctype html>
<html lang="zh-CN"><head><meta charset="utf-8">
<title>maku-hime</title>
<style>
  body{margin:0;min-height:100vh;display:grid;place-items:center;background:#101011;color:#e6e8ea;
       font:15px/1.7 system-ui,"Noto Sans CJK SC",sans-serif}
  .card{max-width:640px;padding:36px 40px;border:1px solid #2f3134;border-radius:14px;background:#17181a}
  h1{margin:0 0 8px;font-size:22px}code{background:#24262a;padding:2px 6px;border-radius:5px;color:#ffb027}
  p{color:#a2a7ae}li{margin:6px 0}
</style></head><body><div class="card">
<h1>maku-hime 服务已启动，但没找到前端资源</h1>
<p>接口是通的（试试 <code>/api/health</code>），只是没有构建出来的页面。</p>
<ul>
  <li>开发：先执行 <code>npm run build</code>，或 <code>npm run dev</code> 用 Vite 的 5173 端口</li>
  <li>或用 <code>MAKUHIME_WEB_DIR=/path/to/dist</code> 指向构建产物</li>
  <li>OBS 浏览器源需要页面：<code>/obs</code></li>
</ul>
</div></body></html>"#,
    )
    .into_response()
}
