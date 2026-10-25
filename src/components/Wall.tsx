import { useLayoutEffect, useMemo, useRef, useState } from "react";
import { useStore } from "../store";
import type { EnterKind, Event, GuardLevel, Medal, UiConfig, User } from "../types";
import { cn, formatPrice, GUARD_LABEL } from "../util";

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

interface WallProps {
  roomId: number;
  config: UiConfig;
  className?: string;
  /** 悬浮窗/OBS 模式：去掉卡片背景，透明底 */
  bare?: boolean;
  showMeta?: boolean;
}

/** 弹幕墙：官方 chat-history-panel 的排版，加上自动跟随与「N 条新弹幕」提示。 */
export function Wall({ roomId, config, className, bare, showMeta }: WallProps) {
  const events = useStore((state) => state.events[roomId] ?? EMPTY_EVENTS);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const [following, setFollowing] = useState(true);
  const [pending, setPending] = useState(0);

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

  // 新弹幕到达：跟随中就贴底，否则累计「N 条新弹幕」
  useLayoutEffect(() => {
    const container = scrollRef.current;
    if (!container) return;
    if (following) {
      container.scrollTop = container.scrollHeight;
      setPending(0);
    } else if (lastKey !== null) {
      setPending((value) => value + 1);
    }
  }, [lastKey, following, config.max_rows]);

  // 首次进入 / 切换房间时贴底
  useLayoutEffect(() => {
    const container = scrollRef.current;
    if (!container) return;
    container.scrollTop = container.scrollHeight;
    setFollowing(true);
    setPending(0);
  }, [roomId]);

  const handleScroll = () => {
    const container = scrollRef.current;
    if (!container) return;
    const distance = container.scrollHeight - container.scrollTop - container.clientHeight;
    const atBottom = distance < 12;
    setFollowing(atBottom);
    if (atBottom) setPending(0);
  };

  const jumpToLatest = () => {
    const container = scrollRef.current;
    if (!container) return;
    container.scrollTo({ top: container.scrollHeight, behavior: "smooth" });
    setFollowing(true);
    setPending(0);
  };

  return (
    <div
      className={cn("wall", bare && "wall-bare", className)}
      data-theme={config.theme}
      style={{
        fontSize: `${config.font_size}px`,
        ["--wall-rows" as string]: config.max_rows,
      }}
    >
      <div className="wall-scroll" ref={scrollRef} onScroll={handleScroll}>
        <div className={cn("wall-inner", config.compact && "is-compact")}>
          {rows.map((event) => (
            <EventRow key={event.id} event={event} config={config} showMeta={showMeta} />
          ))}
          {rows.length === 0 && (
            <div className="wall-empty">
              <p>还没有弹幕</p>
              <span>在左侧加入一个正在直播的房间试试</span>
            </div>
          )}
        </div>
      </div>

      {!following && pending > 0 && (
        <button className="wall-new-pill" type="button" onClick={jumpToLatest}>
          ↓ {pending} 条新弹幕
        </button>
      )}
    </div>
  );
}

function EventRow({
  event,
  config,
  showMeta,
}: {
  event: Event;
  config: UiConfig;
  showMeta?: boolean;
}) {
  switch (event.type) {
    case "chat":
      return <ChatRow event={event} config={config} />;
    case "gift":
      return <GiftRow event={event} config={config} />;
    case "guard":
      return <GuardRow event={event} config={config} />;
    case "super_chat":
      return <SuperChatRow event={event} />;
    case "enter":
      return (
        <div className="row row-system">
          <span className="system-text">
            {config.show_avatar ? <MiniAvatar user={event.user} /> : null}
            <b>{event.user.name}</b> {ENTER_LABEL[event.kind]}
            {showMeta ? <span className="row-meta">{clock(event.ts)}</span> : null}
          </span>
        </div>
      );
    case "like":
      return (
        <div className="row row-system">
          <span className="system-text">
            {event.user ? <b>{event.user.name}</b> : "观众"} 为主播点赞了
            {event.total ? `（累计 ${event.total}）` : ""}
          </span>
        </div>
      );
    case "notice":
      return (
        <div className="row row-system">
          <span className="system-text">{event.text}</span>
        </div>
      );
    case "remove":
      return (
        <div className="row row-system">
          <span className="system-text">一条醒目留言已结束</span>
        </div>
      );
    default:
      return null;
  }
}

function ChatRow({ event, config }: { event: Extract<Event, { type: "chat" }>; config: UiConfig }) {
  const { user, text, emoticon } = event;
  return (
    <div className="row row-chat">
      {config.show_avatar && <Avatar user={user} />}
      <div className="row-body">
        {user.medal && <MedalBadge medal={user.medal} />}
        <span
          className={cn("uname", user.admin && "is-admin", user.guard !== "none" && "is-guard")}
          style={user.name_color ? { color: user.name_color } : undefined}
          title={user.uid ? `UID ${user.uid}` : undefined}
        >
          {user.name}
        </span>
        <span className="separator"> : </span>
        {emoticon ? (
          <img className="emoticon" src={emoticon} alt={text} title={text} loading="lazy" />
        ) : (
          <span className="text">{text}</span>
        )}
      </div>
    </div>
  );
}

function GiftRow({ event, config }: { event: Extract<Event, { type: "gift" }>; config: UiConfig }) {
  const { user, gift } = event;
  return (
    <div className="row row-gift">
      {config.show_avatar && <Avatar user={user} />}
      <div className="row-body">
        {user.medal && <MedalBadge medal={user.medal} />}
        <span className="uname">{user.name}</span>
        <span className="gift-action"> {gift.action || "赠送"} </span>
        <span className="gift-name">{gift.name}</span>
        <span className="gift-num"> x{gift.num}</span>
        {gift.total > 0 && <span className="gift-price"> ¥{formatPrice(gift.total)}</span>}
      </div>
    </div>
  );
}

function GuardRow({ event, config }: { event: Extract<Event, { type: "guard" }>; config: UiConfig }) {
  const { user, level, item, num } = event;
  return (
    <div className="row row-guard">
      {config.show_avatar && <Avatar user={user} />}
      <div className="row-body">
        <span className="uname">{user.name}</span>
        <span className="gift-action"> 开通了 </span>
        <span className={`guard-name guard-${level}`}>
          {item || GUARD_LABEL[level]}
          {num > 1 ? ` x${num}` : ""}
        </span>
      </div>
    </div>
  );
}

function SuperChatRow({ event }: { event: Extract<Event, { type: "super_chat" }> }) {
  return (
    <div className="row row-sc">
      <div
        className="sc-card"
        style={{
          background: `linear-gradient(135deg, ${event.bg_start}, ${event.bg_end})`,
        }}
      >
        <div className="sc-head">
          <span className="sc-name">{event.user.name}</span>
          <span className="sc-price" style={{ color: event.price_color }}>
            ¥{formatPrice(event.price)}
          </span>
        </div>
        <div className="sc-text">{event.text}</div>
      </div>
    </div>
  );
}

export function Avatar({ user }: { user: User }) {
  return (
    <span className="avatar">
      {user.face ? (
        <img src={user.face} alt={user.name} referrerPolicy="no-referrer" />
      ) : (
        <span className="avatar-fallback">{user.name.slice(0, 1) || "?"}</span>
      )}
    </span>
  );
}

function MiniAvatar({ user }: { user: User }) {
  return <span className="mini-avatar">{user.name.slice(0, 1) || "?"}</span>;
}

export function MedalBadge({ medal }: { medal: Medal }) {
  const gradient = `linear-gradient(90deg, ${medal.start || medal.border || "#5C968E"}, ${medal.end || medal.border || "#5C968E"})`;
  return (
    <span className="medal" title={`${medal.name} · ${medal.anchor || "粉丝勋章"}`}>
      <span className="medal-name" style={{ background: gradient }}>
        {medal.name}
      </span>
      <span
        className="medal-level"
        style={{ color: medal.text || medal.border || "#fff" }}
      >
        {medal.level}
      </span>
    </span>
  );
}

function clock(ts: number): string {
  const date = new Date(ts);
  return `${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

export type { GuardLevel };
