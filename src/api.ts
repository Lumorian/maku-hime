import type {
  AppConfig,
  AreaOptions,
  CoverOutcome,
  Emoticon,
  EmoticonPackage,
  Event,
  LiveStart,
  QrPoll,
  QrSession,
  RoomSettings,
  RoomState,
  Snapshot,
} from "./types";

/** 探测到的服务地址前缀（空字符串表示同源）。 */
let resolvedBase: string | null = null;

/**
 * 服务端（或 Tauri 初始化脚本）注入的访问令牌。
 *
 * 桌面窗口的来源是 tauri://localhost，拿不到服务下发的 Cookie，只能靠这个；
 * 浏览器面板则由服务端直接注入到页面里。别的网站的页面拿不到它，
 * 所以即使猜到了端口也调不动 /api。
 */
function apiToken(): string {
  const injected = (window as unknown as { __MAKU_HIME_TOKEN__?: string })
    .__MAKU_HIME_TOKEN__;
  return typeof injected === "string" ? injected : "";
}

/** 需要携带令牌的请求头；没有令牌就不加（静态预览时会直接 401）。 */
function authHeaders(): Record<string, string> {
  const token = apiToken();
  return token ? { "x-maku-token": token } : {};
}

function candidateBases(): string[] {
  const injected = (window as unknown as { __MAKU_HIME_PORT__?: number })
    .__MAKU_HIME_PORT__;
  // 只认「注入的端口」和「同源」两个候选：
  // 扫 3939-3948 这类跨源探测在新版 CORS 下必然失败，也没有意义。
  return injected ? [`http://127.0.0.1:${injected}`, ""] : [""];
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
  const url = origin.replace(/^http/, "ws") + "/api/ws";
  // WebSocket 握手带不了自定义请求头，令牌只能走查询参数（服务端也认这个）
  const token = apiToken();
  return token ? `${url}?token=${encodeURIComponent(token)}` : url;
}

export class ApiError extends Error {
  readonly code: string;
  /** B 站自己的错误码（-352、60034 之类）；本服务自身产生的错误为 null。 */
  readonly upstream: number | null;
  constructor(message: string, code = "unknown", upstream: number | null = null) {
    super(message);
    this.name = "ApiError";
    this.code = code;
    this.upstream = upstream;
  }
}

async function send<T>(base: string, path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${base}${path}`, {
    ...init,
    headers: {
      "Content-Type": "application/json",
      ...authHeaders(),
      ...(init?.headers ?? {}),
    },
  });
  const text = await response.text();
  const payload = text ? JSON.parse(text) : null;
  if (!response.ok) {
    const message =
      payload?.error?.message ?? `请求失败（HTTP ${response.status}）`;
    throw new ApiError(
      message,
      payload?.error?.code ?? "http_error",
      typeof payload?.error?.upstream === "number" ? payload.error.upstream : null,
    );
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

  startLive: (roomId: number) =>
    request<LiveStart>("/api/live/start", {
      method: "POST",
      body: JSON.stringify({ room_id: roomId }),
    }),

  stopLive: (roomId: number) =>
    request<{ ok: boolean }>("/api/live/stop", {
      method: "POST",
      body: JSON.stringify({ room_id: roomId }),
    }),

  roomSettings: (roomId: number) =>
    request<RoomSettings>(`/api/rooms/${roomId}/settings`),

  saveRoomSettings: (
    roomId: number,
    patch: { title?: string; area_id?: number; announcement?: string },
  ) =>
    request<RoomSettings>(`/api/rooms/${roomId}/settings`, {
      method: "POST",
      body: JSON.stringify(patch),
    }),

  roomAreas: (roomId: number) =>
    request<AreaOptions>(`/api/rooms/${roomId}/areas`),

  uploadCover: (roomId: number, body: { data?: string; url?: string }) =>
    request<CoverOutcome>(`/api/rooms/${roomId}/cover`, {
      method: "POST",
      body: JSON.stringify(body),
    }),

  emoticons: (roomId: number) =>
    request<EmoticonPackage[]>(`/api/rooms/${roomId}/emoticons`),

  sendEmoticon: (roomId: number, emoticon: Emoticon) =>
    request<{ ok: boolean }>("/api/send/emoticon", {
      method: "POST",
      body: JSON.stringify({
        room_id: roomId,
        unique_name: emoticon.unique_name,
        text: emoticon.text,
        id: emoticon.id,
        url: emoticon.url,
        width: emoticon.width,
        height: emoticon.height,
      }),
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

  exportHistoryUrl: async (roomId: number, format: "csv" | "json" | "jsonl") => {
    // 浏览器下载是顶层导航，带不了自定义请求头；跨源时 Cookie 也不会带，
    // 所以令牌走查询参数。
    const params = new URLSearchParams({ format });
    const token = apiToken();
    if (token) params.set("token", token);
    return `${await apiBase()}/api/history/${roomId}/export?${params.toString()}`;
  },

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
