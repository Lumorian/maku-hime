import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import { useStore } from "../store";
import type { Event, RoomState } from "../types";
import { cn, clockOf, eventSummary, formatPrice } from "../util";
import { IconClose, IconTrash } from "./icons";

const KINDS: Array<{ value: string; label: string }> = [
  { value: "chat", label: "弹幕" },
  { value: "gift", label: "礼物" },
  { value: "guard", label: "上舰" },
  { value: "super_chat", label: "醒目留言" },
  { value: "enter", label: "进房" },
  { value: "like", label: "点赞" },
];

interface HistoryPanelProps {
  room: RoomState | null;
  onClose: () => void;
}

export function HistoryPanel({ room, onClose }: HistoryPanelProps) {
  const toast = useStore((state) => state.toast);
  const [kinds, setKinds] = useState<string[]>(["chat", "gift", "guard", "super_chat"]);
  const [keyword, setKeyword] = useState("");
  // 输入时不必每个按键都打一次接口
  const [debouncedKeyword, setDebouncedKeyword] = useState("");
  const [events, setEvents] = useState<Event[]>([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    const timer = window.setTimeout(() => setDebouncedKeyword(keyword), 250);
    return () => window.clearTimeout(timer);
  }, [keyword]);

  const load = useCallback(async () => {
    if (!room) return;
    setLoading(true);
    try {
      const list = await api.history(room.room_id, kinds, debouncedKeyword, 800);
      setEvents(list);
    } catch (error) {
      toast("error", error instanceof Error ? error.message : "读取历史失败");
    } finally {
      setLoading(false);
    }
  }, [room, kinds, debouncedKeyword, toast]);

  useEffect(() => {
    void load();
  }, [load]);

  const toggleKind = (kind: string) => {
    setKinds((current) =>
      current.includes(kind) ? current.filter((item) => item !== kind) : [...current, kind],
    );
  };

  const exportAs = async (format: "csv" | "json" | "jsonl") => {
    if (!room) return;
    const url = await api.exportHistoryUrl(room.room_id, format);
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = "";
    document.body.appendChild(anchor);
    anchor.click();
    anchor.remove();
  };

  const clear = async () => {
    if (!room) return;
    if (!window.confirm(`确定清空房间 ${room.room_id} 的本地历史？`)) return;
    try {
      await api.clearHistory(room.room_id);
      setEvents([]);
      toast("success", "历史已清空");
    } catch (error) {
      toast("error", error instanceof Error ? error.message : "清空失败");
    }
  };

  return (
    <aside className="drawer">
      <header className="drawer-header">
        <h2>历史记录</h2>
        <button type="button" className="icon-button" onClick={onClose} title="关闭">
          <IconClose size={16} />
        </button>
      </header>

      <div className="history-toolbar">
        <div className="chips">
          {KINDS.map((kind) => (
            <button
              key={kind.value}
              type="button"
              className={cn("chip", kinds.includes(kind.value) && "is-active")}
              onClick={() => toggleKind(kind.value)}
            >
              {kind.label}
            </button>
          ))}
        </div>
        <input
          className="history-search"
          value={keyword}
          placeholder="搜索昵称 / 内容"
          onChange={(event) => setKeyword(event.target.value)}
        />
        <div className="history-actions">
          <button type="button" onClick={() => void exportAs("csv")}>
            导出 CSV
          </button>
          <button type="button" onClick={() => void exportAs("json")}>
            导出 JSON
          </button>
          <button type="button" className="danger" onClick={() => void clear()}>
            <IconTrash size={14} />
            清空
          </button>
        </div>
      </div>

      <div className="history-list">
        {loading && <p className="history-hint">读取中…</p>}
        {!loading && events.length === 0 && (
          <p className="history-hint">没有匹配的记录。历史只保存在本机，最多保留最近 2000 条。</p>
        )}
        {events.map((event) => (
          <div key={event.id} className={cn("history-row", `is-${event.type}`)}>
            <span className="history-time">{clockOf(event.ts)}</span>
            <span className="history-user">{eventUserName(event)}</span>
            <span className="history-text">{eventSummary(event)}</span>
            {event.type === "super_chat" && (
              <span className="history-price">¥{formatPrice(event.price)}</span>
            )}
          </div>
        ))}
      </div>
    </aside>
  );
}

function eventUserName(event: Event): string {
  switch (event.type) {
    case "chat":
    case "gift":
    case "guard":
    case "super_chat":
    case "enter":
      return event.user.name || "—";
    case "like":
      return event.user?.name ?? "观众";
    default:
      return "";
  }
}
