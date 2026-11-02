import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { ApiError } from "../api";
import type { RoomState, Snapshot } from "../types";
import { LivePanel } from "./LivePanel";

const startLive = vi.fn();

vi.mock("../api", async () => {
  const actual = await vi.importActual<typeof import("../api")>("../api");
  return {
    ...actual,
    api: { ...actual.api, startLive: (...args: unknown[]) => startLive(...args) },
  };
});

function room(patch: Partial<RoomState> = {}): RoomState {
  return {
    room_id: 7734200,
    short_id: 0,
    uid: 123,
    uname: "主播",
    title: "测试直播间",
    area: "虚拟主播",
    area_id: 371,
    parent_area: "虚拟",
    parent_area_id: 9,
    live_status: 0,
    popularity: 0,
    attention: 10,
    cover: "",
    keyframe: "",
    live_time: "",
    description: "",
    online: null,
    online_text: "",
    connected: true,
    status: "ok",
    message_count: 0,
    last_event_at: null,
    error: null,
    ...patch,
  };
}

const snapshot = {
  login: { logged_in: true, uid: 123, uname: "主播", face: "" },
} as unknown as Snapshot;

/** 点「开始直播」，并把失败后的异步状态更新一起冲掉，免得 act 警告刷屏。 */
async function clickStart() {
  render(<LivePanel room={room()} snapshot={snapshot} onClose={() => {}} />);
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: /开始直播/ }));
  });
}

describe("LivePanel", () => {
  beforeEach(() => {
    startLive.mockReset();
  });

  it("60034 时给出换直播姬开播的指引", async () => {
    startLive.mockRejectedValue(
      new ApiError("B 站当前只对直播姬开放开播入口", "live_failed", 60034),
    );

    await clickStart();

    expect(screen.getByText(/收给了直播姬/)).toBeInTheDocument();
    // 指引里要写清楚怎么继续用 OBS 推流，而不是只说一句“失败”
    expect(screen.getByText(/填进 OBS「设置 → 直播 → 自定义」/)).toBeInTheDocument();
  });

  it("别的开播错误只显示原文案，不弹直播姬指引", async () => {
    startLive.mockRejectedValue(
      new ApiError("开播过于频繁，请稍后再试", "live_failed", 60063),
    );

    await clickStart();

    expect(screen.getByText("开播过于频繁，请稍后再试")).toBeInTheDocument();
    expect(screen.queryByText(/收给了直播姬/)).not.toBeInTheDocument();
  });

  it("被要求人脸认证时给出认证页和重试入口", async () => {
    startLive.mockResolvedValue({
      status: "verify",
      verify: {
        code: 60043,
        message: "开播需要实名认证",
        url: "https://www.bilibili.com/h5/risk-control/realname?t=abc",
      },
    });

    await clickStart();

    expect(screen.getByText("这次开播需要人脸认证")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /完成认证，重新开播/ })).toBeInTheDocument();
    // 认证页链接要能复制出来，二维码画不出来时还有这条路
    expect(screen.getByTitle("复制认证链接")).toBeInTheDocument();
  });

  it("拿到推流后显示地址与密钥", async () => {
    startLive.mockResolvedValue({
      status: "stream",
      stream: {
        room_id: 7734200,
        addr: "rtmp://live-push.bilivideo.com/live-bvc/",
        code: "?streamname=live_1&key=abc",
        protocol: "rtmp",
        protocols: [
          {
            protocol: "rtmp",
            addr: "rtmp://live-push.bilivideo.com/live-bvc/",
            code: "?streamname=live_1&key=abc",
          },
        ],
      },
    });

    await clickStart();

    expect(screen.getByDisplayValue("rtmp://live-push.bilivideo.com/live-bvc/")).toBeInTheDocument();
    expect(screen.getByTitle("复制推流密钥")).toBeInTheDocument();
  });
});
