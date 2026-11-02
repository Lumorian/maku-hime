//! maku-hime 本地服务：给桌面面板、浏览器面板、OBS 浏览器源共用的 HTTP + WebSocket 接口。
//!
//! 设计上只有一个数据源（[Engine](makuhime_core::engine::Engine)），所有前端都是它的消费者：
//! - Tauri 桌面窗口：走 HTTP/WS + Tauri IPC 管窗口；
//! - 浏览器打开 http://127.0.0.1:3939 得到同一套面板；
//! - OBS 浏览器源加 http://127.0.0.1:3939/obs?room=<房间号> 就是透明弹幕墙。
//!
//! 安全边界是两层，缺一不可：
//! - [origin]：只接受本机 / 桌面壳来源的请求（挡网页与 DNS rebinding）；
//! - [auth]：所有 /api 接口都要访问令牌（挡同一台机器上的其它进程和残留的浏览器标签页）。

pub mod api;
pub mod assets;
pub mod auth;
pub mod origin;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

use axum::Router;
use axum::http::{HeaderName, Method, header};
use axum::middleware;
use makuhime_core::engine::Engine;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;

/// 路由共享状态。
pub struct AppState {
    pub engine: Engine,
    pub web_dir: Option<PathBuf>,
    /// 只服务本机时为 true：开启严格的 Host / Origin 校验。
    pub local_only: bool,
    /// 实际监听端口，注入到页面里（多实例并存时前端不该串到别人的服务）。
    pub port: u16,
    /// 访问令牌，见 [auth]。所有 /api 接口都要带。
    pub token: String,
    /// TTS 试听的上次调用时间，用来限流（合成一次要花钱、要占额度）。
    pub tts_test_gate: Mutex<Option<Instant>>,
}

pub type SharedState = Arc<AppState>;

/// 组装路由。
pub fn router(state: SharedState) -> Router {
    let local_only = state.local_only;
    let port = state.port;

    // CORS 只是浏览器读响应的门槛；真正的拦截在 origin 中间件（WebSocket 不受 CORS 约束）。
    // 这里只放行桌面壳来源和「本机 + 本实例端口」的来源：
    // 放行任意端口等于把响应交给同机任意网页（它们可以自己起个 localhost:随机端口）。
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(move |origin, _| {
            let Ok(origin) = origin.to_str() else {
                return false;
            };
            if origin == "tauri://localhost" || origin == "http://tauri.localhost" {
                return true;
            }
            let Some(authority) = origin.strip_prefix("http://") else {
                return false;
            };
            let Some((host, actual_port)) = authority.rsplit_once(':') else {
                return false;
            };
            actual_port == port.to_string() && origin::is_loopback_host(host)
        }))
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([
            header::CONTENT_TYPE,
            header::ACCEPT,
            HeaderName::from_static("x-maku-token"),
        ]);

    Router::new()
        .merge(api::routes())
        .merge(assets::routes(state.clone()))
        // 内层：令牌校验。外层：来源校验。两层都要过。
        .layer(middleware::from_fn_with_state(state.clone(), auth::require))
        .layer(middleware::from_fn_with_state(local_only, origin::guard))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

fn shared_state(engine: Engine, web_dir: Option<PathBuf>, addr: SocketAddr) -> SharedState {
    let local_only = origin::is_loopback_host(&addr.ip().to_string());
    if !local_only {
        tracing::warn!(
            %addr,
            "服务监听在非回环地址：Host 校验已关闭，请确认这台机器所在网络可信；\
             接口仍然需要访问令牌"
        );
    }
    Arc::new(AppState {
        engine,
        web_dir,
        local_only,
        port: addr.port(),
        token: auth::token().to_string(),
        tts_test_gate: Mutex::new(None),
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
