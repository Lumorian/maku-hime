import { useEffect, useMemo } from "react";
import { api } from "./api";
import { useSocket } from "./hooks";
import { useActiveRoom, useStore } from "./store";
import { Wall } from "./components/Wall";
import type { OverlayConfig, UiConfig } from "./types";
import { cn, formatCount, onlineLabel } from "./util";
import { IconClose, IconLock, IconSettings, IconUnlock } from "./components/icons";

const isTauri = () =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

type Surface = "overlay" | "obs";

/**
 * 弹幕墙页面：
 * - surface=overlay：桌面悬浮窗（Tauri 窗口，带拖拽/锁定条）
 * - surface=obs：OBS 浏览器源（纯弹幕墙，透明底、无边框）
 */
export default function OverlayPage() {
  useSocket();
  const snapshot = useStore((state) => state.snapshot);
  const setActiveRoom = useStore((state) => state.setActiveRoom);
  const room = useActiveRoom();

  const params = useMemo(() => new URLSearchParams(window.location.search), []);
  const surface = (params.get("surface") as Surface) ?? (isTauri() ? "overlay" : "obs");
  // OBS 页面默认透明；直接浏览器预览时可以加 &bg=dark 让背景可读
  const backdrop = params.get("bg");
  const roomParam = Number(params.get("room") ?? 0);
  // 锁定（点击穿透）状态直接来自配置，避免「本地状态 + 服务端配置」两份真相
  const locked = snapshot?.config.overlay.click_through ?? true;

  // URL 指定房间时优先
  useEffect(() => {
    if (Number.isFinite(roomParam) && roomParam > 0) {
      setActiveRoom(roomParam);
      return;
    }
    const injected = (window as unknown as { __MAKU_HIME_ROOM__?: number | null })
      .__MAKU_HIME_ROOM__;
    if (injected) setActiveRoom(injected);
  }, [roomParam, setActiveRoom, snapshot]);

  // 悬浮窗：把配置里的穿透状态同步给窗口（配置变化时也会重新同步）
  useEffect(() => {
    if (surface !== "overlay" || !isTauri()) return;
    void import("@tauri-apps/api/core")
      .then(({ invoke }) => invoke("set_click_through", { enabled: locked }))
      .catch(() => undefined);
  }, [surface, locked]);

  const overlayConfig: OverlayConfig | null = snapshot?.config.overlay ?? null;
  const uiConfig: UiConfig | null = snapshot?.config.ui ?? null;

  const wallConfig = useMemo<UiConfig | null>(() => {
    if (!uiConfig) return null;
    const themed: UiConfig = overlayConfig
      ? {
          ...uiConfig,
          theme: overlayConfig.theme,
          font_size: overlayConfig.font_size,
          max_rows: Math.max(60, Math.round(overlayConfig.height / 24)),
        }
      : uiConfig;
    return themed;
  }, [uiConfig, overlayConfig]);

  const toggleLock = async () => {
    const next = !locked;
    if (isTauri()) {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("set_click_through", { enabled: next }).catch(() => undefined);
    }
    const current = useStore.getState().snapshot;
    if (current) {
      const config = {
        ...current.config,
        overlay: { ...current.config.overlay, click_through: next, locked: next },
      };
      void api.putConfig(config).catch(() => undefined);
    }
  };

  const openPanel = async () => {
    if (!isTauri()) return;
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("focus_main").catch(() => undefined);
  };

  const closeOverlay = async () => {
    if (!isTauri()) return;
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("close_overlay").catch(() => undefined);
  };

  const startResize = async (event: React.PointerEvent<HTMLDivElement>) => {
    if (!isTauri() || !overlayConfig) return;
    event.preventDefault();
    const startX = event.screenX;
    const startY = event.screenY;
    const startW = overlayConfig.width;
    const startH = overlayConfig.height;
    const { invoke } = await import("@tauri-apps/api/core");
    let lastWidth = startW;
    let lastHeight = startH;
    const move = (moveEvent: PointerEvent) => {
      const width = Math.max(240, Math.round(startW + (moveEvent.screenX - startX)));
      const height = Math.max(160, Math.round(startH + (moveEvent.screenY - startY)));
      lastWidth = width;
      lastHeight = height;
      void invoke("set_overlay_size", { width, height }).catch(() => undefined);
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      // 尺寸写回配置，重开悬浮窗时才能恢复（否则窗口状态有两份真相）
      const current = useStore.getState().snapshot;
      if (current) {
        const config = {
          ...current.config,
          overlay: { ...current.config.overlay, width: lastWidth, height: lastHeight },
        };
        void api.putConfig(config).catch(() => undefined);
      }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  if (!snapshot || !wallConfig || !room) {
    return <div className="overlay-surface" data-theme={overlayConfig?.theme ?? "dark"} />;
  }

  return (
    <div
      className={cn(
        "overlay-surface",
        `surface-${surface}`,
        backdrop ? `backdrop-${backdrop}` : "",
      )}
      data-theme={wallConfig.theme}
      style={{
        ["--overlay-alpha" as string]: String(overlayConfig?.opacity ?? 0.88),
      }}
    >
      {surface === "overlay" && overlayConfig?.show_header && (
        <div className="overlay-chrome" data-tauri-drag-region>
          <span className={cn("live-dot", room.live_status === 1 && "is-live")} />
          <span className="overlay-title" data-tauri-drag-region>
            {room.title || room.uname} · {onlineLabel(room) || room.status}
          </span>
          <span className="overlay-actions">
            <button
              type="button"
              onClick={() => void toggleLock()}
              title={locked ? "解锁（可拖动/点击）" : "锁定（点击穿透）"}
            >
              {locked ? <IconLock size={13} /> : <IconUnlock size={13} />}
            </button>
            <button type="button" onClick={() => void openPanel()} title="打开控制面板">
              <IconSettings size={13} />
            </button>
            <button type="button" onClick={() => void closeOverlay()} title="关闭悬浮窗">
              <IconClose size={13} />
            </button>
          </span>
        </div>
      )}

      <div className="overlay-body">
        <Wall roomId={room.room_id} config={wallConfig} bare />
      </div>

      {surface === "overlay" && (
        <div className="resize-handle" onPointerDown={startResize} title="拖动调整大小" />
      )}

      {surface === "obs" && backdrop && (
        <div className="obs-brand">maku-hime · {formatCount(room.online ?? room.popularity)}</div>
      )}
    </div>
  );
}
