import { useEffect, useState } from "react";
import { api, socketUrl } from "./api";
import { useStore } from "./store";
import type { ServerEvent } from "./types";

/** 建立到本地服务的 WebSocket，断线指数退避重连。 */
export function useSocket(): void {
  const applyServerEvent = useStore((state) => state.applyServerEvent);
  const setStatus = useStore((state) => state.setStatus);

  useEffect(() => {
    let socket: WebSocket | null = null;
    let stopped = false;
    let attempt = 0;
    let timer: number | undefined;

    const scheduleReconnect = () => {
      if (stopped) return;
      attempt += 1;
      const delay = Math.min(1000 * 2 ** Math.min(attempt, 4), 8000);
      window.clearTimeout(timer);
      timer = window.setTimeout(connect, delay);
    };

    const connect = async () => {
      if (stopped) return;
      setStatus("connecting");
      // 顺便拉一次全量状态，避免 WS 握手期间界面空白
      api
        .state()
        .then((snapshot) => applyServerEvent({ type: "snapshot", data: snapshot }))
        .catch(() => undefined);

      try {
        const url = await socketUrl();
        socket = new WebSocket(url);
      } catch {
        scheduleReconnect();
        return;
      }

      socket.onopen = () => {
        attempt = 0;
        setStatus("open");
      };
      socket.onmessage = (message) => {
        try {
          const payload = JSON.parse(message.data) as ServerEvent;
          applyServerEvent(payload);
        } catch {
          // 忽略无法解析的消息
        }
      };
      socket.onclose = () => {
        setStatus("closed");
        scheduleReconnect();
      };
      socket.onerror = () => {
        socket?.close();
      };
    };

    void connect();

    return () => {
      stopped = true;
      window.clearTimeout(timer);
      socket?.close();
    };
  }, [applyServerEvent, setStatus]);
}

/** 定时刷新的当前时间，用于「x 秒前」这类相对时间。 */
export function useNow(intervalMs = 1000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), intervalMs);
    return () => window.clearInterval(id);
  }, [intervalMs]);
  return now;
}
