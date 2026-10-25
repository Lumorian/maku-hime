# B 站官方弹幕面板 1:1 排版规格

> 适用目标:用 Web(CSS)复刻 B 站直播间 `chat-history-panel`(官方弹幕姬面板)的一行排版、
> 配色、勋章胶囊与醒目留言卡片。
> 数据来源:bili-dm/overlay 源码与设计文档(只读)。**每个数值都带出处**;凡是源码里没有、
> 由常量推导或行业惯例补的,标注「推导」/「推测」。
> 基准:**12px 字号 + 20px 行框**,所有 px 均为 CSS 像素;字号变化时按 `scale = font-size / 12`
> 等比缩放(仅限本文标注「随字号缩放」的项)。

## 0. 阅读约定

### 0.1 引用缩写 → 完整路径

| 缩写 | 文件 |
|------|------|
| `wall.py` | `/home/lumorian/Projects/bili-dm/overlay/src/bilidm_overlay/danmaku/wall.py` |
| `theme.py` | `…/danmaku/theme.py` |
| `style.py` | `…/danmaku/style.py` |
| `medal.py` | `…/danmaku/medal.py` |
| `layout.py` | `…/danmaku/layout.py` |
| `lifecycle.py` | `…/danmaku/lifecycle.py` |
| `composer.py` | `…/danmaku/composer.py` |
| `model.py` | `…/danmaku/model.py` |
| `avatars.py` | `…/danmaku/avatars.py` |
| `window.py` | `/home/lumorian/Projects/bili-dm/overlay/src/bilidm_overlay/window.py` |
| `config.py` | `…/bilidm_overlay/config.py` |
| `roomstats.py` | `…/bilidm_overlay/roomstats.py` |
| `parity.md` | `docs/superpowers/specs/2026-10-21-chat-panel-parity-design.md` |
| `scroll.md` | `docs/superpowers/specs/2026-10-23-scroll-history-design.md` |
| `online.md` | `docs/superpowers/specs/2026-10-24-online-users-design.md` |
| `send.md` | `docs/superpowers/specs/2026-10-22-danmaku-send-design.md` |
| `README.md` | `/home/lumorian/Projects/bili-dm/overlay/README.md` |

引用写作 `文件:行号`;文档引用写作 `文档.md 小节`。

### 0.2 证据等级

| 标记 | 含义 |
|------|------|
| 【实测】 | 源码常量,或官方 CSS 经 headless Chrome 量出的盒子(parity.md §2) |
| 【推导】 | 由源码常量算出(会写算式) |
| 【推测】 | 源码与文档都没有,按惯例补的,Web 实现请以实际线上页面为准 |

### 0.3 基准与两条容易踩空的结论

- 官方列表基准字号 **12px**(parity.md §2;`wall.py:112`),本项目配置默认同为 12(`config.py:33`),设置面板范围 9–40px(`settings.py:505`)。
- **`transform: scale(.8333)` 不参与布局**:勋章名排版仍占原宽(两个汉字 ≈24px),只有绘制缩到 83.33%(parity.md §2-1;`medal.py:17-19`)。CSS 里就是普通 `transform`,天然满足。
- **航海锚图标绝对定位、溢出胶囊**:左缘在胶囊左缘外 4px,**不计入胶囊宽度**(parity.md §2-2;`medal.py:39-41`)。

---

## 1. 行度量

### 1.1 盒子层级与数值

| 层 | 属性 | 值 | 出处 |
|----|------|----|------|
| 窗口 | 卡片外留白 | **4px**(四边) | 【实测】`window.py:190` |
| 卡片 | 圆角 | **12px** | 【实测】`wall.py:55`、`window.py:562`、README「圆角卡片」 |
| 卡片 | 底色 | `rgba(主题bg, opacity)`,默认 **0.85**(范围 10–100%) | 【实测】`window.py:551-563`、`config.py:34` |
| 列表 | padding | **5px 10px** | 【实测】`.chat-history-list{padding:5px 10px}` parity.md §2;`wall.py:58-59` |
| 列表 | font-size | **12px** | 【实测】`wall.py:112`、`config.py:33` |
| 行 | padding | **5px**(四边) | 【实测】`.chat-item{padding:5px}` parity.md §2;`wall.py:60` |
| 行 | 行框 line-height | **20px** | 【实测】`.user-name{line-height:20px}` parity.md §2;`wall.py:62` `LINE_HEIGHT_RATIO = 20/12` |
| 行 | 单行总高 | **30px** = 5 + 20 + 5 | 【实测】parity.md §2 `.chat-item.danmaku-item h=30` |
| 行 | 行间距 | **0**;相邻行的 5px 下/上内边距直接拼成 10px 色带 | 【推导】`wall.py:176-179` `height + 2*ROW_PAD` |
| 行 | 正文列起点 | **39px** = 10(列表) + 24(头像) + 5(间距) | 【推导】`wall.py:66` `TEXT_X` |
| 行 | 可用正文宽 | **面板宽 − 49px**(=39 + 10 右侧 padding) | 【推导】`wall.py:349-350` |

多行行块高 = `20 × 行数 + 10`(`layout.py:151-155` 行高累加;`wall.py:179` 再加两侧 padding)。
行高换算(四舍五入到整像素,`wall.py:234-235`):

| font-size | 9 | 10 | 11 | 12 | 13 | 14 | 16 | 18 | 24 |
|-----------|----|----|----|----|----|----|----|----|----|
| line-height | 15 | 17 | 18 | **20** | 22 | 23 | 27 | 30 | 40 |

> **注意**:列表/行 padding、头像边长、正文列 39px **都不随字号缩放**(纯常量,`wall.py:58-66`);
> 只有勋章几何(`wall.py:230-232` `_scale`)与礼物行的 5px/4px 行内 margin(`wall.py:304-312`)
> 乘 `scale`。CSS 移植时同理:字号放大只推高行框,不放大留白。

### 1.2 可直接落地的 CSS 骨架

```css
/* 变量见 §8 */
.chat-history-panel {
  border-radius: 12px;
  background: rgba(var(--bg2-rgb), .85);   /* 深色 16,16,17 */
  overflow: hidden;
  position: relative;
}
.chat-history-list {
  padding: 5px 10px;
  font: 12px/1.6667 var(--font-cjk);        /* 1.6667 × 12 = 20px */
  overflow: hidden;
}
.chat-item {
  position: relative;
  padding: 5px 5px 5px 29px;                /* 左 29 = 头像 24 + 间距 5 */
  line-height: 20px;
}
.chat-item > .avatar {
  position: absolute;
  left: -29px;                              /* 落在列表 10px padding 内 → 面板 x=10 */
  top: calc(var(--row-pad) + var(--line-height) / 2 - 12px);  /* 20px 行框时 = 3px */
  width: 24px; height: 24px; border-radius: 50%;
  object-fit: cover; overflow: hidden;
}
.danmaku-item-left, .danmaku-item-right { display: inline; }
```

行内统一用 `inline` 流(官方即如此,parity.md §1「关键 DOM」);折行交给浏览器,
规则见 §1.4。

### 1.3 像素级对齐细节

- **文字基线**:行框内垂直居中,公式 `baseline = 行顶 + (20 − fontHeight)/2 + ascent`(`wall.py:637`)。CSS 用 `line-height: 20px` 即自动等价。
- **勋章垂直**:胶囊顶 = 行框顶 + `(20 − 15)/2 = 2.5px`(`wall.py:649`),即 `vertical-align: middle` 落在行框中线(parity.md §1 的 `.fans-medal-item-ctnr{vertical-align:middle}`)。
- **头像垂直**:中线对齐该行**正文首行**中线,不是整块居中(`wall.py:615-620`;`tests/test_wall_offscreen.py:506-528` 断言误差 ≤1.5px)。多行行里头像贴第一行。
- **内容裁剪**:绘制裁进 `(10, 5, W−20, H−10)`(`wall.py:418-424`),滚动中的半行被切在留白之外,文字不渗进 5px 内边距。

### 1.4 分隔符与空格

- 昵称与正文之间是 **`" : "`**:一个半角空格 + 半角冒号 + 一个半角空格(`style.py:40`,对齐官方 `username + (6!==t.dmType ? " : " : " ")`,见 `style.py:13` 的引用)。
- 注意 `dmType === 6` 时官方只留一个空格 —— 6 是哪种消息未在源码中定义,**【推测】**为礼物类;本项目**所有** chat/SC 行都用 `" : "`(`wall.py:338, 343`)。
- 空格是独立 token(`layout.py:52-54, 57-71`),所以 `" : "` 可能被折到下一行行首;CJK 昵称本身也可逐字断开。
- 礼物 / 上舰行**不用冒号**,用 margin 顶开:昵称后 5px、动作词后 4px(`wall.py:64-65, 304-312`)。

### 1.5 超长弹幕:换行(**不省略**)

| 规则 | 行为 | 出处 |
|------|------|------|
| 总策略 | 折成多行,行块变高,无 `…` 省略 | `layout.py:74-130`(全文无 ellipsis) |
| ASCII 词 | 整词不断(不在词内换行) | `layout.py:52-54` |
| CJK / 标点 / 空格 | 逐字可断 | 同上 |
| 原子盒子(勋章) | 不可拆,放不下整体换行 | `layout.py:98-107` |
| 单词比整行还宽 | 退化为逐字硬断(对应 `word-break:break-word`) | `layout.py:113-122` |
| 行尾 margin | 丢弃(浏览器同行为) | `layout.py:126-128, 143-148` |
| 空内容 | 仍占一行高 | `layout.py:151-155` |

CSS 对应:`overflow-wrap: anywhere; word-break: break-word;`(`layout.py:3-4` 明确对齐这两条官方声明)。
行数无限(受 `max_rows` 淘汰影响,见 §6.1)。

### 1.6 与官方的已知差异(移植时二选一)

| 项 | 官方 | 本源码 | 建议 |
|----|------|--------|------|
| 行首头像 | 无 | 24px 圆形(`wall.py:760-793`) | 本项目唯一有意偏离(parity.md §3 末段);要 1:1 官方就删掉并让正文列回到 15px(10+5) |
| 行右内边距 | 5px(可用宽 = W−15) | 横向未生效(可用宽 = W−10) | 【实测】`wall.py:349-350`;复刻官方应用 `padding-right: 5px` |
| 醒目留言卡最小高 | `min-height: 38px`(顶栏 34px、头像 30px) | 单行卡 30px | parity.md §4;复刻官方请用 38px |
| 用户等级徽章 `user-level-icon` | 有 | 解析了 `user_level` 但**未绘制**(`model.py:9`,`wall.py` 无对应分支) | 若需要,按官方组件另补 |

> **文档小坑**:`wall.py:7` 与 `wall.py:382-384` 的注释写「line-height 18px = 28px 行」,
> 但常量 `LINE_HEIGHT_RATIO = 20/12`(`wall.py:62`)与 parity.md §2 的实测 **30px 行高**
> 一致。以 **20px 行框 / 30px 行高** 为准,注释是旧的。

---

## 2. 颜色令牌

### 2.1 主题令牌(bili-theme v11.0.1,`theme.py:34-55`)

| CSS 变量 | 官方令牌 | light | dark | 用途 |
|----------|----------|-------|------|------|
| `--bg2` | `--Ga0` | `#F6F7F8` | `#101011` | 面板底色 |
| `--text2` | `--Ga7` | `#61666D` | `#A2A7AE` | 弹幕正文 |
| `--text3` | `--Ga5` | `#9499A0` | `#757A81` | 昵称 |
| `--pay_yellow` | `--Ye5` | `#FFB027` | `#DB8700` | 礼物名/数量/金额/舰长 |
| `--brand_pink` | `--Pi5` | `#FF6699` | `#D44E7D` | 房管/主播标签、直播圆点、发送按钮 |
| `--card-text` | — | `#333333` | `#333333` | SC 卡片正文(亮黄底,永远深色) |
| `--line_regular` | `--Ga2` | `#E3E5E7` | `#2F3134` | 分隔线/卡片描边 |

出处:`theme.py:34-43`(LIGHT)、`theme.py:46-55`(DARK);`theme.py:7-12` 的映射表;parity.md §5 同表。
跨房标签 `uni-live-mirror-tag`:**`#7DABF4`(深浅同色)**(`theme.py:12`)。

### 2.2 语义色(非主题令牌)

| 名称 | 值 | 用途 | 出处 |
|------|----|------|------|
| 系统行描述灰 | `#999999` | 进入直播间/点赞了/直播已结束 | `wall.py:70` |
| 舰队昵称青 | `#00D1F1` | 舰长/提督/总督昵称兜底色 | `wall.py:68` |
| SC 卡起止(默认) | `#F2C55C` → `#E9A100` | 醒目留言渐变的兜底 | `wall.py:83-84` |
| 勋章默认色 | `#9C9C9C`;文字 `#FFFFFF` | 协议缺色时 | `medal.py:66-69` |
| 头像占位圆 | `rgba(72,72,80,.78)` | 头像未就绪 | `wall.py:785` |
| 头像占位字 | `rgb(220,220,225)` | 名字首字 | `wall.py:792` |
| 控制/状态文字 | `#9AA0A6`(dark)/ `#61666D`(light) | 在线徽标、状态条 | `window.py:56, 290` |
| 离线点灰 | `#757A81` | 徽标 `○`/`●` | `window.py:302, 306` |
| 滚动条 | 白 `alpha 70/255≈27%`,悬停 `150/255≈59%`(dark 用白、light 用黑) | 右侧细条 | `wall.py:72-76, 473` |
| 「回到最新」药丸 | `rgba(255,255,255,.16)`,悬停 `.27` | 右下角 | `wall.py:498` |
| 控制按钮玻璃态 | `rgba(255,255,255,.11)`/`.24`(hover)/`.35`(pressed) | 锁/设置 | `window.py:59-66` |
| 输入框底 | `rgba(255,255,255,.10)`(dark)/`.06`(light) | 发送条 | `composer.py:103, 107` |
| 平台标签色 | bilibili `#FB7299`、抖音 `#FE2C55`、快手 `#FF6A00`、斗鱼 `#FF6E00`、虎牙 `#F8B300`、小红书 `#FF2442`、其它 `#8A8B93` | merged 多房间前缀 | `style.py:22-30` |

### 2.3 令牌块

```css
:root[data-theme="dark"] {
  --bg2:#101011; --text2:#A2A7AE; --text3:#757A81;
  --pay_yellow:#DB8700; --brand_pink:#D44E7D;
  --card-text:#333333; --line_regular:#2F3134;
  --system-text:#999999; --guard-name:#00D1F1;
  --dot-offline:#757A81; --muted:#9AA0A6;
}
:root[data-theme="light"] {
  --bg2:#F6F7F8; --text2:#61666D; --text3:#9499A0;
  --pay_yellow:#FFB027; --brand_pink:#FF6699;
  --card-text:#333333; --line_regular:#E3E5E7;
  --system-text:#999999; --guard-name:#00D1F1;
  --dot-offline:#757A81; --muted:#61666D;
}
```

文字色分配(逐行类型见 §5):正文 `--text2`、昵称 `--text3`、付费元素 `--pay_yellow`,
系统描述 `--system-text`,SC 卡片文字 `--card-text`(底色偏暗时翻白,§5.3)。

---

## 3. 粉丝勋章胶囊

### 3.1 几何(12px 基准)

| 部件 | 值 | 出处 |
|------|----|------|
| 容器 `.fans-medal-item-ctnr` | 高 **16px**;margin-left **4px**、margin-right **3px**;`inline-block` + `vertical-align:middle` | 【实测】`medal.py:42-44`;parity.md §2;`chat-panel.html:28` |
| 胶囊 `.fans-medal-item` | 高 **15px**;圆角 **8px**;padding **2px 6px 2px 2px** | 【实测】`medal.py:31-34`;parity.md §2 |
| 名字 `.fans-medal-content` | 缩放到 **83.33%**(仅视觉);右侧留 **2px**;字号同正文 12px | 【实测】`medal.py:35-36`;parity.md §2 |
| 等级槽 `.fans-medal-level` | 高 **12px**;宽 **4px(1 位)/ 8px(2 位)/ 15px(3 位)** | 【实测】`medal.py:47-53`;parity.md §2 |
| 等级字 | 加粗,视觉缩放 **≈0.8406**(=0.8333 × 1.1 × 0.917) | 【推导】`wall.py:688` |
| 舰长款 label | `padding-left: 12px`(给 20px 锚图标让位) | 【实测】`medal.py:38` |
| 锚图标 `.medal-deco.medal-guard` | **20 × 20**,absolute,`left:6px; margin-left:-12px` → 左缘溢出胶囊 **4px**,不计宽 | 【实测】`medal.py:37-41` |
| 胶囊宽度公式 | `pad_left + 名字宽(不缩放) + 2 + 等级槽 + 6`;舰队款 pad_left=14,普通款=2 | 【推导】`medal.py:78-97` |
| 实测量值 | 舰队款 **54.02px**(2+12+24.02+2+8+6);普通款 **42.02px** | 【实测】parity.md §2;`medal.py:81-82` |

垂直落位:胶囊顶 = 行框顶 + **2.5px**(`wall.py:649`);圆角绘制用 `pill_h/2 = 7.5`
(`wall.py:657`),官方 CSS 是 `border-radius: 8px` —— 视觉差 0.5px,以 CSS 8px 为准。

### 3.2 CSS

```css
.fans-medal-item-ctnr {
  display: inline-block; vertical-align: middle;
  height: 16px; margin: 0 3px 0 4px; position: relative;
}
.fans-medal-item {
  display: flex; align-items: center;
  height: 15px; border-radius: 8px; padding: 2px 6px 2px 2px;
  background-image: linear-gradient(45deg, var(--m-start), var(--m-end));
  box-shadow: inset 0 0 0 1px var(--m-border);   /* 官方 1px 描边 */
  color: var(--m-text);                          /* 默认 #FFFFFF */
}
.fans-medal-label { display: flex; align-items: center; }
.fans-medal-label.guard { padding-left: 12px; position: relative; }
.fans-medal-content {
  font-size: 12px; margin-right: 2px;
  transform: scale(.8333); transform-origin: center;
}
.fans-medal-level {
  width: 8px; height: 12px; display: flex; align-items: center; justify-content: center;
  font-size: 12px; font-weight: 700; transform: scale(.8406);
}
.fans-medal-level.one-digit    { width: 4px; }
.fans-medal-level.three-digits { width: 15px; }
.medal-deco.medal-guard {
  position: absolute; left: 6px; margin-left: -12px; top: 50%;
  width: 20px; height: 20px; transform: translateY(-50%);
  background: url(anchor-white.svg) center/contain no-repeat;  /* 官方是 CDN 图 */
}
```

### 3.3 颜色随等级变化的规则

**源码里没有「等级 → 色值」表**。颜色完全由协议里的 `fansMedal` 字段驱动
(`model.py:5-8, 23-32`;`medal.py:100-113`):

| 字段 | 作用 | 缺省 |
|------|------|------|
| `color_start` / `color_end` | 胶囊 45° 渐变(左上→右下)两端 | 三者任一取到即用,`medal.py:104-110` |
| `color_border` | 1px 描边色 | 同上 |
| `color_text` | 名字与等级的字色 | `#FFFFFF`(`medal.py:111`) |
| `guard_level` | 1 总督 / 2 提督 / 3 舰长(`medal.py:27`);>0 才加 12px 图标位 | 0 |
| `level` | **只影响等级槽宽**(4/8/15)与数字内容,不影响颜色 | — |
| `is_light` | **解析但未使用**(`model.py:32, 74-76`;绘制端无引用) | true |

也就是说:**「勋章颜色随等级变化」在实现上是数据驱动的**,B 站服务端按勋章所属粉丝牌给色,
客户端不按数字映射(这解释了为什么同一等级可能有两种色)。

- 【推测】官方粉丝牌按等级档位换色的那套调色板(灰→蓝→紫→金)在本仓库**没有**任何常量,
  不要凭空实现;若线上需要,请从 `DANMU_MSG.info.3` 的 `v2` 字段取真实渐变。

### 3.4 舰长 / 提督 / 总督 配色

| 档位 | `guard_level` | 名称 | 昵称色 | 勋章特征 |
|------|---------------|------|--------|----------|
| 总督 | 1 | 总督 | `#00D1F1` | 12px 图标位 + 20px 锚 |
| 提督 | 2 | 提督 | `#00D1F1` | 同上 |
| 舰长 | 3 | 舰长 | `#00D1F1` | 同上 |

【实测】`medal.py:27`、`wall.py:68, 300-301`、`tests/test_wall_offscreen.py:201`。
**三档昵称色在源码里是同一个值 `#00D1F1`**(仅当协议没给 `name_color` 时兜底);
【推测】线上三档实际色不同(官方按 guard_level 发 `nameColor`),Web 实现请优先用协议 `name_color`。

---

## 4. 头像

| 项 | 值 | 出处 |
|----|----|------|
| 尺寸 | **24 × 24px**(常数,不随字号缩放) | `avatars.py:12` |
| 形状 | **正圆**(椭圆裁剪,非圆角矩形),半径 12px | `wall.py:776-781` |
| 横向 | 固定左列,left = **10px**(列表 padding 内沿) | `wall.py:767` |
| 纵向 | 中线 = 行块顶 + **5 + 20/2 = 15px**;即单行行内 `top: 3px` | `wall.py:615-620` |
| 与正文间距 | **5px**(正文列 39px) | `wall.py:63, 66` |
| 缩放方式 | `KeepAspectRatioByExpanding` + 平滑缩放(等同 `object-fit: cover`) | `wall.py:770-775` |
| 缓存 | 磁盘 sha1(url).png + 内存;未命中返回空并触发异步下载 | `avatars.py:15-23, 39-54` |
| 占位 | 灰圆 `rgba(72,72,80,.78)` + 名字首字 `rgb(220,220,225)`,字号 `int(12×0.65)=7px` | `wall.py:784-793` |
| 占位触发 | 无 url / 未下载完 / 解码失败 | `avatars.py:41-44` |

占位符规则细节:取 `name.strip()[:1]`(可能为 CJK 或 emoji),居中;`name` 为空则只画灰圆
(`wall.py:787-793`)。醒目留言卡片**共用同一列、同一垂直基准**(`wall.py:740-744`),
不要给卡片再加内缩(那是历史 bug,`tests/test_wall_offscreen.py:486-503` 有回归)。

---

## 5. 各消息类型的行内排版

### 5.1 总表

| 协议 `type` | 行内结构 | 颜色分配 | 行内间距 | 出处 |
|--------------|----------|----------|----------|------|
| `chat` | 勋章? · 昵称 · `" : "` · 正文 | 昵称 `name_color` → 舰队 `#00D1F1` → `--text3`;分隔符与正文 `--text2` | 勋章 4/3 | `wall.py:332-346`;`style.py:67-68` |
| `gift` | 昵称 · **赠送** · 礼物名 · **xN** | 昵称同上;**赠送** `--text2`;礼物名与 xN `--pay_yellow` | 昵称后 5、动作后 4、礼物名后 5 | `wall.py:303-313`;`style.py:78-84` |
| `subscribe`(上舰/GuardBuy) | 昵称 · **开通了** · 舰长 | 舰长 `--pay_yellow` | 昵称后 5、动作后 4 | `wall.py:315-321`;`style.py:87-88` |
| `superchat` | 整块卡片:勋章? · 昵称 · `" : "` · 正文;金额右上 | 卡片对比色(§5.3) | 同 chat | `wall.py:333-341, 724-758` |
| `enterroom` | 昵称 · **进入直播间** | 昵称 `--text3`;描述 `#999999` | 4px | `wall.py:323-330`;`style.py:95-96` |
| `like` | 昵称 · **点赞了** | 同上 | 4px | `style.py:93-94` |
| `endlive` | **直播已结束** | `#999999` | — | `style.py:97-98` |
| `superchat_delete` | **醒目留言已删除** | `#999999` | — | `style.py:99-100` |
| 未知类型 | `content` 原样 | `#999999` | — | `style.py:101` |
| merged 前缀 | `platform/rid` | `#999999` | 5px | `wall.py:293-297` |

文案模板(可直接抄):

```
chat        {勋章}{昵称 : }{正文}
gift        {昵称}{赠送}{礼物名}{x20}          # xN 仅 num>1 时出现
subscribe   {昵称}{开通了}{舰长}
enterroom   {昵称}{进入直播间}
like        {昵称}{点赞了}
endlive     直播已结束
```

- `xN` 规则:`num > 1` 才输出,格式小写 `x` + 数字(`style.py:78-80`)。
- 礼物名/数量**同色同字号**,不加粗(`wall.py:308-312`)。
- 系统行**没有** `" : "`,只有 4px 间距(`wall.py:326`);`name` 为空时只画描述。
- 所有非 SC 行的昵称规则:协议 `name_color` 优先 → 舰队 `#00D1F1` → `--text3`(`wall.py:299-301`)。

### 5.2 行内 CSS

```css
.user-name { color: var(--text3); }
.user-name.guard { color: var(--guard-name); }
.user-name.custom { color: var(--name-color); }        /* 协议 name_color */
.danmaku-item-right { color: var(--text2); }

.gift-item .username   { margin-right: 5px; }
.gift-item .action     { color: var(--text2); margin-right: 4px; }
.gift-item .gift-name  { color: var(--pay_yellow); margin-right: 5px; }
.gift-item .gift-num   { color: var(--pay_yellow); }

.interact-row .interact-name { color: var(--text3); margin-right: 4px; }
.interact-row .interact-desc { color: var(--system-text); }
```

> 5px / 4px 两个 margin 会随字号缩放(`wall.py:304-312` 乘 `scale`),而行 padding 不缩放 ——
> 移植时保持这个不对称,否则大字号下右侧会挤。

### 5.3 醒目留言(SuperChat)卡片

| 项 | 值 | 出处 |
|----|----|------|
| 卡片矩形 | x = **10**、宽 = **面板宽 − 20**、高 = **内容块高 + 10** | `wall.py:730-732` |
| 圆角 | **6px** | `wall.py:85, 738` |
| 底色 | 45° 渐变:协议 `bg_color_start/end`,缺省 `#F2C55C → #E9A100` | `wall.py:733-738` |
| 内边距 | 内容区顶 = 卡片顶 + **5px**;左右不再内缩(与普通行共用正文列 39px) | `wall.py:743-747` |
| 头像 | 同普通行:24px、同一列、卡片顶 + 5 + 10 为中线的首行 | `wall.py:744` |
| 金额 | `¥` + 数字(`%g` 去尾零);**粗体**、字号 = 基准 12px;右对齐;盒宽 **80px**;**距卡右 8px、距卡顶 4px** | `wall.py:748-758` |
| 金额色 | 协议 `price_color`,否则 `--card-text #333` | `wall.py:753` |
| 文字色 | 协议 `price_color` → 亮度 > 0.55 用 `#333`、否则 `#FFFFFF` | `wall.py:100-105` |
| 文字亮度公式 | `(0.299R + 0.587G + 0.114B) / 255` | `wall.py:88-97` |
| 昵称/正文 | **同色**(卡片对比色),不含主题 `--text2` | `wall.py:337-340` |

官方对照(复刻时可切换):`superChat-card-detail{margin:0 -5px; min-height:38px}`、
`.card-item-middle-top{height:34px;padding:0 5px}`、头像 30px、`.card-item-middle-top-right{color:#333}`
(parity.md §4)。

```css
.sc-card {
  margin: 0;                       /* 官方 -5px 会吃掉行 padding */
  padding: 5px 5px 5px 29px;
  min-height: 38px;                /* 官方值;本源码单行 30px */
  border-radius: 6px;
  background: linear-gradient(45deg, var(--sc-start, #F2C55C), var(--sc-end, #E9A100));
  color: #333;                     /* 暗色卡切 #FFF,见亮度规则 */
  position: relative;
}
.sc-card .price {
  position: absolute; top: 4px; right: 8px; width: 80px;
  text-align: right; font-weight: 700; font-size: 12px;
}
```

> 注意:金额的 80px 盒**不做避让**,超长正文首行会与金额重叠(`wall.py:754-758` 无占位逻辑)。
> Web 版可给首行加 `padding-right: 88px` 修掉,但这会偏离源码。

---

## 6. 交互

### 6.1 滚动模型(像素级)

- `offset` = **距底部往上翻了多少像素**;0 = 贴底跟随最新;上限 = `内容总高 − 可用高`(`scroll.md` §3;`lifecycle.py:99-114`)。
- 内容不满一屏:`max_offset = 0`,顶对齐,无滚动条(`lifecycle.py:99, 102`)。
- 像素粒度:视口返回 `(row, y)`,y 可为负,行被裁一半 —— 不整行跳变(`scroll.md` §2;`lifecycle.py:103-114`)。
- 往上翻时来新弹幕:`offset += 新增行高`,画面**不漂**(`wall.py:168-170`;`lifecycle.py:127-136`)。
- 滚到底自动恢复跟随;到顶后 clamp 写回内部状态(`wall.py:412-414`;`lifecycle.py:73-77`)。
- 行数上限 `max_rows`:默认 **80**(配置)/ 组件默认 30,设置范围 **5–200**(`config.py:31`;`lifecycle.py:25`;`settings.py:523`)。

### 6.2 滚轮与键盘

| 输入 | 位移 | 出处 |
|------|------|------|
| 滚轮一格(120 单位) | **3 行 = 3 × 行高**;12px 字号时 = **60px** | `wall.py:507-517` |
| PageUp / PageDown | 一屏 = `max(40, 列表高 − 40)` | `window.py:469-473` |
| Home / End | 最旧 / 回最新 | `window.py:474-477` |
| 托盘「回到最新弹幕」 | 同 End | `window.py:690-691` |

### 6.3 右侧细滚动条

| 项 | 值 | 出处 |
|----|----|------|
| 宽 | **4px**,圆头(半径 2px) | `wall.py:72, 477-478` |
| 横向位置 | `x = 面板宽 − 10 + 3`,并 clamp 到 `面板宽 − 5`(不压正文) | `wall.py:443-444` |
| 纵向 | 轨道 = `5px … 高−5px` | `wall.py:441-445` |
| 滑块高 | `max(20px, 轨道高 × 可用高/内容高)` | `wall.py:457` |
| 最小滑块 | **20px** | `wall.py:74` |
| 位置映射 | `pos = 1 − offset/max_offset`(**底=0 时滑块在最下**) | `wall.py:460` |
| 命中区 | 视觉 4px,**命中放宽到 12px**(左右各 +4) | `wall.py:447-449` |
| 颜色 | dark 白 / light 黑;常态 alpha **70/255 ≈ 27%**,悬停或拖动 **150/255 ≈ 59%** | `wall.py:75-76, 467-474` |
| 出现条件 | 仅 `max_offset > 0` | `wall.py:454-455` |
| 交互 | 点轨道任意处先跳过去再进入拖动 | `wall.py:534-540, 584-600` |

### 6.4 「↓ N 条新弹幕」药丸

| 项 | 值 | 出处 |
|----|----|------|
| 尺寸 | **84 × 20px**,圆角 = 高/2 = **10px** | `wall.py:78-79, 499` |
| 位置 | 右下:`right = 宽 − 84 − 12`,`bottom = 高 − 20 − 8` | `wall.py:480-489` |
| 底色 | `rgba(255,255,255,.16)`,悬停 `.27` | `wall.py:498` |
| 文字 | 主题 `--text2`,字号 **0.85 × 基准**(12px → **10px**),居中 | `wall.py:500-504` |
| 文案 | 有隐藏行:`↓ {N} 条新弹幕`;否则 `↓ 回到最新` | `wall.py:503` |
| N 的统计 | 视口**下方**(更新一侧)连续未显示的行数 | `wall.py:602-611` |
| 出现条件 | 非跟随态 且 可滚 | `wall.py:493` |
| 点击 | 回到底部 | `wall.py:530-533` |

> `JUMP_HINT_ROWS = 4`(`wall.py:80`)定义后**从未使用**,别照抄成"显示 4 行"。

### 6.5 点击穿透锁定态

- 默认**锁定**(穿透):`click_through = True`(`config.py:36`)。
- 锁定态输入区域 = **仅按钮带矩形**(右上一行),其余整块放行鼠标(`window.py:367-378`)。
- 锁定态关闭键盘交互(不抢焦点),解锁时打开(`window.py:359-361`;`send.md` §2.4)。
- 锁定态输入框只读,占位文案 `解锁后可发送弹幕`;解锁后 `发送弹幕…`(`composer.py:33-34, 138-144`)。
- 切换只改输入区域,不重建表面(`window.py:338-357` 注释解释了重建会丢 layer-shell 角色)。

### 6.6 拖拽 / 缩放

| 项 | 值 | 出处 |
|----|----|------|
| 拖动 | **解锁后窗口任意位置**按住左键拖(墙把非滚动条/非药丸的按下事件 `ignore()` 放行) | `window.py:392-413`;`wall.py:519-541` |
| 缩放 | **右下角 14 × 14px** 命中区,**无可见手柄** | `window.py:53, 501-505` |
| 最小尺寸 | **280 × 160** | `window.py:51-52` |
| 尺寸计算 | `max(MIN, 原尺寸 + 指针位移)` | `window.py:419-424` |
| 位置计算 | 按下时窗口位置 + **指针全局位移**(不用本地坐标累计) | `window.py:427-439` |
| 持久化 | 松手写 `position/width/height` 到 config | `window.py:507-510` |
| 初始位置 | 默认 `(24, 100)`;不在任何屏幕内时回退 `(24, 100)` | `config.py:28`;`window.py:483-499` |
| 重绘节拍 | **30ms**(≈33fps);无新消息不重绘 | `window.py:50`;`window.py:666-675` |
| 空闲隐藏 | 翻着历史时**不隐藏** | `window.py:666-675`;`scroll.md` §4 |

### 6.7 底部发送条(与面板同宽)

| 项 | 值 | 出处 |
|----|----|------|
| 条高 | 最小 **32px**;输入框/按钮高 **26px** | `composer.py:30-31` |
| 内边距 / 间距 | `10px 3px 8px 3px`,间距 **6px** | `composer.py:55-56` |
| 输入框 | 圆角 **12px**,padding `2px 8px`,底色 `rgba(255,255,255,.10)`(dark) | `composer.py:107-110` |
| 字数 | **20** 字上限,计数 `N/20` | `composer.py:29, 203` |
| 发送按钮 | 圆角 **11px**,padding `0 10px`,底 `--brand_pink`,白字 | `composer.py:125-131` |
| 回执 | 就地提示,**4000ms** 后让位给字数 | `composer.py:35, 177-185` |

---

## 7. 在线人数徽标

### 7.1 排版

| 项 | 值 | 出处 |
|----|----|------|
| 位置 | 控制按钮带**左侧**,与 🔒/⚙ 同一行 | `window.py:527-535` |
| 带宽/高 | 边距 `10px 6px 10px 0`,按钮 22px → 带高 **28px** | `window.py:524`;`online.md`「UI」 |
| 字号 | **11px**;颜色 dark `#9AA0A6` / light `#61666D` | `window.py:290-291` |
| 直播圆点 | `●` 用 **`--brand_pink`**;离线 `#757A81` | `window.py:302, 306, 310` |
| 控制按钮 | 22 × 22,圆角 11px,图标 15px,字号 13px | `window.py:56-67, 565-571` |
| 断线状态条 | `#9AA0A6` 11px,padding-left 6px,文案 `dmnotifier 离线,连接中…` | `window.py:161-163, 178` |

### 7.2 文案模板

| 场景 | 文案 | 出处 |
|------|------|------|
| 单房间直播中 | `● 直播中 · 在线 1.2万` | `window.py:300-304`;`online.md`「UI」 |
| 单房间未开播 | `● 未开播`(**灰点**,不是 ○) | `window.py:302-304` |
| 单房间轮播 | `● 轮播中` | `roomstats.py:75-76` |
| 在线数未知 | `● 直播中 · 在线数未知` | `roomstats.py:90` |
| 只有人气 | `● 直播中 · 人气 345.7万` | `roomstats.py:88-89` |
| 多房间(有直播) | `● 2 个直播间直播中 · 在线 3.4万` | `window.py:307-312` |
| 多房间(全离线) | `○ 2 个直播间均未开播` | `window.py:305-306` |

> ⚠️ **文档与实现不一致**:`online.md`「UI」写的是「未开播 ○ 未开播;轮播 ◐ 轮播中」,
> 但代码里单房间**永远用 `●`**(离线时改成灰 `#757A81`),`○` 只在「多房间全离线」时出现
> (`window.py:304 vs 306`)。做 1:1 复刻时按**官方画面**决定用哪个,别照抄文档。

结构(渲染后):`<span class="dot">●</span> 直播中<em> · 在线 </em><span>1.2万</span>`。

### 7.3 数字格式化规则

`format_count`(`roomstats.py:93-109`):

| 输入 n | 输出 | 规则 |
|--------|------|------|
| 0 / 负数 / 坏值 | `0` | `max(0, int(v or 0))` |
| 9999 | `9999` | `n < 10000` 原样,不加 `+` |
| 10000 | `1万` | `(n/1e4).toFixed(1)` 后去掉 `.0` |
| 12345 | `1.2万` | 同上 |
| 130000 | `13万` | 13.0 → 去 `.0` |
| 99999 | `10万` | 9.9999 → 10.0 → 去 `.0` |
| 100000000 | `1亿` | `n ≥ 1e8` 切亿,去 `.0` |
| 123456789 | `1.2亿` | 同上 |

关键:**`13万+` / `9999+` 这两种带加号的形式只来自接口的 `onlineNumText`**
(`roomstats.py:59, 84-87`;`online.md`「数据口径」),本项目的 `format_count` **永不产出 `+`**。
优先级:`online_text`(官方缩写)> `online`(`format_count`)> `popularity`(标「人气」)> 「在线数未知」。

```js
// 与 roomstats.py:93-109 等价(注意 Python f-string 用 banker's rounding)
function formatCount(v) {
  const n = Math.max(0, Math.trunc(Number(v) || 0));
  if (n < 10000) return String(n);
  if (n >= 1e8) return (n / 1e8).toFixed(1).replace(/\.0$/, '') + '亿';
  return (n / 1e4).toFixed(1).replace(/\.0$/, '') + '万';
}
```

> 【实测】舍入差异:`format(1.25, '.1f')` 在 Python 里是 `1.2`(round-half-even),
> 而 JS `(1.25).toFixed(1)` 是 `1.3`。要逐字复刻需自己写 round-half-even,
> 差一位数字的概率极低,通常可忽略。

### 7.4 刷新与容错

- 间隔默认 **30s**,最小 **5s**(`config.py:42`;`statspoller.py:62, 75`;`online.md`「刷新策略」)。
- 失败**保留上一轮快照**;在线接口挂了徽标退化为「人气 x」,整块不消失(`roomstats.py:236-242`)。
- tooltip:`主播名 · 状态` / 标题 / `分区:x` / `在线用户:x` / `人气:x` / `粉丝:x`,块间空行(`window.py:314-336`)。

---

## 8. 参数总表(可直接抄成 design token)

```css
:root {
  /* 结构(固定 px,不随字号缩放) */
  --panel-gap: 4px;            /* 窗口内边距 window.py:190 */
  --panel-radius: 12px;        /* wall.py:55 */
  --panel-pad-x: 10px;         /* wall.py:58 */
  --panel-pad-y: 5px;          /* wall.py:59 */
  --row-pad: 5px;              /* wall.py:60 */
  --avatar-size: 24px;         /* avatars.py:12 */
  --avatar-gap: 5px;           /* wall.py:63 */
  --text-col: 39px;            /* wall.py:66 口径见 §1.1 */

  /* 字体(随字号缩放) */
  --font-size: 12px;           /* config.py:33 */
  --line-height: 20px;         /* round(font-size*20/12) wall.py:62 */
  --name-margin: 5px;          /* wall.py:64 */
  --action-margin: 4px;        /* wall.py:65 */
  --interact-gap: 4px;         /* wall.py:326 */

  /* 勋章(随字号缩放) */
  --medal-ctnr-h: 16px;  --medal-gap-l: 4px;  --medal-gap-r: 3px;
  --medal-h: 15px;       --medal-radius: 8px;  --medal-pad: 2px 6px 2px 2px;
  --medal-guard-pad: 12px; --medal-anchor: 20px; --medal-anchor-left: 6px;
  --medal-anchor-pull: -12px;  --medal-text-scale: .8333;

  /* 醒目留言 */
  --sc-radius: 6px;  --sc-price-w: 80px;  --sc-price-inset: 8px 4px;  /* wall.py:85,755 */

  /* 滚动 */
  --scrollbar-w: 4px;      /* wall.py:72 */
  --scrollbar-gap: 3px;    /* wall.py:73 */
  --scrollbar-min: 20px;   /* wall.py:74 */
  --jump-pill-w: 84px;  --jump-pill-h: 20px;  /* wall.py:78-79 */

  /* 窗口 */
  --win-min-w: 280px; --win-min-h: 160px; --resize-grab: 14px;  /* window.py:51-53 */
  --panel-opacity: .85;                                        /* config.py:34 */
}
```

---

## 9. 不确定 / 推测清单

| # | 项 | 结论 | 依据 |
|---|----|------|------|
| 1 | 舰长/提督/总督三档独立昵称色 | 源码只有 `#00D1F1` 一个值,三档共用 | 【实测】`wall.py:68`;真正的分档色【推测】 |
| 2 | 勋章"随等级换色"的调色板 | 仓库无任何等级→色表,颜色来自协议字段 | 【实测】`medal.py:100-113`;官方调色板【推测】 |
| 3 | `dmType === 6` 只留一个空格 | 官方 minified JS 片段,6 的含义未知 | 【推测】`style.py:13` |
| 4 | 官方行右内边距 | 本项目横向未生效(可用宽比官方多 5px) | 【实测】`wall.py:349-350` |
| 5 | SC 卡最小高 | 官方 38px / 本项目单行 30px | 【实测】parity.md §4 vs `wall.py:731` |
| 6 | 未开播圆点符号 | 文档 `○`,实现 `●`(灰) | 【实测】`window.py:304` vs `online.md` |
| 7 | 轮播符号 `◐` | 文档有,代码无(只用文字「轮播中」) | 【实测】`roomstats.py:75-77` |
| 8 | 行高注释 18px | 与常量/实测矛盾,以 20px 为准 | 【实测】`wall.py:62` vs `wall.py:7, 382-384` |
| 9 | `JUMP_HINT_ROWS` | 定义未使用 | 【实测】`wall.py:80` |
| 10 | 用户等级徽章 | 解析但未渲染 | 【实测】`model.py:9` |
| 11 | 勋章圆角 | 绘制用 `高/2=7.5px`,CSS 为 `8px` | 【实测】`wall.py:657` vs `medal.py:32` |

---

## 10. 出处索引

| 主题 | 位置 |
|------|------|
| 官方盒子量测(30px 行 / 54.02px 胶囊 / 20px 行框) | `parity.md` §2 |
| 官方行内 DOM 与文案 | `parity.md` §1;`chat-panel.html` |
| 主题令牌 | `theme.py:34-55`;`parity.md` §5 |
| 勋章几何 | `medal.py:26-97`;`parity.md` §2 |
| 行内排版与折行 | `layout.py:44-155` |
| 行绘制 / 勋章 / SC / 头像 | `wall.py:614-793` |
| 滚动与视口 | `lifecycle.py:19-148`;`scroll.md` §2-§4;`wall.py:439-611` |
| 窗口交互(穿透/拖拽/缩放) | `window.py:338-510`;`README.md`「窗口交互」 |
| 在线徽标与数字格式 | `window.py:279-336`;`roomstats.py:72-109`;`online.md` |
| 发送条 | `composer.py:29-135`;`send.md` §1 |
| 默认配置(尺寸/字号/行数/透明度) | `config.py:28-47` |
