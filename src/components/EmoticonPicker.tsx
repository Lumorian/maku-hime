import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import type { Emoticon, EmoticonPackage } from "../types";
import { cn, safeImageUrl } from "../util";

interface EmoticonPickerProps {
  roomId: number;
  /** 大表情（带 emoticon_unique）：点一下直接发出去 */
  onPick: (emoticon: Emoticon) => void | Promise<void>;
  /** 文本表情（小黄脸 / 颜文字 / 热词）：塞进输入框，可以连着插好几个再一起发 */
  onInsert: (text: string) => void;
  onClose: () => void;
}

/**
 * 表情面板：第一个页签是「全部」，后面按表情包分页签。
 *
 * 表情来自两个接口：直播表情（通用表情 / 粉丝团，带 emoticon_unique）和评论区表情
 * （小黄脸、颜文字、热词，只有文本）。点法跟官方一致——
 * - 带标识的（大表情）点一下直接发；
 * - 只有文本的（小黄脸这类）填进输入框，方便一次插入好几个发一条。
 */
export function EmoticonPicker({ roomId, onPick, onInsert, onClose }: EmoticonPickerProps) {
  const [packages, setPackages] = useState<EmoticonPackage[] | null>(null);
  const [active, setActive] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState<string[]>([]);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const tabsRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    let stopped = false;
    api
      .emoticons(roomId)
      .then((list) => {
        if (stopped) return;
        setPackages(list);
        setActive(0);
      })
      .catch((cause: unknown) => {
        if (stopped) return;
        setPackages([]);
        setError(cause instanceof Error ? cause.message : "表情加载失败");
      });
    return () => {
      stopped = true;
    };
  }, [roomId]);

  // 点面板外面或按 Esc 收起
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    const onDown = (event: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) {
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("mousedown", onDown);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("mousedown", onDown);
    };
  }, [onClose]);

  /** 切页签时把当前页签滚进视野，免得选中的藏在右边看不见。 */
  const revealActiveTab = useCallback((index: number) => {
    const row = tabsRef.current;
    if (!row) return;
    const tab = row.children[index] as HTMLElement | undefined;
    tab?.scrollIntoView({ block: "nearest", inline: "nearest" });
  }, []);

  /** 第一个页签：把所有包摊平去重，跟 B 站面板的「全部表情」一致。 */
  const tabs = useMemo(() => {
    if (!packages || packages.length === 0) return [];
    const seen = new Set<string>();
    const all: Emoticon[] = [];
    for (const pack of packages) {
      for (const emoticon of pack.emoticons) {
        const key = `${emoticon.url}|${emoticon.text}`;
        if (seen.has(key)) continue;
        seen.add(key);
        all.push(emoticon);
      }
    }
    return [{ name: "全部", emoticons: all }, ...packages];
  }, [packages]);

  const current = tabs[active] ?? null;

  /**
   * 页签行是横向滚动的：原生滚动条会盖住页签（WebKitGTK 的滚动条是浮在内容上的），
   * 所以整条藏掉（见 CSS），改为滚轮横向滚。
   *
   * 必须挂原生非 passive 监听：React 的 onWheel 走的是 passive 监听，
   * preventDefault 不生效，滚轮会顺带把后面的弹幕墙一起滚走。
   */
  useEffect(() => {
    const row = tabsRef.current;
    if (!row) return;
    const onWheel = (event: WheelEvent) => {
      // 触控板的横向滑动交给浏览器自己处理
      if (Math.abs(event.deltaX) >= Math.abs(event.deltaY)) return;
      if (row.scrollWidth <= row.clientWidth + 1) return;
      event.preventDefault();
      row.scrollLeft += event.deltaY;
    };
    row.addEventListener("wheel", onWheel, { passive: false });
    return () => row.removeEventListener("wheel", onWheel);
  }, [tabs.length]);

  const pick = async (emoticon: Emoticon) => {
    // 只有文本的表情（小黄脸 / 颜文字 / 热词）走插入，大表情直接发
    if (!emoticon.unique_name.trim()) {
      if (emoticon.text) onInsert(emoticon.text);
      return;
    }
    const key = `${emoticon.unique_name}-${emoticon.url}`;
    if (sending.includes(key)) return;
    setSending((list) => [...list, key]);
    try {
      await onPick(emoticon);
    } catch {
      // 发送失败的提示由调用方负责（主面板 / 悬浮窗各自弹 toast），这里只负责收起忙碌态
    } finally {
      setSending((list) => list.filter((item) => item !== key));
    }
  };

  return (
    <div
      className="emoticon-pop"
      ref={rootRef}
      role="dialog"
      aria-label="表情面板"
      data-no-drag
    >
      {packages === null && <p className="emoticon-hint">正在加载表情…</p>}

      {error && <p className="emoticon-hint is-error">{error}</p>}

      {packages && packages.length === 0 && !error && (
        <p className="emoticon-hint">这个账号没有可用表情</p>
      )}

      {tabs.length > 0 && (
        <>
          <div className="emoticon-tabs" role="tablist" ref={tabsRef}>
            {tabs.map((pack, index) => (
              <button
                key={`${pack.name}-${index}`}
                type="button"
                role="tab"
                aria-selected={index === active}
                className={cn("emoticon-tab", index === active && "is-active")}
                onClick={() => {
                  setActive(index);
                  revealActiveTab(index);
                }}
                title={pack.name}
              >
                {pack.name}
              </button>
            ))}
          </div>

          <div className="emoticon-grid">
            {current?.emoticons.map((emoticon, index) => {
              // 表情图片地址来自 B 站接口，远端可控，先过白名单
              const src = safeImageUrl(emoticon.url);
              if (!src) return null;
              const key = `${emoticon.unique_name}-${emoticon.url}`;
              const busy = sending.includes(key);
              const isSticker = Boolean(emoticon.unique_name.trim());
              // 装扮表情（bfs/garb）B 站只认官方客户端发的大表情，第三方发文本不会被渲染
              const garbNote = emoticon.url.includes("/bfs/garb/")
                ? "｜装扮表情，直播间可能只显示成文字"
                : "";
              return (
                <button
                  key={`${emoticon.url}-${index}`}
                  type="button"
                  className={cn("emoticon-item", busy && "is-sending")}
                  title={
                    isSticker
                      ? `${emoticon.text || emoticon.unique_name}（点击直接发送）${garbNote}`
                      : `${emoticon.text}（点击插入输入框）${garbNote}`
                  }
                  aria-busy={busy}
                  onClick={() => void pick(emoticon)}
                >
                  <img
                    src={src}
                    alt={emoticon.text}
                    loading="lazy"
                    referrerPolicy="no-referrer"
                  />
                </button>
              );
            })}
          </div>

          <p className="emoticon-tip">
            小黄脸这类表情点一下是插进输入框，可以连着插好几个一起发；大表情点一下直接发送。
          </p>
        </>
      )}
    </div>
  );
}
