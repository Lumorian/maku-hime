# 幕姬 maku-hime · Logo 设计 Brief & 生图提示词

> 一句话设定：**幕** = 那块透明、可点击穿透的幕布；**姬** = 幕布后面替你念弹幕的公主。
> 标记必须在 32×32 的托盘尺寸下还能同时说出「幕布 / 公主 / 弹幕在流动」这三件事。

---

## 0. 项目上下文（校准用，不必粘给模型）

- 产品：轻量、可穿屏的 B 站弹幕姬。Rust 引擎 + Tauri 壳，一份引擎喂三种展示面（控制面板 / 悬浮窗 / OBS 浏览器源）。
- 性格：工具、克制、本地优先、零外部服务。不是玩具，也不是二次元周边。
- 视觉现状：深色 UI（`#0D0E10`）、12/16px 圆角、1.8px 圆头线性图标、粉→橙渐变按钮、`Inter` + 思源黑体、拉丁名用等宽字体。
- 现有标记：圆角方块 + 白色幕帘 `m`（**方案 A 已落地**）。母版 `docs/logo/maku-hime-logo.png`，派生图标由 `scripts/gen-icons.py` 生成，规则见 §11。

---

## 1. 品牌资产：已经定死的东西

| 项 | 值 |
| --- | --- |
| 中文名 | 幕姬（两个字都要能单独站住） |
| 拉丁名 | maku-hime（连字符保留，等宽字体排） |
| 主色（渐变） | `#FF6699` 樱花粉 → `#FF9E6D` 暖橙，135° |
| 辅色 | 舰长青 `#00D1F1`、礼物金 `#FFB027` |
| 底色 | 深 `#0D0E10` / 浅 `#F3F4F6` |
| 中文衬底 | 思源黑体 / Noto Sans CJK SC，Heavy |
| 拉丁衬底 | Inter 600；正文与拉丁小字用等宽 JetBrains Mono |
| 线宽语言 | 24 网格、1.8px 圆头描边 |
| 圆角 | 图标底 rx = 边长 × 0.28；UI 12px / 16px |

⚠️ 容易踩的坑：B 站官方粉是 `#FB7299`，我们是 `#FF6699`（更亮、更偏洋红）。AI 很容易把我们的粉拉回官方色，出图后核对 HEX，必要时后期拉回来。

---

## 2. 三条概念方向（选一条去生图，不要混）

### A · 幕帘之 M —— 主推，做应用图标
舞台幕布的**垂幔**（curtain swag）是两段下凹的圆弧——两段圆弧并排，天然就是一个小写 **`m`**。
`m` = **m**aku-hime = **幕**。一个笔划同时拿下「名字首字母」和「幕」的字义。
谷底垂下的短竖线既是幕布褶皱，也是「幕」字的竖笔；右侧三道横线是飞过的弹幕；右上角一颗四点星是「姬」。

**为什么选它**：几何、元素可数（4 个）、32px 不糊、能做单色、与现有「粉橙底 + 白色线稿」的资产完全兼容。

### B · 姫カット剪影 —— 做头像 / 吉祥物
公主切（齐刘海 + 两侧长发垂到下颌）在 ACG 语境里一望即知是「姬」，而且**两侧垂发本来就是幕布**。
用无五官的极简几何剪影：刘海是水平的幕布顶梁，两侧长发是垂落的幕布，发缝间露出三道横向光带 = 弹幕。

**为什么选它**：把「姬」讲清楚，做一个有脸但不卖萌的品牌人格；适合 GitHub 头像、宣发位。

### C · 开幕缝 —— 做 README 头图 / 启动画面
一块 16:9 的圆角幕布，中间一道斜向拉开的分缝，缝里透出粉色光与三道弹幕。
「穿屏」这件事，用**幕布被掀开一条缝**来讲最直接。

**取舍**：A 是主标，B 是人格，C 是场景。三者共用「等宽圆头描边 + 粉橙渐变 + 白线」这套语言就不会散。

### 关于旧鸟居标记（已废弃）
初版标记是圆角方块 + 白色简化鸟居（上梁 + 两根立柱）。鸟居在日本语境里是「神域入口」，配「姬」语义打架；
它的**上梁弧线**和幕布垂幔又恰好是同一个形状，于是按下面的路径演化，已经替换完毕：
把上梁加弯成两段下凹弧、去掉两根立柱、补三道弹幕线与一颗四角星。
给设计师/模型的一句话：`evolve the existing torii-like glyph into a curtain swag that reads as a lowercase m`。

---

## 3. 一致性锁（每次生图都把这四行贴在提示词最前面）

```text
统一设定（任何变体都不改）：
纯白 #FFFFFF 单色图形 · 等宽圆头描边，线宽 = 画布宽度 5.5% · 视觉元素不超过 4 个 ·
主体图形占画布 58%、光学居中 · 扁平矢量，无任何文字。
```

---

## 4. 主提示词 · 方案 A

### 4.1 中文版（即梦 / Seedream / 豆包 / 通义万相 / 混元 / GPT-Image 都能用）

```text
设计一枚应用图标 logo，正方形 1:1 构图。

【底】
圆角正方形徽章，圆角半径 = 边长的 28%，铺满画面并保留约 6% 外边距；
底色为 135° 线性渐变，从左下樱花粉 #FF6699 过渡到右上暖橙 #FF9E6D；
渐变纯净平滑，无噪点、无颗粒、无内阴影、无高光反射、无玻璃或金属质感。

【符号】
图形为纯白色 #FFFFFF 的极简几何符号，等宽圆头描边，线宽 = 画布宽度的 5.5%，
手感像一支很粗的马克笔，线端圆润，没有尖角，由四个元素组成：
1. 主体是一道舞台幕布的垂幔：两段平滑下凹的圆弧并排相连，整体读作一个小写字母 m；
2. 两段圆弧的谷底各向下延伸一小段短竖线，是幕布的褶皱，也是「幕」字的竖笔；
3. 弧线右侧收笔处向右伸出一道微微上扬的短横线，像幕布被掀起的一角；
4. 在 m 右下方的空白处，三道水平、左长右短、粗细一致的短线，表示横向飞过的弹幕；
5. 右上角一颗四角星（四个尖角、中间实心），直径约为画布宽度的 14%。

【构图】
整体居中，图形总宽约占画布 58%，四周留白充足，左右光学平衡（不是死板几何对称）；
缩到 32×32 像素时仍能读出 m 加三条线。

【风格】
扁平矢量 logo，几何构成，日式极简标志设计，单一亮度，等宽线条，SVG 风格，
边缘锐利干净；参考应用图标 / 系统托盘图标 / 唱片封套那种克制。

【禁止】
不要任何文字、字母、汉字、假名、数字；不要 3D、立体、投影、辉光、霓虹；
不要写实质感、不要剪纸毛毡黏土风；不要复杂背景、不要发光描边；
不要在徽章外再套一圈边框；不要在一张图里拼多个方案。
```

### 4.2 English（MJ / SDXL / Flux / Imagen / Firefly 更听话）

```text
Flat vector app icon logo, 1:1 square canvas, geometrically constructed.

BADGE: rounded square, corner radius 28% of side length, 6% outer margin, filled with a smooth 135-degree linear gradient from cherry pink #FF6699 to warm orange #FF9E6D. Clean and matte: no noise, no inner shadow, no gloss.

GLYPH: pure white #FFFFFF mark, uniform rounded-cap stroke weight equal to 5.5% of canvas width, thick marker feel, four elements only:
(1) two side-by-side downward-curving arcs forming a lowercase letter m that reads as a theatre curtain swag;
(2) a short vertical stroke hanging from each arc valley like a curtain fold;
(3) one short upward-tilted dash at the right end, like the corner of a lifted curtain;
(4) three horizontal speed dashes of decreasing length to the lower right, like bullet comments flying across the screen;
(5) a single four-pointed sparkle star, upper right, diameter about 14% of canvas.

LAYOUT: optically centered, glyph width about 58% of canvas, generous padding, readable at 32 pixels.

STYLE: Japanese minimalist logo design, Swiss geometric mark, flat, single tonal value for the glyph, even stroke, crisp clean SVG edges, no ornament.

ABSOLUTELY NO: text, letters, kanji, kana, numbers, watermark; 3D, bevel, emboss, drop shadow, glow, neon, glass, chrome, metal, texture, grain, noise; photorealism; multiple concepts in one image; outer ring or double border; mascot face.
```

### 4.3 反向提示词（SD / ComfyUI 丢进 negative；MJ 用 `--no`）

```text
text, letters, words, typography, watermark, signature, kanji, hiragana, katakana, numbers,
photorealistic, 3d render, bevel, emboss, drop shadow, inner shadow, glow, neon, lens flare, bloom, gradient mesh,
glass, chrome, metal, plastic, texture, grain, noise, brush strokes, sketch, paper texture, watercolor,
cluttered, multiple logos, grid of logos, mockup, business card, poster, ribbon banner,
blurry edges, inconsistent stroke width, thin spindly lines, low contrast, clip art, cartoon face, anime eyes, extra shapes, scenery, background pattern, frame, border, ring, vignette
```

---

## 5. 方案 A 的四个变体（结构不变，只换一个变量）

### A1 · 单色版（favicon / 托盘 / 单色印刷 / OBS 水印）
去渐变、去底，只留形状：

```text
同上，但改为单色：纯白 #FFFFFF 背景，图形为樱花粉 #FF6699 实色填充；
不要渐变、不要圆角底、不要任何阴影；图形占画布 72%；
同样的四元素结构：两段下凹弧组成 m、谷底短竖、右侧三道弹幕线、一颗四角星。
```

### A2 · 负形藏「姬」
在两段弧的空隙里藏一顶三尖小皇冠——远看是 m，近看有冠：

```text
同上，但两段圆弧之间的负空间（空隙）被塑形为一顶极简的三尖小皇冠，
皇冠是负形（背景色透出），不是额外的白色线条；其余部分完全不变。
```

### A3 · 弹幕感加强（宣发位用）

```text
同上，但把三道弹幕线换成三条更细、略带透视与速度残影的横线，
并在最上方一条线的末端加一枚小小的对话气泡轮廓；
其余部分（圆角底、渐变、m 形垂幔、四角星）完全不变。
```

### A4 · 深浅两套
深色版底换 `#0D0E10`（光晕参考 `src/styles/tokens.css` 的 `--bg-grad` 做法），图形保持纯白；
浅色版底 `#FFFFFF`，图形用 `#FF6699 → #FF9E6D` 渐变填充。两版元素位置必须逐像素一致。

---

## 6. 方案 B · 吉祥物「幕姬」

```text
一枚圆形头像用的扁平矢量插画：一个公主切发型（齐刘海 + 两侧长发垂到下颌）的女孩正面胸像，
风格是极简几何剪影——脸只保留两个小圆点眼睛和一条极短的微笑线，没有鼻子，不要浓妆，不要高光；
两侧垂落的头发被处理成幕布的褶皱，发缝之间横向穿过三道粉色光带（弹幕）；
她一只手（画面左下）轻轻捏着幕布的一角，像披着一件幕布披风；
配色只有三色：纯白 #FFFFFF、樱花粉 #FF6699、以及极少的舰长青 #00D1F1 点缀（发饰或光带）；
粗而均匀的圆头描边，描边宽度 = 画布宽度的 4%；纯色深底 #0D0E10；
扁平矢量，无渐变叠加、无 3D、无写实光影、无纹理；不要任何文字。
```

---

## 7. 方案 C · 开幕缝（README 头图 / 启动画面 / OBS 待机卡）

```text
16:9 的扁平矢量插画：一块细圆角的深色幕布（#0D0E10）占据整个画面，
幕布被从中间斜向掀开一道细长的缝，缝隙里透出柔和的樱花粉 #FF6699 到暖橙 #FF9E6D 的光；
缝隙中横向飞出三道白色弹幕速度线和一颗四角星，像有消息正从里面涌出来；
幕布边缘有极简的褶皱线，只有两三道，不要繁复；
整体是贴纸质感的扁平插画，无 3D、无写实光影、无玻璃质感、无噪点；不要任何文字；
左下方留出大面积纯深色空白，后续用于排版标题。
```

---

## 8. 字标与横版 lockup

**建议：AI 只出图形，字标自己排。** 生图模型写「幕姬」两个字经常缺笔、写成伪汉字，返工比重排还贵。

- 中文：思源黑体 / Noto Sans CJK SC **Heavy**，字距 +0.02em，两个字紧排；
- 拉丁：`Inter` 600，或等宽 `JetBrains Mono` 400 + 字距 +0.08em，放在中文右下，字号 = 中文的 38%；
- 图形与字标间距 = 图形边长的 22%；整体左对齐到同一条视觉基线上。

如果你确实想让 AI 一起排，用这段（并默认它会被重排一遍）：

```text
横向 lockup，画布 4:1：左侧是图标（圆角正方形徽章 + 粉橙渐变 + 白色幕帘 m 符号），
右侧是两个中文黑体字「幕姬」，思源黑体 Heavy 质感，纯白 #FFFFFF，字形端正、笔划完整、不要连笔不要变形；
中文下方一行小号等宽拉丁字「maku-hime」，字距放宽，颜色 #A4ABB4；
全部左对齐、基线对齐，纯深色背景 #0D0E10，扁平，无 3D、无描边、无投影。
```

---

## 9. 生成参数与工作流

| 用途 | 比例 | 出图尺寸 | 参数建议 |
| --- | --- | --- | --- |
| 图标母版 | 1:1 | 1024 → 上采样 2048 | SDXL/Flux：CFG 4–6、steps 32；Flux guidance 3.0 |
| 九宫格探索 | 1:1 | 1024 | 一次要 9 个变体，再挑 1 个定 seed |
| 横版 lockup | 4:1 | 2048×512 | 元素少容易崩，多出几张 |
| README 头图 | 16:9 | 1920×1080 | 这张允许一点柔光 |
| 吉祥物头像 | 1:1 | 1024 | 描边一致性最难，挑线条最匀的那张 |

**迭代纪律（很重要）**：

1. 先跑九宫格找方向，别在单张上死磕；
2. 选定一张后**锁 seed**，然后一次只改一个词（先改构图，再改线宽，最后改配色）；
3. 微调用 img2img 低重绘（denoise 0.25–0.35），比重新抽卡稳定；
4. 每张出图先缩到 32px 看一眼，糊了就整张作废，不要指望后期救；
5. MJ 追加：`--ar 1:1 --style raw --s 50 --no text,letters,shadow,gradient mesh`；
6. GPT-Image / 即梦这类自然语言模型：把 §4.1 整段粘过去，不要拆碎。

---

## 10. 验收清单（AI 出图的通病，逐条打勾）

- [ ] 缩到 32×32，还能读出 **m** 和三条弹幕线吗？（读不出就作废）
- [ ] 两段圆弧的**粗细是否完全一致**（AI 十张有八张一粗一细）
- [ ] 四个圆角半径是否一致且 ≈ 边长 28%
- [ ] 有没有偷偷生成了假文字、假假名、假签名
- [ ] 有没有多出一圈边框、外发光、投影、内阴影
- [ ] 渐变是不是 135°，色相有没有偏紫或偏黄；粉色有没有被拉到 B 站官方 `#FB7299`
- [ ] 图形是**光学居中**还是死板几何居中（视觉重心偏右是常态，要调）
- [ ] 视觉元素是否 ≤ 4 个（多一个就少一分识别度）
- [ ] 元素之间有没有「粘」在一起（缩小后会糊成一坨）
- [ ] 深浅两版逐像素位置是否一致

---

## 11. 落地：已经做完的部分（改图标照着走）

**母版是位图，不转矢量。** 图形只有「白 + 渐变」两层，栅格化后喂给 `npx tauri icon` 的
质量和手描 SVG 没差别，省掉了描摹后再修碎路径的一整轮返工；28px 的侧栏小标给 192px 位图（约 6.8×），
高 DPI 与后续放大都够用。

| 位置 | 内容 | 谁生成 |
| --- | --- | --- |
| `docs/logo/maku-hime-logo.png` | 设计师交付源图（1254×1254，白底 + 圆角徽章），**不要覆盖** | 设计 |
| `docs/logo/icon-1024.png` | 裁掉白边、圆角外透明的图标母版 | `scripts/gen-icons.py` |
| `src-tauri/icons/*` | 32/64/128/128@2x/icon.png + ico + icns + Windows Store 方块图 | `npx tauri icon` |
| `src/assets/logo.png` | 面板侧栏 28px 品牌标（192×192） | `scripts/gen-icons.py` |
| `public/favicon.png`、`favicon.ico`、`apple-touch-icon.png` | 浏览器标签页 / 添加到桌面 | `scripts/gen-icons.py` |
| `README.md` 顶部 | 引用 `docs/logo/icon-1024.png` | — |

一条命令重跑全部：

```bash
python3 scripts/gen-icons.py   # 需要 Pillow；会自动调用 npx tauri icon
```

脚本只做几何处理（按饱和度找徽章边界 → 裁白边 → 缩放到目标尺寸 → 套圆角 alpha），
**不重绘图形**，所以换母版就等于换全套图标。

### 母版实测几何（改版时用来核对新稿）

| 项 | 实测值 | 备注 |
| --- | --- | --- |
| 画布 | 1254×1254，四周白边 47px | 交付图有白边，必须裁，否则图标发虚 |
| 徽章 | 1160×1160 | 裁完就是满画布 |
| 圆角半径 | 222.5px ≈ 边长 **19.2%** | 拟合残差 0.27px（brief 里写的 28% 与成稿不符，以成稿为准） |
| 渐变 | 左下粉 → 右上橙，对角线 45° | 左上 ≈ `#FF6D8C`、中心 `#FC7976`、右上偏橙 |
| 描边 | ≈78px ≈ 边长 **6.2%** | 三根竖笔同粗：x∈[229,306]、[489,567]、[744,821] |
| 白色 | `#FEFEFE`（254） | 白底 253、图形 254，只差 1 级，抠图别用"近白"阈值，要用饱和度找徽章 |

### 还需要人补的三份（尚未生产）

- **纯单色版**（白底粉图形 / 深底白图形）：OBS 水印、单色印刷、favicon 极简版；
- **横版 lockup**（图形 + 「幕姬」+ maku-hime）：见 §8，字标自己排，不要 AI 排字；
- **吉祥物头像**（方案 B）与 **README 头图**（方案 C）：目前 README 头图位置放的是图标。

---

## 12. 一页速查

```text
【概念】舞台幕布垂幔 = 小写 m = 幕；谷底短竖 = 幕字竖笔；三道横线 = 弹幕；四角星 = 姬。
【底】圆角方 28%（生图用；成稿实测 19.2%），135° #FF6699 → #FF9E6D，无阴影无噪点。
【形】纯白，等宽圆头描边 5.5%，4 个元素，占画布 58%，光学居中。
【禁】文字 / 3D / 投影 / 辉光 / 纹理 / 多重方案。
【验】32px 可读，弧线同粗，圆角一致，无假字，色准 #FF6699。
```
