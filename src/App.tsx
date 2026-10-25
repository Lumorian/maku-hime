import { useCallback, useEffect, useState } from "react";
import { api, apiBase } from "./api";
import { Composer, RoomHeader } from "./components/Header";
import { HistoryPanel } from "./components/HistoryPanel";
import { LoginModal } from "./components/LoginModal";
import { SettingsPanel } from "./components/SettingsPanel";
import { Sidebar } from "./components/Sidebar";
import { Toasts } from "./components/Toasts";
import { Wall } from "./components/Wall";
import { useSocket } from "./hooks";
import { useActiveRoom, useStore } from "./store";
import type { AppConfig } from "./types";

const isTauri = () =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

type Panel = "none" | "settings" | "history";

export default function App() {
  useSocket();
  const snapshot = useStore((state) => state.snapshot);
  const status = useStore((state) => state.status);
  const toast = useStore((state) => state.toast);
  const room = useActiveRoom();

  const [panel, setPanel] = useState<Panel>("none");
  const [loginOpen, setLoginOpen] = useState(false);
  const [overlayEnabled, setOverlayEnabled] = useState(false);

  // 主题跟随配置
  useEffect(() => {
    document.documentElement.dataset.theme = snapshot?.config.ui.theme ?? "dark";
  }, [snapshot?.config.ui.theme]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setPanel("none");
        setLoginOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // 桌面端：同步悬浮窗是否打开
  useEffect(() => {
    if (!isTauri()) return;
    void import("@tauri-apps/api/core")
      .then(({ invoke }) => invoke<boolean>("overlay_visible"))
      .then(setOverlayEnabled)
      .catch(() => undefined);
  }, []);

  const saveConfig = useCallback(
    async (next: AppConfig) => {
      const current = useStore.getState().snapshot;
      if (current) {
        useStore.getState().applySnapshot({ ...current, config: next });
      }
      try {
        const saved = await api.putConfig(next);
        const latest = useStore.getState().snapshot;
        if (latest) {
          useStore.getState().applySnapshot({ ...latest, config: saved });
        }
      } catch (error) {
        toast("error", error instanceof Error ? error.message : "保存设置失败");
      }
    },
    [toast],
  );

  const toggleOverlay = useCallback(async () => {
    if (isTauri()) {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        const visible = await invoke<boolean>("toggle_overlay", {
          roomId: room?.room_id ?? null,
        });
        setOverlayEnabled(visible);
      } catch (error) {
        toast("error", error instanceof Error ? error.message : "悬浮窗操作失败");
      }
      return;
    }
    const base = await apiBase();
    const url = `${base || ""}/overlay?surface=obs${room ? `&room=${room.room_id}` : ""}`;
    window.open(url, "maku-hime-overlay", "width=420,height=700,menubar=no,toolbar=no");
  }, [room, toast]);

  if (!snapshot) {
    return (
      <div className="boot">
        <div>
          <p>正在连接本地服务…</p>
          <p style={{ fontSize: 12, color: "var(--text-3)" }}>
            如果一直停在这里，请确认 maku-hime 服务已启动。
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="app">
      <Sidebar snapshot={snapshot} onLogin={() => setLoginOpen(true)} />

      <main className="main">
        <RoomHeader
          room={room}
          snapshot={snapshot}
          onOpenHistory={() => setPanel(panel === "history" ? "none" : "history")}
          onOpenSettings={() => setPanel(panel === "settings" ? "none" : "settings")}
          onOpenOverlay={() => void toggleOverlay()}
          overlayEnabled={overlayEnabled}
        />

        {room ? (
          <Wall roomId={room.room_id} config={snapshot.config.ui} />
        ) : (
          <div className="empty-room">在左边添加一个直播间，开始收弹幕</div>
        )}

        <Composer room={room} snapshot={snapshot} onLogin={() => setLoginOpen(true)} />
      </main>

      {panel === "settings" && (
        <SettingsPanel
          config={snapshot.config}
          snapshot={snapshot}
          room={room}
          onSave={(next) => void saveConfig(next)}
          onClose={() => setPanel("none")}
          onOpenOverlay={() => void toggleOverlay()}
          overlayEnabled={overlayEnabled}
        />
      )}

      {panel === "history" && <HistoryPanel room={room} onClose={() => setPanel("none")} />}

      {loginOpen && <LoginModal onClose={() => setLoginOpen(false)} />}

      <Toasts />
      <div className={`conn-pill conn-${status}`}>
        {status === "open" ? "● 已连接本地服务" : status === "connecting" ? "○ 连接中…" : "○ 连接已断开"}
      </div>
    </div>
  );
}
