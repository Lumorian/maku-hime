//! maku-hime 本地服务：给桌面面板、浏览器面板、OBS 浏览器源共用的 HTTP + WebSocket 接口。
//!
//! 设计上只有一个数据源（[Engine](makuhime_core::engine::Engine)），所有前端都是它的消费者：
//! - Tauri 桌面窗口：走 HTTP/WS + Tauri IPC 管窗口；
//! - 浏览器打开 http://127.0.0.1:3939 得到同一套面板；
//! - OBS 浏览器源加 http://127.0.0.1:3939/obs?room=<房间号> 就是透明弹幕墙。
//!
//! 安全边界见 [origin]：以回环地址监听时，只接受本机 / 桌面壳来源的请求。

pub mod api;
pub mod assets;
pub mod origin;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::middleware;
use makuhime_core::engine::Engine;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tower_http::trace::TraceLayer;

/// 路由共享状态。
pub struct AppState {
    pub engine: Engine,
    pub web_dir: Option<PathBuf>,
    /// 只服务本机时为 true：开启严格的 Host / Origin 校验。
    pub local_only: bool,
}

pub type SharedState = Arc<AppState>;

/// 组装路由。
pub fn router(state: SharedState) -> Router {
    let local_only = state.local_only;

    // CORS 只是浏览器读响应的门槛；真正的拦截在 origin 中间件（WebSocket 不受 CORS 约束）。
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _| {
            let origin = origin.as_bytes();
            origin == b"tauri://localhost"
                || origin == b"http://tauri.localhost"
                || origin == b"http://127.0.0.1"
                || origin.starts_with(b"http://127.0.0.1:")
                || origin == b"http://localhost"
                || origin.starts_with(b"http://localhost:")
        }))
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .merge(api::routes())
        .merge(assets::routes(state.clone()))
        .layer(middleware::from_fn_with_state(local_only, origin::guard))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

fn shared_state(engine: Engine, web_dir: Option<PathBuf>, addr: SocketAddr) -> SharedState {
    Arc::new(AppState {
        engine,
        web_dir,
        local_only: origin::is_loopback_host(&addr.ip().to_string()),
    })
}

fn run(listener: tokio::net::TcpListener, state: SharedState) {
    let app = router(state);
    tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, app).await {
            tracing::error!(%err, "HTTP 服务退出");
        }
    });
}

/// 同步绑定端口（桌面端要在创建窗口前就知道端口号）。
/// 端口被占用时自动向后找最多 10 个。
pub fn bind_sync(host: &str, port: u16) -> std::io::Result<(std::net::TcpListener, SocketAddr)> {
    let mut last_err = None;
    for offset in 0..10u16 {
        let candidate = port.saturating_add(offset);
        let addr: SocketAddr = format!("{host}:{candidate}")
            .parse()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        match std::net::TcpListener::bind(addr) {
            Ok(listener) => {
                listener.set_nonblocking(true)?;
                let actual = listener.local_addr()?;
                return Ok((listener, actual));
            }
            Err(err) => last_err = Some(err),
        }
    }
    Err(last_err.unwrap_or_else(|| std::io::Error::other("没有可用端口")))
}

/// 在一个已经绑定好的监听上启动服务。
pub async fn serve_listener(
    listener: std::net::TcpListener,
    engine: Engine,
    web_dir: Option<PathBuf>,
) -> std::io::Result<SocketAddr> {
    let listener = tokio::net::TcpListener::from_std(listener)?;
    let addr = listener.local_addr()?;
    engine.set_server_url(format!("http://{addr}"));
    run(listener, shared_state(engine, web_dir, addr));
    Ok(addr)
}

/// 监听地址，端口被占用时自动向后找最多 10 个。
pub async fn bind(host: &str, port: u16) -> std::io::Result<(tokio::net::TcpListener, SocketAddr)> {
    let mut last_err = None;
    for offset in 0..10u16 {
        let candidate = port.saturating_add(offset);
        let addr: SocketAddr = format!("{host}:{candidate}")
            .parse()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => {
                let actual = listener.local_addr()?;
                return Ok((listener, actual));
            }
            Err(err) => last_err = Some(err),
        }
    }
    Err(last_err.unwrap_or_else(|| std::io::Error::other("没有可用端口")))
}

/// 启动服务并返回实际监听地址。调用方负责保持 future 存活。
pub async fn serve(engine: Engine, host: &str, port: u16) -> std::io::Result<SocketAddr> {
    serve_with_web_dir(engine, host, port, assets::resolve_web_dir()).await
}

/// 启动服务并指定前端构建产物目录（桌面端打包后用 resources/dist）。
pub async fn serve_with_web_dir(
    engine: Engine,
    host: &str,
    port: u16,
    web_dir: Option<PathBuf>,
) -> std::io::Result<SocketAddr> {
    let (listener, addr) = bind(host, port).await?;
    engine.set_server_url(format!("http://{addr}"));
    run(listener, shared_state(engine, web_dir, addr));
    Ok(addr)
}
