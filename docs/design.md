# maku-hime 设计说明

> 目标：一个「装好就能用」的 B 站弹幕姬——原生直连 B 站直播协议，零外部服务依赖，
> 同一份引擎同时喂饱桌面面板、可穿透悬浮窗和 OBS 浏览器源。

## 1. 分层

```
┌──────────────────────────── 前端（同一份 React 代码） ────────────────────────────┐
│  控制面板 index.html        悬浮窗 obs.html?surface=overlay      OBS 墙 obs.html?surface=obs │
└───────────────┬──────────────────────────┬──────────────────────────────┬────────┘
                │ HTTP + WebSocket         │ HTTP + WebSocket              │ HTTP + WebSocket
┌───────────────▼──────────────────────────▼──────────────────────────────▼────────┐
│                        makuhime-server（axum 0.8，默认 127.0.0.1:3939）            │
│  REST：房间 / 发送 / 表情 / 直播间设置 / 登录 / 配置 / 历史 / 导出   WS：/api/ws      │
└───────────────────────────────┬──────────────────────────────────────────────────┘
                                │ 进程内直接调用
┌───────────────────────────────▼──────────────────────────────────────────────────┐
│                              makuhime-core（纯 Rust）                             │
│  engine  ── 房间订阅 / 事件分发 / 去重 / 历史 / TTS / 登录 / 发送                  │
│  client  ── WebSocket 连接、认证、心跳、断线重连、看门狗                           │
│  packet  ── 16 字节头编解码、zlib/brotli 嵌套解压                                  │
│  parser  ── JSON 与 protobuf（SEND_GIFT_V2 / INTERACT_WORD_V2）→ LiveEvent          │
│  http    ── 带 UA/Cookie 的请求、WBI 签名、multipart 上传                          │
│  room    ── 房间信息、在线人数（高能榜）、弹幕服务器发现                            │
│  manage  ── 直播间设置：标题 / 分区 / 封面（裁剪压缩 + 审核状态）/ 公告             │
│  emoticon── 表情面板（GetEmoticons）与表情发送                                     │
└──────────────────────────────────────────────────────────────────────────────────┘
                                ▲
┌───────────────────────────────┴──────────────────────────────────────────────────┐
│  src-tauri：极薄的桌面壳。启动引擎（进程内）+ 本地服务，管理主窗口与透明悬浮窗，      │
│  提供 穿透 / 置顶 / 尺寸 / 托盘 等窗口指令；数据依旧走本地 HTTP/WS。                  │
└──────────────────────────────────────────────────────────────────────────────────┘
```

事件从客户端到引擎走**有界 channel**（512）：消费端慢时反压到 WebSocket 读取侧，
宁可让 TCP 缓冲堆积，也不在内存里无限排队；历史落盘则完全脱离 runtime（见 §5）。

这样拆分的好处：

- **一份引擎，三种展示**：桌面窗口、悬浮窗、OBS 页面、浏览器面板都连同一个本地服务，
  不存在「桌面端功能比网页多」的割裂；
- **窗口无关**：悬浮窗只是另一个 webview，点击穿透/置顶是壳层能力，Web 端可以降级成弹窗；
- **可独立部署**：`maku-hime-server` 能单独跑，当本地弹幕 API 或给 OBS 用。

## 2. B 站协议要点

实现细节来自真实抓包与两份参考实现（arcana-world 依赖的 blivedm-go、UniBarrage）：

| 步骤 | 接口 / 行为 |
|------|-------------|
| 真实房间号 | `GET room/v1/Room/room_init?id=`（短号也支持） |
| 房间信息 | `GET xlive/web-room/v1/index/getRoomBaseInfo?room_ids=`（标题/分区/粉丝/人气/开播状态） |
| 在线人数 | `GET xlive/general-interface/v1/rank/getOnlineGoldRank` 的 `onlineNum`（**不是** get_info 的 online，那是热度） |
| 弹幕服务器 | `GET xlive/web-room/v1/index/getDanmuInfo`，**必须 WBI 签名**，否则 -352 风控 |
| WebSocket | `wss://{host}/sub`，16 字节大端头：len、16、protover、op、seq |
| 认证 | op=7，JSON `{uid:0, roomid, protover:3, buvid, platform:"web", type:2, key:token}` |
| 心跳 | op=2，30 秒一次；op=3 回复携带人气值（大端 u32） |
| 解压 | protover 2 = zlib，3 = brotli；解压后是**若干完整帧**，需要递归解析 |
| 发送弹幕 | `POST msg/send` 表单，带 Cookie 与 csrf（cookie 里的 bili_jct） |
| 开播 / 停播 | `POST room/v1/Room/startLive` / `stopLive` 表单，带 Cookie、csrf 与`area_v2`；返回 rtmp / srt 推流地址与密钥 |
| 直播间设置 | `preLive/PreLive`、`preLive/UpdatePreLiveInfo`、`v2/room/AnchorChangeRoomArea`、`room/AnnounceCommit`，按直播姬签名（同上） |
| 分区目录 | `preLive/GetAreaListForLive`（大分区 → 小分区，注意 **id 是字符串**）、`room/v1/Area/getMyChooseArea` |
| 封面上传 | `POST api.bilibili.com/x/upload/web/image`（multipart：bucket=live、dir=new_room_cover）→ 拿 location 再提交到 UpdatePreLiveInfo |
| 表情面板 | `GET xlive/web-ucenter/v2/emoticon/GetEmoticons?platform=pc&room_id=`；文字在 `emoji` 字段，唯一标识在 `emoticon_unique`（`official_331` 这种） |
| 发表情 | 同一个 `msg/send`，**只发文本**（`msg` 填表情文字，不带 `dm_type`）：B 站服务端自己把文本识别成表情——「冲鸭」→ `official_332` 大表情，`[滑稽]` → 行内小表情。客户端主动带 `dm_type=1` + `emoticon_unique` 反而会被 `10203 表情发送失败` 一律拒绝（实测：换参数名 / 加 wbi 签名 / 加设备指纹 / 改 multipart 都不行，只有官方客户端那条链路能发） |

消息侧：`DANMU_MSG` 仍是 JSON；**礼物与进房已经迁移到 protobuf**
（`SEND_GIFT_V2` / `INTERACT_WORD_V2` 的 `data.pb` 是 base64 protobuf）。
为了不引入 prost/protoc，`pb.rs` 实现了一个够用的 wire 解析器，
字段号来自抓包比对（见 `parser.rs` 里 `parse_gift_v2` 的注释）。

参考实现里几个坑，这里都绕开了：

1. blivedm-go 重连只睡 3ms → 这里指数退避 1s→30s；
2. 认证失败被静默丢弃 → 这里上报并触发重连；
3. 没有心跳看门狗，半死 TCP 不会重连 → 这里超过 2.5 个心跳周期没收到帧就主动重连；
4. 每个子包一个 goroutine 导致乱序 → 这里顺序处理；
5. token 只取一次 → 这里每次重连都重新拿 token 与 host；
6. 登录态下 getDanmuInfo 返回的 token 与账号绑定：认证包里的 uid 必须是登录 uid，
   握手也要带同一份 Cookie。只发 `uid: 0` 而不带 Cookie 会被服务端直接 reset
   （表现为连上即「Connection reset without closing handshake」，无限重连却收不到弹幕）。

## 3. 事件模型

```rust
Event { id, ts, room_id, #[serde(flatten)] body: LiveEvent }

LiveEvent::Chat { user, text, emoticon }
LiveEvent::Gift { user, gift }
LiveEvent::Guard { user, level, num, price, item }
LiveEvent::SuperChat { user, text, price, duration, bg_start, bg_end, price_color, id }
LiveEvent::Enter { user, kind }        // 进房/关注/分享/互粉/点赞
LiveEvent::Like { user, count, total }
LiveEvent::Popularity / Watched / OnlineRank / LiveStatus / RoomUpdate / Notice / Remove
```

- `id` 用于去重（`info.9.ct`、礼物批次、SC id，兜底是 `cmd-uid-ts`）；
- 前端拿到的是扁平 JSON（`type` 为判别字段），历史落盘也是同一份结构。

## 4. 悬浮窗与穿透

- 桌面端用一个无边框、透明、置顶的 Tauri 窗口加载 `obs.html?surface=overlay`；
- **单实例**（`tauri-plugin-single-instance`）：第二次启动只聚焦已有窗口，
  带 `--overlay` 时顺便打开悬浮窗，避免两个进程各占端口、各订阅一份弹幕；
- **几何持久化**：窗口的 `Moved` / `Resized` 事件先更新壳里的几何状态，
  停止变化 400ms 后由引擎统一写回 `config.toml`（唯一写盘入口），
  这样拖动过程中不会把磁盘写爆；niri 下没有可靠的位置事件，拖动结束后壳层轮询 IPC
  真实几何再写盘；浮动化完成前的平铺几何由 `geometry_ready` 守卫，不会污染配置；
- 服务端对 HTTP/WS 统一做来源校验（`origin.rs`）：Host 必须是回环地址（否则拒绝，防 DNS rebinding），
  带 Origin 的请求必须是本机页面 / Tauri 壳 / 与 Host 同源；
  穿透通过 `set_ignore_cursor_events`（GTK 的输入区域）；Wayland 原生窗口下由合成器决定是否采纳，
  不采纳时退化成普通置顶窗（启动日志会提示）。用 `GDK_BACKEND=x11` 走 XWayland 时，
  XShape 的输入区域会被 XWayland 转成 `wl_surface.set_input_region`，实测可用；
- 锁定（穿透）**只留顶部控制条可点**：壳层把“控制条高度 + 4px 窗口内边距”作为输入区域
  交给 `wl_surface.set_input_region`（GTK3 的 Wayland 后端没实现输入区域，壳层里用一小段 C
  直接调 Wayland 接口），其余区域鼠标直接穿到下面的窗口；合成器不采纳时退回整窗
  `set_ignore_cursor_events(true)`，这时解锁入口在主窗口标题栏的「解锁悬浮窗」与托盘菜单。
  锁定时输入条变只读（提示「解锁后可发送弹幕」）、键盘翻页也一并释放；顶栏隐藏时同样只能从
  主窗口 / 托盘解锁。默认配置是**解锁**，打开悬浮窗即可拖动、发弹幕；
- **界面 1:1 照官方聊天面板**：`obs.html?surface=overlay` 用独立的皮肤（`src/styles/bd.css`）
  渲染 —— 12px 圆角卡片（28px 控制条 + 弹幕墙 + 32px 发送条）、行首 24px 圆形头像、
  行内官方排版、4px 自定义滚动条 + 滚轮一次 3 行、PageUp/Down + Home/End、「↓ N 条新弹幕」
  药丸、右下角 14px 无手柄缩放热区、深/浅两套官方配色；几何与文案见
  [`docs/research/chat-panel-parity.md`](research/chat-panel-parity.md)，参照实现是 bili-dm 的浮窗。
  控制面板与 OBS 墙用同一套 `Wall` 组件的 `panel` 变体，不受这套皮肤影响；
- **拖动**：解锁后按住面板任意非交互区域拖动。niri 不允许客户端定位普通窗口，因此 niri
  会话里由 WebView 逐帧发坐标、壳层经 niri IPC `MoveFloatingWindow` 跟手，拖动结束时读回
  真实几何写配置；其它环境交给窗口系统原生拖动（Tauri `startDragging`）；
- **niri 浮动化**：普通窗口在 niri 会被塞进平铺布局。悬浮窗创建后壳层通过 niri IPC
  `MoveWindowToFloating` 把它移入浮动布局，再用 `SetWindowWidth/Height` 与
  `MoveFloatingWindow` 恢复配置里的尺寸位置；niri 报告的浮动坐标带固定 y 偏移，创建时校准一次。
  niri 不可用时完全跳过，不影响其它桌面；
- 悬浮窗底部的发送条与主面板共用 `/api/send`，服务端本地回显后所有展示面同步出现，不需要前端
  各插一条；
- OBS 浏览器源用 `obs.html?surface=obs`：全透明、无边框、带文字描边，
  直接加到 OBS「浏览器」源即可。

## 5. 历史与 TTS

- 历史：内存环形缓冲（每房间默认 2000 条）+ `~/.local/share/maku-hime/history/{room}.jsonl`。
  查询走内存，**写盘走独立线程**（每条消息只投递到 channel，线程空闲 500ms 批量 flush，
  不阻塞 tokio worker）；读尾部时按字节对齐换行再用 `from_utf8_lossy` 解析，
  1MB 边界切在多字节字符中间也不会整份历史丢失；导出在阻塞线程池里做，最多 50 万条；
- TTS：队列上限可配，满了丢弃并计数；后端有 `auto`（探测 edge-playback / ekho /
  espeak-ng / spd-say）、`command`（自定义 argv）、`webview`（浏览器 speechSynthesis）、
  `mimo`（小米 MiMo 云端 TTS）、`off`；文本会去掉 `[表情]` 与链接，并按字数截断。
- MiMo 的请求/响应格式与 dmnotifier 的 tts 插件一致：`POST {base_url}/chat/completions`，
  `api-key` 请求头，assistant 单条消息，`audio.format = "wav"`，音频 base64 在
  `choices[0].message.audio.data`；壳层把 WAV 写临时文件后交给 mpv / ffplay / paplay 播放，
  失败原因会以 notice 推到界面。API Key 与其余配置一起存 `config.toml`，写盘权限 0600，
  Debug 输出脱敏。

## 6. 配置

`~/.config/maku-hime/config.toml`，字段与 `AppConfig` 一一对应；
`~/.config/maku-hime/credentials.json`（0600）保存扫码登录得到的 SESSDATA / bili_jct。

## 7. 已知限制

- Wayland（niri / Hyprland 等）上 GTK 窗口的输入区域依赖合成器，部分环境无效；
  实在不行可以把悬浮窗当作普通置顶窗用，或直接用 OBS 浏览器源。niri 已做浮动化与手动拖动适配，
  其它平铺合成器仍可能把悬浮窗塞进布局；锁定“只保留顶栏可点”依赖合成器的输入区域支持，
  不生效时退化成整窗穿透；
- 开播接口有风控与认证要求（实名 / 人脸），失败原因会翻译后显示在开播面板；推流地址与密钥
  只在本次开播面板中返回，不落盘；
- 开播 / 停播按直播姬客户端身份请求：`platform=pc_link`，附 `access_key`、`appkey`、
  `build`、`version`、`ts` 与 `md5(query + appsecret)` 签名，UA 用 `LiveHime/…`，
  Cookie 补 `appkey` / `device_platform` / `buvid3`；否则会被 `60034` 拒绝；
- 真被 `60034` 拒绝时（账号或分区被单独限制），服务端把 B 站错误码放进 `error.upstream`，
  前端据此在面板里给「用直播姬开播 + 换分区重试」的指引；
- `60024` / `60043` 是「这一次开播要人脸认证」，不是账号没实名。开播接口返回
  `{status:"verify"}` 而不是推流信息：后端用 `risk_extra.v_voucher` 调
  `x/gaia-vgate/v2/register`（浏览器身份，直播姬 UA 会被拒），把 content 解成 token、
  混淆后拼成 `/h5/risk-control/realname?t=…`，前端画成二维码供扫码认证；
- OBS 页面与浏览器面板的音频/语音由系统决定，WebView 里的 speechSynthesis 在
  WebKitGTK 上不一定有中文音色，建议用 `command` 后端；
- B 站接口有风控，高频刷新房间信息会触发 -352；这里默认 5 分钟刷新一次房间信息、
  30 秒刷新一次在线人数，正常使用不会触发。
