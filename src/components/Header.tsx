import { useRef, useState } from "react";
import { api } from "../api";
import { useStore } from "../store";
import type { RoomState, Snapshot } from "../types";
import type { Emoticon } from "../types";
import { cn, formatCount, liveBadge, onlineLabel, timeAgo } from "../util";
import { EmoticonPicker } from "./EmoticonPicker";
import {
  IconBroadcast,
  IconHistory,
  IconRefresh,
  IconSend,
  IconSettings,
  IconSliders,
  IconSmile,
  IconUnlock,
  IconWindow,
} from "./icons";

interface RoomHeaderProps {
  room: RoomState | null;
  snapshot: Snapshot | null;
  onOpenHistory: () => void;
  onOpenSettings: () => void;
  onOpenOverlay: () => void;
  onUnlockOverlay: () => void;
  onOpenLive: () => void;
  onOpenRoomSettings: () => void;
  overlayEnabled: boolean;
}

export function RoomHeader({
  room,
  snapshot,
  onOpenHistory,
  onOpenSettings,
  onOpenOverlay,
  onUnlockOverlay,
  onOpenLive,
  onOpenRoomSettings,
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
        {snapshot?.login.logged_in && snapshot.login.uid === room.uid && (
          <button
            type="button"
            className={cn("ghost", "live-button", room.live_status === 1 && "is-live")}
            onClick={onOpenLive}
            title={room.live_status === 1 ? "直播中：查看推流信息或停播" : "开播并获取推流地址"}
          >
            <IconBroadcast size={15} />
            {room.live_status === 1 ? "直播中" : "开播"}
          </button>
        )}
        {snapshot?.login.logged_in && snapshot.login.uid === room.uid && (
          <button
            type="button"
            className="ghost"
            onClick={onOpenRoomSettings}
            title="改直播间标题、分区、封面与公告"
          >
            <IconSliders size={15} />
            直播间设置
          </button>
        )}
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

      {snapshot?.config.overlay.click_through && (
        <div className="header-hint">
          悬浮窗已锁定（点击穿透），需要拖动或直接发弹幕时
          <button type="button" className="hint-link" onClick={onUnlockOverlay}>
            <IconUnlock size={12} />
            解锁悬浮窗
          </button>
        </div>
      )}

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
  const [pickerOpen, setPickerOpen] = useState(false);
  const inputRef = useRef<HTMLInputElement | null>(null);

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

  /** 大表情点一下直接发出去，与官方面板一致。 */
  const sendEmoticon = async (emoticon: Emoticon) => {
    if (!room) return;
    try {
      await api.sendEmoticon(room.room_id, emoticon);
    } catch (error) {
      toast("error", error instanceof Error ? error.message : "表情发送失败");
    }
  };

  /** 小黄脸这类文本表情插到光标处，方便一次插好几个再发。 */
  const insertEmoticon = (value: string) => {
    if (!value) return;
    const input = inputRef.current;
    const start = input?.selectionStart ?? text.length;
    const end = input?.selectionEnd ?? start;
    const next = `${text.slice(0, start)}${value}${text.slice(end)}`.slice(0, maxLength);
    const caret = Math.min(start + value.length, next.length);
    setText(next);
    requestAnimationFrame(() => {
      input?.focus();
      input?.setSelectionRange(caret, caret);
    });
  };

  return (
    <div className="composer-shell">
      {pickerOpen && room && loggedIn && (
        <EmoticonPicker
          key={room.room_id}
          roomId={room.room_id}
          onPick={sendEmoticon}
          onInsert={insertEmoticon}
          onClose={() => setPickerOpen(false)}
        />
      )}
      <form className="composer" onSubmit={submit}>
        <input
          ref={inputRef}
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
        {loggedIn && room && (
          <button
            type="button"
            className={cn("composer-emoji", pickerOpen && "is-active")}
            onClick={() => setPickerOpen((open) => !open)}
            title="表情"
            aria-label="表情"
            aria-expanded={pickerOpen}
          >
            <IconSmile size={16} />
          </button>
        )}
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
    </div>
  );
}
