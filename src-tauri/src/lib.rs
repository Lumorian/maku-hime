//! maku-hime 桌面壳：启动引擎与本地服务，管理主窗口和可穿屏悬浮窗。
//!
//! 数据流与浏览器面板完全一致：窗口只是个 WebView，接口都走 127.0.0.1 上的
//! HTTP/WebSocket 服务；桌面壳额外提供窗口操作（穿透、置顶、缩放、托盘）。

use std::path::PathBuf;
use std::sync::Mutex;

use makuhime_core::config::AppConfig;
use makuhime_core::engine::Engine;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

/// 桌面壳运行时状态。
pub struct DesktopState {
    /// 本地服务端口（供窗口注入）
    port: u16,
    /// 悬浮窗当前是否穿透
    click_through: Mutex<bool>,
}

fn overlay_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("overlay")
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
    let state = app.state::<DesktopState>();
    *state.click_through.lock().unwrap() = enabled;
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
    let click_through = *state.click_through.lock().unwrap();

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
                click_through: Mutex::new(config.overlay.click_through),
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
