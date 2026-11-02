import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { Wall } from "./Wall";
import { useStore } from "../store";
import type { Event, UiConfig } from "../types";

const CONFIG: UiConfig = {
  theme: "dark",
  font_size: 13,
  opacity: 1,
  max_rows: 3,
  show_avatar: true,
  show_enter: false,
  show_like: false,
  show_gift: true,
  compact: false,
};

function baseUser(patch: Partial<Event extends { type: "chat" } ? never : object> = {}) {
  return {
    uid: 1,
    name: "观众甲",
    face: "",
    level: 3,
    guard: "none" as const,
    medal: null,
    admin: false,
    vip: false,
    name_color: null,
    ...patch,
  };
}

function chat(
  id: string,
  text: string,
  patch: { medal?: boolean; name?: string; emots?: { text: string; url: string; width: number; height: number }[] } = {},
): Event {
  return {
    id,
    ts: 1,
    room_id: 1,
    type: "chat",
    user: {
      ...baseUser(),
      name: patch.name ?? "观众甲",
      medal: patch.medal
        ? {
            name: "会考上",
            level: 14,
            anchor: "主播",
            room_id: 9,
            start: "#5C968E",
            end: "#5C968E",
            border: "#5C968E",
            text: "#ffffff",
            guard: "none",
            light: true,
          }
        : null,
    },
    text,
    emoticon: null,
    emots: patch.emots ?? [],
  };
}

function enter(id: string): Event {
  return { id, ts: 1, room_id: 1, type: "enter", user: baseUser(), kind: "enter" };
}

function seed(events: Event[], config: Partial<UiConfig> = {}, variant: "panel" | "bili" = "panel") {
  useStore.setState({
    snapshot: null,
    events: { 1: events },
    seen: {},
    activeRoom: 1,
  });
  return render(<Wall roomId={1} config={{ ...CONFIG, ...config }} variant={variant} />);
}

function mockScrollMetrics(element: HTMLElement, height: number, viewport: number) {
  Object.defineProperty(element, "scrollHeight", { configurable: true, value: height });
  Object.defineProperty(element, "clientHeight", { configurable: true, value: viewport });
}

describe("Wall", () => {
  beforeEach(() => {
    useStore.setState({ events: {}, seen: {}, snapshot: null });
    // jsdom 没有实现 scrollTo
    HTMLElement.prototype.scrollTo = vi.fn();
  });

  it("渲染聊天弹幕、昵称与粉丝勋章", () => {
    seed([chat("a", "这波操作太顶了", { medal: true })]);
    expect(screen.getByText("这波操作太顶了")).toBeInTheDocument();
    expect(screen.getByText("观众甲")).toBeInTheDocument();
    expect(screen.getByText("会考上")).toBeInTheDocument();
  });

  it("正文里的内联小表情渲染成图片，而不是 [滑稽] 文本", () => {
    seed([
      chat("a", "哈哈[滑稽]", {
        emots: [{ text: "[滑稽]", url: "https://i0.hdslb.com/e.png", width: 20, height: 20 }],
      }),
    ]);
    expect(screen.queryByText("哈哈[滑稽]")).toBeNull();
    const image = screen.getByTitle("[滑稽]");
    expect(image.tagName).toBe("IMG");
    expect(image).toHaveAttribute("src", "https://i0.hdslb.com/e.png");
    expect(screen.getByText("哈哈")).toBeInTheDocument();
  });

  it("按配置过滤进房消息", () => {
    const events = [chat("a", "正常弹幕"), enter("b")];
    const { unmount } = seed(events, { show_enter: false });
    expect(screen.queryByText(/进入直播间/)).not.toBeInTheDocument();
    unmount();

    seed(events, { show_enter: true });
    expect(screen.getByText(/进入直播间/)).toBeInTheDocument();
  });

  it("只渲染最近 max_rows 条", () => {
    seed([chat("a", "第一条"), chat("b", "第二条"), chat("c", "第三条"), chat("d", "第四条")], {
      max_rows: 3,
    });
    expect(screen.queryByText("第一条")).not.toBeInTheDocument();
    expect(screen.getByText("第四条")).toBeInTheDocument();
  });

  it("上翻后新弹幕只计数、不打断阅读，点药丸回到最新", () => {
    const { container } = seed([chat("a", "旧消息")], { max_rows: 50 });
    const scroller = container.querySelector(".wall-scroll") as HTMLElement;
    const pill = () => container.querySelector(".wall-new-pill") as HTMLElement | null;
    mockScrollMetrics(scroller, 1000, 500);

    // 用户往上翻：此时还没有新弹幕，不应出现提示
    scroller.scrollTop = 0;
    fireEvent.scroll(scroller);
    expect(pill()).toBeNull();

    // 来了一条新弹幕：不应该自动跳到底部，而是出现提示
    act(() => {
      useStore.getState().pushEvent(chat("b", "新消息"));
    });
    expect(screen.getByText("新消息")).toBeInTheDocument();
    expect(pill()?.textContent).toContain("1");
    expect(pill()?.textContent).toContain("条新弹幕");

    // 点一下回到最新，提示消失
    fireEvent.click(pill() as HTMLElement);
    expect(pill()).toBeNull();
  });

  it("bili 皮肤：官方排版（半角冒号分隔、头像列、滚动条）与回到最新药丸", () => {
    const { container } = seed([chat("a", "旧消息", { medal: true })], { max_rows: 50 }, "bili");
    // 皮肤根类与自定义滚动条
    expect(container.querySelector(".wall-bd")).not.toBeNull();
    expect(container.querySelector(".wall-scrollbar")).not.toBeNull();
    // 昵称与正文之间用官方的半角冒号（两侧各一个空格）
    expect(container.querySelector(".chat-sep")?.textContent).toBe(" : ");
    // 行首 24px 头像（正文列由 CSS 的 padding-left 29px 提供）
    expect(container.querySelector(".chat-item .chat-avatar")).not.toBeNull();

    const scroller = container.querySelector(".wall-scroll") as HTMLElement;
    mockScrollMetrics(scroller, 1000, 500);
    scroller.scrollTop = 200;
    fireEvent.scroll(scroller);
    // bili-dm：上翻就出现 84x20 药丸，没有新弹幕时文案是回到最新
    const pill = container.querySelector(".wall-new-pill") as HTMLElement;
    expect(pill.className).toContain("is-bili");
    expect(pill.textContent).toBe("↓ 回到最新");
  });
});
