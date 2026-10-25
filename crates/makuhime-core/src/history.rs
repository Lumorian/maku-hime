//! 弹幕历史：内存环形缓冲 + 按房间的 JSONL 落盘 + CSV / JSON 导出。
//!
//! 文件位置：~/.local/share/maku-hime/history/{room_id}.jsonl
//! 每行一个 [Event] 的 JSON，方便直接用 jq / 脚本二次处理。
//!
//! 写盘放在独立线程里：弹幕高峰时每条消息都同步 write+flush 会阻塞 tokio worker，
//! 这里改成「内存里立刻可见 + 通过 channel 交给写线程批量 flush」。

use std::collections::{HashMap, VecDeque};
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::time::Duration;

use chrono::{Local, TimeZone};

use crate::error::Result;
use crate::model::{Event, LiveEvent};

/// 写线程收到的操作。
enum WriteOp {
    Line { room_id: u64, line: String },
    Clear { room_id: u64, ack: Sender<()> },
    Forget { room_id: u64 },
    Flush(Sender<()>),
}

struct RoomMemory {
    events: VecDeque<Event>,
    loaded: bool,
}

impl RoomMemory {
    fn new() -> Self {
        Self {
            events: VecDeque::new(),
            loaded: false,
        }
    }
}

pub struct HistoryStore {
    dir: PathBuf,
    limit: usize,
    rooms: Mutex<HashMap<u64, RoomMemory>>,
    writer: Sender<WriteOp>,
}

impl HistoryStore {
    pub fn new(dir: PathBuf, limit: usize) -> Self {
        let writer = spawn_writer(dir.clone());
        Self {
            dir,
            limit: limit.max(50),
            rooms: Mutex::new(HashMap::new()),
            writer,
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn file_path(&self, room_id: u64) -> PathBuf {
        self.dir.join(format!("{room_id}.jsonl"))
    }

    pub fn append(&self, event: &Event) {
        let limit = self.limit;
        {
            let mut rooms = self.rooms.lock().unwrap();
            let room = rooms.entry(event.room_id).or_insert_with(RoomMemory::new);
            if !room.loaded {
                let (bytes, truncated) = read_tail_bytes(&self.file_path(event.room_id));
                room.events = parse_tail(&bytes, truncated, limit);
                room.loaded = true;
            }
            room.events.push_back(event.clone());
            while room.events.len() > limit {
                room.events.pop_front();
            }
        }
        if let Ok(line) = serde_json::to_string(event) {
            let _ = self.writer.send(WriteOp::Line {
                room_id: event.room_id,
                line,
            });
        }
    }

    /// 房间里内存保留的最近若干条。
    pub fn recent(&self, room_id: u64, limit: usize) -> Vec<Event> {
        let limit = limit.min(self.limit);
        let mut rooms = self.rooms.lock().unwrap();
        let room = rooms.entry(room_id).or_insert_with(RoomMemory::new);
        if !room.loaded {
            let (bytes, truncated) = read_tail_bytes(&self.file_path(room_id));
            room.events = parse_tail(&bytes, truncated, self.limit);
            room.loaded = true;
        }
        let skip = room.events.len().saturating_sub(limit);
        room.events.iter().skip(skip).cloned().collect()
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
                Some(needle) => searchable_text(event).to_lowercase().contains(needle),
            })
            .collect();
        if out.len() > limit {
            out = out.split_off(out.len() - limit);
        }
        out
    }

    /// 导出整个房间的落盘历史。
    pub fn export(&self, room_id: u64, format: ExportFormat) -> Result<String> {
        self.flush();
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

    /// 把写线程里缓冲的内容刷到磁盘（导出、测试用）。
    pub fn flush(&self) {
        let (tx, rx) = channel();
        if self.writer.send(WriteOp::Flush(tx)).is_ok() {
            let _ = rx.recv_timeout(Duration::from_secs(3));
        }
    }

    fn read_all(&self, room_id: u64) -> Result<Vec<Event>> {
        // 防止一个跑了很久的房间把内存吃满：最多读最近 MAX_EXPORT 条
        const MAX_EXPORT: usize = 500_000;
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
                if events.len() >= MAX_EXPORT {
                    break;
                }
            }
        }
        Ok(events)
    }

    /// 退订房间时丢掉内存缓冲与打开的文件句柄（落盘文件保留）。
    pub fn forget_room(&self, room_id: u64) {
        self.rooms.lock().unwrap().remove(&room_id);
        let _ = self.writer.send(WriteOp::Forget { room_id });
    }

    pub fn clear(&self, room_id: u64) -> Result<()> {
        self.rooms.lock().unwrap().remove(&room_id);
        let (tx, rx) = channel();
        let _ = self.writer.send(WriteOp::Clear { room_id, ack: tx });
        let _ = rx.recv_timeout(Duration::from_secs(3));
        Ok(())
    }

    /// 房间历史文件大小（字节），用于设置页显示。
    pub fn file_size(&self, room_id: u64) -> u64 {
        std::fs::metadata(self.file_path(room_id))
            .map(|meta| meta.len())
            .unwrap_or(0)
    }
}

/// 写线程：串行持有各房间的文件句柄，空闲 500ms 刷一次盘。
fn spawn_writer(dir: PathBuf) -> Sender<WriteOp> {
    let (tx, rx) = channel::<WriteOp>();
    let thread = std::thread::Builder::new()
        .name("makuhime-history".into())
        .spawn(move || {
            let mut writers: HashMap<u64, BufWriter<std::fs::File>> = HashMap::new();
            let flush_all = |writers: &mut HashMap<u64, BufWriter<std::fs::File>>| {
                for writer in writers.values_mut() {
                    let _ = writer.flush();
                }
            };
            loop {
                match rx.recv_timeout(Duration::from_millis(500)) {
                    Ok(WriteOp::Line { room_id, line }) => {
                        // 文件打不开就跳过这条，不要把整个写线程拖死
                        if let std::collections::hash_map::Entry::Vacant(slot) =
                            writers.entry(room_id)
                        {
                            let path = dir.join(format!("{room_id}.jsonl"));
                            if let Some(parent) = path.parent() {
                                let _ = std::fs::create_dir_all(parent);
                            }
                            match OpenOptions::new().create(true).append(true).open(&path) {
                                Ok(file) => {
                                    slot.insert(BufWriter::new(file));
                                }
                                Err(err) => {
                                    tracing::warn!(%err, path = %path.display(), "历史文件打开失败，跳过写入");
                                    continue;
                                }
                            }
                        }
                        if let Some(writer) = writers.get_mut(&room_id) {
                            let _ = writeln!(writer, "{line}");
                        }
                    }
                    Ok(WriteOp::Clear { room_id, ack }) => {
                        if let Some(mut writer) = writers.remove(&room_id) {
                            let _ = writer.flush();
                        }
                        let _ = std::fs::remove_file(dir.join(format!("{room_id}.jsonl")));
                        let _ = ack.send(());
                    }
                    Ok(WriteOp::Forget { room_id }) => {
                        if let Some(mut writer) = writers.remove(&room_id) {
                            let _ = writer.flush();
                        }
                    }
                    Ok(WriteOp::Flush(ack)) => {
                        flush_all(&mut writers);
                        let _ = ack.send(());
                    }
                    Err(RecvTimeoutError::Timeout) => flush_all(&mut writers),
                    Err(RecvTimeoutError::Disconnected) => {
                        flush_all(&mut writers);
                        return;
                    }
                }
            }
        });
    if let Err(err) = thread {
        tracing::error!(%err, "历史写线程启动失败，历史将不会落盘");
    }
    tx
}

/// 读取文件末尾一段字节（最多约 1MB）。
/// 返回 (字节, 是否从文件中途开始)——中途开始说明第一行可能被截断。
fn read_tail_bytes(path: &Path) -> (Vec<u8>, bool) {
    let mut out = Vec::new();
    let Ok(mut file) = std::fs::File::open(path) else {
        return (out, false);
    };
    let Ok(meta) = file.metadata() else {
        return (out, false);
    };
    const TAIL_BYTES: u64 = 1 << 20;
    let start = meta.len().saturating_sub(TAIL_BYTES);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return (out, false);
    }
    if file.read_to_end(&mut out).is_err() {
        return (Vec::new(), start > 0);
    }
    (out, start > 0)
}

/// 解析行式 JSONL 的尾部。
/// skip_partial_line=true 时丢掉第一行（它可能从多字节 UTF-8 字符中间被切开）。
fn parse_tail(bytes: &[u8], skip_partial_line: bool, limit: usize) -> VecDeque<Event> {
    let bytes = if skip_partial_line {
        match bytes.iter().position(|byte| *byte == b'\n') {
            Some(index) => &bytes[index + 1..],
            None => &[],
        }
    } else {
        bytes
    };
    // from_utf8_lossy：即使切在多字节字符中间也不会报错，最坏只是丢掉半行
    let text = String::from_utf8_lossy(bytes);
    let mut out = VecDeque::new();
    for line in text.lines().rev().take(limit) {
        if let Ok(event) = serde_json::from_str::<Event>(line) {
            out.push_front(event);
        }
    }
    out
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
        assert!(store.query(1, &[], Some("不存在"), 10).is_empty());
    }

    #[test]
    fn persists_and_reloads_from_disk() {
        let dir = temp_dir("disk");
        {
            let store = HistoryStore::new(dir.clone(), 100);
            store.append(&chat_event(42, "落盘一条"));
            store.append(&Event {
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
            });
            store.flush();
        }
        // 新实例模拟重启：内存为空，但能读回落盘内容
        let store = HistoryStore::new(dir, 100);
        let recent = store.recent(42, 10);
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].kind(), "chat");
        assert!(store.file_size(42) > 0);
    }

    #[test]
    fn parses_tail_from_truncated_multibyte_start() {
        let line = serde_json::to_string(&chat_event(5, "中文内容")).unwrap();
        let mut bytes = vec![0xE4, 0xB8]; // 半个「中」字，模拟 1MB 边界切在多字节中间
        bytes.push(b'\n');
        bytes.extend_from_slice(line.as_bytes());
        bytes.push(b'\n');
        let events = parse_tail(&bytes, true, 10);
        assert_eq!(events.len(), 1, "应该只解析出后半段完整的行");
        assert_eq!(events[0].kind(), "chat");
    }

    #[test]
    fn parse_tail_handles_invalid_utf8() {
        let mut bytes = b"not-json-line\n".to_vec();
        bytes.extend_from_slice(&[0xFF, 0xFE, 0x0A]);
        let events = parse_tail(&bytes, false, 10);
        assert!(events.is_empty());
    }

    #[test]
    fn forget_room_drops_memory_but_keeps_file() {
        let dir = temp_dir("forget");
        let store = HistoryStore::new(dir, 100);
        store.append(&chat_event(77, "保留文件"));
        store.flush();
        store.forget_room(77);
        assert_eq!(store.recent(77, 10).len(), 1, "重新从磁盘恢复");
        assert!(store.file_size(77) > 0);
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
    fn export_format_parsing() {
        assert_eq!(ExportFormat::parse("CSV"), ExportFormat::Csv);
        assert_eq!(ExportFormat::parse("jsonl").extension(), "jsonl");
        assert_eq!(
            ExportFormat::parse("json").content_type(),
            "application/json; charset=utf-8"
        );
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
