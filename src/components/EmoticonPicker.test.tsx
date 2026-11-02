import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { EmoticonPicker } from "./EmoticonPicker";

const emoticons = vi.fn();

vi.mock("../api", async () => {
  const actual = await vi.importActual<typeof import("../api")>("../api");
  return {
    ...actual,
    api: { ...actual.api, emoticons: (...args: unknown[]) => emoticons(...args) },
  };
});

const packages = [
  {
    id: 1,
    name: "官方表情",
    kind: 1,
    emoticons: [
      {
        id: 11,
        unique_name: "official_11",
        text: "[笑]",
        url: "https://i0.hdslb.com/a.png",
        width: 20,
        height: 20,
        perm: 0,
      },
    ],
  },
  {
    id: 3,
    name: "小黄脸",
    kind: 100,
    emoticons: [
      {
        id: 31,
        unique_name: "",
        text: "[滑稽]",
        url: "https://i0.hdslb.com/c.png",
        width: 0,
        height: 0,
        perm: 0,
      },
    ],
  },
  {
    id: 2,
    name: "粉丝牌",
    kind: 2,
    emoticons: [
      {
        id: 21,
        unique_name: "room_21",
        text: "[妙啊]",
        url: "https://i0.hdslb.com/b.png",
        width: 60,
        height: 60,
        perm: 1,
      },
    ],
  },
];

describe("EmoticonPicker", () => {
  beforeEach(() => {
    emoticons.mockReset();
    emoticons.mockResolvedValue(packages);
  });

  it("按表情包分页签展示表情", async () => {
    await act(async () => {
      render(<EmoticonPicker roomId={7734200} onPick={() => {}} onInsert={() => {}} onClose={() => {}} />);
    });

    expect(await screen.findByRole("tab", { name: "官方表情" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "粉丝牌" })).toBeInTheDocument();
    expect(screen.getByTitle("[笑]（点击直接发送）")).toBeInTheDocument();
  });

  it("「全部」页签把各包表情摊平去重", async () => {
    await act(async () => {
      render(<EmoticonPicker roomId={7734200} onPick={() => {}} onInsert={() => {}} onClose={() => {}} />);
    });

    const all = await screen.findByRole("tab", { name: "全部" });
    expect(all).toHaveAttribute("aria-selected", "true");
    // 两个包里的表情都要出现在「全部」里
    expect(screen.getByTitle("[笑]（点击直接发送）")).toBeInTheDocument();
    expect(screen.getByTitle("[妙啊]（点击直接发送）")).toBeInTheDocument();
  });

  it("点表情直接把它交给发送逻辑", async () => {
    const onPick = vi.fn();
    await act(async () => {
      render(<EmoticonPicker roomId={7734200} onPick={onPick} onInsert={() => {}} onClose={() => {}} />);
    });

    await act(async () => {
      fireEvent.click(await screen.findByTitle("[笑]（点击直接发送）"));
    });

    expect(onPick).toHaveBeenCalledTimes(1);
    expect(onPick.mock.calls[0][0].unique_name).toBe("official_11");
  });

  it("只有文本的表情插进输入框，不直接发送", async () => {
    const onPick = vi.fn();
    const onInsert = vi.fn();
    await act(async () => {
      render(
        <EmoticonPicker
          roomId={7734200}
          onPick={onPick}
          onInsert={onInsert}
          onClose={() => {}}
        />,
      );
    });

    await act(async () => {
      fireEvent.click(await screen.findByTitle("[滑稽]（点击插入输入框）"));
    });

    expect(onInsert).toHaveBeenCalledWith("[滑稽]");
    expect(onPick).not.toHaveBeenCalled();
  });

  it("页签行用滚轮横向滚动，不再被原生滚动条盖住", async () => {
    await act(async () => {
      render(<EmoticonPicker roomId={7734200} onPick={() => {}} onInsert={() => {}} onClose={() => {}} />);
    });

    const row = await screen.findByRole("tablist");
    // jsdom 不做布局：手动造出「内容比可视区宽」的状态
    Object.defineProperty(row, "scrollWidth", { configurable: true, value: 400 });
    Object.defineProperty(row, "clientWidth", { configurable: true, value: 380 });
    (row as HTMLElement).scrollLeft = 0;

    const wheel = new WheelEvent("wheel", { deltaY: 120, cancelable: true, bubbles: true });
    await act(async () => {
      row.dispatchEvent(wheel);
    });

    expect((row as HTMLElement).scrollLeft).toBe(120);
    // preventDefault 生效说明挂的是非 passive 监听，滚轮不会顺带滚走弹幕墙
    expect(wheel.defaultPrevented).toBe(true);
  });

  it("加载失败时给出原因而不是空白面板", async () => {
    emoticons.mockRejectedValue(new Error("账号未登录"));
    await act(async () => {
      render(<EmoticonPicker roomId={7734200} onPick={() => {}} onInsert={() => {}} onClose={() => {}} />);
    });

    expect(await screen.findByText("账号未登录")).toBeInTheDocument();
  });
});
