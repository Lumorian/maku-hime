import { useEffect, useRef, useState } from "react";
import QRCode from "qrcode";
import { api } from "../api";
import { useStore } from "../store";
import type { QrPoll } from "../types";
import { IconClose } from "./icons";

interface LoginModalProps {
  onClose: () => void;
}

export function LoginModal({ onClose }: LoginModalProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const toast = useStore((state) => state.toast);
  const [qrUrl, setQrUrl] = useState<string | null>(null);
  const [key, setKey] = useState<string | null>(null);
  const [poll, setPoll] = useState<QrPoll | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = async () => {
    setError(null);
    setPoll(null);
    try {
      const session = await api.qrCode();
      setKey(session.qrcode_key);
      setQrUrl(session.url);
    } catch (err) {
      setError(err instanceof Error ? err.message : "生成二维码失败");
    }
  };

  useEffect(() => {
    void refresh();
  }, []);

  useEffect(() => {
    if (!qrUrl || !canvasRef.current) return;
    QRCode.toCanvas(canvasRef.current, qrUrl, {
      width: 220,
      margin: 1,
      color: { dark: "#16181b", light: "#ffffff" },
    }).catch(() => setError("二维码绘制失败"));
  }, [qrUrl]);

  useEffect(() => {
    if (!key) return;
    let stopped = false;
    const timer = window.setInterval(async () => {
      try {
        const result = await api.qrPoll(key);
        if (stopped) return;
        setPoll(result);
        if (result.status === "success") {
          stopped = true;
          window.clearInterval(timer);
          toast("success", `已登录：${result.profile?.uname ?? ""}`);
          onClose();
        }
        if (result.status === "expired") {
          stopped = true;
          window.clearInterval(timer);
        }
      } catch (err) {
        if (!stopped) setError(err instanceof Error ? err.message : "轮询失败");
      }
    }, 2000);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [key, onClose, toast]);

  const statusText: Record<string, string> = {
    pending: "用 B 站手机客户端扫码",
    scanned: "已扫码，请在手机上确认登录",
    expired: "二维码已过期",
    error: "登录失败",
    success: "登录成功",
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(event) => event.stopPropagation()}>
        <header className="modal-header">
          <h2>扫码登录</h2>
          <button type="button" className="icon-button" onClick={onClose} title="关闭">
            <IconClose size={16} />
          </button>
        </header>
        <div className="qr-box">
          {error ? (
            <p className="qr-error">{error}</p>
          ) : (
            <canvas ref={canvasRef} width={220} height={220} />
          )}
        </div>
        <p className="qr-status">
          {poll ? statusText[poll.status] ?? poll.message : "正在获取二维码…"}
        </p>
        {(poll?.status === "expired" || error) && (
          <button type="button" className="primary" onClick={() => void refresh()}>
            刷新二维码
          </button>
        )}
        <p className="qr-hint">
          登录只用于发弹幕，凭据保存在本机 <code>~/.config/maku-hime/credentials.json</code>。
        </p>
      </div>
    </div>
  );
}
