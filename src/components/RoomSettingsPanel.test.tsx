import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen } from "@testing-library/react";
import type { RoomSettings, Snapshot } from "../types";
import { RoomSettingsPanel } from "./RoomSettingsPanel";

const roomSettings = vi.fn();
const roomAreas = vi.fn();
const saveRoomSettings = vi.fn();
const uploadCover = vi.fn();

vi.mock("../api", async () => {
  const actual = await vi.importActual<typeof import("../api")>("../api");
  return {
    ...actual,
    api: {
      ...actual.api,
      roomSettings: (...args: unknown[]) => roomSettings(...args),
      roomAreas: (...args: unknown[]) => roomAreas(...args),
      saveRoomSettings: (...args: unknown[]) => saveRoomSettings(...args),
      uploadCover: (...args: unknown[]) => uploadCover(...args),
    },
  };
});

const settings: RoomSettings = {
  room_id: 7734200,
  title: "旧标题",
  area_id: 371,
  area_name: "虚拟主播",
  parent_name: "虚拟",
  cover: "https://i0.hdslb.com/bfs/live/a.jpg",
  cover_status: 0,
  cover_reason: "",
  announcement: "旧公告",
  live_status: 1,
};

const room = {
  room_id: 7734200,
  uid: 123,
  uname: "主播",
  title: "旧标题",
  live_status: 1,
} as unknown as Parameters<typeof RoomSettingsPanel>[0]["room"];

const snapshot = {
  login: { logged_in: true, uid: 123, uname: "主播", face: "" },
} as unknown as Snapshot;

async function renderPanel() {
  await act(async () => {
    render(<RoomSettingsPanel room={room} snapshot={snapshot} onClose={() => {}} />);
  });
}

describe("RoomSettingsPanel", () => {
  beforeEach(() => {
    roomSettings.mockReset();
    roomAreas.mockReset();
    saveRoomSettings.mockReset();
    uploadCover.mockReset();
    roomSettings.mockResolvedValue(settings);
    roomAreas.mockResolvedValue({
      areas: [
        { id: 371, name: "虚拟主播", parent: "虚拟" },
        { id: 86, name: "游戏赛事", parent: "赛事" },
      ],
      recent: [{ id: 86, name: "游戏赛事", parent: "赛事" }],
    });
  });

  it("读出当前标题、分区与公告", async () => {
    await renderPanel();

    expect(await screen.findByDisplayValue("旧标题")).toBeInTheDocument();
    expect(screen.getByDisplayValue("旧公告")).toBeInTheDocument();
    // 当前分区在大分区/小分区两个下拉里都要对上
    expect(screen.getByDisplayValue("虚拟")).toBeInTheDocument();
    expect(screen.getByDisplayValue("虚拟主播")).toBeInTheDocument();
    // 最近用过的分区给快捷入口
    expect(screen.getByRole("button", { name: "赛事 · 游戏赛事" })).toBeInTheDocument();
  });

  it("只提交改动过的字段", async () => {
    saveRoomSettings.mockResolvedValue({ ...settings, title: "新标题" });
    await renderPanel();

    const input = await screen.findByDisplayValue("旧标题");
    fireEvent.change(input, { target: { value: "新标题" } });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /保存标题/ }));
    });

    expect(saveRoomSettings).toHaveBeenCalledWith(7734200, { title: "新标题" });
  });

  it("没有改动时不请求保存", async () => {
    await renderPanel();
    await screen.findByDisplayValue("旧标题");

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /保存标题/ }));
    });

    expect(saveRoomSettings).not.toHaveBeenCalled();
  });

  it("把封面审核状态显示清楚", async () => {
    roomSettings.mockResolvedValue({
      ...settings,
      cover_status: -1,
      cover_reason: "图片含二维码",
    });
    await renderPanel();

    expect(await screen.findByText("未通过：图片含二维码")).toBeInTheDocument();
  });
});
