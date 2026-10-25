import { useState } from "react";
import { api } from "../api";
import { useStore } from "../store";
import type { Snapshot } from "../types";
import { cn, formatCount } from "../util";
import { IconPlus, IconTrash, Logo } from "./icons";

interface SidebarProps {
  snapshot: Snapshot;
  onLogin: () => void;
}

export function Sidebar({ snapshot, onLogin }: SidebarProps) {
  const activeRoom = useStore((state) => state.activeRoom);
  const setActiveRoom = useStore((state) => state.setActiveRoom);
  const toast = useStore((state) => state.toast);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);

  const addRoom = async (event: React.FormEvent) => {
    event.preventDefault();
    const raw = input.trim();
    const roomId = Number(raw.replace(/^https?:\/\/live\.bilibili\.com\//, ""));
    if (!Number.isFinite(roomId) || roomId <= 0) {
      toast("warn", "请输入正确的直播间号");
      return;
    }
    setBusy(true);
    try {
      const room = await api.addRoom(roomId);
      setInput("");
      setActiveRoom(room.room_id);
      toast("success", "已订阅 " + (room.uname || room.room_id));
    } catch (error) {
      toast("error", error instanceof Error ? error.message : "订阅失败");
    } finally {
      setBusy(false);
    }
  };

  const removeRoom = async (roomId: number, uname: string) => {
    try {
      await api.removeRoom(roomId);
      toast("info", "已取消订阅 " + (uname || roomId));
    } catch (error) {
      toast("error", error instanceof Error ? error.message : "取消失败");
    }
  };

  const copyServer = async () => {
    try {
      await navigator.clipboard.writeText(snapshot.server_url);
      toast("success", "已复制服务地址");
    } catch {
      toast("warn", "复制失败，请手动复制 " + snapshot.server_url);
    }
  };

  return (
    <aside className="sidebar">
      <div className="brand">
        <Logo />
        <div className="brand-text">
          <strong>幕姬</strong>
          <span>maku-hime</span>
        </div>
      </div>

      <div className="sidebar-heading">
        <span>直播间</span>
        <em>{snapshot.rooms.length}</em>
      </div>

      <ul className="room-list">
        {snapshot.rooms.map((room) => {
          const live = room.live_status === 1;
          return (
            <li
              key={room.room_id}
              className={cn("room-item", room.room_id === activeRoom && "is-active")}
            >
              <button
                type="button"
                className="room-select"
                aria-current={room.room_id === activeRoom}
                onClick={() => setActiveRoom(room.room_id)}
              >
                <span className={cn("live-dot", live && "is-live", room.connected && "is-connected")} />
                <span className="room-item-main">
                  <span className="room-item-title">{room.title || room.uname || `房间 ${room.room_id}`}</span>
                  <span className="room-item-sub">
                    {room.uname || "—"}
                    {room.online != null ? ` · ${formatCount(room.online)} 在线` : ""}
                  </span>
                </span>
              </button>
              <button
                type="button"
                className="room-remove"
                title="取消订阅"
                aria-label={`取消订阅 ${room.uname || room.room_id}`}
                onClick={() => void removeRoom(room.room_id, room.uname)}
              >
                <IconTrash size={14} />
              </button>
            </li>
          );
        })}
        {snapshot.rooms.length === 0 && (
          <li className="room-empty">还没有订阅房间，先加一个正在直播的房间号。</li>
        )}
      </ul>

      <form className="add-room" onSubmit={addRoom}>
        <input
          value={input}
          onChange={(event) => setInput(event.target.value)}
          placeholder="房间号 / 直播间链接"
          inputMode="numeric"
          disabled={busy}
        />
        <button type="submit" disabled={busy || !input.trim()} title="订阅">
          <IconPlus size={16} />
        </button>
      </form>

      <div className="sidebar-footer">
        <button type="button" className="account" onClick={onLogin}>
          {snapshot.login.logged_in ? (
            <>
              {snapshot.login.face ? (
                <img src={snapshot.login.face} alt="" referrerPolicy="no-referrer" />
              ) : (
                <span className="account-fallback">{snapshot.login.uname.slice(0, 1)}</span>
              )}
              <span className="account-name">{snapshot.login.uname || "已登录"}</span>
            </>
          ) : (
            <>
              <span className="account-fallback">?</span>
              <span className="account-name">扫码登录</span>
            </>
          )}
        </button>
        <button type="button" className="server-url" onClick={copyServer} title="点击复制">
          {snapshot.server_url || "本地服务"}
        </button>
      </div>
    </aside>
  );
}
