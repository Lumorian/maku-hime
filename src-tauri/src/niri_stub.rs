//! 非 Linux 平台的 niri 占位：接口与 `niri` 模块一致，但永远不可用。

#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub id: u64,
    pub pid: u32,
    pub title: String,
    pub is_floating: bool,
    pub positioned: bool,
    pub x: f64,
    pub y: f64,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub struct Prepared {
    pub id: u64,
    pub offset_x: f64,
    pub offset_y: f64,
}

pub fn available() -> bool {
    false
}

pub fn window_for(_pid: u32, _title: &str) -> Option<WindowInfo> {
    None
}

pub fn window_by_id(_id: u64) -> Option<WindowInfo> {
    None
}

pub fn prepare(
    _pid: u32,
    _title: &str,
    _x: i32,
    _y: i32,
    _width: u32,
    _height: u32,
) -> Option<Prepared> {
    None
}

pub fn float(_id: u64) -> bool {
    false
}

pub fn move_to(_id: u64, _x: f64, _y: f64) -> bool {
    false
}

pub fn resize(_id: u64, _width: u32, _height: u32) -> bool {
    false
}

pub fn geometry(_id: u64, _offset_x: f64, _offset_y: f64) -> Option<(i32, i32, u32, u32)> {
    None
}
