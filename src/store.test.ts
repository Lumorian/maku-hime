import { beforeEach, describe, expect, it } from "vitest";
import { useStore } from "./store";
import type { Event, Snapshot } from "./types";

function chatEvent(id: string, text = "hi"): Event {
  return {
    id,
    ts: Date.now(),
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
    text,
    emoticon: null,
  };
}

function snapshot(maxRows = 50): Snapshot {
  return {
    version: "0.1.0",
    server_url: "http://127.0.0.1:3939",
    started_at: 0,
    rooms: [],
    login: { logged_in: false, uid: 0, uname: "", face: "" },
    config: {
      server: { host: "127.0.0.1", port: 3939, open_browser: false },
      ui: {
        theme: "dark",
        font_size: 13,
        opacity: 1,
        max_rows: maxRows,
        show_avatar: true,
        show_enter: false,
        show_like: false,
        show_gift: true,
        compact: false,
      },
      overlay: {
        width: 380,
        height: 640,
        x: 0,
        y: 0,
        font_size: 13,
        opacity: 0.9,
        click_through: true,
        always_on_top: true,
        theme: "dark",
        show_header: true,
        locked: true,
      },
      danmaku: {
        auto_reconnect: true,
        reconnect_max_seconds: 30,
        heartbeat_seconds: 30,
        keep_history: true,
        history_limit: 2000,
        stats_interval_seconds: 30,
      },
      tts: {
        enabled: false,
        provider: "auto",
        command: [],
        voice: "",
        rate: 1,
        volume: 1,
        read_chat: true,
        read_gift: true,
        read_guard: true,
        read_super_chat: true,
        min_super_chat_price: 0,
        max_chars: 60,
        max_queue: 32,
      },
      send: { color: 16777215, font_size: 25, mode: 1, max_length: 20 },
      rooms: [],
    },
    tts_backend: "off",
    tts_dropped: 0,
    history_dir: "/tmp",
  };
}

function reset() {
  useStore.setState({
    snapshot: null,
    events: {},
    seen: {},
    activeRoom: null,
    toasts: [],
    status: "closed",
  });
}

describe("store", () => {
  beforeEach(reset);

  it("同 id 的事件只保留一条", () => {
    useStore.getState().pushEvent(chatEvent("a"));
    useStore.getState().pushEvent(chatEvent("a"));
    expect(useStore.getState().events[1]).toHaveLength(1);
  });

  it("不同事件按到达顺序追加", () => {
    useStore.getState().pushEvent(chatEvent("a", "一"));
    useStore.getState().pushEvent(chatEvent("b", "二"));
    const texts = useStore.getState().events[1].map((event) =>
      event.type === "chat" ? event.text : "",
    );
    expect(texts).toEqual(["一", "二"]);
  });

  it("超过上限时裁掉最旧的，并把 id 从去重集合里移除", () => {
    useStore.setState({ snapshot: snapshot(50) });
    // cap = max(400, max_rows*2) = 400
    for (let index = 0; index < 450; index += 1) {
      useStore.getState().pushEvent(chatEvent(`e${index}`));
    }
    const events = useStore.getState().events[1];
    expect(events).toHaveLength(400);
    expect(events[0].id).toBe("e50");
    // 被挤出去的旧 id 应该允许再次进入（否则集合只增不减）
    useStore.getState().pushEvent(chatEvent("e0"));
    expect(useStore.getState().events[1].at(-1)?.id).toBe("e0");
  });

  it("切换快照时修正 activeRoom", () => {
    const snap = snapshot();
    snap.rooms = [
      {
        room_id: 42,
        short_id: 0,
        uid: 1,
        uname: "主播",
        title: "房间",
        area: "",
        parent_area: "",
        live_status: 1,
        popularity: 0,
        attention: 0,
        cover: "",
        keyframe: "",
        live_time: "",
        description: "",
        online: null,
        online_text: "",
        connected: true,
        status: "",
        message_count: 0,
        last_event_at: null,
        error: null,
      },
    ];
    useStore.getState().applySnapshot(snap);
    expect(useStore.getState().activeRoom).toBe(42);
  });

  it("notice 变成 toast，speak 走浏览器语音（不报错）", () => {
    useStore.getState().applyServerEvent({
      type: "notice",
      data: { level: "warn", message: "触发风控" },
    });
    expect(useStore.getState().toasts[0]?.message).toBe("触发风控");
    expect(() =>
      useStore.getState().applyServerEvent({
        type: "speak",
        data: { text: "你好", kind: "chat", backend: "webview" },
      }),
    ).not.toThrow();
  });
});
