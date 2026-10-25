import type { Event, GuardLevel, RoomState } from "./types";

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

export function cn(...values: Array<string | false | null | undefined>): string {
  return values.filter(Boolean).join(" ");
}
