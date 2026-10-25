import { describe, expect, it, vi, afterEach } from "vitest";
import {
  eventSummary,
  formatCount,
  formatPrice,
  guardClass,
  liveBadge,
  onlineLabel,
  timeAgo,
} from "./util";
import type { Event, RoomState } from "./types";

function room(patch: Partial<RoomState> = {}): RoomState {
  return {
    room_id: 1,
    short_id: 0,
    uid: 2,
    uname: "主播",
    title: "标题",
    area: "分区",
    parent_area: "父分区",
    live_status: 1,
    popularity: 0,
    attention: 0,
    cover: "",
    keyframe: "",
    live_time: "",
    description: "",
    online: null,
    online_text: "",
    connected: false,
    status: "",
    message_count: 0,
    last_event_at: null,
    error: null,
    ...patch,
  };
}

describe("formatCount", () => {
  it("按 B 站习惯缩写，并向下截断", () => {
    expect(formatCount(0)).toBe("0");
    expect(formatCount(9999)).toBe("9999");
    expect(formatCount(10_000)).toBe("1万");
    expect(formatCount(12_345)).toBe("1.2万");
    expect(formatCount(999_999)).toBe("99.9万");
    expect(formatCount(120_000_000)).toBe("1.2亿");
  });

  it("对空值和不合理输入兜底", () => {
    expect(formatCount(null)).toBe("0");
    expect(formatCount(undefined)).toBe("0");
    expect(formatCount(-5)).toBe("0");
  });
});

describe("liveBadge / onlineLabel", () => {
  it("按开播状态给文案", () => {
    expect(liveBadge(room({ live_status: 1 }))).toEqual({ dot: "live", text: "直播中" });
    expect(liveBadge(room({ live_status: 2 })).text).toBe("轮播中");
    expect(liveBadge(room({ live_status: 0 })).dot).toBe("offline");
  });

  it("在线人数优先用官方缩写，其次本地格式化，最后退回人气", () => {
    expect(onlineLabel(room({ online_text: "13万+", online: 130_000 }))).toBe("在线 13万+");
    expect(onlineLabel(room({ online: 12_345 }))).toBe("在线 1.2万");
    expect(onlineLabel(room({ popularity: 1_234_567 }))).toBe("人气 123.4万");
    expect(onlineLabel(room({ live_status: 0 }))).toBe("");
  });
});

describe("eventSummary / formatPrice / guardClass", () => {
  const chat: Event = {
    id: "1",
    ts: 0,
    room_id: 1,
    type: "chat",
    user: {
      uid: 1,
      name: "观众",
      face: "",
      level: 1,
      guard: "none",
      medal: null,
      admin: false,
      vip: false,
      name_color: null,
    },
    text: "你好",
    emoticon: null,
  };

  it("聊天/礼物/醒目留言各有一句摘要", () => {
    expect(eventSummary(chat)).toBe("你好");
    expect(
      eventSummary({
        ...chat,
        type: "gift",
        gift: {
          id: 1,
          name: "辣条",
          num: 3,
          price: 0.1,
          total: 0.3,
          coin: "gold",
          icon: "",
          batch: null,
          action: "投喂",
        },
      } as Event),
    ).toBe("送出 辣条 x3");
  });

  it("价格去掉多余小数", () => {
    expect(formatPrice(30)).toBe("30");
    expect(formatPrice(30.5)).toBe("30.50");
    expect(formatPrice(Number.NaN)).toBe("0");
  });

  it("舰队等级映射到样式类名", () => {
    expect(guardClass("captain")).toBe("guard-captain");
    expect(guardClass("none")).toBe("");
  });
});

describe("timeAgo", () => {
  afterEach(() => vi.useRealTimers());

  it("相对时间按区间文案", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-10-26T12:00:00+08:00"));
    const now = Date.now();
    expect(timeAgo(now)).toBe("刚刚");
    expect(timeAgo(now - 30_000)).toBe("30 秒前");
    expect(timeAgo(now - 5 * 60_000)).toBe("5 分钟前");
    expect(timeAgo(now - 3 * 3600_000)).toBe("3 小时前");
    expect(timeAgo(null)).toBe("—");
  });
});
