//! 直播间信息与弹幕服务器发现。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::http::HttpClient;

/// 直播间基础信息（能匿名拿到的部分）。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct RoomInfo {
    pub room_id: u64,
    pub short_id: u64,
    /// 主播 uid
    pub uid: u64,
    pub uname: String,
    pub title: String,
    /// 小分区
    pub area: String,
    /// 大分区
    pub parent_area: String,
    /// 0 未开播 / 1 直播中 / 2 轮播
    pub live_status: i64,
    /// 人气值（热度，不是在线人数）
    pub popularity: u64,
    /// 粉丝数
    pub attention: u64,
    pub cover: String,
    pub keyframe: String,
    pub live_time: String,
    pub description: String,
}

impl RoomInfo {
    pub fn is_live(&self) -> bool {
        self.live_status == 1
    }

    pub fn status_text(&self) -> &'static str {
        match self.live_status {
            1 => "直播中",
            2 => "轮播中",
            _ => "未开播",
        }
    }
}

/// 弹幕服务器连接信息。
#[derive(Debug, Clone, Default)]
pub struct DanmuInfo {
    pub token: String,
    pub hosts: Vec<String>,
}

/// 弹幕服务器默认 host（拿不到 token 时兜底）。
pub const DEFAULT_DANMU_HOST: &str = "broadcastlv.chat.bilibili.com";

/// 把短号 / 真实号统一成真实房间号。
pub async fn resolve_room_id(http: &HttpClient, rid: u64) -> Result<u64> {
    let url = format!("https://api.live.bilibili.com/room/v1/Room/room_init?id={rid}");
    match http.api(&url).await {
        Ok(value) => {
            let room_id = value
                .get("data")
                .and_then(|d| d.get("room_id"))
                .and_then(Value::as_u64)
                .unwrap_or(rid);
            Ok(if room_id == 0 { rid } else { room_id })
        }
        Err(err) => {
            tracing::debug!(%err, rid, "room_init 失败，按原样使用房间号");
            Ok(rid)
        }
    }
}

/// 拉取直播间信息：优先 getRoomBaseInfo（字段全，含粉丝数/分区），失败退回 get_info。
pub async fn fetch_room_info(http: &HttpClient, rid: u64) -> Result<RoomInfo> {
    let room_id = resolve_room_id(http, rid).await?;
    let base_url = format!(
        "https://api.live.bilibili.com/xlive/web-room/v1/index/getRoomBaseInfo?req_biz=web_room_componet&room_ids={room_id}&from=0"
    );
    if let Ok(value) = http.api(&base_url).await
        && let Some(info) = parse_room_base(&value, room_id)
    {
        return Ok(info);
    }
    fetch_room_info_legacy(http, room_id).await
}

fn parse_room_base(value: &Value, fallback_id: u64) -> Option<RoomInfo> {
    let by_id = value.get("data")?.get("by_room_ids")?.as_object()?;
    let entry = by_id
        .get(&fallback_id.to_string())
        .or_else(|| by_id.values().next())?;
    Some(RoomInfo {
        room_id: entry
            .get("room_id")
            .and_then(Value::as_u64)
            .unwrap_or(fallback_id),
        short_id: entry.get("short_id").and_then(Value::as_u64).unwrap_or(0),
        uid: entry.get("uid").and_then(Value::as_u64).unwrap_or(0),
        uname: string_field(entry, "uname"),
        title: string_field(entry, "title"),
        area: string_field(entry, "area_name"),
        parent_area: string_field(entry, "parent_area_name"),
        live_status: entry
            .get("live_status")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        popularity: entry.get("online").and_then(Value::as_u64).unwrap_or(0),
        attention: entry.get("attention").and_then(Value::as_u64).unwrap_or(0),
        cover: string_field(entry, "cover"),
        keyframe: string_field(entry, "keyframe"),
        live_time: string_field(entry, "live_time"),
        description: string_field(entry, "description"),
    })
}

async fn fetch_room_info_legacy(http: &HttpClient, room_id: u64) -> Result<RoomInfo> {
    let url = format!("https://api.live.bilibili.com/room/v1/Room/get_info?room_id={room_id}");
    let value = http.api(&url).await?;
    let data = value
        .get("data")
        .ok_or_else(|| Error::other("get_info 返回缺少 data"))?;
    Ok(RoomInfo {
        room_id: data
            .get("room_id")
            .and_then(Value::as_u64)
            .unwrap_or(room_id),
        short_id: data.get("short_id").and_then(Value::as_u64).unwrap_or(0),
        uid: data.get("uid").and_then(Value::as_u64).unwrap_or(0),
        uname: String::new(),
        title: string_field(data, "title"),
        area: string_field(data, "area_name"),
        parent_area: string_field(data, "parent_area_name"),
        live_status: data.get("live_status").and_then(Value::as_i64).unwrap_or(0),
        popularity: data.get("online").and_then(Value::as_u64).unwrap_or(0),
        attention: data.get("attention").and_then(Value::as_u64).unwrap_or(0),
        cover: string_field(data, "user_cover"),
        keyframe: string_field(data, "keyframe"),
        live_time: string_field(data, "live_time"),
        description: string_field(data, "description"),
    })
}

/// 当前在线人数（官方高能榜的 onlineNum）与官方缩写文本。
pub async fn fetch_online(
    http: &HttpClient,
    room_id: u64,
    anchor_uid: u64,
) -> Result<(Option<u64>, String)> {
    let url = format!(
        "https://api.live.bilibili.com/xlive/general-interface/v1/rank/getOnlineGoldRank?roomId={room_id}&ruid={anchor_uid}&page=1&pageSize=1"
    );
    let value = http.api(&url).await?;
    let data = value.get("data").unwrap_or(&Value::Null);
    let count = data
        .get("onlineNum")
        .and_then(Value::as_u64)
        .or_else(|| {
            data.get("onlineNumText")
                .and_then(Value::as_str)
                .and_then(crate::model::parse_short_count)
        })
        // 0 视为「没取到」，免得面板上出现「在线 0」
        .filter(|value| *value > 0);
    let text = data
        .get("onlineNumText")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Ok((count, text))
}

/// getDanmuInfo 需要 WBI 签名；失败时退回默认 host + 空 token（仍然能收到公开弹幕）。
pub async fn fetch_danmu_info(http: &HttpClient, room_id: u64) -> Result<DanmuInfo> {
    let unsigned = format!(
        "https://api.live.bilibili.com/xlive/web-room/v1/index/getDanmuInfo?id={room_id}&type=0"
    );
    let signed = match http.wbi_sign(&unsigned).await {
        Ok(signed) => signed,
        Err(err) => {
            tracing::warn!(%err, room_id, "WBI 签名失败，尝试未签名请求");
            unsigned
        }
    };

    match http.api(&signed).await {
        Ok(value) => {
            let data = value.get("data").cloned().unwrap_or(Value::Null);
            let token = data
                .get("token")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let hosts: Vec<String> = data
                .get("host_list")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(|h| h.get("host").and_then(Value::as_str))
                        .filter(|host| !host.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            if hosts.is_empty() {
                return Ok(DanmuInfo {
                    token,
                    hosts: vec![DEFAULT_DANMU_HOST.to_string()],
                });
            }
            Ok(DanmuInfo { token, hosts })
        }
        Err(err) => {
            tracing::warn!(%err, room_id, "getDanmuInfo 失败，使用默认弹幕服务器");
            Ok(DanmuInfo {
                token: String::new(),
                hosts: vec![DEFAULT_DANMU_HOST.to_string()],
            })
        }
    }
}

fn string_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE_INFO: &str = r#"{
        "code": 0,
        "data": {
            "by_room_ids": {
                "7734200": {
                    "room_id": 7734200, "short_id": 6, "uid": 50329118,
                    "title": "【直播】TES vs G2", "area_name": "游戏赛事",
                    "parent_area_name": "赛事", "live_status": 1,
                    "online": 144530136, "attention": 17099844,
                    "uname": "哔哩哔哩英雄联盟赛事",
                    "cover": "https://i0.hdslb.com/bfs/live/a.jpg",
                    "live_time": "2026-10-15 15:50:15"
                }
            }
        }
    }"#;

    #[test]
    fn parses_room_base_info() {
        let value: Value = serde_json::from_str(BASE_INFO).unwrap();
        let info = parse_room_base(&value, 7734200).unwrap();
        assert_eq!(info.room_id, 7734200);
        assert_eq!(info.uname, "哔哩哔哩英雄联盟赛事");
        assert_eq!(info.area, "游戏赛事");
        assert!(info.is_live());
        assert_eq!(info.popularity, 144_530_136);
        assert_eq!(info.attention, 17_099_844);
    }

    #[test]
    fn parsed_info_survives_json_roundtrip() {
        let value: Value = serde_json::from_str(BASE_INFO).unwrap();
        let info = parse_room_base(&value, 7734200).unwrap();
        let text = serde_json::to_string(&info).unwrap();
        let back: RoomInfo = serde_json::from_str(&text).unwrap();
        assert_eq!(back, info);
    }
}
