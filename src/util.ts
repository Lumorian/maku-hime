import type { ChatEmoticon, Event, GuardLevel, RoomState } from "./types";

/** 按 B 站习惯缩写数字：9999 / 1.2万 / 13万 / 1.2亿 */
export function formatCount(value: number | null | undefined): string {
  const number = Math.max(0, Math.floor(value ?? 0));
  if (number < 10_000) return String(number);
  // 按 B 站习惯向下截断到一位小数：999999 → 99.9万（四舍五入会变成 100万）
  const unit = number >= 100_000_000 ? 100_000_000 : 10_000;
  const scaled = Math.floor((number / unit) * 10) / 10;
  return `${trimZero(scaled.toFixed(1))}${unit === 100_000_000 ? "亿" : "万"}`;
}

function trimZero(text: string): string {
  return text.endsWith(".0") ? text.slice(0, -2) : text;
}

/** 在线人数文案：优先官方缩写，其次本地缩写，最后退回人气。 */
export function onlineLabel(room: RoomState): string {
  if (!room) return "";
  if (room.online_text) return `在线 ${room.online_text}`;
  if (room.online != null) return `在线 ${formatCount(room.online)}`;
  if (room.popularity) return `人气 ${formatCount(room.popularity)}`;
  return room.live_status === 1 ? "直播中" : "";
}

export function liveBadge(room: RoomState): { dot: string; text: string } {
  if (room.live_status === 1) {
    return { dot: "live", text: "直播中" };
  }
  if (room.live_status === 2) {
    return { dot: "replay", text: "轮播中" };
  }
  return { dot: "offline", text: "未开播" };
}

export function timeAgo(ts: number | null | undefined): string {
  if (!ts) return "—";
  const diff = Math.max(0, Date.now() - ts) / 1000;
  if (diff < 10) return "刚刚";
  if (diff < 60) return `${Math.floor(diff)} 秒前`;
  if (diff < 3600) return `${Math.floor(diff / 60)} 分钟前`;
  if (diff < 86_400) return `${Math.floor(diff / 3600)} 小时前`;
  const date = new Date(ts);
  return `${date.getMonth() + 1}-${date.getDate()} ${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

export function clockOf(ts: number): string {
  const date = new Date(ts);
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

export const GUARD_LABEL: Record<GuardLevel, string> = {
  none: "",
  governor: "总督",
  admiral: "提督",
  captain: "舰长",
};

export function guardClass(level: GuardLevel): string {
  return level === "none" ? "" : `guard-${level}`;
}

/** 腾讯/哔哩哩风格的价格格式：30 → 30，128.5 → 128.5 */
export function formatPrice(price: number): string {
  if (!Number.isFinite(price)) return "0";
  return price % 1 === 0 ? String(price) : price.toFixed(2);
}

/** 弹幕正文切成「文字 / 内联表情」片段，供弹幕墙渲染。 */
export type ChatSegment =
  | { kind: "text"; value: string }
  | { kind: "emote"; emote: ChatEmoticon };

/**
 * 把正文按 B 站下发的内联表情标记切开。
 *
 * `[滑稽]` 这类标记在正文里是普通字符，B 站另外给一张对照表，客户端自己替换成图。
 * 匹配时长的标记优先，避免 `[doge]` 把 `[doge_金箍]` 的前缀吃掉。
 */
export function splitChatText(
  text: string,
  emots: ChatEmoticon[] | null | undefined,
): ChatSegment[] {
  const usable = (emots ?? [])
    .filter((emote) => emote && emote.text && emote.url)
    .slice()
    .sort((left, right) => right.text.length - left.text.length);
  if (!text) return [];
  if (usable.length === 0) return [{ kind: "text", value: text }];

  const segments: ChatSegment[] = [];
  let buffer = "";
  let index = 0;
  while (index < text.length) {
    const hit = usable.find((emote) => text.startsWith(emote.text, index));
    if (hit) {
      if (buffer) {
        segments.push({ kind: "text", value: buffer });
        buffer = "";
      }
      segments.push({ kind: "emote", emote: hit });
      index += hit.text.length;
      continue;
    }
    buffer += text[index];
    index += 1;
  }
  if (buffer) segments.push({ kind: "text", value: buffer });
  return segments;
}

export function eventSummary(event: Event): string {
  switch (event.type) {
    case "chat":
      return event.text;
    case "gift":
      return `送出 ${event.gift.name} x${event.gift.num}`;
    case "guard":
      return `开通了 ${event.item}`;
    case "super_chat":
      return `醒目留言 ¥${formatPrice(event.price)}`;
    case "enter":
      return "进入直播间";
    case "like":
      return "点赞";
    default:
      return event.type;
  }
}

export function isSystemEvent(event: Event): boolean {
  return (
    event.type === "popularity" ||
    event.type === "watched" ||
    event.type === "online_rank" ||
    event.type === "live_status" ||
    event.type === "room_update"
  );
}

/** data: URI 里只接受常见的位图格式（SVG 可以内嵌脚本，直接排除）。 */
const SAFE_DATA_IMAGE = /^data:image\/(png|jpe?g|gif|webp|avif|bmp)[;,]/i;

/** 去掉控制字符与空白（`java\nscript:` 这类变形靠它们绕过前缀判断）。 */
function stripControlChars(raw: string): string {
  let out = "";
  for (const char of raw) {
    const code = char.codePointAt(0) ?? 0;
    if (code > 0x20 && code !== 0x7f) out += char;
  }
  return out;
}

/**
 * 过滤 `<img src>` 的地址。
 *
 * 头像、表情包、铭牌图标这些地址都来自 B 站接口，属于「远端可控的字符串」。
 * 直接塞进 src，至少能用 `data:` / `file:` 把用户浏览器的请求导向任意地址，
 * 所以这里只放行 http(s) 和常见位图的 data URI，其余一律当作没有图片。
 */
export function safeImageUrl(raw: string | null | undefined): string | undefined {
  if (!raw) return undefined;
  // 先去掉空白与控制字符：`java\nscript:` 这类变形绕不过去
  const cleaned = stripControlChars(raw);
  if (!cleaned) return undefined;
  if (cleaned.toLowerCase().startsWith("data:")) {
    return SAFE_DATA_IMAGE.test(cleaned) ? cleaned : undefined;
  }
  try {
    const url = new URL(cleaned, window.location.origin);
    if (url.protocol === "http:" || url.protocol === "https:") return url.href;
  } catch {
    return undefined;
  }
  return undefined;
}

export function cn(...values: Array<string | false | null | undefined>): string {
  return values.filter(Boolean).join(" ");
}
