import { useState } from "react";
import { api } from "../api";
import { useStore } from "../store";
import type { RoomState, Snapshot } from "../types";
import { cn, formatCount, liveBadge, onlineLabel, timeAgo } from "../util";
import {
  IconHistory,
  IconRefresh,
  IconSend,
  IconSettings,
  IconWindow,
} from "./icons";

interface RoomHeaderProps {
  room: RoomState | null;
  snapshot: Snapshot | null;
  onOpenHistory: () => void;
  onOpenSettings: () => void;
  onOpenOverlay: () => void;
  overlayEnabled: boolean;
}

export function RoomHeader({
  room,
  snapshot,
  onOpenHistory,
  onOpenSettings,
  onOpenOverlay,
  overlayEnabled,
}: RoomHeaderProps) {
  const toast = useStore((state) => state.toast);
  const [refreshing, setRefreshing] = useState(false);

  if (!room) {
    return (
      <header className="room-header">
        <div className="room-title">
          <h1>maku-hime</h1>
          <span className="badge badge-muted">未订阅房间</span>
        </div>
        <div className="header-actions">
          <button type="button" onClick={onOpenSettings} className="ghost">
            <IconSettings size={15} />
            设置
          </button>
        </div>
      </header>
    );
  }

  const badge = liveBadge(room);

  const refresh = async () => {
    setRefreshing(true);
    try {
      await api.refreshRoom(room.room_id);
      toast("success", "房间信息已刷新");
    } catch (error) {
      toast("error", error instanceof Error ? error.message : "刷新失败");
    } finally {
      setRefreshing(false);
    }
  };

  const connection = room.connected
    ? "已连接"
    : room.error
      ? "连接中断"
      : "连接中…";

  return (
    <header className="room-header">
      <div className="room-title">
        <span className={cn("badge", badge.dot === "live" ? "badge-live" : "badge-muted")}>
          <span className={cn("live-dot", badge.dot === "live" && "is-live")} />
          {badge.text}
          {room.live_status === 1 && room.online_text ? ` · ${onlineLabel(room)}` : ""}
        </span>
        <h1 title={room.title}>{room.title || `房间 ${room.room_id}`}</h1>
        <span className={cn("conn", room.connected ? "is-on" : "is-off")}>{connection}</span>
      </div>

      <div className="room-meta">
        <span title="主播">{room.uname || "未知主播"}</span>
        <span title="分区">
          {room.parent_area ? `${room.parent_area} · ${room.area}` : room.area || "—"}
        </span>
        <span title="人气值（热度）">人气 {formatCount(room.popularity)}</span>
        <span title="粉丝数">粉丝 {formatCount(room.attention)}</span>
        {room.online != null && (
          <span title="高能榜在线人数">在线 {room.online_text || formatCount(room.online)}</span>
        )}
        <span title="本房间收到的事件数">{room.message_count} 条</span>
        <span title="最近一条弹幕时间">最近 {timeAgo(room.last_event_at)}</span>
      </div>

      <div className="header-actions">
        <button type="button" className="ghost" onClick={refresh} disabled={refreshing}>
          <IconRefresh size={15} />
          刷新
        </button>
        <button type="button" className="ghost" onClick={onOpenHistory}>
          <IconHistory size={15} />
          历史
        </button>
        <button type="button" className="ghost" onClick={onOpenOverlay}>
          <IconWindow size={15} />
          {overlayEnabled ? "关闭悬浮窗" : "悬浮窗"}
        </button>
        <button type="button" className="ghost" onClick={onOpenSettings}>
          <IconSettings size={15} />
          设置
        </button>
      </div>

      {snapshot?.login.logged_in === false && (
        <div className="header-hint">只看弹幕不需要登录；想发弹幕请先在左下角扫码登录。</div>
      )}
    </header>
  );
}

interface ComposerProps {
  room: RoomState | null;
  snapshot: Snapshot | null;
  onLogin: () => void;
}

export function Composer({ room, snapshot, onLogin }: ComposerProps) {
  const toast = useStore((state) => state.toast);
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);

  const maxLength = snapshot?.config.send.max_length ?? 20;
  const loggedIn = snapshot?.login.logged_in ?? false;
  const disabled = !room || !loggedIn || sending;
  const remaining = maxLength - text.length;

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!room || !text.trim() || sending) return;
    setSending(true);
    try {
      await api.send(room.room_id, text.trim());
      setText("");
    } catch (error) {
      const message = error instanceof Error ? error.message : "发送失败";
      toast("error", message);
    } finally {
      setSending(false);
    }
  };

  return (
    <form className="composer" onSubmit={submit}>
      <input
        value={text}
        onChange={(event) => setText(event.target.value)}
        maxLength={maxLength + 10}
        placeholder={
          !room
            ? "先订阅一个直播间"
            : loggedIn
              ? "说点什么…（回车发送）"
              : "扫码登录后才能发弹幕"
        }
        disabled={disabled || !loggedIn}
        aria-label="弹幕输入框"
      />
      <span className={cn("composer-counter", remaining < 0 && "is-over")}>
        {text.length}/{maxLength}
      </span>
      {loggedIn ? (
        <button type="submit" disabled={disabled || !text.trim()}>
          <IconSend size={15} />
          发送
        </button>
      ) : (
        <button type="button" onClick={onLogin}>
          扫码登录
        </button>
      )}
    </form>
  );
}
