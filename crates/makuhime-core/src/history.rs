//! 弹幕历史：内存环形缓冲 + 按房间的 JSONL 落盘 + CSV / JSON 导出。
//!
//! 文件位置：~/.local/share/maku-hime/history/{room_id}.jsonl
//! 每行一个 [Event] 的 JSON，方便直接用 jq / 脚本二次处理。

use std::collections::{HashMap, VecDeque};
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Local, TimeZone};

use crate::error::Result;
use crate::model::{Event, LiveEvent};

struct RoomLog {
    events: VecDeque<Event>,
    writer: Option<BufWriter<std::fs::File>>,
    loaded: bool,
}

impl RoomLog {
    fn new() -> Self {
        Self {
            events: VecDeque::new(),
            writer: None,
            loaded: false,
        }
    }
}

pub struct HistoryStore {
    dir: PathBuf,
    limit: usize,
    logs: Mutex<HashMap<u64, RoomLog>>,
}

impl HistoryStore {
    pub fn new(dir: PathBuf, limit: usize) -> Self {
        Self {
            dir,
            limit: limit.max(50),
            logs: Mutex::new(HashMap::new()),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn file_path(&self, room_id: u64) -> PathBuf {
        self.dir.join(format!("{room_id}.jsonl"))
    }

    pub fn append(&self, event: &Event) {
        let mut logs = self.logs.lock().unwrap();
        let limit = self.limit;
        let dir = self.dir.clone();
        let log = logs.entry(event.room_id).or_insert_with(RoomLog::new);
        if !log.loaded {
            log.events = load_tail(&dir.join(format!("{}.jsonl", event.room_id)), limit);
            log.loaded = true;
        }
        log.events.push_back(event.clone());
        while log.events.len() > limit {
            log.events.pop_front();
        }

        if log.writer.is_none() {
            if std::fs::create_dir_all(&dir).is_ok() {
                let path = dir.join(format!("{}.jsonl", event.room_id));
                if let Ok(file) = OpenOptions::new().create(true).append(true).open(path) {
                    log.writer = Some(BufWriter::new(file));
                }
            }
        }
        if let Some(writer) = log.writer.as_mut() {
            if let Ok(line) = serde_json::to_string(event) {
                let _ = writeln!(writer, "{line}");
                let _ = writer.flush();
            }
        }
    }

    /// 房间里内存保留的最近若干条。
    pub fn recent(&self, room_id: u64, limit: usize) -> Vec<Event> {
        let mut logs = self.logs.lock().unwrap();
        let dir = self.dir.clone();
        let cap = self.limit;
        let log = logs.entry(room_id).or_insert_with(RoomLog::new);
        if !log.loaded {
            log.events = load_tail(&dir.join(format!("{room_id}.jsonl")), cap);
            log.loaded = true;
        }
        let skip = log.events.len().saturating_sub(limit);
        log.events.iter().skip(skip).cloned().collect()
    }

    /// 带筛选的查询：kind 过滤 + 关键字（昵称或内容）+ 条数上限。
    pub fn query(
        &self,
        room_id: u64,
        kinds: &[String],
        keyword: Option<&str>,
        limit: usize,
    ) -> Vec<Event> {
        let keyword = keyword
            .map(|k| k.trim().to_lowercase())
            .filter(|k| !k.is_empty());
        let events = self.recent(room_id, self.limit);
        let mut out: Vec<Event> = events
            .into_iter()
            .filter(|event| kinds.is_empty() || kinds.iter().any(|kind| kind == event.kind()))
            .filter(|event| match &keyword {
                None => true,
                Some(needle) => {
                    let haystack = searchable_text(event).to_lowercase();
                    haystack.contains(needle)
                }
            })
            .collect();
        if out.len() > limit {
            out = out.split_off(out.len() - limit);
        }
        out
    }

    /// 导出整个房间的落盘历史。
    pub fn export(&self, room_id: u64, format: ExportFormat) -> Result<String> {
        let events = self.read_all(room_id)?;
        Ok(match format {
            ExportFormat::Json => {
                serde_json::to_string_pretty(&events).unwrap_or_else(|_| "[]".into())
            }
            ExportFormat::Jsonl => events
                .iter()
                .filter_map(|event| serde_json::to_string(event).ok())
                .collect::<Vec<_>>()
                .join("\n"),
            ExportFormat::Csv => to_csv(&events),
        })
    }

    pub fn export_file_name(&self, room_id: u64, format: ExportFormat) -> String {
        let stamp = Local::now().format("%Y%m%d-%H%M%S");
        format!("maku-hime-{room_id}-{stamp}.{}", format.extension())
    }

    fn read_all(&self, room_id: u64) -> Result<Vec<Event>> {
        let path = self.file_path(room_id);
        let file = match std::fs::File::open(&path) {
            Ok(file) => file,
            Err(_) => return Ok(self.recent(room_id, self.limit)),
        };
        let reader = BufReader::new(file);
        let mut events = Vec::new();
        for line in reader.lines().map_while(std::result::Result::ok) {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(event) = serde_json::from_str::<Event>(&line) {
                events.push(event);
            }
        }
        Ok(events)
    }

    pub fn clear(&self, room_id: u64) -> Result<()> {
        {
            let mut logs = self.logs.lock().unwrap();
            if let Some(log) = logs.get_mut(&room_id) {
                log.events.clear();
                log.writer = None;
            }
        }
        let path = self.file_path(room_id);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    /// 房间历史文件大小（字节），用于设置页显示。
    pub fn file_size(&self, room_id: u64) -> u64 {
        std::fs::metadata(self.file_path(room_id))
            .map(|meta| meta.len())
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Json,
    Jsonl,
    Csv,
}

impl ExportFormat {
    pub fn parse(raw: &str) -> Self {
        match raw.trim().to_lowercase().as_str() {
            "csv" => ExportFormat::Csv,
            "jsonl" | "ndjson" => ExportFormat::Jsonl,
            _ => ExportFormat::Json,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Json => "json",
            ExportFormat::Jsonl => "jsonl",
            ExportFormat::Csv => "csv",
        }
    }

    pub fn content_type(self) -> &'static str {
        match self {
            ExportFormat::Json => "application/json; charset=utf-8",
            ExportFormat::Jsonl => "application/x-ndjson; charset=utf-8",
            ExportFormat::Csv => "text/csv; charset=utf-8",
        }
    }
}

fn load_tail(path: &Path, limit: usize) -> VecDeque<Event> {
    let mut out = VecDeque::new();
    let Ok(mut file) = std::fs::File::open(path) else {
        return out;
    };
    let Ok(meta) = file.metadata() else {
        return out;
    };
    const TAIL_BYTES: u64 = 1 << 20; // 只读文件最后 1MB，足够覆盖 limit 条
    let start = meta.len().saturating_sub(TAIL_BYTES);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return out;
    }
    let mut buffer = String::new();
    if file.read_to_string(&mut buffer).is_err() {
        return out;
    }
    let mut lines: Vec<&str> = buffer.lines().collect();
    if start > 0 && !lines.is_empty() {
        // 第一行多半是被截断的半行，丢掉
        lines.remove(0);
    }
    for line in lines.iter().rev().take(limit) {
        if let Ok(event) = serde_json::from_str::<Event>(line) {
            out.push_front(event);
        }
    }
    out
}

fn searchable_text(event: &Event) -> String {
    match &event.body {
        LiveEvent::Chat { user, text, .. } => format!("{} {text}", user.name),
        LiveEvent::Gift { user, gift } => format!("{} {} {}", user.name, gift.name, gift.num),
        LiveEvent::Guard { user, item, .. } => format!("{} {item}", user.name),
        LiveEvent::SuperChat { user, text, .. } => format!("{} {text}", user.name),
        LiveEvent::Enter { user, kind } => format!("{} {}", user.name, kind.label()),
        LiveEvent::Like { user, .. } => user.clone().map(|u| u.name).unwrap_or_default(),
        LiveEvent::Notice { text } => text.clone(),
        _ => String::new(),
    }
}

fn to_csv(events: &[Event]) -> String {
    let mut out = String::from("id,time,type,room_id,uid,user,text,amount,count,item\n");
    for event in events {
        let (uid, user, text, amount, count, item) = match &event.body {
            LiveEvent::Chat { user, text, .. } => (
                user.uid,
                user.name.clone(),
                text.clone(),
                None,
                None,
                String::new(),
            ),
            LiveEvent::Gift { user, gift } => (
                user.uid,
                user.name.clone(),
                String::new(),
                Some(gift.total),
                Some(gift.num as u64),
                gift.name.clone(),
            ),
            LiveEvent::Guard {
                user,
                num,
                price,
                item,
                ..
            } => (
                user.uid,
                user.name.clone(),
                String::new(),
                Some(*price),
                Some(*num as u64),
                item.clone(),
            ),
            LiveEvent::SuperChat {
                user, text, price, ..
            } => (
                user.uid,
                user.name.clone(),
                text.clone(),
                Some(*price),
                None,
                "醒目留言".into(),
            ),
            LiveEvent::Enter { user, kind } => (
                user.uid,
                user.name.clone(),
                kind.label().to_string(),
                None,
                None,
                String::new(),
            ),
            LiveEvent::Like { user, count, .. } => (
                user.clone().map(|u| u.uid).unwrap_or(0),
                user.clone().map(|u| u.name).unwrap_or_default(),
                String::new(),
                None,
                Some(*count),
                String::new(),
            ),
            LiveEvent::Watched { num, text } => (
                0,
                String::new(),
                text.clone(),
                None,
                Some(*num),
                String::new(),
            ),
            LiveEvent::OnlineRank { count, .. } => (
                0,
                String::new(),
                String::new(),
                None,
                Some(*count),
                "在线人数".into(),
            ),
            LiveEvent::Popularity { value } => (
                0,
                String::new(),
                String::new(),
                None,
                Some(*value),
                "人气".into(),
            ),
            LiveEvent::LiveStatus { live_status } => (
                0,
                String::new(),
                if *live_status == 1 {
                    "开播"
                } else {
                    "下播"
                }
                .into(),
                None,
                None,
                String::new(),
            ),
            LiveEvent::RoomUpdate { .. } => {
                (0, String::new(), String::new(), None, None, String::new())
            }
            LiveEvent::Notice { text } => {
                (0, String::new(), text.clone(), None, None, String::new())
            }
            LiveEvent::Remove { id } => (
                0,
                String::new(),
                format!("删除 {id}"),
                None,
                None,
                String::new(),
            ),
        };
        let time = format_time(event.ts);
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{}\n",
            csv_escape(&event.id),
            time,
            event.kind(),
            event.room_id,
            uid,
            csv_escape(&user),
            csv_escape(&text),
            amount.map(|v| format!("{v:.2}")).unwrap_or_default(),
            count.map(|v| v.to_string()).unwrap_or_default(),
            csv_escape(&item),
        ));
    }
    out
}

fn csv_escape(raw: &str) -> String {
    if raw.contains([',', '"', '\n']) {
        format!("\"{}\"", raw.replace('"', "\"\""))
    } else {
        raw.to_string()
    }
}

fn format_time(ts: i64) -> String {
    match Local.timestamp_millis_opt(ts).single() {
        Some(time) => time.format("%Y-%m-%d %H:%M:%S").to_string(),
        None => String::new(),
    }
}

/// 供前端显示「3 分钟前」之类的相对时间时使用的工具。
pub fn human_time(ts: i64) -> String {
    let now = chrono::Local::now().timestamp_millis();
    let diff = (now - ts).max(0) / 1000;
    match diff {
        0..=9 => "刚刚".into(),
        10..=59 => format!("{diff} 秒前"),
        60..=3599 => format!("{} 分钟前", diff / 60),
        3600..=86_399 => format!("{} 小时前", diff / 3600),
        _ => DateTime::<chrono::Utc>::from_timestamp_millis(ts)
            .map(|t| t.with_timezone(&Local).format("%m-%d %H:%M").to_string())
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EnterKind, Gift, User};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("makuhime-history-{tag}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn chat_event(room: u64, text: &str) -> Event {
        Event {
            id: format!("chat-{text}"),
            ts: 1_792_951_572_000,
            room_id: room,
            body: LiveEvent::Chat {
                user: User {
                    uid: 1,
                    name: "观众甲".into(),
                    ..Default::default()
                },
                text: text.into(),
                emoticon: None,
            },
        }
    }

    #[test]
    fn appends_and_queries_in_memory() {
        let dir = temp_dir("memory");
        let store = HistoryStore::new(dir, 100);
        store.append(&chat_event(1, "第一条"));
        store.append(&chat_event(1, "第二条"));
        let recent = store.recent(1, 10);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[1].kind(), "chat");
        let filtered = store.query(1, &["chat".into()], Some("第二"), 10);
        assert_eq!(filtered.len(), 1);
        let none = store.query(1, &[], Some("不存在"), 10);
        assert!(none.is_empty());
    }

    #[test]
    fn persists_and_reloads_from_disk() {
        let dir = temp_dir("disk");
        {
            let store = HistoryStore::new(dir.clone(), 100);
            store.append(&chat_event(42, "落盘一条"));
            let gift = Event {
                id: "gift-1".into(),
                ts: 1_792_951_573_000,
                room_id: 42,
                body: LiveEvent::Gift {
                    user: User {
                        uid: 2,
                        name: "送礼人".into(),
                        ..Default::default()
                    },
                    gift: Gift {
                        name: "辣条".into(),
                        num: 3,
                        price: 0.1,
                        total: 0.3,
                        ..Default::default()
                    },
                },
            };
            store.append(&gift);
        }
        // 新实例模拟重启：内存为空，但能读回落盘内容
        let store = HistoryStore::new(dir.clone(), 100);
        let recent = store.recent(42, 10);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].kind(), "chat");
        assert!(store.file_size(42) > 0);
    }

    #[test]
    fn exports_csv_with_escaping() {
        let dir = temp_dir("csv");
        let store = HistoryStore::new(dir, 100);
        store.append(&chat_event(7, "含,逗号\"和引号"));
        let csv = store.export(7, ExportFormat::Csv).unwrap();
        let mut lines = csv.lines();
        assert_eq!(
            lines.next().unwrap(),
            "id,time,type,room_id,uid,user,text,amount,count,item"
        );
        let row = lines.next().unwrap();
        assert!(row.contains("\"含,逗号\"\"和引号\""));
        assert!(row.contains(",chat,"));
    }

    #[test]
    fn exports_json_and_clears() {
        let dir = temp_dir("json");
        let store = HistoryStore::new(dir, 100);
        store.append(&chat_event(9, "导出"));
        let json = store.export(9, ExportFormat::Json).unwrap();
        assert!(json.contains("\"type\": \"chat\""));
        store.clear(9).unwrap();
        assert!(store.recent(9, 10).is_empty());
        assert_eq!(store.file_size(9), 0);
    }

    #[test]
    fn format_parse_and_human_time() {
        assert_eq!(ExportFormat::parse("CSV"), ExportFormat::Csv);
        assert_eq!(ExportFormat::parse("jsonl").extension(), "jsonl");
        assert_eq!(human_time(chrono::Local::now().timestamp_millis()), "刚刚");
    }

    #[test]
    fn enter_kind_is_searchable() {
        let dir = temp_dir("search");
        let store = HistoryStore::new(dir, 100);
        store.append(&Event {
            id: "enter-1".into(),
            ts: 1,
            room_id: 5,
            body: LiveEvent::Enter {
                user: User {
                    uid: 3,
                    name: "路人乙".into(),
                    ..Default::default()
                },
                kind: EnterKind::Follow,
            },
        });
        assert_eq!(store.query(5, &["enter".into()], Some("关注"), 10).len(), 1);
    }
}
