import { useState } from "react";
import { api } from "../api";
import { useStore } from "../store";
import type {
  AppConfig,
  OverlayConfig,
  RoomState,
  SendConfig,
  Snapshot,
  Theme,
  TtsConfig,
  TtsProvider,
  UiConfig,
} from "../types";
import { IconClose, IconCopy, IconWindow } from "./icons";

interface SettingsPanelProps {
  config: AppConfig;
  snapshot: Snapshot;
  room: RoomState | null;
  onSave: (config: AppConfig) => void;
  onClose: () => void;
  onOpenOverlay: () => void;
  overlayEnabled: boolean;
}

export function SettingsPanel({
  config,
  snapshot,
  room,
  onSave,
  onClose,
  onOpenOverlay,
  overlayEnabled,
}: SettingsPanelProps) {
  const toast = useStore((state) => state.toast);
  const [ttsCommand, setTtsCommand] = useState(config.tts.command.join(" "));
  const [testing, setTesting] = useState(false);

  const setUi = (patch: Partial<UiConfig>) =>
    onSave({ ...config, ui: { ...config.ui, ...patch } });
  const setOverlay = (patch: Partial<OverlayConfig>) =>
    onSave({ ...config, overlay: { ...config.overlay, ...patch } });
  const setTts = (patch: Partial<TtsConfig>) =>
    onSave({ ...config, tts: { ...config.tts, ...patch } });
  const setSend = (patch: Partial<SendConfig>) =>
    onSave({ ...config, send: { ...config.send, ...patch } });

  const obsUrl = room
    ? `${snapshot.server_url || "http://127.0.0.1:3939"}/obs?room=${room.room_id}`
    : `${snapshot.server_url || "http://127.0.0.1:3939"}/obs`;

  const copy = async (value: string, label: string) => {
    try {
      await navigator.clipboard.writeText(value);
      toast("success", `已复制${label}`);
    } catch {
      toast("warn", `复制失败，请手动复制：${value}`);
    }
  };

  const commitTtsCommand = () => {
    const argv = ttsCommand
      .split(/\s+/)
      .map((part) => part.trim())
      .filter(Boolean);
    setTts({ command: argv });
  };

  const testTts = async () => {
    setTesting(true);
    try {
      const result = await api.ttsTest("你好，我是幕姬，这是一条测试语音");
      toast("info", `TTS 后端：${result.backend}`);
    } catch (error) {
      toast("error", error instanceof Error ? error.message : "试听失败");
    } finally {
      setTesting(false);
    }
  };

  return (
    <aside className="drawer">
      <header className="drawer-header">
        <h2>设置</h2>
        <button type="button" className="icon-button" onClick={onClose} title="关闭">
          <IconClose size={16} />
        </button>
      </header>

      <div className="drawer-body">
        <Section title="外观">
          <Row label="主题">
            <Segmented
              value={config.ui.theme}
              options={[
                { value: "dark", label: "深色" },
                { value: "light", label: "浅色" },
              ]}
              onChange={(value) => setUi({ theme: value as Theme })}
            />
          </Row>
          <Row label={`字号 ${config.ui.font_size}px`}>
            <input
              type="range"
              min={11}
              max={20}
              value={config.ui.font_size}
              onChange={(event) => setUi({ font_size: Number(event.target.value) })}
            />
          </Row>
          <Row label={`弹幕保留 ${config.ui.max_rows} 条`}>
            <input
              type="range"
              min={100}
              max={2000}
              step={100}
              value={config.ui.max_rows}
              onChange={(event) => setUi({ max_rows: Number(event.target.value) })}
            />
          </Row>
          <Row label="紧凑模式">
            <Switch value={config.ui.compact} onChange={(value) => setUi({ compact: value })} />
          </Row>
        </Section>

        <Section title="显示内容">
          <Row label="头像">
            <Switch value={config.ui.show_avatar} onChange={(value) => setUi({ show_avatar: value })} />
          </Row>
          <Row label="礼物 / 上舰">
            <Switch value={config.ui.show_gift} onChange={(value) => setUi({ show_gift: value })} />
          </Row>
          <Row label="进房 / 关注">
            <Switch value={config.ui.show_enter} onChange={(value) => setUi({ show_enter: value })} />
          </Row>
          <Row label="点赞">
            <Switch value={config.ui.show_like} onChange={(value) => setUi({ show_like: value })} />
          </Row>
        </Section>

        <Section title="悬浮窗">
          <Row label="点击穿透">
            <Switch
              value={config.overlay.click_through}
              onChange={(value) => setOverlay({ click_through: value, locked: value })}
            />
          </Row>
          <Row label="始终置顶">
            <Switch
              value={config.overlay.always_on_top}
              onChange={(value) => setOverlay({ always_on_top: value })}
            />
          </Row>
          <Row label={`尺寸 ${config.overlay.width}×${config.overlay.height}`}>
            <div className="inline-inputs">
              <input
                type="number"
                min={240}
                max={1600}
                value={config.overlay.width}
                onChange={(event) => setOverlay({ width: Number(event.target.value) })}
              />
              <input
                type="number"
                min={160}
                max={1600}
                value={config.overlay.height}
                onChange={(event) => setOverlay({ height: Number(event.target.value) })}
              />
            </div>
          </Row>
          <Row label={`透明度 ${Math.round(config.overlay.opacity * 100)}%`}>
            <input
              type="range"
              min={20}
              max={100}
              value={Math.round(config.overlay.opacity * 100)}
              onChange={(event) => setOverlay({ opacity: Number(event.target.value) / 100 })}
            />
          </Row>
          <div className="row-actions">
            <button type="button" className="primary" onClick={onOpenOverlay}>
              <IconWindow size={15} />
              {overlayEnabled ? "关闭悬浮窗" : "打开悬浮窗"}
            </button>
          </div>
          <div className="field-block">
            <span className="field-label">OBS 浏览器源地址</span>
            <div className="copy-row">
              <input readOnly value={obsUrl} onFocus={(event) => event.target.select()} />
              <button type="button" className="icon-button" onClick={() => copy(obsUrl, " OBS 地址")}>
                <IconCopy size={15} />
              </button>
            </div>
            <p className="field-help">
              在 OBS 里添加「浏览器」源，宽高按提示填，勾选「关闭源时关闭页面」不需要；
              页面背景透明，可直接叠加在直播画面上。
            </p>
          </div>
        </Section>

        <Section title="语音播报">
          <Row label="开启播报">
            <Switch value={config.tts.enabled} onChange={(value) => setTts({ enabled: value })} />
          </Row>
          <Row label="后端">
            <select
              value={config.tts.provider}
              onChange={(event) => setTts({ provider: event.target.value as TtsProvider })}
            >
              <option value="auto">自动（系统命令）</option>
              <option value="webview">浏览器语音</option>
              <option value="command">自定义命令</option>
              <option value="off">关闭</option>
            </select>
          </Row>
          <div className="field-block">
            <span className="field-label">命令行（用空格分隔，{"{text}"} 会被替换）</span>
            <input
              value={ttsCommand}
              placeholder="espeak-ng -v zh {text}"
              onChange={(event) => setTtsCommand(event.target.value)}
              onBlur={commitTtsCommand}
            />
            <p className="field-help">
              当前生效后端：{snapshot.tts_backend || "off"}。也可以用 piper、edge-playback、ekho。
            </p>
          </div>
          <Row label="朗读弹幕">
            <Switch value={config.tts.read_chat} onChange={(value) => setTts({ read_chat: value })} />
          </Row>
          <Row label="朗读礼物">
            <Switch value={config.tts.read_gift} onChange={(value) => setTts({ read_gift: value })} />
          </Row>
          <Row label="朗读上舰">
            <Switch value={config.tts.read_guard} onChange={(value) => setTts({ read_guard: value })} />
          </Row>
          <Row label="朗读醒目留言">
            <Switch
              value={config.tts.read_super_chat}
              onChange={(value) => setTts({ read_super_chat: value })}
            />
          </Row>
          <Row label={`单条最多 ${config.tts.max_chars} 字`}>
            <input
              type="range"
              min={20}
              max={120}
              step={10}
              value={config.tts.max_chars}
              onChange={(event) => setTts({ max_chars: Number(event.target.value) })}
            />
          </Row>
          <div className="row-actions">
            <button type="button" onClick={testTts} disabled={testing}>
              {testing ? "播放中…" : "试听"}
            </button>
          </div>
        </Section>

        <Section title="发送">
          <Row label="弹幕颜色">
            <input
              type="color"
              value={colorToHex(config.send.color)}
              onChange={(event) => setSend({ color: parseInt(event.target.value.slice(1), 16) })}
            />
          </Row>
          <Row label={`最多 ${config.send.max_length} 字`}>
            <input
              type="range"
              min={10}
              max={40}
              value={config.send.max_length}
              onChange={(event) => setSend({ max_length: Number(event.target.value) })}
            />
          </Row>
        </Section>

        <Section title="数据">
          <div className="field-block">
            <span className="field-label">历史目录</span>
            <div className="copy-row">
              <input readOnly value={snapshot.history_dir} onFocus={(event) => event.target.select()} />
              <button
                type="button"
                className="icon-button"
                onClick={() => copy(snapshot.history_dir, "历史目录")}
              >
                <IconCopy size={15} />
              </button>
            </div>
          </div>
          <div className="row-notes">
            <span>版本 {snapshot.version}</span>
            <span>TTS 丢弃 {snapshot.tts_dropped} 条</span>
          </div>
        </Section>
      </div>
    </aside>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="settings-section">
      <h3>{title}</h3>
      <div className="settings-body">{children}</div>
    </section>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="settings-row">
      <span>{label}</span>
      <div className="settings-control">{children}</div>
    </label>
  );
}

function Switch({ value, onChange }: { value: boolean; onChange: (value: boolean) => void }) {
  return (
    <button
      type="button"
      className={`switch ${value ? "is-on" : ""}`}
      role="switch"
      aria-checked={value}
      onClick={() => onChange(!value)}
    >
      <span />
    </button>
  );
}

function Segmented({
  value,
  options,
  onChange,
}: {
  value: string;
  options: Array<{ value: string; label: string }>;
  onChange: (value: string) => void;
}) {
  return (
    <div className="segmented">
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          className={value === option.value ? "is-active" : ""}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

function colorToHex(value: number): string {
  return "#" + (value & 0xffffff).toString(16).padStart(6, "0");
}
