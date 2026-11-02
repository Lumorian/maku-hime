//! niri 窗口布局集成（Linux / Wayland）。
//!
//! niri 是平铺合成器：普通窗口会被塞进布局，客户端既不能定位也不能自由拖动。
//! bili-dm 用 layer-shell 解决；这里走 niri 自己的 IPC：
//! - `MoveWindowToFloating` 把悬浮窗移出平铺布局；
//! - `MoveFloatingWindow` / `SetWindowWidth` / `SetWindowHeight` 恢复位置与尺寸；
//! - `Windows` 查询读取实际位置（niri 报告的浮动坐标带一个固定标题栏偏移，
//!   创建时校准一次，保存时再减掉）。
//!
//! 只使用标准库与 serde_json；niri 不在时 [available] 返回 false，调用方完全跳过。

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// niri 管理的窗口快照。
#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub id: u64,
    pub pid: u32,
    pub title: String,
    pub is_floating: bool,
    /// tile_pos_in_workspace_view 是否存在；刚映射 / 隐藏时为 false
    pub positioned: bool,
    /// niri 报告的窗口左上角；浮动窗口的 y 含固定标题栏偏移
    pub x: f64,
    pub y: f64,
    pub width: u32,
    pub height: u32,
}

/// 悬浮窗浮动化 + 几何恢复后的结果。
#[derive(Debug, Clone)]
pub struct Prepared {
    pub id: u64,
    /// 报告坐标 - 实际坐标（niri 的浮动窗口 y 通常为 +42）
    pub offset_x: f64,
    pub offset_y: f64,
}

pub fn available() -> bool {
    socket_path().is_some()
}

/// 找到属于本进程、指定标题的窗口。
pub fn window_for(pid: u32, title: &str) -> Option<WindowInfo> {
    windows()?
        .into_iter()
        .find(|window| window.pid == pid && window.title == title)
}

pub fn window_by_id(id: u64) -> Option<WindowInfo> {
    windows()?.into_iter().find(|window| window.id == id)
}

/// 把悬浮窗移出平铺布局，并按配置恢复尺寸/位置。
///
/// 窗口刚创建时可能还没被 niri 映射，这里最多等 3 秒。
pub fn prepare(pid: u32, title: &str, x: i32, y: i32, width: u32, height: u32) -> Option<Prepared> {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut info = loop {
        if let Some(info) = window_for(pid, title) {
            break info;
        }
        if Instant::now() >= deadline {
            tracing::warn!(pid, title, "niri 中没有找到悬浮窗，跳过浮动化");
            return None;
        }
        std::thread::sleep(Duration::from_millis(80));
    };

    if !info.is_floating {
        if !float(info.id) {
            tracing::warn!(id = info.id, "niri 浮动化请求失败");
            return None;
        }
        // 等合成器应用浮动状态，否则后续 resize/move 仍按平铺窗口处理
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(50));
            info = window_by_id(info.id)?;
            if info.is_floating {
                break;
            }
        }
        if !info.is_floating {
            tracing::warn!(id = info.id, "niri 没有把窗口切到浮动布局");
            return None;
        }
    }

    let resized = resize(info.id, width, height);
    let moved = move_to(info.id, x as f64, y as f64);
    if !resized || !moved {
        tracing::warn!(
            id = info.id,
            resized,
            moved,
            "niri 几何恢复不完整，后续仍可拖动"
        );
    }

    // 浮动后 niri 需要一两帧才给出位置；没有位置时先不校准偏移，保存几何会再查
    let mut info = window_by_id(info.id)?;
    for _ in 0..10 {
        if info.positioned {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
        info = window_by_id(info.id)?;
    }
    Some(Prepared {
        id: info.id,
        offset_x: if moved && info.positioned {
            info.x - x as f64
        } else {
            0.0
        },
        offset_y: if moved && info.positioned {
            info.y - y as f64
        } else {
            0.0
        },
    })
}

pub fn float(id: u64) -> bool {
    action(json!({ "MoveWindowToFloating": { "id": id } }))
}

pub fn move_to(id: u64, x: f64, y: f64) -> bool {
    action(json!({
        "MoveFloatingWindow": {
            "id": id,
            "x": { "SetFixed": x },
            "y": { "SetFixed": y },
        }
    }))
}

pub fn resize(id: u64, width: u32, height: u32) -> bool {
    let width_ok = action(json!({
        "SetWindowWidth": { "id": id, "change": { "SetFixed": width } }
    }));
    let height_ok = action(json!({
        "SetWindowHeight": { "id": id, "change": { "SetFixed": height } }
    }));
    width_ok && height_ok
}

/// 读取窗口实际几何（扣掉 niri 的坐标偏移）。
pub fn geometry(id: u64, offset_x: f64, offset_y: f64) -> Option<(i32, i32, u32, u32)> {
    let info = window_by_id(id)?;
    if !info.positioned {
        return None;
    }
    Some((
        (info.x - offset_x).round() as i32,
        (info.y - offset_y).round() as i32,
        info.width.max(1),
        info.height.max(1),
    ))
}

fn windows() -> Option<Vec<WindowInfo>> {
    let value = request(json!("Windows"))?;
    let list = value.pointer("/Ok/Windows")?.as_array()?;
    let mut out = Vec::with_capacity(list.len());
    for entry in list {
        let Some(id) = entry.get("id").and_then(Value::as_u64) else {
            continue;
        };
        // 刚打开 / 不可见 / 正在切换布局时 tile_pos_in_workspace_view 可能是 null。
        // 浮动化前的窗口也会是 null，不能因此跳过——否则永远找不到要浮动的悬浮窗。
        let pos = entry
            .pointer("/layout/tile_pos_in_workspace_view")
            .and_then(Value::as_array);
        let x = pos
            .and_then(|pos| pos.first())
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let y = pos
            .and_then(|pos| pos.get(1))
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let size = entry
            .pointer("/layout/window_size")
            .and_then(Value::as_array);
        let width = size
            .and_then(|size| size.first())
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        let height = size
            .and_then(|size| size.get(1))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        out.push(WindowInfo {
            id,
            pid: entry.get("pid").and_then(Value::as_u64).unwrap_or(0) as u32,
            title: entry
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            is_floating: entry
                .get("is_floating")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            positioned: pos.is_some(),
            x,
            y,
            width,
            height,
        });
    }
    Some(out)
}

fn action(value: Value) -> bool {
    request(json!({ "Action": value }))
        .map(|response| response.get("Ok").is_some())
        .unwrap_or(false)
}

fn request(request: Value) -> Option<Value> {
    let path = socket_path()?;
    let mut stream = UnixStream::connect(path).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_millis(800)))
        .ok()?;
    stream
        .set_write_timeout(Some(Duration::from_millis(800)))
        .ok()?;
    let mut line = serde_json::to_vec(&request).ok()?;
    line.push(b'\n');
    stream.write_all(&line).ok()?;
    let mut response = String::new();
    BufReader::new(stream).read_line(&mut response).ok()?;
    serde_json::from_str(&response).ok()
}

/// NIRI_SOCKET 优先；没有时在 XDG_RUNTIME_DIR 里找 niri.*.sock。
fn socket_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("NIRI_SOCKET") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Some(path);
        }
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            let uid = std::fs::metadata("/proc/self").ok()?.uid();
            Some(PathBuf::from(format!("/run/user/{uid}")))
        })?;
    std::fs::read_dir(runtime)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with("niri.") && name.ends_with(".sock"))
                .unwrap_or(false)
        })
}
