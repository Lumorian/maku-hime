import { create } from "zustand";
import type { Event, ServerEvent, Snapshot } from "./types";

export interface Toast {
  id: number;
  level: "info" | "success" | "warn" | "error";
  message: string;
}

export type ConnectionStatus = "connecting" | "open" | "closed";

interface AppStore {
  snapshot: Snapshot | null;
  events: Record<number, Event[]>;
  status: ConnectionStatus;
  activeRoom: number | null;
  toasts: Toast[];
  connectionAttempts: number;

  applyServerEvent: (event: ServerEvent) => void;
  applySnapshot: (snapshot: Snapshot) => void;
  pushEvent: (event: Event) => void;
  setStatus: (status: ConnectionStatus) => void;
  setActiveRoom: (roomId: number | null) => void;
  toast: (level: Toast["level"], message: string) => void;
  dismissToast: (id: number) => void;
  clearRoomEvents: (roomId: number) => void;
}

let toastSeq = 0;

export const useStore = create<AppStore>((set, get) => ({
  snapshot: null,
  events: {},
  status: "connecting",
  activeRoom: null,
  toasts: [],
  connectionAttempts: 0,

  applyServerEvent: (event) => {
    switch (event.type) {
      case "snapshot":
        get().applySnapshot(event.data);
        break;
      case "event":
        get().pushEvent(event.data);
        break;
      case "notice":
        get().toast(
          event.data.level === "error" ? "error" : "warn",
          event.data.message,
        );
        break;
      case "speak":
        speakIfWebview(event.data.text, event.data.backend);
        break;
      default:
        break;
    }
  },

  applySnapshot: (snapshot) => {
    const current = get();
    const roomIds = new Set(snapshot.rooms.map((room) => room.room_id));
    let activeRoom = current.activeRoom;
    if (activeRoom === null || !roomIds.has(activeRoom)) {
      activeRoom = snapshot.rooms[0]?.room_id ?? null;
    }
    set({ snapshot, activeRoom });
  },

  pushEvent: (event) => {
    const state = get();
    const cap = Math.max(400, (state.snapshot?.config.ui.max_rows ?? 300) * 2);
    const list = state.events[event.room_id] ?? [];
    if (list.some((item) => item.id === event.id)) return;
    const next = list.length >= cap ? list.slice(list.length - cap + 1) : list.slice();
    next.push(event);
    set({ events: { ...state.events, [event.room_id]: next } });
  },

  setStatus: (status) => {
    const state = get();
    if (status === "connecting") {
      set({ status, connectionAttempts: state.connectionAttempts + 1 });
      return;
    }
    set({ status });
  },

  setActiveRoom: (roomId) => set({ activeRoom: roomId }),

  toast: (level, message) => {
    const id = ++toastSeq;
    set((state) => ({ toasts: [...state.toasts, { id, level, message }] }));
    window.setTimeout(() => {
      set((state) => ({ toasts: state.toasts.filter((item) => item.id !== id) }));
    }, 4200);
  },

  dismissToast: (id) =>
    set((state) => ({ toasts: state.toasts.filter((item) => item.id !== id) })),

  clearRoomEvents: (roomId) =>
    set((state) => ({ events: { ...state.events, [roomId]: [] } })),
}));

/** WebView 后端：交给浏览器 speechSynthesis。 */
function speakIfWebview(text: string, backend: string) {
  if (backend !== "webview") return;
  if (typeof window === "undefined" || !("speechSynthesis" in window)) return;
  try {
    const utterance = new SpeechSynthesisUtterance(text);
    utterance.lang = "zh-CN";
    window.speechSynthesis.speak(utterance);
  } catch {
    // 浏览器不支持时静默失败
  }
}

export function useActiveRoom() {
  const snapshot = useStore((state) => state.snapshot);
  const activeRoom = useStore((state) => state.activeRoom);
  return snapshot?.rooms.find((room) => room.room_id === activeRoom) ?? null;
}
