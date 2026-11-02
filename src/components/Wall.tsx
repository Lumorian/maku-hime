import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useStore } from "../store";
import type { ChatEmoticon, EnterKind, Event, GuardLevel, Medal, UiConfig, User } from "../types";
import { cn, formatPrice, GUARD_LABEL, safeImageUrl, splitChatText } from "../util";

// 稳定的空数组引用：selector 每次返回新对象会让 useSyncExternalStore 死循环
const EMPTY_EVENTS: Event[] = [];

const ENTER_LABEL: Record<EnterKind, string> = {
  enter: "进入直播间",
  follow: "关注了主播",
  share: "分享了直播间",
  special_follow: "特别关注了主播",
  mutual_follow: "和主播互粉了",
  like: "为主播点赞了",
  other: "互动",
};

/** 排版皮肤：panel = 控制面板/OBS（贴底聊天流），bili = 悬浮窗（1:1 bili-dm 还原） */
export type WallVariant = "panel" | "bili";

interface WallProps {
  roomId: number;
  config: UiConfig;
  className?: string;
  /** 悬浮窗/OBS 模式：去掉卡片背景，透明底 */
  bare?: boolean;
  showMeta?: boolean;
  variant?: WallVariant;
}

const SCROLLBAR_TRACK_PAD = 5;
const SCROLLBAR_MIN_THUMB = 20;
const WHEEL_NOTCH = 120;
const WHEEL_ROWS = 3;

/** 弹幕墙：官方 chat-history-panel 的排版，加上自动跟随与「N 条新弹幕」提示。 */
export function Wall({ roomId, config, className, bare, showMeta, variant = "panel" }: WallProps) {
  const bili = variant === "bili";
  const events = useStore((state) => state.events[roomId] ?? EMPTY_EVENTS);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const barRef = useRef<HTMLDivElement | null>(null);
  const thumbRef = useRef<HTMLDivElement | null>(null);
  const [following, setFollowing] = useState(true);
  const [pending, setPending] = useState(0);
  const [scrollable, setScrollable] = useState(false);
  const [barActive, setBarActive] = useState(false);
  const pendingNewRef = useRef(0);

  // 官方行高：12px 字号配 20px 行盒，其余字号等比换算（9→15 … 24→40）
  const lineHeight = Math.round((config.font_size * 20) / 12);

  const visible = useMemo(
    () =>
      events.filter((event) => {
        switch (event.type) {
          case "enter":
            return config.show_enter;
          case "like":
            return config.show_like;
          case "gift":
          case "guard":
            return config.show_gift;
          case "popularity":
          case "watched":
          case "online_rank":
          case "room_update":
          case "live_status":
            return false;
          default:
            return true;
        }
      }),
    [events, config.show_enter, config.show_like, config.show_gift],
  );

  const rows = visible.slice(-config.max_rows);
  // 用「最后一条的 id」而不是数组长度：到了 max_rows 之后长度恒定，
  // 只看长度会导致自动跟随静默失效（新弹幕来了却不再滚动）。
  const lastKey = rows.length > 0 ? rows[rows.length - 1].id : null;

  /** 自绘滚动条：4px 宽、20px 最小滑块，跟随像素级滚动位置。 */
  const syncBar = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    const bar = barRef.current;
    const thumb = thumbRef.current;
    const maxOffset = el.scrollHeight - el.clientHeight;
    const canScroll = maxOffset > 1;
    if (bar) bar.style.visibility = canScroll ? "visible" : "hidden";
    setScrollable(canScroll);
    if (!thumb || !canScroll) return;
    const trackH = Math.max(0, el.clientHeight - SCROLLBAR_TRACK_PAD * 2);
    const ratio = el.clientHeight / Math.max(1, el.scrollHeight);
    const thumbH = Math.min(trackH, Math.max(SCROLLBAR_MIN_THUMB, trackH * ratio));
    const pos = (el.scrollTop / maxOffset) * Math.max(0, trackH - thumbH);
    thumb.style.height = thumbH + "px";
    thumb.style.transform = "translateY(" + pos + "px)";
  }, []);

  /** 统计「完全落在视口下方」的连续行数；jsdom 没有布局时退回事件计数器。 */
  const countHiddenRows = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    const inner = el.firstElementChild;
    if (!inner) return;
    const bottom = el.scrollTop + el.clientHeight;
    let hidden = 0;
    let measured = false;
    for (const node of Array.from(inner.children)) {
      const item = node as HTMLElement;
      if (!item.classList.contains("chat-item")) continue;
      if (item.offsetHeight > 0) measured = true;
      if (item.offsetTop + item.offsetHeight > bottom + 1) hidden += 1;
    }
    // 没有真实布局（单测环境）时，用「翻了之后到达的条数」兜底
    if (!measured) return;
    setPending(hidden);
  }, []);

  const syncScroll = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    syncBar();
    const distance = el.scrollHeight - el.scrollTop - el.clientHeight;
    const atBottom = distance < 4;
    setFollowing(atBottom);
    if (atBottom) {
      pendingNewRef.current = 0;
      setPending(0);
      return;
    }
    countHiddenRows();
  }, [countHiddenRows, syncBar]);

  // 新弹幕到达：跟随中就贴底，否则累计「N 条新弹幕」。
  // 注意要比较「最后一条是否真的变了」——否则用户一往上翻（following 变化）
  // 就会被误记成一条新弹幕。
  const lastKeyRef = useRef<string | null>(null);
  useLayoutEffect(() => {
    const container = scrollRef.current;
    if (!container) return;
    const isNew = lastKey !== null && lastKey !== lastKeyRef.current;
    lastKeyRef.current = lastKey;
    if (following) {
      container.scrollTop = container.scrollHeight;
      pendingNewRef.current = 0;
      setPending(0);
      syncBar();
    } else if (isNew) {
      pendingNewRef.current += 1;
      setPending(pendingNewRef.current);
      countHiddenRows();
      syncBar();
    }
  }, [lastKey, following, config.max_rows, countHiddenRows, syncBar]);

  // 首次进入 / 切换房间时贴底
  useLayoutEffect(() => {
    const container = scrollRef.current;
    if (!container) return;
    container.scrollTop = container.scrollHeight;
    setFollowing(true);
    setPending(0);
    pendingNewRef.current = 0;
    syncBar();
  }, [roomId, syncBar]);

  // 尺寸变化（拖拽窗口、字号调整）后重新量一次滚动条
  useEffect(() => {
    const el = scrollRef.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => syncScroll());
    observer.observe(el);
    if (el.firstElementChild) observer.observe(el.firstElementChild);
    return () => observer.disconnect();
  }, [syncScroll, variant]);

  // bili 皮肤：滚轮一格 = 3 行（120 单位），原生滚动逐像素，不用平滑
  useEffect(() => {
    const el = scrollRef.current;
    if (!el || !bili) return;
    const onWheel = (event: WheelEvent) => {
      if (event.ctrlKey) return;
      event.preventDefault();
      const notches = event.deltaMode === 1 ? event.deltaY : event.deltaY / WHEEL_NOTCH;
      el.scrollTop += notches * WHEEL_ROWS * lineHeight;
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [bili, lineHeight]);

  // bili 皮肤：PageUp/PageDown 翻一屏（留 40px 重叠），Home/End 到首尾
  useEffect(() => {
    const el = scrollRef.current;
    if (!el || !bili) return;
    const onKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable)) {
        return;
      }
      const page = Math.max(40, el.clientHeight - 40);
      switch (event.key) {
        case "PageUp":
          el.scrollTop -= page;
          break;
        case "PageDown":
          el.scrollTop += page;
          break;
        case "Home":
          el.scrollTop = 0;
          break;
        case "End":
          el.scrollTop = el.scrollHeight;
          break;
        default:
          return;
      }
      event.preventDefault();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [bili]);

  const handleScroll = () => {
    syncScroll();
  };

  /** 点击滚动条轨道：先跳到点击位置，再进入拖拽。 */
  const handleBarPointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    const el = scrollRef.current;
    const bar = barRef.current;
    if (!el || !bar) return;
    event.preventDefault();
    const rect = bar.getBoundingClientRect();
    const trackH = Math.max(1, rect.height);
    const maxOffset = Math.max(0, el.scrollHeight - el.clientHeight);
    const ratio = el.clientHeight / Math.max(1, el.scrollHeight);
    const thumbH = Math.min(trackH, Math.max(SCROLLBAR_MIN_THUMB, trackH * ratio));
    const room = Math.max(1, trackH - thumbH);
    const pointerY = event.clientY - rect.top;
    const thumbTop = (el.scrollTop / Math.max(1, maxOffset)) * room;
    const onThumb = pointerY >= thumbTop && pointerY <= thumbTop + thumbH;
    const grab = onThumb ? pointerY - thumbTop : thumbH / 2;
    if (!onThumb) el.scrollTop = ((pointerY - grab) / room) * maxOffset;
    const startY = event.clientY;
    const startTop = el.scrollTop;
    setBarActive(true);
    const onMove = (moveEvent: PointerEvent) => {
      el.scrollTop = startTop + ((moveEvent.clientY - startY) / room) * maxOffset;
    };
    const onUp = () => {
      setBarActive(false);
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };

  const jumpToLatest = () => {
    const container = scrollRef.current;
    if (!container) return;
    container.scrollTop = container.scrollHeight;
    setFollowing(true);
    setPending(0);
    pendingNewRef.current = 0;
    syncBar();
  };

  const showPill = bili ? !following && scrollable : !following && pending > 0;

  return (
    <div
      className={cn("wall", bili && "wall-bd", bare && "wall-bare", className)}
      data-theme={config.theme}
      data-avatars={config.show_avatar ? "on" : "off"}
      style={{
        fontSize: config.font_size + "px",
        ["--wall-rows" as string]: config.max_rows,
        ["--line-height" as string]: lineHeight + "px",
      }}
    >
      <div className="wall-scroll" ref={scrollRef} onScroll={handleScroll}>
        <div className={cn("wall-inner", config.compact && "is-compact")}>
          {rows.map((event) => (
            <EventRow key={event.id} event={event} config={config} showMeta={showMeta} bili={bili} />
          ))}
          {rows.length === 0 && (
            <div className="wall-empty">
              <p>还没有弹幕</p>
              {!bili && <span>在左侧加入一个正在直播的房间试试</span>}
            </div>
          )}
        </div>
      </div>

      {bili && (
        <div
          className={cn("wall-scrollbar", barActive && "is-active")}
          ref={barRef}
          role="scrollbar"
          aria-orientation="vertical"
          onPointerDown={handleBarPointerDown}
        >
          <div className="wall-scrollbar-thumb" ref={thumbRef} />
        </div>
      )}

      {showPill && (
        <button
          className={cn("wall-new-pill", bili && "is-bili")}
          type="button"
          onClick={jumpToLatest}
        >
          {bili && pending === 0 ? "↓ 回到最新" : "↓ " + pending + " 条新弹幕"}
        </button>
      )}
    </div>
  );
}

function EventRow({
  event,
  config,
  showMeta,
  bili,
}: {
  event: Event;
  config: UiConfig;
  showMeta?: boolean;
  bili?: boolean;
}) {
  switch (event.type) {
    case "chat":
      return <ChatRow event={event} config={config} bili={bili} />;
    case "gift":
      return <GiftRow event={event} config={config} bili={bili} />;
    case "guard":
      return <GuardRow event={event} config={config} bili={bili} />;
    case "super_chat":
      return <SuperChatRow event={event} config={config} bili={bili} />;
    case "enter":
      return (
        <div className="chat-item item-system">
          {config.show_avatar &&
            (bili ? <Avatar user={event.user} /> : event.user && <MiniAvatar user={event.user} />)}
          <span className="interact-name">{event.user.name}</span>
          <span className="interact-desc">{ENTER_LABEL[event.kind]}</span>
          {showMeta ? <span className="row-meta">{clock(event.ts)}</span> : null}
        </div>
      );
    case "like":
      return (
        <div className="chat-item item-system">
          {config.show_avatar && bili && <Avatar user={event.user} />}
          <span className="interact-name">{event.user ? event.user.name : "观众"}</span>
          <span className="interact-desc">
            为主播点赞了{event.total ? "（累计 " + event.total + "）" : ""}
          </span>
        </div>
      );
    case "notice":
      return (
        <div className="chat-item item-system">
          <span className="interact-desc">{event.text}</span>
        </div>
      );
    case "remove":
      return (
        <div className="chat-item item-system">
          <span className="interact-desc">醒目留言已删除</span>
        </div>
      );
    default:
      return null;
  }
}

function ChatRow({
  event,
  config,
  bili,
}: {
  event: Extract<Event, { type: "chat" }>;
  config: UiConfig;
  bili?: boolean;
}) {
  const { user, text, emoticon, emots } = event;
  // 表情图片地址来自远端，先过一遍白名单再交给 <img>
  const emoticonSrc = safeImageUrl(emoticon);
  // 正文里的 [滑稽] 这类内联小表情：B 站只给对照表，需要自己替换成图
  const segments = splitChatText(text, emots);
  return (
    <div className="chat-item item-chat">
      {config.show_avatar && <Avatar user={user} />}
      {user.medal && <MedalBadge medal={user.medal} />}
      <span
        className={cn("user-name", user.admin && "is-admin", user.guard !== "none" && "is-guard")}
        style={user.name_color ? { color: user.name_color } : undefined}
        title={user.uid ? "UID " + user.uid : undefined}
      >
        {user.name}
      </span>
      {bili ? <span className="chat-sep"> : </span> : <span className="chat-sep">：</span>}
      {emoticonSrc ? (
        <img
          className="chat-emoticon"
          src={emoticonSrc}
          alt={text}
          title={text}
          loading="lazy"
          referrerPolicy="no-referrer"
        />
      ) : (
        <span className="chat-text">
          {segments.map((segment, index) =>
            segment.kind === "emote" ? (
              <EmoteImage key={`${segment.emote.text}-${index}`} emote={segment.emote} />
            ) : (
              <span key={`text-${index}`}>{segment.value}</span>
            ),
          )}
        </span>
      )}
    </div>
  );
}

/** 正文里的行内小表情：尺寸跟着行高走，太大的图按行高缩放。 */
function EmoteImage({ emote }: { emote: ChatEmoticon }) {
  const src = safeImageUrl(emote.url);
  if (!src) return <span>{emote.text}</span>;
  return (
    <img
      className="chat-emoticon is-inline"
      src={src}
      alt={emote.text}
      title={emote.text}
      loading="lazy"
      referrerPolicy="no-referrer"
    />
  );
}

function GiftRow({
  event,
  config,
  bili,
}: {
  event: Extract<Event, { type: "gift" }>;
  config: UiConfig;
  bili?: boolean;
}) {
  const { user, gift } = event;
  return (
    <div className="chat-item item-gift">
      {config.show_avatar && <Avatar user={user} />}
      {user.medal && <MedalBadge medal={user.medal} />}
      <span className="user-name">{user.name}</span>
      <span className="gift-action">{gift.action || "赠送"}</span>
      <span className="gift-name">{gift.name}</span>
      {gift.num > 1 && <span className="gift-num">x{gift.num}</span>}
      {!bili && gift.total > 0 && <span className="gift-price">¥{formatPrice(gift.total)}</span>}
    </div>
  );
}

function GuardRow({
  event,
  config,
  bili,
}: {
  event: Extract<Event, { type: "guard" }>;
  config: UiConfig;
  bili?: boolean;
}) {
  const { user, level, item, num } = event;
  return (
    <div className="chat-item item-guard">
      {config.show_avatar && <Avatar user={user} />}
      {user.medal && <MedalBadge medal={user.medal} />}
      <span className="user-name">{user.name}</span>
      <span className="gift-action">开通了</span>
      <span className={cn("gift-name", "guard-name", "guard-" + level)}>
        {item || GUARD_LABEL[level]}
      </span>
      {!bili && num > 1 && <span className="gift-num">x{num}</span>}
    </div>
  );
}

function SuperChatRow({
  event,
  config,
  bili,
}: {
  event: Extract<Event, { type: "super_chat" }>;
  config: UiConfig;
  bili?: boolean;
}) {
  const start = event.bg_start || "#F2C55C";
  const end = event.bg_end || "#E9A100";
  const color = contrastColor(start, end);
  return (
    <div className="chat-item item-sc">
      <div
        className="sc-card"
        style={{ background: "linear-gradient(45deg, " + start + ", " + end + ")", color: color }}
      >
        {bili && config.show_avatar && <Avatar user={event.user} />}
        {bili && event.user.medal && <MedalBadge medal={event.user.medal} />}
        <span className="sc-name" style={{ color: color }}>
          {event.user.name}
        </span>
        <span className="sc-price" style={{ color: event.price_color || "#333333" }}>
          ¥{formatPrice(event.price)}
        </span>
        <span className="sc-text">{event.text}</span>
      </div>
    </div>
  );
}

/** 行首 24px 圆形头像；没有用户（如点赞）时只画灰圆，与 bili-dm 一致 */
export function Avatar({ user }: { user?: User | null }) {
  const face = safeImageUrl(user?.face);
  const name = user?.name ?? "";
  return (
    <span className="chat-avatar">
      {face ? (
        <img src={face} alt={name} loading="lazy" referrerPolicy="no-referrer" />
      ) : (
        <span className="chat-avatar-fallback">{name.slice(0, 1)}</span>
      )}
    </span>
  );
}

function MiniAvatar({ user }: { user: User }) {
  return <span className="mini-avatar">{user.name.slice(0, 1) || "?"}</span>;
}

function GuardAnchor() {
  return (
    <svg className="fans-medal-guard" viewBox="0 0 20 20" width="20" height="20" aria-hidden="true">
      <path
        d="M10 2.6a2.2 2.2 0 0 0-2.2 2.2c0 .8.4 1.5 1.1 1.9v2.1H6.8v1.5h2.1v5.2c-2.1-.3-3.8-1.7-4.4-3.7l1.1.4.5-1.4-2.6-.9-.7 2.9 1.5.4c.8 2.7 3.2 4.5 6.1 4.5s5.3-1.8 6.1-4.5l1.5-.4-.7-2.9-2.6.9.5 1.4 1.1-.4c-.6 2-2.3 3.4-4.4 3.7V10.3h2.1V8.8h-2.1V6.7c.7-.4 1.1-1.1 1.1-1.9A2.2 2.2 0 0 0 10 2.6z"
        fill="currentColor"
      />
    </svg>
  );
}

export function MedalBadge({ medal }: { medal: Medal }) {
  const start = medal.start || medal.border || "#9C9C9C";
  const end = medal.end || medal.border || "#9C9C9C";
  const border = medal.border || start;
  const hasGuard = Boolean(medal.guard) && medal.guard !== "none";
  const digits = String(medal.level).length;
  return (
    <span
      className="fans-medal-item-ctnr"
      title={(medal.anchor || "粉丝勋章") + " · 等级 " + medal.level}
    >
      <span
        className="fans-medal-item"
        style={{
          backgroundImage: "linear-gradient(45deg, " + start + ", " + end + ")",
          boxShadow: "inset 0 0 0 1px " + border,
          color: medal.text || "#FFFFFF",
        }}
      >
        <span className={cn("fans-medal-label", hasGuard && "has-guard")}>
          {hasGuard && <GuardAnchor />}
          <span className="fans-medal-content">{medal.name}</span>
        </span>
        <span
          className={cn(
            "fans-medal-level",
            digits === 1 && "one-digit",
            digits >= 3 && "three-digits",
          )}
        >
          {medal.level}
        </span>
      </span>
    </span>
  );
}

function parseHex(color: string): [number, number, number] | null {
  const match = /^#?([0-9a-f]{3}|[0-9a-f]{6})$/i.exec((color || "").trim());
  if (!match) return null;
  let hex = match[1];
  if (hex.length === 3) hex = hex.split("").map((c) => c + c).join("");
  return [
    parseInt(hex.slice(0, 2), 16),
    parseInt(hex.slice(2, 4), 16),
    parseInt(hex.slice(4, 6), 16),
  ];
}

/** 醒目留言：按渐变色亮度挑黑/白文字（阈值 0.55，官方同款） */
function contrastColor(start: string, end: string): string {
  const a = parseHex(start);
  const b = parseHex(end);
  if (!a && !b) return "#333333";
  const rgb: [number, number, number] = a && b
    ? [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2, (a[2] + b[2]) / 2]
    : (a || b)!;
  const luminance = (0.299 * rgb[0] + 0.587 * rgb[1] + 0.114 * rgb[2]) / 255;
  return luminance > 0.55 ? "#333333" : "#FFFFFF";
}

function clock(ts: number): string {
  const date = new Date(ts);
  const hour = String(date.getHours()).padStart(2, "0");
  const minute = String(date.getMinutes()).padStart(2, "0");
  return hour + ":" + minute;
}

export type { GuardLevel };
