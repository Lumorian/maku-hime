import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "./api";
import { useSocket } from "./hooks";
import { useActiveRoom, useStore } from "./store";
import { LoginModal } from "./components/LoginModal";
import { OverlayComposer } from "./components/OverlayComposer";
import { Wall } from "./components/Wall";
import type { OverlayConfig, UiConfig } from "./types";
import { cn, formatCount, liveBadge, onlineLabel } from "./util";
import { IconClose, IconLock, IconSettings, IconUnlock } from "./components/icons";

const isTauri = () =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

type Surface = "overlay" | "obs";

/** 窗口内边距（.surface-overlay 的 padding）——锁定态输入区域要把这段算上 */
const SURFACE_PADDING = 4;

/**
 * 弹幕墙页面：
 * - surface=overlay：桌面悬浮窗（1:1 还原 bili-dm 的浮窗交互）
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
  const locked = snapshot?.config.overlay.click_through ?? false;

  const overlayConfig: OverlayConfig | null = snapshot?.config.overlay ?? null;
  const uiConfig: UiConfig | null = snapshot?.config.ui ?? null;
  const overlayWidth = overlayConfig?.width;
  const overlayHeight = overlayConfig?.height;
  const activeRoomId = room?.room_id;

  const [loginOpen, setLoginOpen] = useState(false);

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

  // 控制条高度：锁定态只留这一段可点，用户才能点按钮解锁
  const chromeRef = useRef<HTMLDivElement | null>(null);
  const [chromeHeight, setChromeHeight] = useState(0);
  const showHeader = overlayConfig?.show_header ?? true;

  useEffect(() => {
    if (surface !== "overlay") return;
    const element = chromeRef.current;
    if (!element) {
      setChromeHeight(0);
      return;
    }
    const sync = () => setChromeHeight(element.offsetHeight);
    sync();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(sync);
    observer.observe(element);
    return () => observer.disconnect();
  }, [surface, showHeader]);

  // 锁定态的输入区域 = 卡片上边距 + 控制条高度
  const lockHeaderHeight = chromeHeight > 0 ? chromeHeight + SURFACE_PADDING : 0;

  // 悬浮窗：把穿透状态（含控制条高度）同步给整个窗口
  useEffect(() => {
    if (surface !== "overlay" || !isTauri()) return;
    void import("@tauri-apps/api/core")
      .then(({ invoke }) =>
        invoke("set_click_through", {
          enabled: locked,
          headerHeight: locked ? lockHeaderHeight : 0,
        }),
      )
      .catch(() => undefined);
  }, [surface, locked, lockHeaderHeight]);

  // 设置里改尺寸时即时应用（拖动右下角缩放走 set_overlay_size）
  useEffect(() => {
    if (surface !== "overlay" || !isTauri() || !overlayWidth || !overlayHeight) return;
    void import("@tauri-apps/api/core")
      .then(({ invoke }) =>
        invoke("set_overlay_size", { width: overlayWidth, height: overlayHeight }),
      )
      .catch(() => undefined);
  }, [surface, overlayWidth, overlayHeight]);

  // 页面就绪后让壳层再校正一次 niri 浮动位置（首次映射后合成器可能套用默认位置）
  useEffect(() => {
    if (surface !== "overlay" || !isTauri() || !activeRoomId) return;
    void import("@tauri-apps/api/core")
      .then(({ invoke }) => invoke("overlay_ready"))
      .catch(() => undefined);
  }, [surface, activeRoomId]);

  useEffect(() => {
    if (!loginOpen) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setLoginOpen(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [loginOpen]);

  /** 行高与 bili-dm 一致：12px 字号 → 20px 行高，其它字号按比例缩放 */
  const lineHeight = useMemo(
    () => Math.round(((uiConfig?.font_size ?? 12) * 20) / 12),
    [uiConfig?.font_size],
  );

  const wallConfig = useMemo<UiConfig | null>(() => {
    if (!uiConfig) return null;
    if (!overlayConfig) return uiConfig;
    return {
      ...uiConfig,
      theme: overlayConfig.theme,
      font_size: overlayConfig.font_size,
      // 浮窗是「自动滚到底」的日志流，行数给足即可，超出部分靠滚动
      max_rows: Math.max(80, Math.ceil(overlayConfig.height / Math.max(14, lineHeight))),
    };
  }, [uiConfig, overlayConfig, lineHeight]);

  const toggleLock = async () => {
    const next = !locked;
    if (isTauri()) {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("set_click_through", {
        enabled: next,
        headerHeight: next ? lockHeaderHeight : 0,
      }).catch(() => undefined);
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

  /**
   * 拖动窗口：解锁态下任意位置都能拖（bili-dm 同款）。
   * 锁定态整窗穿透、只有控制条可点，此时不拖。
   */
  const startWindowDrag = useCallback(
    async (event: React.PointerEvent<HTMLDivElement>) => {
      if (surface !== "overlay" || !isTauri() || event.button !== 0 || locked) return;
      const target = event.target as HTMLElement | null;
      if (
        target?.closest(
          "button, input, textarea, select, a, [data-no-drag], .modal-backdrop, .resize-handle, .emoticon-pop",
        )
      ) {
        return;
      }
      event.preventDefault();
      const { invoke } = await import("@tauri-apps/api/core");
      try {
        await invoke("start_overlay_drag");
      } catch {
        // 壳层启动失败时退回 Tauri 前端 API
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        await getCurrentWindow().startDragging().catch(() => undefined);
      }
    },
    [surface, locked],
  );

  const startResize = async (event: React.PointerEvent<HTMLDivElement>) => {
    if (!isTauri() || !overlayConfig) return;
    event.preventDefault();
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    const { invoke } = await import("@tauri-apps/api/core");
    let lastWidth = overlayConfig.width;
    let lastHeight = overlayConfig.height;
    let lastScreenX = event.screenX;
    let lastScreenY = event.screenY;
    const move = (moveEvent: PointerEvent) => {
      // Wayland / WebKitGTK 的 screenX 有时恒为 0，优先用 movementX 增量
      const dx = moveEvent.movementX || moveEvent.screenX - lastScreenX;
      const dy = moveEvent.movementY || moveEvent.screenY - lastScreenY;
      if (moveEvent.screenX || moveEvent.screenY) {
        lastScreenX = moveEvent.screenX;
        lastScreenY = moveEvent.screenY;
      }
      lastWidth = Math.max(280, Math.round(lastWidth + dx));
      lastHeight = Math.max(160, Math.round(lastHeight + dy));
      void invoke("set_overlay_size", {
        width: lastWidth,
        height: lastHeight,
      }).catch(() => undefined);
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
      void invoke("save_overlay_geometry").catch(() => undefined);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  };

  if (!snapshot || !wallConfig || !room) {
    return <div className="overlay-surface" data-theme={overlayConfig?.theme ?? "dark"} />;
  }

  const badge = liveBadge(room);
  const detail = onlineLabel(room);
  const statText =
    detail && detail !== badge.text
      ? badge.text + " · " + detail
      : badge.dot === "live"
        ? badge.text + " · 在线数未知"
        : badge.text;
  const tooltip = [
    room.uname + " · " + badge.text,
    room.title,
    [room.parent_area, room.area].filter(Boolean).join(" · "),
    detail,
    detail.startsWith("在线") && room.popularity ? "人气 " + formatCount(room.popularity) : "",
    room.attention != null ? "粉丝 " + formatCount(room.attention) : "",
  ]
    .filter(Boolean)
    .join("\n");

  return (
    <div
      className={cn(
        "overlay-surface",
        "surface-" + surface,
        backdrop ? "backdrop-" + backdrop : "",
        locked && surface === "overlay" ? "is-locked" : "",
      )}
      data-theme={wallConfig.theme}
      style={{
        ["--overlay-alpha" as string]: String(overlayConfig?.opacity ?? 0.85),
      }}
      onPointerDown={(event) => void startWindowDrag(event)}
    >
      {surface === "overlay" ? (
        <>
          <div className="bd-card">
            {showHeader && (
              <div className="bd-controls" ref={chromeRef} title={tooltip}>
                <span className="bd-stat">
                  <span className="bd-dot" data-state={badge.dot}>
                    ●
                  </span>
                  <span className="bd-stat-text">{statText}</span>
                </span>
                <span className="bd-buttons">
                  <button
                    type="button"
                    className="bd-btn"
                    onClick={() => void toggleLock()}
                    title={locked ? "点这里解锁（锁定中只有这一条可点）" : "锁定：其余区域点击穿透"}
                    aria-label={locked ? "解锁悬浮窗" : "锁定悬浮窗"}
                  >
                    {locked ? <IconLock size={14} /> : <IconUnlock size={14} />}
                  </button>
                  <button
                    type="button"
                    className="bd-btn"
                    onClick={() => void openPanel()}
                    title="打开控制面板"
                    aria-label="打开控制面板"
                  >
                    <IconSettings size={14} />
                  </button>
                  <button
                    type="button"
                    className="bd-btn"
                    onClick={() => void closeOverlay()}
                    title="关闭悬浮窗"
                    aria-label="关闭悬浮窗"
                  >
                    <IconClose size={14} />
                  </button>
                </span>
              </div>
            )}

            <div className="bd-body">
              <Wall roomId={room.room_id} config={wallConfig} bare variant="bili" />
            </div>

            {overlayConfig?.show_composer && (
              <OverlayComposer
                room={room}
                snapshot={snapshot}
                locked={locked}
                onLogin={() => setLoginOpen(true)}
              />
            )}
          </div>

          {!room.connected && (
            <div className="bd-status">
              {room.error ? "服务未连接，重试中…" : "连接中…"}
            </div>
          )}

          {!locked && (
            <div
              className="resize-handle"
              data-no-drag
              onPointerDown={(event) => void startResize(event)}
              title="拖动调整大小"
            />
          )}
        </>
      ) : (
        <div className="overlay-body">
          <Wall roomId={room.room_id} config={wallConfig} bare />
        </div>
      )}

      {surface === "obs" && backdrop && (
        <div className="obs-brand">
          maku-hime · {formatCount(room.online ?? room.popularity)}
        </div>
      )}

      {surface === "overlay" && loginOpen && (
        <LoginModal onClose={() => setLoginOpen(false)} />
      )}
    </div>
  );
}
