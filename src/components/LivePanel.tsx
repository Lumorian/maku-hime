import { useEffect, useRef, useState } from "react";
import QRCode from "qrcode";
import { api, ApiError } from "../api";
import { useStore } from "../store";
import type { FaceChallenge, LiveStream, RoomState, Snapshot, StreamEndpoint } from "../types";
import { IconBroadcast, IconClose, IconCopy, IconEye, IconEyeOff } from "./icons";

interface LivePanelProps {
  room: RoomState;
  snapshot: Snapshot;
  onClose: () => void;
}

/**
 * 开播面板：调 startLive 拿推流地址 / 密钥，复制到 OBS；直播中也可以停播。
 *
 * 推流密钥只在打开面板、点击「开始直播」后出现在内存里，不写进配置、不进快照。
 * 面板会在页面上遮挡显示，避免直播画面被截屏时直接露出密钥。
 */
export function LivePanel({ room, snapshot, onClose }: LivePanelProps) {
  const toast = useStore((state) => state.toast);
  const [busy, setBusy] = useState(false);
  const [stream, setStream] = useState<LiveStream | null>(null);
  const [verify, setVerify] = useState<FaceChallenge | null>(null);
  const [error, setError] = useState<LiveFailure | null>(null);
  const [showKey, setShowKey] = useState(false);

  const live = room.live_status === 1;
  const loggedIn = snapshot.login.logged_in;
  const isAnchor = loggedIn && snapshot.login.uid === room.uid;

  const copy = async (value: string, label: string) => {
    try {
      await navigator.clipboard.writeText(value);
      toast("success", `已复制${label}`);
    } catch {
      // 不要把推流密钥显示在 toast 里（直播画面可能被截屏）
      toast("warn", `复制失败，请手动选中${label}`);
    }
  };

  const start = async () => {
    setBusy(true);
    setError(null);
    try {
      const result = await api.startLive(room.room_id);
      setVerify(null);
      if (result.status === "verify") {
        setStream(null);
        setVerify(result.verify);
        toast("warn", "这次开播要先做人脸认证");
        return;
      }
      setStream(result.stream);
      setShowKey(false);
      toast("success", "开播成功：把推流地址和密钥填入 OBS");
    } catch (cause) {
      const failure = toFailure(cause, "开播失败");
      setError(failure);
      toast("error", failure.message);
    } finally {
      setBusy(false);
    }
  };

  const stop = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.stopLive(room.room_id);
      setStream(null);
      setVerify(null);
      toast("success", "已停播");
    } catch (cause) {
      const failure = toFailure(cause, "停播失败");
      setError(failure);
      toast("error", failure.message);
    } finally {
      setBusy(false);
    }
  };

  const endpoints = stream ? dedupeEndpoints(stream.protocols) : [];

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div
        className="modal live-modal"
        role="dialog"
        aria-label="开播"
        onClick={(event) => event.stopPropagation()}
      >
        <header className="modal-header">
          <h2>
            <IconBroadcast size={16} /> {live ? "直播中" : "开播"} · {room.uname || "本直播间"}
          </h2>
          <button type="button" className="icon-button" onClick={onClose} title="关闭">
            <IconClose size={16} />
          </button>
        </header>

        <div className="live-room">
          <strong>{room.title || `房间 ${room.room_id}`}</strong>
          <span>
            {room.parent_area ? `${room.parent_area} · ${room.area}` : room.area || "未设置分区"}
            {` · 房间 ${room.room_id}`}
          </span>
        </div>

        {!loggedIn && (
          <p className="live-hint">开播需要登录主播账号：先扫码登录，再回到这里。</p>
        )}
        {loggedIn && !isAnchor && (
          <p className="live-hint">只有主播本人（UID {room.uid}）可以开播，当前登录的是 {snapshot.login.uname}。</p>
        )}

        {error && <p className="live-error">{error.message}</p>}
        {error?.upstream === 60034 && (
          <HimeOnlyHelp onCopy={() => void copy(HIME_DOWNLOAD, "直播姬下载地址")} />
        )}

        {verify ? (
          <VerifyPanel
            verify={verify}
            busy={busy}
            onRetry={() => void start()}
            onCopy={() => void copy(verify.url ?? "", "认证链接")}
          />
        ) : stream ? (
          <div className="live-push">
            <div className="field-block">
              <span className="field-label">推流地址（服务器）</span>
              <div className="copy-row">
                <input readOnly value={stream.addr} onFocus={(event) => event.target.select()} />
                <button
                  type="button"
                  className="icon-button"
                  title="复制推流地址"
                  onClick={() => void copy(stream.addr, "推流地址")}
                >
                  <IconCopy size={15} />
                </button>
              </div>
            </div>
            <div className="field-block">
              <span className="field-label">推流密钥</span>
              <div className="copy-row">
                <input
                  readOnly
                  type={showKey ? "text" : "password"}
                  value={stream.code}
                  onFocus={(event) => event.target.select()}
                />
                <button
                  type="button"
                  className="icon-button"
                  title={showKey ? "隐藏密钥" : "显示密钥"}
                  onClick={() => setShowKey((value) => !value)}
                >
                  {showKey ? <IconEyeOff size={15} /> : <IconEye size={15} />}
                </button>
                <button
                  type="button"
                  className="icon-button"
                  title="复制推流密钥"
                  onClick={() => void copy(stream.code, "推流密钥")}
                >
                  <IconCopy size={15} />
                </button>
              </div>
            </div>

            {endpoints.length > 1 && (
              <div className="live-protocols">
                <span className="field-label">其它协议</span>
                {endpoints
                  .filter((endpoint) => !sameEndpoint(endpoint, stream))
                  .map((endpoint) => (
                    <button
                      key={`${endpoint.protocol}-${endpoint.addr}`}
                      type="button"
                      className="live-protocol"
                      onClick={() => void copy(`${endpoint.addr}${endpoint.code}`, `${endpoint.protocol} 推流链接`)}
                    >
                      <span>{endpoint.protocol}</span>
                      <code>{endpoint.addr}</code>
                      <IconCopy size={13} />
                    </button>
                  ))}
              </div>
            )}

            <p className="live-hint">
              在 OBS「设置 → 直播」里选「自定义」，服务填推流地址、串流密钥填上面的密钥；
              推流开始后 B 站才会显示直播画面。密钥相当于直播间密码，不要发给别人。
            </p>
          </div>
        ) : (
          <p className="live-hint">
            {live
              ? "直播间正在直播。重新获取推流信息会生成一套新的地址和密钥，OBS 里旧的会失效。"
              : "点击开始后会立刻在 B 站创建直播，再把推流地址填进 OBS。首次开播可能需要在 B 站完成实名 / 人脸认证。"}
          </p>
        )}

        <div className="row-actions">
          {isAnchor && (
            <button type="button" onClick={() => void start()} disabled={busy}>
              <IconBroadcast size={15} />
              {busy ? "处理中…" : stream || live ? "重新获取推流信息" : "开始直播"}
            </button>
          )}
          {(live || stream) && isAnchor && (
            <button type="button" className="ghost" onClick={() => void stop()} disabled={busy}>
              停播
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

interface VerifyPanelProps {
  verify: FaceChallenge;
  busy: boolean;
  onRetry: () => void;
  onCopy: () => void;
}

/**
 * 60024 / 60043：B 站要求这一次开播先做人脸认证。
 *
 * 认证页链接由后端从 v_voucher 换出来，这里画成二维码，手机 B 站扫码就能唤起人脸；
 * 认证完点「完成认证，重新开播」再走一次 startLive。
 */
function VerifyPanel({ verify, busy, onRetry, onCopy }: VerifyPanelProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [qrFailed, setQrFailed] = useState(false);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !verify.url) return;
    QRCode.toCanvas(canvas, verify.url, { width: 168, margin: 1 }).catch(() => {
      // WebView 里画不出来就退回到「复制链接」这条路
      setQrFailed(true);
    });
  }, [verify.url]);

  return (
    <div className="live-verify">
      <strong>这次开播需要人脸认证</strong>
      <p>
        B 站对这次开播发起了身份核验（不是账号没实名）。用手机 B 站扫下面的码，
        按提示完成人脸后，回到这里点「完成认证，重新开播」。
      </p>
      {verify.url ? (
        <>
          {!qrFailed && <canvas ref={canvasRef} className="live-verify-qr" />}
          <div className="live-help-link">
            <code>{verify.url.slice(0, 52)}…</code>
            <button type="button" className="icon-button" title="复制认证链接" onClick={onCopy}>
              <IconCopy size={14} />
            </button>
          </div>
        </>
      ) : (
        <p className="live-hint">
          没换出认证链接，可以用 B 站 App 或直播姬随便开一次播完成人脸，再回来重试。
        </p>
      )}
      <button type="button" onClick={onRetry} disabled={busy}>
        <IconBroadcast size={15} />
        {busy ? "处理中…" : "完成认证，重新开播"}
      </button>
    </div>
  );
}
interface LiveFailure {
  message: string;
  /** B 站错误码；拿不到就是 null */
  upstream: number | null;
}

function toFailure(cause: unknown, fallback: string): LiveFailure {
  if (cause instanceof ApiError) return { message: cause.message, upstream: cause.upstream };
  if (cause instanceof Error) return { message: cause.message, upstream: null };
  return { message: fallback, upstream: null };
}

const HIME_DOWNLOAD = "https://link.bilibili.com/p/eden/download";

/**
 * 60034：B 站只对直播姬开放开播入口。
 *
 * 开播参数本来就按直播姬客户端签名（见 core 的 live.rs），走到这里说明账号或分区
 * 被单独限制了，只能告诉主播怎么继续拿到推流。
 */
function HimeOnlyHelp({ onCopy }: { onCopy: () => void }) {
  return (
    <div className="live-help">
      <strong>B 站把开播入口收给了直播姬，按直播姬客户端签名也没过去</strong>
      <p>说明这个账号或当前分区被单独限制了，不是登录或分区填错。想继续用 OBS 推流：</p>
      <ol>
        <li>
          用直播姬开播，从它的推流信息（或 B 站直播中心的「开播设置 → 第三方推流」）复制地址和密钥，
          填进 OBS「设置 → 直播 → 自定义」；弹幕照样在这个面板看；
        </li>
        <li>
          换个分区再试：手游、虚拟主播这类分区常被划成直播姬专属。在 B 站直播设置里换成常规分区，
          回到面板点一次「刷新房间信息」，再点开播；
        </li>
        <li>赶上 B 站维护时段的话，过一会儿重试就行。</li>
      </ol>
      <div className="live-help-link">
        <code>link.bilibili.com/p/eden/download</code>
        <button type="button" className="icon-button" title="复制下载地址" onClick={onCopy}>
          <IconCopy size={14} />
        </button>
      </div>
    </div>
  );
}

function sameEndpoint(endpoint: StreamEndpoint, stream: LiveStream): boolean {
  return endpoint.addr === stream.addr && endpoint.code === stream.code;
}

function dedupeEndpoints(list: StreamEndpoint[]): StreamEndpoint[] {
  const seen = new Set<string>();
  return list.filter((endpoint) => {
    const key = `${endpoint.protocol}|${endpoint.addr}|${endpoint.code}`;
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}
