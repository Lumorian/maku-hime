import { useEffect, useMemo, useState } from "react";
import { api } from "../api";
import { useStore } from "../store";
import type { AreaOptions, RoomArea, RoomSettings, RoomState, Snapshot } from "../types";
import { cn } from "../util";
import { IconClose, IconImage, IconRefresh } from "./icons";

interface RoomSettingsPanelProps {
  room: RoomState;
  snapshot: Snapshot;
  onClose: () => void;
}

/** 封面审核状态 → 展示文案。 */
function coverStatusText(settings: RoomSettings): string {
  switch (settings.cover_status) {
    case 1:
      return "已通过审核";
    case 0:
      return "审核中";
    case -1:
      return settings.cover_reason
        ? `未通过：${settings.cover_reason}`
        : "未通过审核";
    default:
      return "状态未知";
  }
}

/**
 * 直播间设置：标题、分区、封面、公告。
 *
 * 只有主播本人能改（服务端也会再校验一次）。封面是「上传 → 提交 → 审核」三步，
 * 所以上传成功不等于立刻生效，这里把审核状态一并显示出来。
 */
export function RoomSettingsPanel({ room, snapshot, onClose }: RoomSettingsPanelProps) {
  const toast = useStore((state) => state.toast);
  const [settings, setSettings] = useState<RoomSettings | null>(null);
  const [options, setOptions] = useState<AreaOptions | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [title, setTitle] = useState("");
  const [parentName, setParentName] = useState("");
  const [areaId, setAreaId] = useState(0);
  const [announcement, setAnnouncement] = useState("");

  const [coverData, setCoverData] = useState<string | null>(null);
  const [coverUrl, setCoverUrl] = useState("");
  const [coverBusy, setCoverBusy] = useState(false);

  const isAnchor = snapshot.login.logged_in && snapshot.login.uid === room.uid;

  const load = async () => {
    setLoading(true);
    setError(null);
    try {
      const [next, list] = await Promise.all([
        api.roomSettings(room.room_id),
        api.roomAreas(room.room_id),
      ]);
      setSettings(next);
      setOptions(list);
      setTitle(next.title);
      setAreaId(next.area_id);
      setAnnouncement(next.announcement);
      const matched = list.areas.find((area) => area.id === next.area_id);
      setParentName(matched?.parent ?? next.parent_name);
      setCoverData(null);
      setCoverUrl("");
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "读取直播间设置失败");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [room.room_id]);

  const parents = useMemo(() => {
    const map = new Map<string, RoomArea[]>();
    for (const area of options?.areas ?? []) {
      const list = map.get(area.parent) ?? [];
      list.push(area);
      map.set(area.parent, list);
    }
    return [...map.entries()].map(([name, areas]) => ({ name, areas }));
  }, [options]);

  const children = useMemo(
    () => parents.find((item) => item.name === parentName)?.areas ?? [],
    [parents, parentName],
  );

  const save = async () => {
    if (!settings) return;
    const patch: { title?: string; area_id?: number; announcement?: string } = {};
    if (title.trim() !== settings.title) patch.title = title.trim();
    if (areaId !== settings.area_id) patch.area_id = areaId;
    if (announcement.trim() !== settings.announcement) patch.announcement = announcement.trim();
    if (Object.keys(patch).length === 0) {
      toast("info", "没有改动需要保存");
      return;
    }
    setSaving(true);
    try {
      const next = await api.saveRoomSettings(room.room_id, patch);
      setSettings(next);
      setTitle(next.title);
      setAreaId(next.area_id);
      setAnnouncement(next.announcement);
      toast("success", "直播间设置已保存");
    } catch (cause) {
      toast("error", cause instanceof Error ? cause.message : "保存失败");
    } finally {
      setSaving(false);
    }
  };

  const pickFile = (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    if (file.size > 20 << 20) {
      toast("error", "封面图片超过 20MB，请先压缩");
      return;
    }
    const reader = new FileReader();
    reader.onload = () => {
      if (typeof reader.result === "string") {
        setCoverData(reader.result);
        setCoverUrl("");
      }
    };
    reader.onerror = () => toast("error", "读取图片失败");
    reader.readAsDataURL(file);
  };

  const uploadCover = async () => {
    if (!coverData && !coverUrl.trim()) {
      toast("warn", "先选一张图片或粘贴图片地址");
      return;
    }
    setCoverBusy(true);
    try {
      const outcome = await api.uploadCover(
        room.room_id,
        coverData ? { data: coverData } : { url: coverUrl.trim() },
      );
      const status =
        outcome.status === 1
          ? "已通过审核"
          : outcome.status === -1
            ? `未通过：${outcome.reason || "内容不合规"}`
            : "已提交，等待审核";
      toast("success", `封面${status}`);
      setCoverData(null);
      setCoverUrl("");
      // 重新读一次，拿到最新封面与审核状态
      const next = await api.roomSettings(room.room_id);
      setSettings(next);
    } catch (cause) {
      toast("error", cause instanceof Error ? cause.message : "上传封面失败");
    } finally {
      setCoverBusy(false);
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div
        className="modal room-settings"
        role="dialog"
        aria-label="直播间设置"
        onClick={(event) => event.stopPropagation()}
      >
        <header className="modal-header">
          <h2>直播间设置 · {room.uname || `房间 ${room.room_id}`}</h2>
          <button type="button" className="icon-button" onClick={onClose} title="关闭">
            <IconClose size={16} />
          </button>
        </header>

        {!isAnchor && (
          <p className="live-hint">只有主播本人能改直播间设置，当前登录的不是这个直播间的主播。</p>
        )}

        {error && <p className="live-error">{error}</p>}

        {loading ? (
          <p className="live-hint">正在读取直播间设置…</p>
        ) : (
          settings && (
            <div className="settings-body">
              <div className="field-block">
                <span className="field-label">直播间标题</span>
                <input
                  value={title}
                  maxLength={40}
                  onChange={(event) => setTitle(event.target.value)}
                  placeholder="标题会显示在直播间和分区列表里"
                />
                <span className="field-hint">{title.length}/40</span>
              </div>

              <div className="field-block">
                <span className="field-label">直播分区</span>
                <div className="area-row">
                  <select
                    value={parentName}
                    onChange={(event) => {
                      setParentName(event.target.value);
                      const first = parents.find((item) => item.name === event.target.value)
                        ?.areas[0];
                      if (first) setAreaId(first.id);
                    }}
                  >
                    <option value="">选择大分区</option>
                    {parents.map((item) => (
                      <option key={item.name} value={item.name}>
                        {item.name}
                      </option>
                    ))}
                  </select>
                  <select
                    value={areaId || ""}
                    onChange={(event) => setAreaId(Number(event.target.value))}
                  >
                    <option value="">选择小分区</option>
                    {children.map((area) => (
                      <option key={area.id} value={area.id}>
                        {area.name}
                      </option>
                    ))}
                  </select>
                </div>
                {options && options.recent.length > 0 && (
                  <div className="area-recent">
                    <span className="field-hint">最近用过：</span>
                    {options.recent.map((area) => (
                      <button
                        key={area.id}
                        type="button"
                        className={cn("chip", area.id === areaId && "is-active")}
                        onClick={() => {
                          setAreaId(area.id);
                          setParentName(area.parent);
                        }}
                      >
                        {area.parent} · {area.name}
                      </button>
                    ))}
                  </div>
                )}
                <span className="field-hint">
                  改成「仅直播姬」的分区后，第三方推流会被 B 站挡掉（60034）。
                </span>
              </div>

              <div className="field-block">
                <span className="field-label">直播间封面</span>
                <div className="cover-row">
                  <div className="cover-preview">
                    {(coverData || coverUrl.trim() || settings.cover) ? (
                      <img
                        src={coverData ?? (coverUrl.trim() || settings.cover)}
                        alt="直播间封面预览"
                        referrerPolicy="no-referrer"
                      />
                    ) : (
                      <span>还没有封面</span>
                    )}
                  </div>
                  <div className="cover-side">
                    <span className={cn("cover-status", settings.cover_status === -1 && "is-bad")}>
                      {coverStatusText(settings)}
                    </span>
                    <label className="file-button">
                      <IconImage size={14} />
                      选择本地图片
                      <input type="file" accept="image/*" onChange={pickFile} hidden />
                    </label>
                    <div className="cover-url">
                      <input
                        value={coverUrl}
                        placeholder="或粘贴图片链接（https）"
                        onChange={(event) => {
                          setCoverUrl(event.target.value);
                          if (event.target.value.trim()) setCoverData(null);
                        }}
                      />
                    </div>
                    <button
                      type="button"
                      className="primary"
                      onClick={() => void uploadCover()}
                      disabled={coverBusy || (!coverData && !coverUrl.trim())}
                    >
                      {coverBusy ? "上传中…" : "上传封面"}
                    </button>
                    <span className="field-hint">统一裁成 16:9 再上传，提交后需要 B 站审核。</span>
                  </div>
                </div>
              </div>

              <div className="field-block">
                <span className="field-label">直播间公告</span>
                <textarea
                  value={announcement}
                  maxLength={500}
                  rows={3}
                  onChange={(event) => setAnnouncement(event.target.value)}
                  placeholder="留空表示清空公告"
                />
                <span className="field-hint">{announcement.length}/500</span>
              </div>

              <div className="row-actions">
                <button
                  type="button"
                  className="ghost"
                  onClick={() => void load()}
                  disabled={saving || coverBusy}
                >
                  <IconRefresh size={15} />
                  重新读取
                </button>
                <button
                  type="button"
                  className="primary"
                  onClick={() => void save()}
                  disabled={saving || !isAnchor}
                >
                  {saving ? "保存中…" : "保存标题 / 分区 / 公告"}
                </button>
              </div>
            </div>
          )
        )}
      </div>
    </div>
  );
}
