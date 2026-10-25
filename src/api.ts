import type {
  AppConfig,
  Event,
  QrPoll,
  QrSession,
  RoomState,
  Snapshot,
} from "./types";

/** 探测到的服务地址前缀（空字符串表示同源）。 */
let resolvedBase: string | null = null;

function candidateBases(): string[] {
  const injected = (window as unknown as { __MAKU_HIME_PORT__?: number })
    .__MAKU_HIME_PORT__;
  const list: string[] = [];
  if (injected) list.push(`http://127.0.0.1:${injected}`);
  for (let port = 3939; port < 3949; port += 1) {
    list.push(`http://127.0.0.1:${port}`);
  }
  // 同源：Vite dev 代理，或直接访问 http://127.0.0.1:3939
  list.push("");
  return list;
}

export async function apiBase(): Promise<string> {
  if (resolvedBase !== null) return resolvedBase;
  for (const base of candidateBases()) {
    try {
      const response = await fetch(`${base}/api/health`, {
        signal: AbortSignal.timeout(1500),
      });
      if (response.ok) {
        resolvedBase = base;
        return base;
      }
    } catch {
      // 试下一个端口
    }
  }
  resolvedBase = "";
  return "";
}

/** 后端换了端口（重启、多实例）时，让下一次调用重新探测。 */
export function resetApiBase(): void {
  resolvedBase = null;
}

export async function socketUrl(): Promise<string> {
  const base = await apiBase();
  const origin = base || window.location.origin;
  return origin.replace(/^http/, "ws") + "/api/ws";
}

export class ApiError extends Error {
  readonly code: string;
  constructor(message: string, code = "unknown") {
    super(message);
    this.name = "ApiError";
    this.code = code;
  }
}

async function send<T>(base: string, path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${base}${path}`, {
    ...init,
    headers: {
      "Content-Type": "application/json",
      ...(init?.headers ?? {}),
    },
  });
  const text = await response.text();
  const payload = text ? JSON.parse(text) : null;
  if (!response.ok) {
    const message =
      payload?.error?.message ?? `请求失败（HTTP ${response.status}）`;
    throw new ApiError(message, payload?.error?.code ?? "http_error");
  }
  return payload as T;
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const base = await apiBase();
  try {
    return await send<T>(base, path, init);
  } catch (error) {
    // fetch 自身的网络错误（TypeError）通常意味着服务换了端口，
    // 重新探测一次再试，避免一次重启就要刷新页面
    if (!(error instanceof TypeError)) throw error;
    resetApiBase();
    return send<T>(await apiBase(), path, init);
  }
}

export const api = {
  state: () => request<Snapshot>("/api/state"),

  addRoom: (roomId: number) =>
    request<RoomState>("/api/rooms", {
      method: "POST",
      body: JSON.stringify({ room_id: roomId }),
    }),

  removeRoom: (roomId: number) =>
    request<{ removed: boolean }>(`/api/rooms/${roomId}`, { method: "DELETE" }),

  refreshRoom: (roomId: number) =>
    request<RoomState>(`/api/rooms/${roomId}/refresh`, { method: "POST" }),

  send: (roomId: number, text: string) =>
    request<{ ok: boolean }>("/api/send", {
      method: "POST",
      body: JSON.stringify({ room_id: roomId, text }),
    }),

  history: (roomId: number, kinds: string[], keyword: string, limit = 800) => {
    const params = new URLSearchParams();
    if (kinds.length) params.set("kinds", kinds.join(","));
    if (keyword.trim()) params.set("q", keyword.trim());
    params.set("limit", String(limit));
    return request<Event[]>(`/api/history/${roomId}?${params.toString()}`);
  },

  clearHistory: (roomId: number) =>
    request<{ ok: boolean }>(`/api/history/${roomId}`, { method: "DELETE" }),

  exportHistoryUrl: async (roomId: number, format: "csv" | "json" | "jsonl") =>
    `${await apiBase()}/api/history/${roomId}/export?format=${format}`,

  qrCode: () => request<QrSession>("/api/login/qrcode"),

  qrPoll: (key: string) =>
    request<QrPoll>(`/api/login/qrcode/poll?key=${encodeURIComponent(key)}`),

  logout: () => request<{ ok: boolean }>("/api/login/logout", { method: "POST" }),

  getConfig: () => request<AppConfig>("/api/config"),

  putConfig: (config: AppConfig) =>
    request<AppConfig>("/api/config", {
      method: "PUT",
      body: JSON.stringify(config),
    }),

  ttsTest: (text: string) =>
    request<{ ok: boolean; backend: string }>("/api/tts/test", {
      method: "POST",
      body: JSON.stringify({ text }),
    }),
};
