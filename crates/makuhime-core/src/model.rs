//! 归一化后的弹幕事件模型：前端渲染、历史落盘、TTS 播报都用它。

use serde::{Deserialize, Serialize};

/// 舰队等级（大航海）。B 站用 1/2/3 表示总督/提督/舰长。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GuardLevel {
    #[default]
    None,
    Governor,
    Admiral,
    Captain,
}

impl GuardLevel {
    pub fn from_level(level: i64) -> Self {
        match level {
            1 => GuardLevel::Governor,
            2 => GuardLevel::Admiral,
            3 => GuardLevel::Captain,
            _ => GuardLevel::None,
        }
    }

    pub fn level(self) -> u8 {
        match self {
            GuardLevel::None => 0,
            GuardLevel::Governor => 1,
            GuardLevel::Admiral => 2,
            GuardLevel::Captain => 3,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            GuardLevel::None => "",
            GuardLevel::Governor => "总督",
            GuardLevel::Admiral => "提督",
            GuardLevel::Captain => "舰长",
        }
    }
}

/// 粉丝勋章。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Medal {
    pub name: String,
    pub level: u32,
    /// 所属主播昵称
    pub anchor: String,
    pub room_id: u64,
    pub start: String,
    pub end: String,
    pub border: String,
    pub text: String,
    pub guard: GuardLevel,
    pub light: bool,
}

/// 消息发送者。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct User {
    pub uid: u64,
    pub name: String,
    pub face: String,
    pub level: u32,
    pub guard: GuardLevel,
    pub medal: Option<Medal>,
    pub admin: bool,
    pub vip: bool,
    pub name_color: Option<String>,
}

/// 礼物。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Gift {
    pub id: u64,
    pub name: String,
    pub num: u32,
    /// 单价，单位人民币元
    pub price: f64,
    /// 合计金额，单位人民币元
    pub total: f64,
    /// gold / silver
    pub coin: String,
    pub icon: String,
    pub batch: Option<String>,
    /// 动作文案，例如「投喂」
    pub action: Option<String>,
}

/// 互动类型（进房 / 关注 / 分享 / 点赞）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EnterKind {
    #[default]
    Enter,
    Follow,
    Share,
    SpecialFollow,
    MutualFollow,
    Like,
    Other,
}

impl EnterKind {
    pub fn from_msg_type(kind: i64) -> Self {
        match kind {
            1 => EnterKind::Enter,
            2 => EnterKind::Follow,
            3 => EnterKind::Share,
            4 => EnterKind::SpecialFollow,
            5 => EnterKind::MutualFollow,
            6 => EnterKind::Like,
            _ => EnterKind::Other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            EnterKind::Enter => "进入直播间",
            EnterKind::Follow => "关注了主播",
            EnterKind::Share => "分享了直播间",
            EnterKind::SpecialFollow => "特别关注了主播",
            EnterKind::MutualFollow => "和主播互粉了",
            EnterKind::Like => "为主播点赞了",
            EnterKind::Other => "互动",
        }
    }
}

/// 弹幕正文里内联的小表情（\`[滑稽]\` 这类）。
///
/// B 站不把这些表情塞进 \`emoticon\`（那是大表情），而是在 \`info[0][15].extra.emots\`
/// 里下发一张「文本 → 图片」的对照表：正文仍然是 \`[滑稽]\`，客户端自己替换成图。
/// 这里按同样的方式把对照表带出来，前端渲染时把命中的文本换成图片。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ChatEmoticon {
    /// 正文里出现的那个标记，例如 \`[滑稽]\`
    pub text: String,
    pub url: String,
    pub width: u32,
    pub height: u32,
}

/// 一条归一化的弹幕事件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LiveEvent {
    /// 聊天弹幕
    Chat {
        user: User,
        text: String,
        /// 表情大图 URL（普通文字弹幕为 None）
        emoticon: Option<String>,
        /// 正文里内联的小表情对照表；老数据（旧 JSONL / 旧前端）里可能没有
        #[serde(default)]
        emots: Vec<ChatEmoticon>,
    },
    /// 礼物
    Gift { user: User, gift: Gift },
    /// 上舰
    Guard {
        user: User,
        level: GuardLevel,
        num: u32,
        price: f64,
        item: String,
    },
    /// 醒目留言
    SuperChat {
        user: User,
        text: String,
        price: f64,
        duration: u64,
        bg_start: String,
        bg_end: String,
        price_color: String,
        id: u64,
    },
    /// 进房 / 关注 / 分享 / 点赞
    Enter { user: User, kind: EnterKind },
    /// 点赞（累计点赞数可空）
    Like {
        user: Option<User>,
        count: u64,
        total: Option<u64>,
    },
    /// 人气值（心跳回复）
    Popularity { value: u64 },
    /// 看过人数
    Watched { num: u64, text: String },
    /// 高能榜在线人数
    OnlineRank { count: u64, text: String },
    /// 开播状态变化：1 直播中 / 0 未开播
    LiveStatus { live_status: i64 },
    /// 直播间资料变化（标题 / 分区 / 粉丝数）
    RoomUpdate {
        title: Option<String>,
        area: Option<String>,
        live_status: Option<i64>,
        fans: Option<u64>,
    },
    /// 系统通知
    Notice { text: String },
    /// 房间公告板（醒目留言删除等）
    Remove { id: u64 },
}

impl LiveEvent {
    /// 事件类型标识，与前端 CSS 类名 / 历史筛选用同一套字符串。
    pub fn kind(&self) -> &'static str {
        match self {
            LiveEvent::Chat { .. } => "chat",
            LiveEvent::Gift { .. } => "gift",
            LiveEvent::Guard { .. } => "guard",
            LiveEvent::SuperChat { .. } => "super_chat",
            LiveEvent::Enter { .. } => "enter",
            LiveEvent::Like { .. } => "like",
            LiveEvent::Popularity { .. } => "popularity",
            LiveEvent::Watched { .. } => "watched",
            LiveEvent::OnlineRank { .. } => "online_rank",
            LiveEvent::LiveStatus { .. } => "live_status",
            LiveEvent::RoomUpdate { .. } => "room_update",
            LiveEvent::Notice { .. } => "notice",
            LiveEvent::Remove { .. } => "remove",
        }
    }

    /// 给 TTS / 通知用的一句话摘要。
    pub fn tts_text(&self) -> Option<String> {
        match self {
            LiveEvent::Chat { user, text, .. } => Some(format!("{}说{}", user.name, text)),
            LiveEvent::Gift { user, gift } => {
                Some(format!("{}送出{}个{}", user.name, gift.num, gift.name))
            }
            LiveEvent::Guard {
                user,
                level,
                num,
                item,
                ..
            } => Some(if *num > 1 {
                format!("{}开通了{}个{}", user.name, num, item)
            } else {
                format!("{}开通了{}", user.name, level.label())
            }),
            LiveEvent::SuperChat {
                user, text, price, ..
            } => Some(format!("{}的醒目留言，{}元，{}", user.name, price, text)),
            LiveEvent::Enter { user, kind } => match kind {
                EnterKind::Enter => Some(format!("{}进入直播间", user.name)),
                EnterKind::Follow => Some(format!("{}关注了主播", user.name)),
                _ => None,
            },
            _ => None,
        }
    }
}

/// 带元信息的事件：前端与历史记录里的一条。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// 去重与前端 key
    pub id: String,
    /// 毫秒时间戳
    pub ts: i64,
    pub room_id: u64,
    #[serde(flatten)]
    pub body: LiveEvent,
}

impl Event {
    pub fn kind(&self) -> &'static str {
        self.body.kind()
    }
}

/// 把 B 站惯例的十进制颜色转成 #RRGGBB。
pub fn color_hex(value: i64) -> Option<String> {
    if value <= 0 {
        return None;
    }
    Some(format!("#{:06X}", value as u32 & 0x00FF_FFFF))
}

/// 把 B 站缩写数字（13万+ / 9999+ / 1.2亿）解析成数值，取不到返回 None。
pub fn parse_short_count(text: &str) -> Option<u64> {
    // 官方文案会带「+」后缀（9999+ / 13万+），先去掉再识别单位
    let trimmed = text.trim().trim_end_matches('+').trim();
    if trimmed.is_empty() {
        return None;
    }
    let (number, unit) = if let Some(v) = trimmed.strip_suffix('亿') {
        (v, 100_000_000f64)
    } else if let Some(v) = trimmed.strip_suffix('万') {
        (v, 10_000f64)
    } else {
        (trimmed, 1f64)
    };
    number
        .trim()
        .parse::<f64>()
        .ok()
        .map(|v| (v * unit).round() as u64)
}

/// 按 B 站习惯缩写数字：9999 / 1.2万 / 13万 / 1.2亿。
pub fn format_count(value: u64) -> String {
    if value < 10_000 {
        return value.to_string();
    }
    if value >= 100_000_000 {
        let text = format!("{:.1}", value as f64 / 100_000_000.0);
        return format!("{}亿", text.trim_end_matches(".0"));
    }
    let text = format!("{:.1}", value as f64 / 10_000.0);
    format!("{}万", text.trim_end_matches(".0"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guard_level_mapping() {
        assert_eq!(GuardLevel::from_level(3), GuardLevel::Captain);
        assert_eq!(GuardLevel::from_level(1).label(), "总督");
        assert_eq!(GuardLevel::from_level(0), GuardLevel::None);
        assert_eq!(GuardLevel::Captain.level(), 3);
    }

    #[test]
    fn formats_counts_like_bilibili() {
        assert_eq!(format_count(0), "0");
        assert_eq!(format_count(9999), "9999");
        assert_eq!(format_count(10_000), "1万");
        assert_eq!(format_count(12_345), "1.2万");
        assert_eq!(format_count(999_999), "100万");
        assert_eq!(format_count(120_000_000), "1.2亿");
    }

    #[test]
    fn parses_short_counts() {
        assert_eq!(parse_short_count("13万+"), Some(130_000));
        assert_eq!(parse_short_count("9999+"), Some(9999));
        assert_eq!(parse_short_count("1.2亿"), Some(120_000_000));
        assert_eq!(parse_short_count(""), None);
    }

    #[test]
    fn colors_become_hex() {
        assert_eq!(color_hex(12_478_086).as_deref(), Some("#BE6686"));
        assert_eq!(color_hex(0), None);
    }
}
