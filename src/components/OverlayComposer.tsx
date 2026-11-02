import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { useStore } from "../store";
import type { Emoticon, RoomState, Snapshot } from "../types";
import { cn } from "../util";
import { EmoticonPicker } from "./EmoticonPicker";
import { IconSmile } from "./icons";

interface OverlayComposerProps {
  room: RoomState;
  snapshot: Snapshot;
  locked: boolean;
  onLogin: () => void;
}

/** 发送结果提示的展示时长（bili-dm 同款 4s） */
const FEEDBACK_MS = 4000;

/**
 * 悬浮窗底部的发送条：与 bili-dm 一致——回车发送、右侧 N/20 字数、
 * 发送结果就地提示 4s，锁定态整条只读并提示解锁。
 *
 * 发送走和主面板同一个 /api/send，服务端会本地回显，所以这里不需要自己插一条。
 */
export function OverlayComposer({ room, snapshot, locked, onLogin }: OverlayComposerProps) {
  const toast = useStore((state) => state.toast);
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [hint, setHint] = useState<{ text: string; error: boolean } | null>(null);
  const hintTimer = useRef<number | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);

  const loggedIn = snapshot.login.logged_in;
  const maxLength = snapshot.config.send.max_length ?? 20;
  const editable = loggedIn && !locked;
  const disabled = !editable || sending;

  const clearHintTimer = () => {
    if (hintTimer.current != null) {
      window.clearTimeout(hintTimer.current);
      hintTimer.current = null;
    }
  };

  const flashHint = (next: { text: string; error: boolean } | null) => {
    clearHintTimer();
    setHint(next);
    if (next) {
      hintTimer.current = window.setTimeout(() => {
        setHint(null);
        hintTimer.current = null;
      }, FEEDBACK_MS);
    }
  };

  useEffect(() => clearHintTimer, []);

  /** 锁定 / 退出登录时收起表情面板，避免半开的浮层压在弹幕上 */
  useEffect(() => {
    if (!editable) setPickerOpen(false);
  }, [editable]);

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    const value = text.trim();
    if (!value || disabled) return;
    setSending(true);
    try {
      await api.send(room.room_id, value);
      setText("");
      flashHint({ text: "已发送", error: false });
    } catch (error) {
      const message = error instanceof Error ? error.message : "发送失败";
      flashHint({ text: message, error: true });
      toast("error", message);
    } finally {
      setSending(false);
    }
  };

  /** 大表情点一下直接发；锁定态不让点，避免误触。 */
  const sendEmoticon = async (emoticon: Emoticon) => {
    if (disabled) return;
    try {
      await api.sendEmoticon(room.room_id, emoticon);
    } catch (error) {
      const message = error instanceof Error ? error.message : "表情发送失败";
      flashHint({ text: message, error: true });
      toast("error", message);
    }
  };

  /** 小黄脸这类文本表情插到光标处：一次插好几个再发出去。 */
  const insertEmoticon = (value: string) => {
    if (!value || !editable) return;
    const input = inputRef.current;
    const start = input?.selectionStart ?? text.length;
    const end = input?.selectionEnd ?? start;
    const next = `${text.slice(0, start)}${value}${text.slice(end)}`.slice(0, maxLength);
    const caret = Math.min(start + value.length, next.length);
    setText(next);
    flashHint(null);
    requestAnimationFrame(() => {
      input?.focus();
      input?.setSelectionRange(caret, caret);
    });
  };

  const placeholder = locked
    ? "解锁后可发送弹幕"
    : loggedIn
      ? "发送弹幕…"
      : "扫码登录后可发送弹幕";

  return (
    <div className="bd-composer-shell" data-no-drag>
      {pickerOpen && editable && (
        <EmoticonPicker
          key={room.room_id}
          roomId={room.room_id}
          onPick={sendEmoticon}
          onInsert={insertEmoticon}
          onClose={() => setPickerOpen(false)}
        />
      )}
      <form className="bd-composer" onSubmit={submit} data-no-drag>
        <input
          ref={inputRef}
          value={text}
          onChange={(event) => {
            setText(event.target.value);
            if (hint) flashHint(null);
          }}
          maxLength={maxLength}
          readOnly={locked}
          disabled={!loggedIn || sending}
          placeholder={placeholder}
          title={locked ? "先点右上角锁按钮解锁" : "回车发送"}
          aria-label="悬浮窗弹幕输入框"
        />
        {hint ? (
          <span className={cn("bd-hint", hint.error && "is-error")} title={hint.text}>
            {hint.text}
          </span>
        ) : (
          editable &&
          text.length > 0 && (
            <span className="bd-count">
              {text.length}/{maxLength}
            </span>
          )
        )}
        {editable && (
          <button
            type="button"
            className={cn("bd-emoji", pickerOpen && "is-active")}
            onClick={() => setPickerOpen((open) => !open)}
            title="表情"
            aria-label="表情"
            aria-expanded={pickerOpen}
          >
            <IconSmile size={14} />
          </button>
        )}
        {loggedIn ? (
          <button
            type="submit"
            className="bd-send"
            disabled={disabled || !text.trim()}
            title="发送弹幕（回车）"
          >
            发送
          </button>
        ) : (
          <button
            type="button"
            className="bd-send is-login"
            onClick={onLogin}
            title="扫码登录"
          >
            登录
          </button>
        )}
      </form>
    </div>
  );
}
