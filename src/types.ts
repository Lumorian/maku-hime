// 与 Rust 端 serde 序列化结果一一对应的类型定义。

export type Theme = "dark" | "light";
export type TtsProvider = "auto" | "webview" | "command" | "off";
export type GuardLevel = "none" | "governor" | "admiral" | "captain";
export type EnterKind =
  | "enter"
  | "follow"
  | "share"
  | "special_follow"
  | "mutual_follow"
  | "like"
  | "other";

export interface Medal {
  name: string;
  level: number;
  anchor: string;
  room_id: number;
  start: string;
  end: string;
  border: string;
  text: string;
  guard: GuardLevel;
  light: boolean;
}

export interface User {
  uid: number;
  name: string;
  face: string;
  level: number;
  guard: GuardLevel;
  medal: Medal | null;
  admin: boolean;
  vip: boolean;
  name_color: string | null;
}

export interface Gift {
  id: number;
  name: string;
  num: number;
  price: number;
  total: number;
  coin: string;
  icon: string;
  batch: string | null;
  action: string | null;
}

export type LiveEventBody =
  | { type: "chat"; user: User; text: string; emoticon: string | null }
  | { type: "gift"; user: User; gift: Gift }
  | {
      type: "guard";
      user: User;
      level: GuardLevel;
      num: number;
      price: number;
      item: string;
    }
  | {
      type: "super_chat";
      user: User;
      text: string;
      price: number;
      duration: number;
      bg_start: string;
      bg_end: string;
      price_color: string;
      id: number;
    }
  | { type: "enter"; user: User; kind: EnterKind }
  | { type: "like"; user: User | null; count: number; total: number | null }
  | { type: "popularity"; value: number }
  | { type: "watched"; num: number; text: string }
  | { type: "online_rank"; count: number; text: string }
  | { type: "live_status"; live_status: number }
  | {
      type: "room_update";
      title: string | null;
      area: string | null;
      live_status: number | null;
      fans: number | null;
    }
  | { type: "notice"; text: string }
  | { type: "remove"; id: number };

export type Event = { id: string; ts: number; room_id: number } & LiveEventBody;
export type EventKind = LiveEventBody["type"];

export interface RoomState {
  room_id: number;
  short_id: number;
  uid: number;
  uname: string;
  title: string;
  area: string;
  parent_area: string;
  live_status: number;
  popularity: number;
  attention: number;
  cover: string;
  keyframe: string;
  live_time: string;
  description: string;
  online: number | null;
  online_text: string;
  connected: boolean;
  status: string;
  message_count: number;
  last_event_at: number | null;
  error: string | null;
}

export interface LoginState {
  logged_in: boolean;
  uid: number;
  uname: string;
  face: string;
}

export interface ServerConfig {
  host: string;
  port: number;
  open_browser: boolean;
}

export interface UiConfig {
  theme: Theme;
  font_size: number;
  opacity: number;
  max_rows: number;
  show_avatar: boolean;
  show_enter: boolean;
  show_like: boolean;
  show_gift: boolean;
  compact: boolean;
}

export interface OverlayConfig {
  width: number;
  height: number;
  x: number;
  y: number;
  font_size: number;
  opacity: number;
  click_through: boolean;
  always_on_top: boolean;
  theme: Theme;
  show_header: boolean;
  locked: boolean;
}

export interface DanmakuConfig {
  auto_reconnect: boolean;
  reconnect_max_seconds: number;
  heartbeat_seconds: number;
  keep_history: boolean;
  history_limit: number;
  stats_interval_seconds: number;
}

export interface TtsConfig {
  enabled: boolean;
  provider: TtsProvider;
  command: string[];
  voice: string;
  rate: number;
  volume: number;
  read_chat: boolean;
  read_gift: boolean;
  read_guard: boolean;
  read_super_chat: boolean;
  min_super_chat_price: number;
  max_chars: number;
  max_queue: number;
}

export interface SendConfig {
  color: number;
  font_size: number;
  mode: number;
  max_length: number;
}

export interface AppConfig {
  server: ServerConfig;
  ui: UiConfig;
  overlay: OverlayConfig;
  danmaku: DanmakuConfig;
  tts: TtsConfig;
  send: SendConfig;
  rooms: number[];
}

export interface Snapshot {
  version: string;
  server_url: string;
  started_at: number;
  rooms: RoomState[];
  login: LoginState;
  config: AppConfig;
  tts_backend: string;
  tts_dropped: number;
  history_dir: string;
}

export interface QrSession {
  url: string;
  qrcode_key: string;
}

export interface Profile {
  uid: number;
  uname: string;
  face: string;
}

export interface QrPoll {
  status: "pending" | "scanned" | "expired" | "success" | "error";
  message: string;
  profile?: Profile;
}

export type ServerEvent =
  | { type: "event"; data: Event }
  | { type: "snapshot"; data: Snapshot }
  | { type: "speak"; data: { text: string; kind: string; backend: string } }
  | { type: "notice"; data: { level: string; message: string } };
