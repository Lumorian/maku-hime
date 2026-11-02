//! 把 B 站原始的 JSON / protobuf 消息解析成归一化的 [LiveEvent]。
//!
//! 解析原则：
//! 1. 只依赖已经抓包验证过的字段，字段缺失就退化，不 panic；
//! 2. 新版 protobuf 消息（SEND_GIFT_V2 / INTERACT_WORD_V2）与旧版 JSON 消息都支持；
//! 3. 认不出的 cmd 返回 None，由上层统计，不污染弹幕墙。

use base64::Engine;
use serde_json::Value;

use crate::model::{
    ChatEmoticon, EnterKind, Event, Gift, GuardLevel, LiveEvent, Medal, User, color_hex,
};
use crate::pb;

/// 解析一条原始消息。用于 `DANMU_MSG`、`SEND_GIFT_V2` 之类带 cmd 的消息体。
pub fn parse(raw: &Value) -> Option<LiveEvent> {
    let cmd = raw.get("cmd")?.as_str()?;
    let cmd = cmd.split(':').next().unwrap_or(cmd);
    let event = match cmd {
        "DANMU_MSG" => parse_danmu(raw),
        "SEND_GIFT" => parse_gift_json(raw),
        "SEND_GIFT_V2" => parse_gift_v2(raw),
        "SUPER_CHAT_MESSAGE" => parse_super_chat(raw),
        "SUPER_CHAT_MESSAGE_DELETE" => parse_super_chat_delete(raw),
        "GUARD_BUY" => parse_guard_json(raw),
        "USER_TOAST_MSG" => parse_user_toast(raw, false),
        "USER_TOAST_MSG_V2" => parse_user_toast(raw, true),
        "INTERACT_WORD" => parse_interact_json(raw),
        "INTERACT_WORD_V2" => parse_interact_v2(raw),
        "LIKE_INFO_V3_CLICK" => parse_like_click(raw),
        "LIKE_INFO_V3_UPDATE" => parse_like_update(raw),
        "WATCHED_CHANGE" => parse_watched(raw),
        "ONLINE_RANK_COUNT" => parse_online_rank_count(raw),
        "ONLINE_RANK_V2" => parse_online_rank_count(raw),
        "ROOM_REAL_TIME_MESSAGE_UPDATE" => parse_room_update(raw),
        "LIVE" => Some(LiveEvent::LiveStatus { live_status: 1 }),
        "PREPARING" => Some(LiveEvent::LiveStatus { live_status: 0 }),
        _ => None,
    }?;
    Some(event)
}

/// 解析 {"cmd":..., "data":...} 形式的原始消息并包装成带元信息的 [Event]。
pub fn parse_event(raw: &Value, room_id: u64, fallback_ts: i64) -> Option<Event> {
    let body = parse(raw)?;
    let cmd = raw
        .get("cmd")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let ts = event_timestamp(raw).unwrap_or(fallback_ts);
    Some(Event {
        id: event_id(&cmd, raw, ts),
        ts,
        room_id,
        body,
    })
}

fn event_id(cmd: &str, raw: &Value, ts: i64) -> String {
    // 优先用协议里的稳定 id，保证重连/重复投递时能去重
    if let Some(id) = raw
        .get("data")
        .and_then(|data| data.get("id").or_else(|| data.get("msg_id")))
        .and_then(value_as_string)
        .filter(|id| !id.is_empty() && id != "0")
    {
        return format!("{cmd}-{id}");
    }
    if let Some(ct) = raw
        .get("info")
        .and_then(|info| info.get(9))
        .and_then(|v| v.get("ct"))
        .and_then(Value::as_str)
        .filter(|ct| !ct.is_empty())
    {
        return format!("{cmd}-{ct}-{ts}");
    }
    if let Some(uid) = raw
        .get("info")
        .and_then(|i| i.get(2))
        .and_then(|i| i.get(0))
        && let Some(uid) = value_as_u64(uid)
    {
        return format!("{cmd}-{uid}-{ts}");
    }
    if let Some(uid) = raw.get("data").and_then(|d| d.get("uid"))
        && let Some(uid) = value_as_u64(uid)
    {
        return format!("{cmd}-{uid}-{ts}");
    }
    format!("{cmd}-{ts}-{}", raw.to_string().len())
}

fn event_timestamp(raw: &Value) -> Option<i64> {
    // DANMU_MSG: info[0][4] 是毫秒时间戳
    if let Some(ts) = raw
        .get("info")
        .and_then(|info| info.get(0))
        .and_then(|meta| meta.get(4))
        .and_then(value_as_i64)
    {
        if ts > 1_000_000_000_000 {
            return Some(ts);
        }
        if ts > 1_000_000_000 {
            return Some(ts * 1000);
        }
    }
    for path in [
        ["data", "timestamp"].as_slice(),
        ["data", "start_time"].as_slice(),
        ["data", "ts"].as_slice(),
        ["data", "send_time"].as_slice(),
    ] {
        if let Some(ts) = get_path(raw, path).and_then(value_as_i64) {
            if ts > 1_000_000_000_000 {
                return Some(ts);
            }
            if ts > 1_000_000_000 {
                return Some(ts * 1000);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 聊天弹幕
// ---------------------------------------------------------------------------

fn parse_danmu(raw: &Value) -> Option<LiveEvent> {
    let info = raw.get("info")?.as_array()?;
    let text = info.get(1)?.as_str()?.to_string();
    let user_arr = info.get(2).and_then(Value::as_array);

    let uid = user_arr
        .and_then(|a| a.first())
        .and_then(value_as_u64)
        .unwrap_or(0);
    let name = user_arr
        .and_then(|a| a.get(1))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let admin = user_arr
        .and_then(|a| a.get(2))
        .map(|v| value_as_i64(v).unwrap_or(0) == 1)
        .unwrap_or(false);
    let vip = user_arr
        .and_then(|a| a.get(3))
        .map(|v| value_as_i64(v).unwrap_or(0) > 0)
        .unwrap_or(false);
    let level = user_arr
        .and_then(|a| a.get(16))
        .and_then(|v| v.get(0))
        .and_then(value_as_u64)
        .or_else(|| info.get(4).and_then(|v| v.get(0)).and_then(value_as_u64))
        .unwrap_or(0) as u32;

    // 舰队等级在 info[7]
    let guard = GuardLevel::from_level(info.get(7).and_then(value_as_i64).unwrap_or(0));

    let mut user = User {
        uid,
        name,
        level,
        guard,
        admin,
        vip,
        ..Default::default()
    };

    // 旧版勋章：info[3] = [等级, 名称, 主播昵称, 主播房间号, 颜色, ..., 边框, 渐变起, 渐变止, 文字色, 舰队等级, 主播uid]
    if let Some(medal) = info.get(3).and_then(parse_medal_legacy) {
        user.medal = Some(medal);
    }

    // 新版用户对象：info[0][15] = {extra, mode, show_player_type, user}
    if let Some(v2) = info
        .first()
        .and_then(|a| a.as_array())
        .and_then(|a| a.get(15))
        && let Some(v2_user) = v2.get("user")
    {
        if let Some(base) = v2_user.get("base") {
            if let Some(face) = base.get("face").and_then(Value::as_str)
                && !face.is_empty()
            {
                user.face = https(face);
            }
            if let Some(color) = base
                .get("name_color")
                .and_then(value_as_i64)
                .and_then(color_hex)
            {
                user.name_color = Some(color);
            }
        }
        if let Some(mut medal) = v2_user.get("medal").and_then(parse_medal_v2) {
            // v2 勋章只有主播 uid，没有昵称；沿用旧字段里的主播昵称
            if medal.anchor.is_empty()
                && let Some(legacy) = &user.medal
            {
                medal.anchor = legacy.anchor.clone();
            }
            user.medal = Some(medal);
        }
    }

    // 表情大图：info[0][12] == 1 时 info[0][13] 是表情对象
    let is_emoticon = info
        .first()
        .and_then(|a| a.as_array())
        .and_then(|a| a.get(12))
        .and_then(value_as_i64)
        .unwrap_or(0)
        == 1;
    let emoticon = if is_emoticon {
        info.first()
            .and_then(|a| a.as_array())
            .and_then(|a| a.get(13))
            .and_then(|v| v.get("url"))
            .and_then(Value::as_str)
            .filter(|url| !url.is_empty())
            .map(https)
    } else {
        None
    };

    // 正文里的内联小表情（[滑稽] 这类）：B 站不会把它们塞进 info[0][13]，
    // 而是在 info[0][15].extra.emots 里给一张「文本 → 图片」的对照表。
    let emots = parse_chat_emots(info);

    Some(LiveEvent::Chat {
        user,
        text,
        emoticon,
        emots,
    })
}

/// 解析内联小表情对照表。
///
/// 形状有两种：`{"[滑稽]": {url, width, height, ...}}` 的对象映射，
/// 或者 `[{emoji: "[滑稽]", url, ...}]` 的数组。两种都认，认不出就退化成空表。
fn parse_chat_emots(info: &[Value]) -> Vec<ChatEmoticon> {
    let Some(extra) = info
        .first()
        .and_then(|meta| meta.get(15))
        .and_then(|v2| v2.get("extra"))
        .and_then(extra_object)
    else {
        return Vec::new();
    };
    let Some(emots) = extra.get("emots") else {
        return Vec::new();
    };

    let mut result = Vec::new();
    match emots {
        Value::Object(map) => {
            for (key, item) in map {
                result.extend(chat_emoticon_of(key, item));
            }
        }
        Value::Array(list) => {
            for item in list {
                let key = item
                    .get("emoji")
                    .or_else(|| item.get("text"))
                    .or_else(|| item.get("descript"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                result.extend(chat_emoticon_of(key, item));
            }
        }
        _ => {}
    }
    // 长标记优先：否则 `[doge]` 会先吃掉 `[doge_金箍]` 的前缀
    result.sort_by_key(|emote| std::cmp::Reverse(emote.text.chars().count()));
    result
}

/// `extra` 既可能是 JSON 字符串（老协议），也可能已经是对象（新协议）。
fn extra_object(value: &Value) -> Option<Value> {
    match value {
        Value::String(raw) => serde_json::from_str::<Value>(raw).ok(),
        Value::Object(_) => Some(value.clone()),
        _ => None,
    }
}

/// 一个内联表情：必须有标记文本和图片地址，否则不值得渲染。
fn chat_emoticon_of(key: &str, item: &Value) -> Option<ChatEmoticon> {
    let text = if key.trim().is_empty() {
        item.get("emoji")
            .or_else(|| item.get("text"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string()
    } else {
        key.to_string()
    };
    if text.trim().is_empty() {
        return None;
    }
    let url = item
        .get("url")
        .and_then(Value::as_str)
        .filter(|url| !url.is_empty())?;
    Some(ChatEmoticon {
        text,
        url: https(url),
        width: item.get("width").and_then(value_as_u64).unwrap_or(0) as u32,
        height: item.get("height").and_then(value_as_u64).unwrap_or(0) as u32,
    })
}

/// 旧版勋章数组。未佩戴勋章时 B 站给空数组。
fn parse_medal_legacy(value: &Value) -> Option<Medal> {
    let medal = value.as_array()?;
    let level = medal.first().and_then(value_as_u64).unwrap_or(0) as u32;
    let name = medal.get(1).and_then(Value::as_str).unwrap_or_default();
    if level == 0 || name.is_empty() {
        return None;
    }
    let color = medal.get(4).and_then(value_as_i64).unwrap_or(0);
    let border = medal
        .get(7)
        .and_then(value_as_i64)
        .and_then(color_hex)
        .or_else(|| color_hex(color));
    Some(Medal {
        name: name.to_string(),
        level,
        anchor: medal
            .get(2)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        room_id: medal.get(3).and_then(value_as_u64).unwrap_or(0),
        start: border.clone().unwrap_or_default(),
        end: border.clone().unwrap_or_default(),
        border: border.unwrap_or_default(),
        text: medal
            .get(10)
            .and_then(value_as_i64)
            .and_then(color_hex)
            .unwrap_or_default(),
        guard: GuardLevel::from_level(medal.get(11).and_then(value_as_i64).unwrap_or(0)),
        light: true,
    })
}

/// 新版勋章对象（带 v2_medal_color_* 的渐变配色）。
fn parse_medal_v2(value: &Value) -> Option<Medal> {
    let level = value.get("level").and_then(value_as_u64).unwrap_or(0) as u32;
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if level == 0 || name.is_empty() {
        return None;
    }
    let color = value.get("color").and_then(value_as_i64).unwrap_or(0);
    let fallback = value
        .get("v2_medal_color_level")
        .and_then(Value::as_str)
        .and_then(css_color_opt)
        .or_else(|| color_hex(color));
    Some(Medal {
        name: name.to_string(),
        level,
        anchor: String::new(),
        room_id: value.get("ruid").and_then(value_as_u64).unwrap_or(0),
        start: value
            .get("v2_medal_color_start")
            .and_then(Value::as_str)
            .and_then(css_color_opt)
            .or_else(|| color_hex(color))
            .unwrap_or_default(),
        end: value
            .get("v2_medal_color_end")
            .and_then(Value::as_str)
            .and_then(css_color_opt)
            .or_else(|| color_hex(color))
            .unwrap_or_default(),
        border: value
            .get("v2_medal_color_border")
            .and_then(Value::as_str)
            .and_then(css_color_opt)
            .or_else(|| color_hex(color))
            .unwrap_or_default(),
        text: fallback.unwrap_or_default(),
        guard: GuardLevel::from_level(value.get("guard_level").and_then(value_as_i64).unwrap_or(0)),
        light: value.get("is_light").and_then(value_as_i64).unwrap_or(1) != 0,
    })
}

/// 颜色字符串来自直播间（粉丝牌 / SuperChat 的字段），前端会直接拼进 style。
/// 只放行 #RGB / #RGBA / #RRGGBB / #RRGGBBAA，其余一律丢掉，
/// 免得 `red;background:url(...)`、`var(--x)` 这类东西混进 CSS。
fn css_color_opt(raw: &str) -> Option<String> {
    let candidate = raw.trim();
    let hex = candidate.strip_prefix('#')?;
    if matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Some(candidate.to_string())
    } else {
        None
    }
}

fn parse_medal_json(value: Option<&Value>) -> Option<Medal> {
    let value = value?;
    let level = value
        .get("medal_level")
        .or_else(|| value.get("level"))
        .and_then(value_as_u64)
        .unwrap_or(0) as u32;
    let name = value
        .get("medal_name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if level == 0 || name.is_empty() {
        return None;
    }
    let start = value
        .get("medal_color_start")
        .and_then(Value::as_str)
        .and_then(css_color_opt)
        .or_else(|| {
            value
                .get("medal_color")
                .and_then(value_as_i64)
                .and_then(color_hex)
        })
        .unwrap_or_default();
    let end = value
        .get("medal_color_end")
        .and_then(Value::as_str)
        .and_then(css_color_opt)
        .unwrap_or_else(|| start.clone());
    let border = value
        .get("medal_color_border")
        .and_then(Value::as_str)
        .and_then(css_color_opt)
        .unwrap_or_else(|| start.clone());
    Some(Medal {
        name: name.to_string(),
        level,
        anchor: String::new(),
        room_id: value
            .get("target_id")
            .or_else(|| value.get("anchor_roomid"))
            .and_then(value_as_u64)
            .unwrap_or(0),
        start,
        end,
        border,
        text: String::new(),
        guard: GuardLevel::from_level(value.get("guard_level").and_then(value_as_i64).unwrap_or(0)),
        light: value
            .get("is_light")
            .or_else(|| value.get("is_lighted"))
            .and_then(value_as_i64)
            .unwrap_or(1)
            != 0,
    })
}

// ---------------------------------------------------------------------------
// 礼物
// ---------------------------------------------------------------------------

fn parse_gift_json(raw: &Value) -> Option<LiveEvent> {
    let data = raw.get("data")?;
    let user = user_from_json(data);
    let price_gold = data.get("price").and_then(value_as_i64).unwrap_or(0) as f64;
    let num = data.get("num").and_then(value_as_u64).unwrap_or(1) as u32;
    let total_gold = data
        .get("total_coin")
        .and_then(value_as_i64)
        .map(|v| v as f64)
        .unwrap_or(price_gold * num as f64);
    Some(LiveEvent::Gift {
        user,
        gift: Gift {
            id: data
                .get("giftId")
                .or_else(|| data.get("gift_id"))
                .and_then(value_as_u64)
                .unwrap_or(0),
            name: data
                .get("giftName")
                .or_else(|| data.get("gift_name"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            num,
            price: price_gold / 1000.0,
            total: total_gold / 1000.0,
            coin: data
                .get("coin_type")
                .and_then(Value::as_str)
                .unwrap_or("gold")
                .to_string(),
            icon: data
                .get("giftIcon")
                .or_else(|| data.get("gift_icon"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            batch: data
                .get("batch_combo_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            action: data
                .get("action")
                .and_then(Value::as_str)
                .map(str::to_string),
        },
    })
}

/// 新版礼物消息：data.pb 是 protobuf。
///
/// 字段映射（由真实抓包比对得出）：
/// - 1 uid / 2 uname / 3 face
/// - 8 粉丝勋章：1 uid, 5 等级, 6 名称, 7 主播房间号, 8/9/10 渐变/边框色
/// - 10 礼物：1 礼物 id, 2 名称, 4 数量, 5 单价(金瓜子), 8 货币, 12 批次号, 18 动作, 35 图标
fn parse_gift_v2(raw: &Value) -> Option<LiveEvent> {
    let encoded = raw.get("data")?.get("pb")?.as_str()?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;

    let uid = pb::varint(&bytes, 1).unwrap_or(0);
    let uname = pb::string(&bytes, 2).unwrap_or_default();
    let face = pb::string(&bytes, 3).unwrap_or_default();
    let gift_msg = pb::message(&bytes, 10)?;

    let medal = pb::message(&bytes, 8).map(|m| Medal {
        name: pb::string(m, 6).unwrap_or_default(),
        level: pb::varint(m, 5).unwrap_or(0) as u32,
        anchor: String::new(),
        room_id: pb::varint(m, 7).unwrap_or(0),
        start: pb::varint(m, 9)
            .and_then(|v| color_hex(v as i64))
            .or_else(|| pb::varint(m, 8).and_then(|v| color_hex(v as i64)))
            .unwrap_or_default(),
        end: pb::varint(m, 10)
            .and_then(|v| color_hex(v as i64))
            .or_else(|| pb::varint(m, 8).and_then(|v| color_hex(v as i64)))
            .unwrap_or_default(),
        border: pb::varint(m, 8)
            .and_then(|v| color_hex(v as i64))
            .unwrap_or_default(),
        text: String::new(),
        guard: GuardLevel::None,
        light: true,
    });

    let num = pb::varint(gift_msg, 4).unwrap_or(1).max(1) as u32;
    let price_gold = pb::varint(gift_msg, 5).unwrap_or(0) as f64;
    let icon = pb::message(gift_msg, 35)
        .and_then(|m| pb::string(m, 1))
        .unwrap_or_default();

    Some(LiveEvent::Gift {
        user: User {
            uid,
            name: uname,
            face,
            medal,
            ..Default::default()
        },
        gift: Gift {
            id: pb::varint(gift_msg, 1).unwrap_or(0),
            name: pb::string(gift_msg, 2).unwrap_or_default(),
            num,
            price: price_gold / 1000.0,
            total: price_gold * num as f64 / 1000.0,
            coin: pb::string(gift_msg, 8).unwrap_or_else(|| "gold".into()),
            icon,
            batch: pb::string(gift_msg, 12),
            action: pb::string(gift_msg, 18),
        },
    })
}

// ---------------------------------------------------------------------------
// 醒目留言 / 上舰
// ---------------------------------------------------------------------------

fn parse_super_chat(raw: &Value) -> Option<LiveEvent> {
    let data = raw.get("data")?;
    let info = data.get("user_info");
    let user = User {
        uid: data
            .get("uid")
            .and_then(value_as_u64)
            .or_else(|| info.and_then(|i| i.get("uid")).and_then(value_as_u64))
            .unwrap_or(0),
        name: info
            .and_then(|i| i.get("uname"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        face: info
            .and_then(|i| i.get("face"))
            .and_then(Value::as_str)
            .map(https)
            .unwrap_or_default(),
        level: info
            .and_then(|i| i.get("user_level"))
            .and_then(value_as_u64)
            .unwrap_or(0) as u32,
        guard: GuardLevel::from_level(
            info.and_then(|i| i.get("guard_level"))
                .and_then(value_as_i64)
                .unwrap_or(0),
        ),
        medal: parse_medal_json(data.get("medal_info")),
        ..Default::default()
    };
    Some(LiveEvent::SuperChat {
        user,
        text: data
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        price: data.get("price").and_then(value_as_i64).unwrap_or(0) as f64,
        duration: data.get("time").and_then(value_as_u64).unwrap_or(0),
        bg_start: data
            .get("background_color_start")
            .and_then(Value::as_str)
            .and_then(css_color_opt)
            .unwrap_or_else(|| "#EDF5FF".to_string()),
        bg_end: data
            .get("background_color_end")
            .and_then(Value::as_str)
            .and_then(css_color_opt)
            .unwrap_or_else(|| "#DBE7FF".to_string()),
        price_color: data
            .get("background_price_color")
            .and_then(Value::as_str)
            .and_then(css_color_opt)
            .unwrap_or_else(|| "#7EA6E0".to_string()),
        id: data.get("id").and_then(value_as_u64).unwrap_or(0),
    })
}

fn parse_super_chat_delete(raw: &Value) -> Option<LiveEvent> {
    let ids = raw.get("data")?.get("ids")?.as_array()?;
    let id = ids.first().and_then(value_as_u64).unwrap_or(0);
    Some(LiveEvent::Remove { id })
}

fn parse_guard_json(raw: &Value) -> Option<LiveEvent> {
    let data = raw.get("data")?;
    let level = GuardLevel::from_level(data.get("guard_level").and_then(value_as_i64).unwrap_or(0));
    let num = data.get("num").and_then(value_as_u64).unwrap_or(1) as u32;
    let price_gold = data.get("price").and_then(value_as_i64).unwrap_or(0) as f64;
    Some(LiveEvent::Guard {
        user: User {
            uid: data.get("uid").and_then(value_as_u64).unwrap_or(0),
            name: data
                .get("username")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            medal: parse_medal_json(data.get("medal_info")),
            ..Default::default()
        },
        level,
        num,
        price: price_gold * num as f64 / 1000.0,
        item: data
            .get("gift_name")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| level.label().to_string()),
    })
}

/// 上舰提示条：旧版是扁平 JSON，V2 是 sender_uinfo / guard_info / pay_info 嵌套。
fn parse_user_toast(raw: &Value, nested: bool) -> Option<LiveEvent> {
    let data = raw.get("data")?;
    let (uid, name, level_value, num, price_gold, item) = if nested {
        (
            data.get("sender_uinfo")
                .and_then(|u| u.get("uid"))
                .and_then(value_as_u64)
                .unwrap_or(0),
            data.get("sender_uinfo")
                .and_then(|u| u.get("base"))
                .and_then(|b| b.get("name"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            data.get("guard_info")
                .and_then(|g| g.get("guard_level"))
                .and_then(value_as_i64)
                .unwrap_or(0),
            data.get("pay_info")
                .and_then(|p| p.get("num"))
                .and_then(value_as_u64)
                .unwrap_or(1) as u32,
            data.get("pay_info")
                .and_then(|p| p.get("price"))
                .and_then(value_as_i64)
                .unwrap_or(0) as f64,
            data.get("gift_info")
                .and_then(|g| g.get("gift_name"))
                .and_then(Value::as_str)
                .map(str::to_string),
        )
    } else {
        (
            data.get("uid").and_then(value_as_u64).unwrap_or(0),
            data.get("username")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            data.get("guard_level").and_then(value_as_i64).unwrap_or(0),
            data.get("num").and_then(value_as_u64).unwrap_or(1) as u32,
            data.get("price").and_then(value_as_i64).unwrap_or(0) as f64,
            data.get("gift_name")
                .and_then(Value::as_str)
                .map(str::to_string),
        )
    };
    let level = GuardLevel::from_level(level_value);
    if level == GuardLevel::None {
        return None;
    }
    Some(LiveEvent::Guard {
        user: User {
            uid,
            name,
            guard: level,
            ..Default::default()
        },
        level,
        num,
        price: price_gold * num as f64 / 1000.0,
        item: item.unwrap_or_else(|| level.label().to_string()),
    })
}

// ---------------------------------------------------------------------------
// 进房 / 互动 / 点赞
// ---------------------------------------------------------------------------

fn parse_interact_json(raw: &Value) -> Option<LiveEvent> {
    let data = raw.get("data")?;
    let kind = EnterKind::from_msg_type(data.get("msg_type").and_then(value_as_i64).unwrap_or(1));
    let user = User {
        uid: data.get("uid").and_then(value_as_u64).unwrap_or(0),
        name: data
            .get("uname")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        face: data
            .get("uinfo")
            .and_then(|u| u.get("base"))
            .and_then(|b| b.get("face"))
            .or_else(|| data.get("face"))
            .and_then(Value::as_str)
            .map(https)
            .unwrap_or_default(),
        guard: GuardLevel::from_level(data.get("guard_level").and_then(value_as_i64).unwrap_or(0)),
        medal: parse_medal_json(data.get("fans_medal")),
        ..Default::default()
    };
    Some(if kind == EnterKind::Like {
        LiveEvent::Like {
            user: Some(user),
            count: 1,
            total: None,
        }
    } else {
        LiveEvent::Enter { user, kind }
    })
}

/// 新版进房消息：data.pb 是 protobuf，字段 1 uid / 2 uname / 5 msg_type / 22.2.2 头像。
fn parse_interact_v2(raw: &Value) -> Option<LiveEvent> {
    let encoded = raw.get("data")?.get("pb")?.as_str()?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    let kind = EnterKind::from_msg_type(pb::varint(&bytes, 5).unwrap_or(1) as i64);
    let user = User {
        uid: pb::varint(&bytes, 1).unwrap_or(0),
        name: pb::string(&bytes, 2).unwrap_or_default(),
        face: pb::path_string(&bytes, &[22, 2, 2]).unwrap_or_default(),
        ..Default::default()
    };
    Some(if kind == EnterKind::Like {
        LiveEvent::Like {
            user: Some(user),
            count: 1,
            total: None,
        }
    } else {
        LiveEvent::Enter { user, kind }
    })
}

fn parse_like_click(raw: &Value) -> Option<LiveEvent> {
    let data = raw.get("data")?;
    let user = User {
        uid: data.get("uid").and_then(value_as_u64).unwrap_or(0),
        name: data
            .get("uname")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        face: data
            .get("uinfo")
            .and_then(|u| u.get("base"))
            .and_then(|b| b.get("face"))
            .and_then(Value::as_str)
            .map(https)
            .unwrap_or_default(),
        medal: parse_medal_json(data.get("fans_medal")),
        ..Default::default()
    };
    Some(LiveEvent::Like {
        user: Some(user),
        count: 1,
        total: None,
    })
}

fn parse_like_update(raw: &Value) -> Option<LiveEvent> {
    let data = raw.get("data")?;
    Some(LiveEvent::Like {
        user: None,
        count: 0,
        total: data.get("click_count").and_then(value_as_u64),
    })
}

// ---------------------------------------------------------------------------
// 房间状态
// ---------------------------------------------------------------------------

fn parse_watched(raw: &Value) -> Option<LiveEvent> {
    let data = raw.get("data")?;
    Some(LiveEvent::Watched {
        num: data.get("num").and_then(value_as_u64).unwrap_or(0),
        text: data
            .get("text_small")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

fn parse_online_rank_count(raw: &Value) -> Option<LiveEvent> {
    let data = raw.get("data")?;
    let text = data
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let count = data
        .get("count")
        .and_then(value_as_u64)
        .or_else(|| crate::model::parse_short_count(&text))
        .unwrap_or(0);
    // 空的高能榜消息（count=0 且没有文案）不该覆盖已有的在线人数
    if count == 0 && text.trim().is_empty() {
        return None;
    }
    Some(LiveEvent::OnlineRank { count, text })
}

fn parse_room_update(raw: &Value) -> Option<LiveEvent> {
    let data = raw.get("data")?;
    Some(LiveEvent::RoomUpdate {
        title: None,
        area: None,
        live_status: None,
        fans: data
            .get("fans")
            .and_then(value_as_u64)
            .or_else(|| data.get("fans_club").and_then(value_as_u64)),
    })
}

// ---------------------------------------------------------------------------
// 小工具
// ---------------------------------------------------------------------------

fn user_from_json(data: &Value) -> User {
    User {
        uid: data.get("uid").and_then(value_as_u64).unwrap_or(0),
        name: data
            .get("uname")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        face: data
            .get("face")
            .and_then(Value::as_str)
            .map(https)
            .unwrap_or_default(),
        level: data.get("user_level").and_then(value_as_u64).unwrap_or(0) as u32,
        guard: GuardLevel::from_level(data.get("guard_level").and_then(value_as_i64).unwrap_or(0)),
        medal: parse_medal_json(data.get("medal_info")),
        admin: false,
        vip: false,
        name_color: data
            .get("name_color")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty() && *v != "#FFFFFF")
            .map(str::to_string),
    }
}

/// B 站部分头像/封面还是 http:// 链接，统一升级成 https，避免混合内容被拦。
fn https(url: &str) -> String {
    if let Some(rest) = url.strip_prefix("http://") {
        format!("https://{rest}")
    } else {
        url.to_string()
    }
}

fn value_as_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(n) => n.as_u64().or_else(|| n.as_i64().map(|v| v.max(0) as u64)),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn value_as_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => n.as_i64().or_else(|| n.as_u64().map(|v| v as i64)),
        Value::String(s) => s.parse().ok(),
        Value::Bool(b) => Some(i64::from(*b)),
        _ => None,
    }
}

fn value_as_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn get_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for segment in path {
        current = match segment.parse::<usize>() {
            Ok(index) => current.get(index)?,
            Err(_) => current.get(*segment)?,
        };
    }
    Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/messages.jsonl");

    fn messages() -> Vec<Value> {
        FIXTURE
            .lines()
            .filter(|line| !line.trim().is_empty())
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    #[test]
    fn every_fixture_line_parses_without_panic() {
        let parsed: Vec<_> = messages().iter().filter_map(parse).collect();
        assert!(
            parsed.len() >= 8,
            "fixture 解析出的消息太少: {}",
            parsed.len()
        );
    }

    #[test]
    fn parses_chat_with_user_and_medal() {
        let raws = messages();
        let chat = raws
            .iter()
            .find_map(|raw| match parse(raw) {
                Some(LiveEvent::Chat { user, text, .. }) => Some((user, text)),
                _ => None,
            })
            .expect("fixture 里应该有聊天弹幕");
        assert!(!chat.0.name.is_empty());
        assert!(!chat.1.is_empty());
        assert!(!chat.0.face.is_empty(), "应从新版用户对象里取到头像");
        let medal = chat.0.medal.expect("该用户佩戴了粉丝勋章");
        assert!(medal.level > 0);
        assert!(medal.start.starts_with('#'));
    }

    #[test]
    fn parses_gift_v2_protobuf() {
        let raws = messages();
        let gift = raws
            .iter()
            .find_map(|raw| match parse(raw) {
                Some(LiveEvent::Gift { user, gift }) => Some((user, gift)),
                _ => None,
            })
            .expect("fixture 里应该有礼物");
        assert!(!gift.0.name.is_empty());
        assert!(!gift.1.name.is_empty(), "礼物名称为空");
        assert!(gift.1.num >= 1);
        assert!(gift.1.total > 0.0, "礼物金额应大于 0");
        assert_eq!(gift.1.coin, "gold");
    }

    #[test]
    fn parses_interact_v2_as_enter() {
        let raws = messages();
        let enter = raws
            .iter()
            .find_map(|raw| match parse(raw) {
                Some(LiveEvent::Enter { user, .. }) => Some(user),
                _ => None,
            })
            .expect("fixture 里应该有进房消息");
        assert!(enter.uid > 0);
        assert!(!enter.name.is_empty());
    }

    #[test]
    fn parses_synthetic_super_chat() {
        let raw: Value = serde_json::from_str(
            r##"{"cmd":"SUPER_CHAT_MESSAGE","data":{"id":12345,"uid":7,"price":30,
            "message":"加油","time":60,"background_color_start":"#EDF5FF",
            "background_color_end":"#DBE7FF","background_price_color":"#7EA6E0",
            "user_info":{"uname":"老板","face":"https://example.com/a.jpg","user_level":22,"guard_level":3},
            "medal_info":{"medal_name":"会考上","medal_level":14,"medal_color_start":"#5C968E","guard_level":0}}}"##,
        )
        .unwrap();
        match parse(&raw) {
            Some(LiveEvent::SuperChat {
                user,
                text,
                price,
                duration,
                id,
                ..
            }) => {
                assert_eq!(user.name, "老板");
                assert_eq!(user.guard, GuardLevel::Captain);
                assert_eq!(text, "加油");
                assert_eq!(price, 30.0);
                assert_eq!(duration, 60);
                assert_eq!(id, 12345);
                assert_eq!(user.medal.unwrap().name, "会考上");
            }
            other => panic!("解析结果不对: {other:?}"),
        }
    }

    #[test]
    fn parses_synthetic_guard_buy() {
        let raw: Value = serde_json::from_str(
            r#"{"cmd":"GUARD_BUY","data":{"uid":9,"username":"舰长新人","guard_level":3,
            "num":1,"price":198000,"gift_name":"舰长"}}"#,
        )
        .unwrap();
        match parse(&raw) {
            Some(LiveEvent::Guard {
                user,
                level,
                price,
                item,
                ..
            }) => {
                assert_eq!(user.name, "舰长新人");
                assert_eq!(level, GuardLevel::Captain);
                assert_eq!(item, "舰长");
                assert_eq!(price, 198.0);
            }
            other => panic!("解析结果不对: {other:?}"),
        }
    }

    #[test]
    fn parses_live_status_and_watched() {
        let live: Value = serde_json::from_str(r#"{"cmd":"LIVE","data":{"roomid":1}}"#).unwrap();
        assert_eq!(parse(&live), Some(LiveEvent::LiveStatus { live_status: 1 }));
        let preparing: Value =
            serde_json::from_str(r#"{"cmd":"PREPARING","data":{"roomid":1}}"#).unwrap();
        assert_eq!(
            parse(&preparing),
            Some(LiveEvent::LiveStatus { live_status: 0 })
        );
        let watched: Value = serde_json::from_str(
            r#"{"cmd":"WATCHED_CHANGE","data":{"num":1234,"text_small":"1234"}}"#,
        )
        .unwrap();
        assert_eq!(
            parse(&watched),
            Some(LiveEvent::Watched {
                num: 1234,
                text: "1234".into()
            })
        );
    }

    #[test]
    fn unknown_cmd_returns_none() {
        let raw: Value = serde_json::from_str(r#"{"cmd":"SOME_NEW_THING","data":{}}"#).unwrap();
        assert!(parse(&raw).is_none());
    }

    #[test]
    fn event_roundtrips_through_json() {
        let raw: Value = serde_json::from_str(
            r#"{"cmd":"SEND_GIFT","data":{"uid":1,"uname":"a","giftName":"辣条","num":2,"price":100,"total_coin":200,"coin_type":"gold"}}"#,
        )
        .unwrap();
        let event = parse_event(&raw, 42, 1_700_000_000_000).expect("解析失败");
        let text = serde_json::to_string(&event).unwrap();
        let back: Event = serde_json::from_str(&text).unwrap();
        assert_eq!(back.room_id, 42);
        assert_eq!(back.kind(), "gift");
        assert_eq!(back, event);
        assert!(
            text.contains("\"type\":\"gift\""),
            "事件应带 type 判别字段: {text}"
        );
    }

    #[test]
    fn css_colors_from_live_room_are_validated() {
        assert_eq!(css_color_opt("#FF6699").as_deref(), Some("#FF6699"));
        assert_eq!(css_color_opt("  #abc ").as_deref(), Some("#abc"));
        assert_eq!(css_color_opt("#12345678").as_deref(), Some("#12345678"));
        assert_eq!(css_color_opt("red"), None);
        assert_eq!(css_color_opt("#ff;background:url(x)"), None);
        assert_eq!(css_color_opt("var(--evil)"), None);
        assert_eq!(css_color_opt(""), None);
        assert_eq!(css_color_opt("expression(alert(1))"), None);
    }
    #[test]
    fn parses_inline_emots_from_extra() {
        // 内联小表情走 info[0][15].extra.emots：文本是 [滑稽]，客户端自己换成图
        let raw = serde_json::json!({
            "cmd": "DANMU_MSG",
            "info": [
                [
                    0, 1, 25, 16777215, 1792951572000i64, 0, 0, "", 0,
                    { "ts": 1, "ct": "abc" }, 0, "", 0, {}, "",
                    {
                        "mode": 0,
                        "extra": "{\"emots\":{\"[滑稽]\":{\"emoji\":\"[滑稽]\",\"url\":\"http://i0.hdslb.com/e.png\",\"width\":20,\"height\":20}}}",
                        "user": { "base": { "face": "https://i0.hdslb.com/f.png", "name_color": 0 } }
                    }
                ],
                "哈哈哈[滑稽]",
                [12345, "观众甲", 0, 0, 0, 10000, 1, ""],
                [], [], 0, 0, "", 0, 0, null, { "ts": 1, "ct": "abc" }, 0, 0, null, null, 0, 0
            ]
        });
        let event = parse(&raw).expect("应解析出聊天弹幕");
        match event {
            LiveEvent::Chat { text, emoticon, emots, .. } => {
                assert_eq!(text, "哈哈哈[滑稽]");
                assert!(emoticon.is_none());
                assert_eq!(emots.len(), 1);
                assert_eq!(emots[0].text, "[滑稽]");
                // 图床地址统一升到 https
                assert_eq!(emots[0].url, "https://i0.hdslb.com/e.png");
                assert_eq!(emots[0].width, 20);
            }
            other => panic!("应该解析成聊天弹幕: {other:?}"),
        }
    }

    #[test]
    fn parses_inline_emots_from_array_shape() {
        let raw = serde_json::json!({
            "cmd": "DANMU_MSG",
            "info": [
                [0, 1, 25, 16777215, 0, 0, 0, "", 0, { "ts": 1, "ct": "abc" }, 0, "", 0, {}, "",
                 { "mode": 0, "extra": { "emots": [ { "emoji": "[doge]", "url": "https://i0.hdslb.com/d.png" } ] },
                   "user": { "base": { "face": "", "name_color": 0 } } }],
                "[doge]",
                [1, "观众乙", 0, 0, 0, 10000, 1, ""],
                [], [], 0, 0, "", 0, 0, null, { "ts": 1, "ct": "abc" }, 0, 0, null, null, 0, 0
            ]
        });
        match parse(&raw) {
            Some(LiveEvent::Chat { emots, .. }) => {
                assert_eq!(emots.len(), 1);
                assert_eq!(emots[0].text, "[doge]");
            }
            other => panic!("应该解析成聊天弹幕: {other:?}"),
        }
    }

}
