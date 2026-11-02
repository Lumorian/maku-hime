//! maku-hime 桌面壳：启动引擎与本地服务，管理主窗口和可穿屏悬浮窗。
//!
//! 数据流与浏览器面板完全一致：窗口只是个 WebView，接口都走 127.0.0.1 上的
//! HTTP/WebSocket 服务；桌面壳额外提供窗口操作（穿透、置顶、缩放、托盘）。

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use makuhime_core::config::AppConfig;
use makuhime_core::engine::Engine;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{
    AppHandle, LogicalSize, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

#[cfg(target_os = "linux")]
mod niri;
#[cfg(not(target_os = "linux"))]
#[path = "niri_stub.rs"]
mod niri;

/// 悬浮窗标题；niri IPC 靠 pid + 标题定位窗口。
const OVERLAY_TITLE: &str = "maku-hime · 弹幕墙";

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
    /// niri 浮动窗口 id；非 niri / 浮动化失败时为 None
    niri_window: Mutex<Option<u64>>,
    /// niri 报告坐标与实际坐标的差（浮动窗口 y 常见 +42）
    niri_offset: Mutex<(f64, f64)>,
    /// niri 浮动化完成前，平铺布局会改写窗口几何，不能写回配置
    geometry_ready: AtomicBool,
    /// 悬浮窗顶部控制条高度（页面测量后上报）；锁定态只保留这段可点。
    /// 默认 32：页面还没上报时也能保住一个可点的条，不至于彻底锁死。
    header_height: AtomicU32,
}

/// 只让悬浮窗顶部控制条可点，其余区域点击穿透（Wayland 输入区域）。
///
/// GTK3 的 Wayland 后端没实现输入区域，这里直接调 wl_surface.set_input_region
/// （实现见 input_region.c）。height <= 0 表示清空限制，整窗可交互。
#[cfg(target_os = "linux")]
fn apply_overlay_input_region(title: &str, height: u32) -> bool {
    use std::ffi::CString;
    unsafe extern "C" {
        fn maku_set_input_region_for_title(title: *const std::ffi::c_char, height: i32) -> i32;
    }
    let Ok(c_title) = CString::new(title) else {
        return false;
    };
    let height = i32::try_from(height).unwrap_or(i32::MAX);
    // SAFETY: 标题是合法的 NUL 结尾字符串，指针只在调用期间使用
    unsafe { maku_set_input_region_for_title(c_title.as_ptr(), height) == 1 }
}

#[cfg(not(target_os = "linux"))]
fn apply_overlay_input_region(_title: &str, _height: u32) -> bool {
    false
}

fn overlay_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("overlay")
}

/// 更新悬浮窗几何信息，并在停止变化 400ms 后写回配置（拖动时会连续触发事件）。
fn remember_geometry(app: &AppHandle, change: impl FnOnce(&mut OverlayGeometry)) {
    let state = app.state::<DesktopState>();
    if !state.geometry_ready.load(Ordering::Relaxed) {
        return;
    }
    // niri 下窗口位置只能通过 IPC 读，Moved 事件里的坐标不可靠
    if state.niri_window.lock().unwrap().is_some() {
        return;
    }
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
        persist_geometry(&handle, snapshot).await;
    });
}

/// 把一份几何信息写回配置（拖动/缩放结束、关闭悬浮窗时调用）。
async fn persist_geometry(app: &AppHandle, geometry: OverlayGeometry) {
    let engine = {
        let state = app.state::<DesktopState>();
        state.engine.lock().unwrap().clone()
    };
    let Some(engine) = engine else {
        return;
    };
    let mut config = engine.config().await;
    config
        .overlay
        .apply_geometry(geometry.x, geometry.y, geometry.width, geometry.height);
    if let Err(err) = engine.update_config(config).await {
        tracing::warn!(%err, "保存悬浮窗位置失败");
    }
}

/// 把新创建的悬浮窗交给 niri 浮动化，并恢复配置里的位置/尺寸。
fn start_niri_overlay(
    app: &AppHandle,
    window: WebviewWindow,
    overlay: &makuhime_core::config::OverlayConfig,
) {
    if !niri::available() {
        return;
    }
    let x = overlay.x;
    let y = overlay.y;
    let width = overlay.width;
    let height = overlay.height;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let pid = std::process::id();
        let title = OVERLAY_TITLE.to_string();
        let prepared =
            tokio::task::spawn_blocking(move || niri::prepare(pid, &title, x, y, width, height))
                .await
                .ok()
                .flatten();
        let state = app.state::<DesktopState>();
        match prepared {
            Some(prepared) => {
                *state.niri_window.lock().unwrap() = Some(prepared.id);
                *state.niri_offset.lock().unwrap() = (prepared.offset_x, prepared.offset_y);
                *state.geometry.lock().unwrap() = OverlayGeometry {
                    x,
                    y,
                    width,
                    height,
                };
                state.geometry_ready.store(true, Ordering::Relaxed);
                tracing::info!(
                    id = prepared.id,
                    offset_x = prepared.offset_x,
                    offset_y = prepared.offset_y,
                    "悬浮窗已浮动化（niri）"
                );
                let _ = window.set_always_on_top(true);

                // niri 在窗口首次映射 / 聚焦后可能把浮动窗放到指针附近（实测会晚 1~3 秒），
                // 这里在接下来 5 秒内持续把它拉回配置位置；用户一旦拖动/缩放就通过
                // geometry 变化检测到并停止纠正。
                let id = prepared.id;
                for _ in 0..20 {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    let state = app.state::<DesktopState>();
                    if *state.niri_window.lock().unwrap() != Some(id) {
                        break;
                    }
                    let current = *state.geometry.lock().unwrap();
                    if current.x != x
                        || current.y != y
                        || current.width != width
                        || current.height != height
                    {
                        break;
                    }
                    let (offset_x, offset_y) = *state.niri_offset.lock().unwrap();
                    if let Some((actual_x, actual_y, actual_w, actual_h)) =
                        niri::geometry(id, offset_x, offset_y)
                    {
                        if (actual_x - x).abs() > 2 || (actual_y - y).abs() > 2 {
                            let _ = niri::move_to(id, x as f64, y as f64);
                        }
                        if actual_w != width || actual_h != height {
                            let _ = niri::resize(id, width, height);
                        }
                    }
                }
            }
            None => tracing::warn!(
                "niri 浮动化失败：悬浮窗会留在平铺布局里，位置保存暂停（可重开悬浮窗重试）"
            ),
        }
    });
}

/// 托盘里的「解锁 / 锁定悬浮窗」：锁定是整窗穿透，顶栏隐藏时用这个兜底。
async fn toggle_overlay_lock(app: AppHandle) {
    let state = app.state::<DesktopState>();
    let engine = { state.engine.lock().unwrap().clone() };
    let Some(engine) = engine else {
        return;
    };
    let header = state.header_height.load(Ordering::Relaxed);
    let mut config = engine.config().await;
    let locked = !config.overlay.click_through;
    config.overlay.click_through = locked;
    config.overlay.locked = locked;
    if let Err(err) = engine.update_config(config).await {
        tracing::warn!(%err, "保存悬浮窗锁定状态失败");
        return;
    }
    apply_lock(&app, locked, header);
    tracing::info!(locked, header, "悬浮窗锁定状态已切换");
}

/// 应用锁定状态：锁定时只留顶部控制条可点（点它就能解锁），其余点击穿透。
///
/// 输入区域设不上（非 Wayland 会话 / 窗口还没建好）时退回整窗穿透，
/// 这样至少不会把下面的窗口挡住——代价是悬浮窗自己的按钮也点不到。
fn apply_lock(app: &AppHandle, locked: bool, header_height: u32) {
    let Some(window) = overlay_window(app) else {
        return;
    };
    if !locked {
        // 解锁：整窗可交互
        apply_overlay_input_region(OVERLAY_TITLE, 0);
        if let Err(err) = window.set_ignore_cursor_events(false) {
            tracing::warn!(%err, "解除悬浮窗穿透失败");
        }
        return;
    }
    if header_height > 0 {
        let applied = apply_overlay_input_region(OVERLAY_TITLE, header_height);
        tracing::info!(header_height, applied, "悬浮窗输入区域已设置");
        if applied {
            // 输入区域已经限制了可点范围，窗口本身不用再整体穿透
            let _ = window.set_ignore_cursor_events(false);
            return;
        }
    }
    tracing::warn!(header_height, "输入区域设置失败，退回整窗穿透");
    if let Err(err) = window.set_ignore_cursor_events(true) {
        tracing::warn!(%err, "当前合成器不支持点击穿透，悬浮窗将作为普通置顶窗");
    }
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
        // 锁定时窗口在往外放行输入，别把键盘焦点抢过来
        if !AppConfig::load().overlay.click_through {
            let _ = window.set_focus();
        }
        return Ok(true);
    }
    create_overlay(&app, room_id)?;
    Ok(true)
}

#[tauri::command]
async fn set_click_through(
    app: AppHandle,
    enabled: bool,
    header_height: Option<u32>,
) -> Result<bool, String> {
    let state = app.state::<DesktopState>();
    if let Some(height) = header_height {
        state.header_height.store(height, Ordering::Relaxed);
    }
    let header = state.header_height.load(Ordering::Relaxed);
    apply_lock(&app, enabled, header);
    tracing::info!(enabled, header, "悬浮窗点击穿透已设置");
    Ok(enabled)
}

/// 页面里拖动右下角时的实时尺寸；niri 下走 IPC，其它环境用窗口 API。
#[tauri::command]
async fn set_overlay_size(app: AppHandle, width: u32, height: u32) -> Result<(), String> {
    let width = width.max(280);
    let height = height.max(160);
    let state = app.state::<DesktopState>();
    {
        let mut geometry = state.geometry.lock().unwrap();
        geometry.width = width;
        geometry.height = height;
    }
    let niri_id = *state.niri_window.lock().unwrap();
    if let Some(id) = niri_id {
        if !niri::resize(id, width, height) {
            return Err("niri 调整悬浮窗尺寸失败".into());
        }
        return Ok(());
    }
    if let Some(window) = overlay_window(&app) {
        window
            .set_size(LogicalSize::new(width as f64, height as f64))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 页面挂载完成：niri 在新窗口首次映射后可能套用浮动默认位置，
/// 这里再按配置校正一次（这时窗口已经稳定，不会再被合成器覆盖）。
#[tauri::command]
async fn overlay_ready(app: AppHandle) -> Result<(), String> {
    let state = app.state::<DesktopState>();
    let niri_id = *state.niri_window.lock().unwrap();
    if let Some(id) = niri_id {
        let geometry = *state.geometry.lock().unwrap();
        let resized = niri::resize(id, geometry.width, geometry.height);
        let moved = niri::move_to(id, geometry.x as f64, geometry.y as f64);
        tracing::debug!(
            id,
            x = geometry.x,
            y = geometry.y,
            width = geometry.width,
            height = geometry.height,
            resized,
            moved,
            "页面就绪后校正 niri 几何"
        );
    }
    Ok(())
}

/// 开始拖动悬浮窗：移动交给窗口系统 / 合成器处理，niri 下轮询真实位置写回配置。
#[tauri::command]
async fn start_overlay_drag(app: AppHandle) -> Result<(), String> {
    let Some(window) = overlay_window(&app) else {
        return Ok(());
    };
    window.start_dragging().map_err(|err| err.to_string())?;

    let state = app.state::<DesktopState>();
    let Some(id) = *state.niri_window.lock().unwrap() else {
        // 其它环境由 Moved 事件防抖保存
        return Ok(());
    };
    let (offset_x, offset_y) = *state.niri_offset.lock().unwrap();
    let start = niri::geometry(id, offset_x, offset_y).map(|(x, y, _, _)| (x, y));
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // niri 不向客户端发 Moved；拖动结束后位置稳定 600ms 再写配置
        let mut last: Option<(i32, i32)> = None;
        let mut stable = 0;
        let mut moved = false;
        let mut idle = 0;
        for _ in 0..80 {
            tokio::time::sleep(Duration::from_millis(150)).await;
            let state = app.state::<DesktopState>();
            if *state.niri_window.lock().unwrap() != Some(id) {
                break;
            }
            let (offset_x, offset_y) = *state.niri_offset.lock().unwrap();
            let Some((x, y, _, _)) = niri::geometry(id, offset_x, offset_y) else {
                continue;
            };
            if Some((x, y)) == start {
                // 只是点了一下（没有拖动）时不要空轮询 12 秒
                if !moved {
                    idle += 1;
                    if idle >= 8 {
                        break;
                    }
                }
                continue;
            }
            moved = true;
            idle = 0;
            if last == Some((x, y)) {
                stable += 1;
                if stable >= 4 {
                    let _ = save_overlay_geometry(app.clone()).await;
                    break;
                }
            } else {
                last = Some((x, y));
                stable = 0;
            }
        }
    });
    Ok(())
}

/// 拖动 / 缩放结束后把真实几何写回配置。niri 的窗口位置只能从 IPC 读。
#[tauri::command]
async fn save_overlay_geometry(app: AppHandle) -> Result<(), String> {
    let state = app.state::<DesktopState>();
    let mut geometry = *state.geometry.lock().unwrap();
    let niri_id = *state.niri_window.lock().unwrap();
    if let Some(id) = niri_id {
        let (offset_x, offset_y) = *state.niri_offset.lock().unwrap();
        if let Some((x, y, width, height)) = niri::geometry(id, offset_x, offset_y) {
            geometry = OverlayGeometry {
                x,
                y,
                width,
                height,
            };
            *state.geometry.lock().unwrap() = geometry;
        }
    }
    tracing::debug!(
        x = geometry.x,
        y = geometry.y,
        width = geometry.width,
        height = geometry.height,
        niri = ?niri_id,
        "保存悬浮窗几何"
    );
    persist_geometry(&app, geometry).await;
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
    let _ = save_overlay_geometry(app.clone()).await;
    if let Some(window) = overlay_window(&app) {
        window.close().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 把访问令牌拼成 JS 字面量。
///
/// 令牌可能是 MAKUHIME_API_TOKEN 指定的任意字符串，直接塞进脚本会被引号/
/// 反斜杠截断（进而变成注入点），所以用 serde_json 序列化。
fn token_literal() -> String {
    serde_json::to_string(makuhime_server::auth::token()).unwrap_or_else(|_| "\"\"".to_string())
}

fn create_main(app: &AppHandle, port: u16) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("maku-hime · 幕姬")
        .inner_size(1180.0, 768.0)
        .min_inner_size(880.0, 560.0)
        .center()
        // 窗口来源是 tauri://localhost，拿不到服务下发的 Cookie，
        // 所以在这里把端口和 API 令牌交给页面。
        .initialization_script(format!(
            "window.__MAKU_HIME_PORT__ = {port}; window.__MAKU_HIME_TOKEN__ = {};",
            token_literal()
        ))
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
    // 新窗口：清掉上一扇窗的 niri 句柄；niri 浮动化完成前不保存几何
    *state.niri_window.lock().unwrap() = None;
    *state.niri_offset.lock().unwrap() = (0.0, 0.0);
    state.geometry_ready.store(false, Ordering::Relaxed);

    let window = WebviewWindowBuilder::new(app, "overlay", WebviewUrl::App("obs.html".into()))
        .title(OVERLAY_TITLE)
        .inner_size(config.overlay.width as f64, config.overlay.height as f64)
        .position(config.overlay.x as f64, config.overlay.y as f64)
        .min_inner_size(280.0, 160.0)
        .decorations(false)
        .transparent(true)
        .always_on_top(config.overlay.always_on_top)
        .skip_taskbar(true)
        .resizable(true)
        .shadow(false)
        .focused(false)
        .initialization_script(format!(
            "window.__MAKU_HIME_PORT__ = {}; window.__MAKU_HIME_ROOM__ = {}; window.__MAKU_HIME_TOKEN__ = {};",
            state.port,
            room_id
                .map(|id| id.to_string())
                .unwrap_or_else(|| "null".into()),
            token_literal()
        ))
        .build()
        .map_err(|e| e.to_string())?;

    // 先整窗穿透/可交互；页面挂载后会带上顶部控制条高度重新设输入区域
    match window.set_ignore_cursor_events(click_through) {
        Ok(()) => tracing::info!(click_through, "悬浮窗穿透设置已应用"),
        Err(err) => tracing::warn!(%err, "当前合成器不支持点击穿透，悬浮窗将作为普通置顶窗"),
    }
    let _ = window.set_always_on_top(config.overlay.always_on_top);

    // niri：普通窗口会被平铺布局接管，这里把它移出平铺并按配置恢复几何。
    // 失败时 geometry_ready 保持 false，避免把平铺布局给的尺寸写回配置。
    if niri::available() {
        start_niri_overlay(app, window.clone(), &config.overlay);
    } else {
        state.geometry_ready.store(true, Ordering::Relaxed);
    }

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
                    geometry.width = (logical.width.round() as u32).max(280);
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
    let lock = MenuItem::with_id(app, "lock", "解锁 / 锁定悬浮窗", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 maku-hime", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &toggle, &lock, &quit])?;

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
            "lock" => {
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    toggle_overlay_lock(handle).await;
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
/// 让 WebView 里访问 127.0.0.1 的请求绕开系统代理。
///
/// 弹幕抓取在 Rust 侧，已经统一 `no_proxy()`（见 `makuhime_core::http::client_builder`），
/// 不受系统代理影响。但面板自身跑在 WebView 里，WebKitGTK / libsoup 会读
/// `http_proxy` 之类的环境变量：用户开着 Clash 又切节点时，面板连本地 3939 的
/// HTTP / WebSocket 也可能被绕进代理而断线。这里只给回环地址补 no_proxy，
/// 不动用户的其它代理设置。
fn bypass_proxy_for_loopback() {
    // SAFETY: 在 Tauri / WebView 启动之前调用，进程里还没有别的线程在读环境变量。
    // （Rust 2024 把 set_var 标成 unsafe，就是要调用方保证这一点。）
    unsafe {
        let current = std::env::var("no_proxy")
            .or_else(|_| std::env::var("NO_PROXY"))
            .unwrap_or_default();
        let mut list = current.trim().to_string();
        for entry in ["127.0.0.1", "localhost", "::1"] {
            let present = list
                .split(',')
                .any(|item| item.trim().eq_ignore_ascii_case(entry));
            if present {
                continue;
            }
            if !list.is_empty() {
                list.push(',');
            }
            list.push_str(entry);
        }
        std::env::set_var("no_proxy", &list);
        std::env::set_var("NO_PROXY", &list);
    }
}

pub fn run() {
    // 必须在 WebView 起来之前：WebKit 的网络进程只在启动时读一次代理设置。
    bypass_proxy_for_loopback();
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
            if argv.iter().any(|arg| arg == "--overlay")
                && let Err(err) = create_overlay(app, None)
            {
                tracing::warn!(%err, "为第二个实例打开悬浮窗失败");
            }
        }))
        .invoke_handler(tauri::generate_handler![
            overlay_visible,
            toggle_overlay,
            set_click_through,
            set_overlay_size,
            overlay_ready,
            start_overlay_drag,
            save_overlay_geometry,
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
                niri_window: Mutex::new(None),
                niri_offset: Mutex::new((0.0, 0.0)),
                geometry_ready: AtomicBool::new(!niri::available()),
                header_height: AtomicU32::new(32),
            });

            create_main(app.handle(), actual_port)?;

            // 启动即开悬浮窗：maku-hime --overlay 或 MAKUHIME_OPEN_OVERLAY=1
            let want_overlay = std::env::args().any(|arg| arg == "--overlay")
                || std::env::var_os("MAKUHIME_OPEN_OVERLAY").is_some();
            if want_overlay && let Err(err) = create_overlay(app.handle(), None) {
                tracing::warn!(%err, "悬浮窗创建失败");
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
