//! maku-hime 桌面壳：启动引擎与本地服务，管理主窗口和可穿屏悬浮窗。
//!
//! 数据流与浏览器面板完全一致：窗口只是个 WebView，接口都走 127.0.0.1 上的
//! HTTP/WebSocket 服务；桌面壳额外提供窗口操作（穿透、置顶、缩放、托盘）。

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use makuhime_core::config::AppConfig;
use makuhime_core::engine::Engine;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

/// 悬浮窗最近一次的位置与尺寸（逻辑像素）。
#[derive(Debug, Clone, Copy)]
struct OverlayGeometry {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

/// 桌面壳运行时状态。
///
/// 悬浮窗的穿透 / 尺寸 / 位置都以配置文件为唯一真相：窗口操作只负责「应用」，
/// 配置由引擎统一写盘（这里拿着 Engine 句柄做防抖保存）。
pub struct DesktopState {
    /// 本地服务端口（供窗口注入）
    port: u16,
    /// 服务启动后填入，用于保存窗口几何信息
    engine: Mutex<Option<Engine>>,
    /// 悬浮窗当前几何信息（拖动/缩放时更新）
    geometry: Mutex<OverlayGeometry>,
    /// 几何变化序号，用于防抖：只有最后一轮变化才落盘
    geometry_seq: AtomicU64,
}

fn overlay_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("overlay")
}

/// 更新悬浮窗几何信息，并在停止变化 400ms 后写回配置（拖动时会连续触发事件）。
fn remember_geometry(app: &AppHandle, change: impl FnOnce(&mut OverlayGeometry)) {
    let state = app.state::<DesktopState>();
    let snapshot = {
        let mut geometry = state.geometry.lock().unwrap();
        change(&mut geometry);
        *geometry
    };
    let seq = state.geometry_seq.fetch_add(1, Ordering::Relaxed) + 1;
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(400)).await;
        let state = handle.state::<DesktopState>();
        if state.geometry_seq.load(Ordering::Relaxed) != seq {
            return; // 还有更新的移动事件，让它来落盘
        }
        let engine = { state.engine.lock().unwrap().clone() };
        let Some(engine) = engine else {
            return;
        };
        let mut config = engine.config().await;
        config
            .overlay
            .apply_geometry(snapshot.x, snapshot.y, snapshot.width, snapshot.height);
        if let Err(err) = engine.update_config(config).await {
            tracing::warn!(%err, "保存悬浮窗位置失败");
        }
    });
}

#[tauri::command]
fn overlay_visible(app: AppHandle) -> bool {
    overlay_window(&app)
        .map(|window| window.is_visible().unwrap_or(false))
        .unwrap_or(false)
}

#[tauri::command]
async fn toggle_overlay(app: AppHandle, room_id: Option<u64>) -> Result<bool, String> {
    if let Some(window) = overlay_window(&app) {
        let visible = window.is_visible().unwrap_or(false);
        if visible {
            window.hide().map_err(|e| e.to_string())?;
            return Ok(false);
        }
        window.show().map_err(|e| e.to_string())?;
        let _ = window.set_focus();
        return Ok(true);
    }
    create_overlay(&app, room_id)?;
    Ok(true)
}

#[tauri::command]
async fn set_click_through(app: AppHandle, enabled: bool) -> Result<bool, String> {
    if let Some(window) = overlay_window(&app) {
        window
            .set_ignore_cursor_events(enabled)
            .map_err(|e| e.to_string())?;
    }
    Ok(enabled)
}

#[tauri::command]
async fn set_overlay_size(app: AppHandle, width: u32, height: u32) -> Result<(), String> {
    if let Some(window) = overlay_window(&app) {
        window
            .set_size(tauri::LogicalSize::new(width, height))
            .map_err(|e| e.to_string())?;
        // 记住新尺寸：之后拖动窗口时按这个尺寸落盘（尺寸本身由页面写配置）
        let state = app.state::<DesktopState>();
        let mut geometry = state.geometry.lock().unwrap();
        geometry.width = width.max(240);
        geometry.height = height.max(160);
    }
    Ok(())
}

#[tauri::command]
async fn focus_main(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        window.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn close_overlay(app: AppHandle) -> Result<(), String> {
    if let Some(window) = overlay_window(&app) {
        window.close().map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn create_main(app: &AppHandle, port: u16) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("maku-hime · 幕姬")
        .inner_size(1180.0, 768.0)
        .min_inner_size(880.0, 560.0)
        .center()
        .initialization_script(&format!("window.__MAKU_HIME_PORT__ = {port};"))
        .build()?;
    let _ = window.set_focus();
    Ok(())
}

fn create_overlay(app: &AppHandle, room_id: Option<u64>) -> Result<(), String> {
    let config = AppConfig::load();
    let state = app.state::<DesktopState>();
    // 穿透状态直接读配置（唯一真相），不再额外维护一份内存状态
    let click_through = config.overlay.click_through;
    // 记录初始几何信息，之后窗口移动/缩放事件会在此基础上更新
    {
        let mut geometry = state.geometry.lock().unwrap();
        *geometry = OverlayGeometry {
            x: config.overlay.x,
            y: config.overlay.y,
            width: config.overlay.width,
            height: config.overlay.height,
        };
    }

    let window = WebviewWindowBuilder::new(app, "overlay", WebviewUrl::App("obs.html".into()))
        .title("maku-hime · 弹幕墙")
        .inner_size(config.overlay.width as f64, config.overlay.height as f64)
        .position(config.overlay.x as f64, config.overlay.y as f64)
        .min_inner_size(240.0, 160.0)
        .decorations(false)
        .transparent(true)
        .always_on_top(config.overlay.always_on_top)
        .skip_taskbar(true)
        .resizable(true)
        .shadow(false)
        .focused(false)
        .initialization_script(&format!(
            "window.__MAKU_HIME_PORT__ = {}; window.__MAKU_HIME_ROOM__ = {};",
            state.port,
            room_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "null".into())
        ))
        .build()
        .map_err(|e| e.to_string())?;

    match window.set_ignore_cursor_events(click_through) {
        Ok(()) => tracing::info!(click_through, "悬浮窗穿透设置已应用"),
        Err(err) => tracing::warn!(%err, "当前合成器不支持点击穿透，悬浮窗将作为普通置顶窗"),
    }
    let _ = window.set_always_on_top(config.overlay.always_on_top);

    // 拖动/缩放后把位置与尺寸写回配置；事件很密集，交给 remember_geometry 防抖
    let window_for_events = window.clone();
    window.on_window_event(move |event| {
        let app = window_for_events.app_handle();
        let scale = window_for_events.scale_factor().unwrap_or(1.0);
        match event {
            WindowEvent::Moved(position) => {
                let logical = position.to_logical::<f64>(scale);
                remember_geometry(app, |geometry| {
                    geometry.x = logical.x.round() as i32;
                    geometry.y = logical.y.round() as i32;
                });
            }
            WindowEvent::Resized(size) => {
                let logical = size.to_logical::<f64>(scale);
                remember_geometry(app, |geometry| {
                    geometry.width = (logical.width.round() as u32).max(240);
                    geometry.height = (logical.height.round() as u32).max(160);
                });
            }
            _ => {}
        }
    });
    Ok(())
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示控制面板", true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "overlay", "打开 / 关闭悬浮窗", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 maku-hime", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &toggle, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("maku-hime · 幕姬")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "overlay" => {
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = toggle_overlay(handle, None).await;
                });
            }
            "quit" => app.exit(0),
            _ => {}
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

fn resolve_web_dir(app: &AppHandle) -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("MAKUHIME_WEB_DIR") {
        let dir = PathBuf::from(dir);
        if dir.join("index.html").is_file() {
            return Some(dir);
        }
    }
    if let Ok(resource) = app.path().resource_dir() {
        let candidate = resource.join("dist");
        if candidate.join("index.html").is_file() {
            return Some(candidate);
        }
    }
    makuhime_server::assets::resolve_web_dir()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    tauri::Builder::default()
        // 单实例：第二次启动只聚焦已有窗口（带 --overlay 时顺便打开悬浮窗），
        // 避免两个进程各占一个端口、各订阅一份弹幕。
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            tracing::info!("检测到已运行的实例，聚焦主窗口");
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
            if argv.iter().any(|arg| arg == "--overlay") {
                if let Err(err) = create_overlay(app, None) {
                    tracing::warn!(%err, "为第二个实例打开悬浮窗失败");
                }
            }
        }))
        .invoke_handler(tauri::generate_handler![
            overlay_visible,
            toggle_overlay,
            set_click_through,
            set_overlay_size,
            focus_main,
            close_overlay
        ])
        .setup(|app| {
            let config = AppConfig::load();
            let host = config.server.host.clone();
            let port = config.server.port;

            // 先同步占住端口，这样窗口里的前端一启动就知道该连哪个端口
            let (listener, addr) = makuhime_server::bind_sync(&host, port)?;
            let actual_port = addr.port();

            app.manage(DesktopState {
                port: actual_port,
                engine: Mutex::new(None),
                geometry: Mutex::new(OverlayGeometry {
                    x: config.overlay.x,
                    y: config.overlay.y,
                    width: config.overlay.width,
                    height: config.overlay.height,
                }),
                geometry_seq: AtomicU64::new(0),
            });

            create_main(app.handle(), actual_port)?;

            // 启动即开悬浮窗：maku-hime --overlay 或 MAKUHIME_OPEN_OVERLAY=1
            let want_overlay = std::env::args().any(|arg| arg == "--overlay")
                || std::env::var_os("MAKUHIME_OPEN_OVERLAY").is_some();
            if want_overlay {
                if let Err(err) = create_overlay(app.handle(), None) {
                    tracing::warn!(%err, "悬浮窗创建失败");
                }
            }

            // 托盘在缺少 appindicator 的桌面上会失败，失败不影响主流程
            if let Err(err) = build_tray(app.handle()) {
                tracing::warn!(%err, "托盘图标创建失败，跳过");
            }

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                match Engine::start(config).await {
                    Ok(engine) => {
                        // 把引擎交给桌面壳，用于保存悬浮窗位置等窗口相关的配置
                        if let Some(state) = handle.try_state::<DesktopState>() {
                            *state.engine.lock().unwrap() = Some(engine.clone());
                        }
                        let web_dir = resolve_web_dir(&handle);
                        match makuhime_server::serve_listener(listener, engine, web_dir).await {
                            Ok(addr) => tracing::info!(%addr, "本地服务已启动"),
                            Err(err) => tracing::error!(%err, "本地服务启动失败"),
                        }
                    }
                    Err(err) => tracing::error!(%err, "引擎启动失败"),
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("maku-hime 启动失败");
}
