# maku-hime · 幕姬

> 轻量、可穿屏的 B 站弹幕姬。原生直连 B 站直播协议，零外部服务；一份引擎同时喂饱桌面面板、悬浮窗和 OBS 浏览器源。

![控制面板](docs/screenshots/panel.png)

| 悬浮窗（可穿屏） | OBS 弹幕墙 |
| --- | --- |
| ![悬浮窗](docs/screenshots/overlay.png) | ![OBS 弹幕墙](docs/screenshots/wall.png) |

| 设置抽屉 | |
| --- | --- |
| ![设置](docs/screenshots/settings.png) | 深浅两套主题、字号/行数/透明度/点击穿透/语音播报都在这里调 |

## 特性

- **原生直连**：自己实现 B 站直播的 WebSocket 协议（16 字节帧头、zlib/brotli 嵌套解压、
  WBI 签名的 getDanmuInfo、心跳与看门狗），不依赖 UniBarrage / blivedm 之类的中间服务，
  填房间号就能收弹幕。
- **完整消息类型**：聊天、礼物（新的 protobuf `SEND_GIFT_V2`）、上舰、醒目留言、进房/关注/
  点赞、人气与在线人数、开播下播。粉丝勋章带渐变配色，长弹幕自动换行。
- **三种展示面**：桌面控制面板、无边框穿透悬浮窗、OBS 浏览器源，全部连同一个本地服务，
  看到的内容完全一致。
- **悬浮窗**：透明、置顶、无边框，排版与交互 1:1 照官方聊天面板做：12px 圆角卡片、行首 24px
  圆形头像、粉丝勋章、深/浅两套官方配色、醒目留言卡片；滚轮一次 3 行 + 4px 可拖滚动条，
  锁定后整窗穿透，解锁后任意位置拖动 + 右下角无手柄缩放；位置尺寸写进配置，下次启动恢复。
  niri 下会自动移出平铺布局并按上次位置浮动，不再需要手写窗口规则。
- **开播助手**：主播本人可在面板里一键开播，拿到 RTMP / SRT 推流地址与密钥，复制进 OBS；
  直播中可停播。推流密钥只在开播面板里出现，不写配置、不落日志。
- **直播间设置**：标题、两级分区（带最近使用）、封面、公告都能在面板里改；封面支持选本地图片
  或粘贴图片链接，后端统一裁成 16:9 再上传，并把 B 站的审核状态（审核中 / 通过 / 被拒理由）显示出来。
- **单实例 + 窗口记忆**：重复启动只聚焦已有窗口（带 `--overlay` 会顺带打开悬浮窗）；
  悬浮窗拖动/缩放后的位置尺寸会写回配置，下次启动恢复。
- **扫码发弹幕**：手机扫二维码登录，SESSDATA / bili_jct 只落在本机（0600 文件）；
  没登录也能只看不发，输入条会明确提示。
- **表情面板**：主面板和悬浮窗的发送条都能调出表情面板，按表情包分页签；
  小黄脸这类文本表情点一下是**插进输入框**（可以连着插好几个一起发），大表情点一下直接发送。
  官方表情、第三方 IP 表情包、粉丝牌自定义表情都在里面（B 站下发什么就能发什么）。
- **房间状态**：标题、分区、主播、粉丝数、人气值，以及官方高能榜的**真实在线人数**
  （不是把热度当在线数），30 秒刷新一次。
- **语音播报**：弹幕 / 礼物 / 上舰 / 醒目留言可分别开关，后端可选系统命令
  （edge-playback、ekho、espeak-ng、spd-say 自动探测）、自定义 argv、浏览器语音，
  或**小米 MiMo 云端 TTS**（填 API Key / 模型 / 音色，合成 WAV 后用本机播放器播放）；
  队列有上限，弹幕洪水时丢新不堆积。
- **历史与导出**：内存环形缓冲 + 按房间的 JSONL 落盘，支持按类型/关键字筛选，
  一键导出 CSV / JSON / JSONL。
- **轻**：Rust 引擎 + Tauri 壳（不是 Electron），只跑服务时几乎不占资源；
  服务端可以单独跑，当本地弹幕 API 用。

## 快速开始

### 依赖

- Rust 1.88+（本项目在 1.99 上开发；用了 edition 2024 的 let-chains）
- Node.js 20+ 与 npm
- Linux 桌面端还需要 WebKitGTK：Arch 上是 `webkit2gtk-4.1`、`gtk3`、`libayatana-appindicator`（托盘可选）

```bash
# Arch
sudo pacman -S webkit2gtk-4.1 gtk3 nodejs npm
```

### 从源码运行

```bash
npm install
npm run build            # 构建前端到 dist/
cargo run -p maku-hime   # 桌面应用（主窗口 + 悬浮窗）
```

只想跑服务（浏览器面板 / OBS）：

```bash
cargo run -p makuhime-server -- --room 7734200 --open
# 控制面板 http://127.0.0.1:3939/
# OBS 弹幕墙 http://127.0.0.1:3939/obs?room=7734200
```

### 打包

```bash
npm run tauri:build                    # 产出 deb / AppImage 到 target/release/bundle
npx tauri build --no-bundle            # 只编译 release 二进制（前端已内嵌）
npm run package                        # 打「解压即用」的 tar.gz（桌面端 + 本地服务 + dist）
```

本地实测两种 Linux 产物都能直接构建：

| 产物 | 大小 | 内容 |
|------|------|------|
| `target/release/bundle/deb/maku-hime_0.1.0_amd64.deb` | 5.2 MB | `/usr/bin/maku-hime`、`/usr/lib/maku-hime/dist`（OBS/面板静态资源）、desktop 文件、32/128/256 图标 |
| `target/release/bundle/appimage/maku-hime_0.1.0_amd64.AppImage` | 104 MB | 自带 WebKitGTK 依赖，`chmod +x` 直接运行 |
| `dist-packages/maku-hime-0.1.0-linux-x86_64.tar.gz` | 8.2 MB | 桌面端 + `maku-hime-server` + `dist/`，解压即用 |

AppImage 首次构建会下载 linuxdeploy / appimagetool（缓存在 `~/.cache/tauri`）。

Arch 这类没有 dpkg 的发行版直接装 deb 不方便，用 `npm run package` 解压即用的包即可。

## 使用

### 订阅房间

左下角输入房间号（短号也行，或直接粘贴 `https://live.bilibili.com/xxx`）回车即可订阅；
订阅列表写进配置，下次启动自动重连。支持多房间同时收，切换房间看各自的弹幕墙。

### 悬浮窗

- 面板右上角「悬浮窗」或托盘菜单打开；
- 启动时想直接开悬浮窗：`cargo run -p maku-hime -- --overlay`（或 `MAKUHIME_OPEN_OVERLAY=1`）；
- **1:1 官方排版**：整窗就是一张 12px 圆角卡片（顶部控制条 + 弹幕墙 + 发送条），行数随窗口
  高度自适应并始终贴住最新一条。行内是官方那套：行首 24px 圆形头像、粉丝勋章（含提督/舰长
  图标）、昵称 ` : ` 正文；礼物 / 上舰 / 进房 / 点赞按官方文案与配色，醒目留言是整块渐变卡片；
  顶部一条 28px 控制条，左边是「● 直播中 · 在线 N」在线徽标，右边是锁 / 设置 / 关闭三个按钮；
  连接异常时卡片下方会有一行灰色状态提示；
- **滚动**：滚轮一次滚 3 行，右侧 4px 半透明滚动条可拖（12px 热区），PageUp / PageDown 翻页、
  Home / End 跳首尾；往上翻之后底部出现「↓ N 条新弹幕」，点一下就回到最新；
- **拖动 + 缩放**：默认解锁，打开就能按住面板任意空白处拖动；右下角 14px 热区可缩放（不画手柄），
  **位置与尺寸会写回 `config.toml` 并在下次启动时恢复**（拖动时做 400ms 防抖，不会频繁写盘）；
  最小尺寸 280x160；
- **发弹幕**：底部输入条和主面板共用同一套登录态，20 字上限并显示「N/20」计数，回车即发、
  发送后在弹幕墙里本地回显，出错时输入条右侧给一行提示。点输入条右侧「登录」可以在悬浮窗里
  直接扫码，不必切回主面板；
- **锁定（点击穿透）**：点顶栏的锁按钮锁定，之后**只有顶栏这一条可点**（再点一下就解锁），
  其余区域鼠标直接穿透到下面的窗口，弹幕照常滚动，输入条同时变只读并提示「解锁后可发送弹幕」；
  顶栏高度由页面实时测量后告诉壳层；
  锁定走的是 Wayland 的 `wl_surface.set_input_region`（GTK3 的 Wayland 后端没实现输入区域，
  壳层里用一小段 C 直接调 Wayland 接口）。合成器不支持输入区域时会退回整窗穿透，
  这时解锁入口在**主窗口顶部的「解锁悬浮窗」**或**托盘菜单的「解锁 / 锁定悬浮窗」**；
- 顶栏隐藏（`show_header = false`）时没有可点区域，同样从主窗口或托盘解锁；
- **niri**：启动/打开悬浮窗时会通过 niri IPC 自动把窗口移入浮动布局、恢复上次位置与尺寸，
  拖动交给合成器处理；不需要再写 `open-floating` 窗口规则。Hyprland / Sway 等合成器
  请按其窗口规则把 `maku-hime` 的悬浮窗设为浮动；
- Wayland 下穿透由合成器决定（niri / Hyprland 较新版本支持），不生效时它就是普通置顶窗。

### OBS 浏览器源

在 OBS 里添加「浏览器」源，URL 填：

```
http://127.0.0.1:3939/obs?room=7734200
```

页面背景全透明、带文字描边，可以直接叠在画面上。想在浏览器里预览时加 `&bg=dark` 给个底板。

### 发弹幕

左下角「扫码登录」→ 用 B 站手机端扫码 → 输入条可用。主面板和悬浮窗底部的输入条都能发，
发送走官方 `msg/send`，错误会翻译成人话（未登录 / 被禁言 / 发送太频繁 / 内容重复 / 风控…）。

输入条右侧的笑脸按钮是表情面板：按表情包分页签。表情列表来自 B 站的
`GetEmoticons`（直播表情）与评论区表情面板（小黄脸 / 颜文字 / 热词 / IP 联动包）。

点法跟官方一致：**带标识的大表情点一下直接发**，**只有文本的表情（小黄脸这类）插进输入框**，
想发一串「[滑稽][doge][看]」就挨个点，最后一起发。没解锁的表情
（粉丝牌等级不够之类）点了会被 B 站拒绝，错误直接显示出来。

### 开播 / 停播

登录主播账号后，房间标题右侧会出现「开播」：

1. 点「开播」→ 确认当前标题/分区 → 「开始直播」；
2. B 站返回推流地址和密钥，点复制填进 OBS「设置 → 直播 → 自定义」；
3. OBS 开始推流后，B 站直播间才会真正出现画面；面板里再点「直播中」可以停播。

首次开播可能需要先在 B 站完成实名认证 / 人脸验证，错误会直接显示在面板里。
推流密钥相当于直播间密码，只显示在本机开播面板，不写进配置也不会进日志。

开播接口只对官方直播姬开放，所以请求按直播姬客户端的规则来：`platform=pc_link`、
固定带 `appkey` / `build` / `version` / `ts` 并做 md5 签名，UA 用 `LiveHime/…`，
Cookie 里补 `appkey` / `device_platform` / `buvid3`。这样就不会再被 `code=60034`
（「系统维护仅支持直播姬开播」）挡掉。

万一仍然返回 60034，说明这个账号或分区被单独限制，面板会给出退路：
用直播姬开播后把推流地址填进 OBS、换个分区重试，或者等维护时段过去。

### 直播间设置

标题右侧的「直播间设置」可以改标题、分区、封面和公告（只有主播本人看得见这个按钮）：

1. 标题最多 40 字，分区选大分区 + 小分区，下面还列着最近用过的分区；
2. 封面可以选本地图片（超过 20MB 会提示先压缩），也可以粘贴 https 图片链接；
   后端统一裁成 16:9、压成 JPEG 再传，传完要在 B 站过审，面板会显示审核状态与拒绝原因；
3. 公告留空就是清空。

改分区要注意：被划到「仅直播姬」的分区之后，第三方推流会被 B 站用 60034 挡掉。

B 站还可能对**某一次开播**要求人脸认证（`60024` / `60043`，跟账号有没有实名无关）。
这时面板不再只报错，而是把 B 站给的 `v_voucher` 换成实名认证页，画成二维码：
手机 B 站扫一下、做完人脸，回来点「完成认证，重新开播」即可。

### 语音播报

设置 → 语音播报：打开开关、选后端、试听。

- `自动`：依次探测 edge-playback → ekho → espeak-ng → spd-say；
- `自定义命令`：填 argv，`{text}` 会替换成播报内容，例如
  `edge-tts --voice zh-CN-XiaoxiaoNeural --text {text} --write-media /tmp/t.mp3 && mpv /tmp/t.mp3`
  这种管线请用 `sh -c`；
- `浏览器语音`：用 WebView 的 speechSynthesis（无依赖，音色看系统）；
- `小米 MiMo`：填 API Key（其余有默认值），请求 `{base_url}/chat/completions`
  生成 WAV，再交给本机 mpv / ffplay / paplay / pw-play 播放。预置音色有
  茉莉 / 冰糖 / 苏打 / 白桦 / Mia / Chloe / Milo / Dean，也可以自己填。API Key 只保存在
  本机 `config.toml`（文件权限 0600），不会出现在日志里。

  ```toml
  [tts]
  provider = "mimo"

  [tts.mimo]
  api_key = "你的 MiMo API Key"
  base_url = "https://api.xiaomimimo.com/v1"
  model = "mimo-v2.5-tts"
  voice = "茉莉"
  ```

### 配置与数据

| 路径 | 内容 |
|------|------|
| `~/.config/maku-hime/config.toml` | 全部设置（外观、悬浮窗、TTS、发送、自动订阅房间） |
| `~/.config/maku-hime/credentials.json` | 扫码登录凭据（0600） |
| `~/.local/share/maku-hime/history/{room}.jsonl` | 弹幕历史，一行一个事件 |

配置示例：

```toml
[server]
port = 3939

[ui]
theme = "dark"
font_size = 13
max_rows = 300
show_avatar = true
show_gift = true

[overlay]
width = 380
height = 640
opacity = 0.88
click_through = false   # 默认解锁（可拖动 / 发弹幕）；需要穿透再改 true
always_on_top = true
show_header = true
show_composer = true

[tts]
enabled = false
provider = "auto"
# 小米 MiMo：provider = "mimo" 时使用
# [tts.mimo]
# api_key = "…"
# base_url = "https://api.xiaomimimo.com/v1"
# model = "mimo-v2.5-tts"
# voice = "茉莉"

[danmaku]
heartbeat_seconds = 30
stats_interval_seconds = 30

rooms = [7734200]
```

### 网络与代理

所有对外请求（B 站接口、弹幕服务器、MiMo 语音）都**直连**，不读取 `http_proxy` /
`https_proxy` / `all_proxy` 这类系统代理设置：开着 Clash / v2rayN 之类的「系统代理」
时来回切节点、临时关代理，都不会把正在收的弹幕打断。WebSocket 走 tokio-tungstenite，
本身也不认系统代理。

必须走代理才能连 B 站（公司网络、海外线路）时，用环境变量显式打开：

```bash
MAKUHIME_SYSTEM_PROXY=1 ./maku-hime-server
# Tauri 桌面版：MAKUHIME_SYSTEM_PROXY=1 ./maku-hime
```

打开后就恢复成普通 HTTP 客户端的行为——**换代理节点依然会断开**，请挑一个稳定的节点。

## 本地接口

服务默认监听 `127.0.0.1:3939`（端口被占用会自动顺延），所有前端共用：

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/api/health` | 健康检查 |
| GET | `/api/state` | 全量快照（房间、登录态、配置） |
| WS | `/api/ws` | 事件流（`event` / `snapshot` / `speak` / `notice`） |
| POST | `/api/rooms` | 订阅房间 `{room_id}` |
| DELETE | `/api/rooms/{id}` | 取消订阅 |
| POST | `/api/rooms/{id}/refresh` | 刷新房间信息 |
| POST | `/api/send` | 发弹幕 `{room_id, text}` |
| POST | `/api/live/start` | 开播 `{room_id}`，返回推流地址 / 密钥 / 协议列表 |
| POST | `/api/live/stop` | 停播 `{room_id}` |
| GET/POST | `/api/rooms/{id}/settings` | 读 / 改直播间设置（标题、分区、公告） |
| GET | `/api/rooms/{id}/areas` | 分区目录 + 最近使用 |
| POST | `/api/rooms/{id}/cover` | 上传封面 `{data}` 或 `{url}` |
| GET | `/api/rooms/{id}/emoticons` | 表情面板（按表情包分组） |
| POST | `/api/send/emoticon` | 发表情 `{room_id, unique_name, text, id, url, width, height}` |
| GET | `/api/history/{id}?kinds=chat,gift&q=&limit=` | 历史查询 |
| GET | `/api/history/{id}/export?format=csv` | 导出 |
| GET | `/api/login/qrcode` / `/api/login/qrcode/poll?key=` | 扫码登录 |
| POST | `/api/login/logout` | 退出登录 |
| GET/PUT | `/api/config` | 读写配置 |
| POST | `/api/tts/test` | 试听 `{text}` |

**访问令牌**：除 `/api/health` 外，所有 `/api` 接口都要带令牌，否则返回 401
`{"error":{"code":"unauthorized"}}`。令牌在首次启动时随机生成，存在
`~/.config/maku-hime/api-token`（0600），也可以用环境变量 `MAKUHIME_API_TOKEN` 指定。
用浏览器打开面板时令牌由页面自动注入（同时种下 `HttpOnly` 的 `maku_token` Cookie），
外部脚本调用要么加请求头 `x-maku-token: <令牌>`，要么加查询参数 `?token=<令牌>`
（WebSocket 只能用查询参数）。**不要**把带令牌的地址发给别人。

## 项目结构

```
crates/makuhime-core/    协议、解析、引擎、历史、TTS、登录、发送
crates/makuhime-server/  axum REST + WebSocket + 静态页面
src-tauri/               Tauri 桌面壳（主窗口、悬浮窗、托盘、窗口指令）
src/                     React 前端（控制面板 + 弹幕墙）
scripts/                 录制真实弹幕、截图、图标生成、开发启动
docs/                    设计说明、协议调研、官方面板排版规格
```

协议细节与架构取舍见 [docs/design.md](docs/design.md)，
调研原文见 [docs/research/bilibili-protocol.md](docs/research/bilibili-protocol.md) 与
[docs/research/chat-panel-parity.md](docs/research/chat-panel-parity.md)。

## 开发

```bash
bash scripts/dev.sh 7734200   # 同时起 Rust 服务（3939）与 Vite（5173，热更新）
cargo test                    # 引擎 / 解析 / 历史 / 路由 / 开播 / TTS 测试（89 个）
npm test                      # 前端组件与状态测试（Vitest + Testing Library，17 个）
npm run lint                  # ESLint（TypeScript + react-hooks）
npm run typecheck             # 前端类型检查
node scripts/screenshot.mjs http://localhost:5173/ shot.png 8000 1440 900   # headless 截图
node scripts/capture.mjs 7734200 60 fixtures/live.jsonl                     # 录制真实弹幕做夹具
```

弹幕夹具 `crates/makuhime-core/tests/fixtures/messages.jsonl` 就是真实抓包裁剪出来的，
解析器的单元测试跑在它上面。前端测试覆盖数字/在线人数/时间格式化、zustand 去重与窗口裁剪，
以及弹幕墙的「只渲染最近 N 条」「上翻不打断阅读 + 新弹幕提示」等行为。
CI（`.github/workflows/ci.yml`）会跑 `cargo fmt --check`、`cargo clippy -D warnings`、
全部 Rust 测试，以及前端的 lint / typecheck / test / build。

### 安全边界

服务默认只监听 `127.0.0.1`，并且对所有请求做来源校验（见
[`crates/makuhime-server/src/origin.rs`](crates/makuhime-server/src/origin.rs)）：

- Host 必须是回环地址，挡住 DNS rebinding；
- 带 `Origin` 的请求必须是本机页面（`http://127.0.0.1:*` / `http://localhost:*`）、
  Tauri 壳（`tauri://localhost`），或与 Host 同源（局域网部署时）；
- WebSocket 同样会校验——浏览器不会为 ws 做同源限制，不校验的话任意网页都能读走你的弹幕；
- 所有 `/api` 接口另有访问令牌校验（见
  [`crates/makuhime-server/src/auth.rs`](crates/makuhime-server/src/auth.rs)），
  同源的恶意页面即使绕过来源校验也读不到数据、改不了配置；
- 页面下发的 CSP 与 Tauri 的 `security.csp` 都禁止内联脚本以外的外部脚本与 `object` /
  `iframe` 嵌入；弹幕墙里的头像 / 表情走 `safeImageUrl`，只允许 `http(s)` 与常见图片
  `data:`；
- 封面抓取只接受公网 `https` 地址，会先解析 DNS 并检查每一跳都不是内网 / 回环地址
  （防 SSRF），登录 Cookie 只发给 `*.bilibili.com` 等 B 站域名，不会被封面链接带走；
- 导出的 CSV 会给 `= + - @` 开头的单元格加前导单引号，避免被人用昵称做公式注入。

登录凭据只保存在 `~/.config/maku-hime/credentials.json`（0600，写入时先落临时文件再 rename）。

## 已知限制

- Wayland 上的点击穿透取决于合成器：原生 Wayland 窗口走 GTK3，输入区域不一定被合成器采纳，
  这时悬浮窗会退化成普通置顶窗。用 `GDK_BACKEND=x11` 启动（走 XWayland）通常能拿到可用的点击穿透：
  `GDK_BACKEND=x11 cargo run -p maku-hime -- --overlay`；
- 锁定依赖 Wayland 输入区域：窗口走 `set_input_region` 只留顶栏可点，
  合成器不认这个能力时退回整窗穿透，此时解锁要靠主窗口顶部或托盘菜单；
- 开播需要主播账号登录，首次可能还要在 B 站完成实名 / 人脸认证；推流本身仍由 OBS 等软件负责；
- 开播接口只认直播姬客户端，本项目按它的 `appkey` / 签名 / UA 发请求；若个别账号或分区
  仍被 `60034` 拒绝，只能改用直播姬开播再把推流地址填进 OBS；
- 小米 MiMo 语音需要能访问 `api.xiaomimimo.com`，并且本机装有 mpv / ffplay / paplay 之一；
- 扫码登录需要能访问 passport.bilibili.com（项目默认直连、不吃系统代理，
  需要走代理请设 `MAKUHIME_SYSTEM_PROXY=1`，见「网络与代理」）；
- getDanmuInfo 有风控（-352），失败时会退回默认弹幕服务器，可能收不到部分私密消息；
- OBS 页面是透明底，用浏览器直接打开看不清楚，加 `&bg=dark` 预览；
- 把 `server.host` 改成 `0.0.0.0` 会把面板暴露到局域网，此时只允许「与 Host 同源」的页面访问，
  且仍然需要访问令牌；请自行确认网络环境可信（局域网内其他人拿到令牌就等于拿到面板）。
- 小米 MiMo 的 `base_url` 只接受公网 `https` 地址：非 https、带账号密码、指向
  本机 / 内网 / `.local` 的地址都会被丢弃并回落到默认值——否则「改一下地址再点试听」
  就能把 API Key 送进内网或明文链路。改配置本身也需要令牌。

## 参考与致谢

- [arcana-world](https://github.com/TarocchiLive/arcana-world)：功能取舍与「弹幕姬」产品形态的参考，
  开播接口按直播姬客户端签名（appkey / md5 sign / LiveHime UA）的做法也对照了它的实现；
- [UniBarrage](https://github.com/BarryWangQwQ/UniBarrage) 与 [blivedm-go](https://github.com/xifan2333/blivedm-go)：B 站协议实现与字段映射的对照来源；
- [dmnotifier](https://github.com/xifan2333/dmnotifier)：插件化消息管线的思路；
- [bili-dm](https://github.com/Lumorian/bili-dm)：官方聊天面板 1:1 排版、在线人数接口与穿透悬浮窗的经验。

## License

MIT
