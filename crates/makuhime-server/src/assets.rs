//! 静态资源：Vite 构建出来的 dist/ 目录。
//!
//! 查找顺序：环境变量 MAKUHIME_WEB_DIR → ./dist → 可执行文件旁的 dist →
//! 开发时的 ../dist → 打包后的 resources/dist。都找不到就返回一个说明页。

use std::path::{Path, PathBuf};

use axum::Router;
use axum::http::{StatusCode, Uri, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use tower_http::services::ServeDir;

use crate::SharedState;

/// 找到前端构建产物目录。
pub fn resolve_web_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("MAKUHIME_WEB_DIR") {
        let dir = PathBuf::from(dir);
        if dir.join("index.html").is_file() {
            return Some(dir);
        }
    }
    let mut candidates = vec![PathBuf::from("dist")];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("dist"));
            candidates.push(dir.join("../dist"));
            candidates.push(dir.join("resources/dist"));
            candidates.push(dir.join("../Resources/dist"));
        }
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

    Router::new()
        .route(
            "/",
            get(move || {
                let path = index_route.clone();
                async move { serve_file(&path).await }
            }),
        )
        .route(
            "/obs",
            get(move || {
                let path = obs_route.clone();
                async move { serve_file(&path).await }
            }),
        )
        .route(
            "/overlay",
            get({
                let obs = obs.clone();
                move || {
                    let path = obs.clone();
                    async move { serve_file(&path).await }
                }
            }),
        )
        .nest_service("/assets", ServeDir::new(dir.join("assets")))
        .fallback_service(
            ServeDir::new(&dir).not_found_service(tower_http::services::ServeFile::new(index)),
        )
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
