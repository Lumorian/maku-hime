# B 站直播弹幕协议实现规格

> 依据：本机只读源码。所有结论标注出处 `文件:行号`，未经验证的点明确写「未验证」。
> 本文档面向 Rust 重写（maku-hime），因此除字段表外还给出可直接照抄的伪代码。

## 0. 证据基线与读法

### 0.1 两个源码根

| 别名 | 实际路径 | 说明 |
| --- | --- | --- |
| `UniBarrage/…` | `/home/lumorian/Projects/bili-dm/UniBarrage/…` | 业务侧：房间预检、事件→统一消息、原始 JSON 补字段 |
| `blivedm-go/…` | `/home/lumorian/go/pkg/mod/github.com/xifan2333/blivedm-go@v1.7.4/…` | 协议侧：HTTP 接口、WBI 签名、WebSocket、包编解码、消息解析 |

`blivedm-go` 是 `UniBarrage/go.mod:23` 钉住的依赖（`github.com/xifan2333/blivedm-go v1.7.4`，`go.sum:176-177`）。它不在 UniBarrage 仓库里，是本机 module cache 中的只读副本；行号对应该版本，换版本需重新核对。

### 0.2 关键事实：UniBarrage 自己不实现协议

UniBarrage 的 B 站链路只有三层，**WebSocket / 包格式 / 解压 / 消息反序列化全部委托给 blivedm-go**：

1. `UniBarrage/bilibili/fetcher.go:62-98`：一个 `get_info` HTTP 调用（只用来校验房间存在、拿真实 room_id）；
2. `UniBarrage/bilibili/bilibili.go:19-314`：`client.NewClient` + 回调注册（`bilibili.go:38`、`bilibili.go:273-302`）；
3. `UniBarrage/bilibili/extractor.go`：从**原始 JSON 字符串**里用 gjson/正则补 blivedm 没解析的字段（勋章渐变、表情、头像）。

所以「照着实现」等价于重新实现 blivedm-go 的 `packet` + `api` + `client` + `message` 四个包。

### 0.3 标注约定

- **【验证】** 源码里有明确代码/结构体/tag 支撑。
- **【结构体】** 源码里定义了 struct + json tag，但**没有绑定到任何 handler**（字段路径只是作者意图，未经运行验证）。
- **【未验证】** 源码没有相关信息，本文不臆测。
- **【缺陷】** 源码行为明显有问题，实现时不要照抄。

---

## 1. 房间信息相关 HTTP 接口

### 1.1 源码里出现过的 B 站 HTTP 端点（穷举）

| 端点 | 出处 | 用途 |
| --- | --- | --- |
| `https://api.live.bilibili.com/room/v1/Room/get_info?room_id={}` | `UniBarrage/bilibili/fetcher.go:64` | 房间存在性 + 真实 room_id + 开播状态 + 标题 |
| `https://api.bilibili.com/x/web-interface/card?mid={}` | `UniBarrage/bilibili/fetcher.go:103` | 用户资料（含粉丝数、头像） |
| `https://api.live.bilibili.com/xlive/web-room/v1/giftPanel/giftConfig?platform=pc&room_id={}` | `UniBarrage/bilibili/gifts/gifts.go:36` | 礼物面板（名称→图标映射） |
| `https://api.live.bilibili.com/room/v1/Room/room_init?id={}` | `blivedm-go/api/info.go:97` | 短号→真实房间号、开播状态 |
| `https://api.live.bilibili.com/xlive/web-room/v1/index/getDanmuInfo?id={}&type=0` | `blivedm-go/api/info.go:81` | 弹幕服务器 host_list + token（详见 §2） |
| `https://api.bilibili.com/x/web-interface/nav` | `blivedm-go/api/info.go:64`、`blivedm-go/api/sign.go:128` | 登录 uid / WBI 密钥 |
| `https://api.live.bilibili.com/msg/send` (POST) | `blivedm-go/api/send.go:58` | 发弹幕（详见 §7.1） |

### 1.2 `get_info`：UniBarrage 唯一使用的房间接口

```
GET https://api.live.bilibili.com/room/v1/Room/get_info?room_id={room_id}
```

- query：`room_id`（必填；**可以是短号**，B 站返回真实号）。拼接方式见 `UniBarrage/bilibili/fetcher.go:64`。
- header：**一个都没有** —— `http.Get(url)` 直连（`fetcher.go:67`），实际 UA 是 Go 默认 `Go-http-client/1.1`，无 Referer、无 Cookie。
  → 结论：`get_info` 不需要 Referer / 登录 Cookie 即可返回数据（这是生产路径，等于实测）。
- 返回体：`{code, message, data}`，`code != 0` 视为错误（`fetcher.go:80-94`）。
- 解析到 `RoomInfo` 的字段（`fetcher.go:53-59`）：

| JSON 路径 | 字段 | 类型 | 备注 |
| --- | --- | --- | --- |
| `data.room_id` | RoomID | int | **真实房间号**；0 表示房间不存在（`bilibili.go:31` 用它判空） |
| `data.short_id` | ShortID | int | 短号 |
| `data.uid` | UID | int | **主播 uid** |
| `data.live_status` | LiveStatus | int | 0 未开播 / 1 直播中 / 2 轮播中（`fetcher.go:57` 注释） |
| `data.title` | Title | string | **标题** |

**源码缺口（重要）**：`RoomInfo` 只声明了上面 5 个字段。**人气、分区、粉丝数三个字段在本接口的解析里完全不存在**（`fetcher.go:53-59` 全文即上述 5 个 tag）。接口实际可能还返回 `online`（热度）、`area_name`、`parent_area_name`、`attention` 等，但**本源码未解析 → 不引用源码即视为「未验证」**。已验证的替代方案见 §1.5 与附录 C。

### 1.3 `room_init`：库内的「真实房间号」接口

```
GET https://api.live.bilibili.com/room/v1/Room/room_init?id={room_id}
```

- header：仅 `User-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:137.0) Gecko/20100101 Firefox/137.0`（`api/info.go:95-97`）。
- 返回字段（`api/info.go:14-36`）：

| JSON 路径 | 类型 | 说明 |
| --- | --- | --- |
| `code` / `message` / `msg` | int / string | 三个都解析了（B 站两种字段名都出现过） |
| `data.room_id` | int | 真实房间号 → `GetRoomRealID` 返回它的十进制字符串（`api/info.go:104-110`） |
| `data.short_id` | int | 短号 |
| `data.uid` | int | 主播 uid |
| `data.live_status` | int | 开播状态 |
| `data.live_time` | int64 | 开播时间 |
| `data.is_hidden` / `is_locked` / `is_portrait` / `encrypted` / `pwd_verified` | bool | 隐藏/锁定/竖屏/加密/密码已验证 |
| `data.hidden_till` / `lock_till` | int | 隐藏/锁定截止 |
| `data.need_p2p` / `room_shield` / `is_sp` / `special_type` | int | 其它标志位 |

**调用链**：UniBarrage 先用 `get_info` 拿真实号（`bilibili.go:23,38`），client 初始化时又用 `room_init` 再解析一次并覆盖 `c.RoomID`（`client/client.go:80-85`）。Rust 实现里**一次 `room_init` 就够**；`get_info` 只在需要 `title` / `live_status` 时补一次。

### 1.4 `card`：粉丝数与用户资料

```
GET https://api.bilibili.com/x/web-interface/card?mid={uid}
```

- header：无（`fetcher.go:106` 用 `http.Get`）。→ 该接口在无 UA/Referer 下可用。
- 返回（`fetcher.go:11-50`）：

| JSON 路径 | 字段 | 类型 | 说明 |
| --- | --- | --- | --- |
| `data.follower` | UserData.Follower | int | **粉丝数** |
| `data.following` | UserData.Following | bool | 是否已关注（需登录才有意义） |
| `data.card.fans` | CardInfo.Fans | int | 另一个「粉丝数」字段，两者语义源码未区分 → 语义**未验证**，实盘核对一次 |
| `data.card.mid` | MID | **string** | 注意是字符串 |
| `data.card.name` / `face` / `sign` / `sex` / `rank` | string | 昵称/头像/签名/性别/等级 |
| `data.card.attention` | int | 关注数 |
| `data.card.level_info.current_level` | int | 用户等级 |
| `data.card.nameplate.{name,image,image_small}` | string | 名牌 |
| `data.card.vip.status` / `vip.label.text` | int / string | 大会员 |

用途：`FetchUserData` 在三处被调用 —— 上舰补头像（`bilibili.go:113`）、点赞头像兜底（`bilibili.go:180,210,224`）。它**不在弹幕主路径**上（弹幕头像直接正则抓 raw，`extractor.go:25-39`）。

### 1.5 主播 uid / 分区 / 人气 —— 本源码的结论

- 主播 uid：`get_info` 的 `data.uid`（`fetcher.go:56`）或 `room_init` 的 `data.uid`（`api/info.go:21`）。**【验证】**
- 分区、人气：**UniBarrage 与 blivedm-go 都没有解析**。`message/utils.go:72-74` 定义了 `OnlineRankCount{Count int}`，但没有任何 handler 绑定它（`client/handler.go:122-202` 的 switch 里没有），**不能当作已验证的人气来源**。
- 已验证可用的替代接口见**附录 C**（同工作区 bili-dm 的实现，与 UniBarrage 无关，但接口 + 字段路径在那边是生产代码）。

### 1.6 必需 header 汇总（源码实证）

| 调用 | UA | Referer | Cookie |
| --- | --- | --- | --- |
| `get_info`（UniBarrage） | 未设置（Go 默认） | 未设置 | 未设置 |
| `card`（UniBarrage） | 未设置 | 未设置 | 未设置 |
| `room_init` | Firefox/137（`api/info.go:96`） | 未设置 | 未设置 |
| `getDanmuInfo` | Firefox/137（`api/info.go:79`） | 未设置 | 有则带（`info.go:78`） |
| `nav`（取 uid） | Firefox/137（`info.go:63`） | 未设置 | 必带 |
| `nav`（取 WBI key） | 未设置（`sign.go:128`） | 未设置 | 未设置，注释称"不修改 header 也能过" |
| WS 握手 | Chrome/102（`client/client.go:107`） | 未设置 | 未设置 |

→ 结论：**这些 HTTP 接口不要求 Referer**（源码全程未设置 Referer 且为生产路径）。Rust 侧仍建议带上 `Referer: https://live.bilibili.com/` 和浏览器 UA 以降低风控风险，但这属于收益**未验证**的加固，不是协议要求。

### 1.7 Rust 伪代码

```rust
// 1) 房间真实号 + 开播状态（一次调用即可）
#[derive(Debug, Clone)]
pub struct RoomInit {
    pub room_id: i64,      // data.room_id
    pub short_id: i64,     // data.short_id
    pub uid: i64,          // data.uid  ← 主播 uid
    pub live_status: i32,  // data.live_status: 0 未开播 1 直播中 2 轮播
    pub live_time: i64,    // data.live_time
}

pub async fn room_init(short_or_real_id: i64) -> Result<RoomInit> {
    let url = format!("https://api.live.bilibili.com/room/v1/Room/room_init?id={short_or_real_id}");
    let v: serde_json::Value = http_get(&url, &[("User-Agent", UA_FIREFOX)]).await?;
    if v["code"].as_i64().unwrap_or(-1) != 0 { bail!("room_init code={}", v["code"]); }
    let d = &v["data"];
    Ok(RoomInit {
        room_id: d["room_id"].as_i64().unwrap_or(0),
        short_id: d["short_id"].as_i64().unwrap_or(0),
        uid: d["uid"].as_i64().unwrap_or(0),
        live_status: d["live_status"].as_i64().unwrap_or(0) as i32,
        live_time: d["live_time"].as_i64().unwrap_or(0),
    })
}

// 2) 标题 + 粉丝数（可选展示）
pub async fn room_title(room_id: i64) -> Result<String> {
    let url = format!("https://api.live.bilibili.com/room/v1/Room/get_info?room_id={room_id}");
    let v = http_get(&url, &[]).await?;               // 源码证明无需 header
    Ok(v["data"]["title"].as_str().unwrap_or("").to_string())
}

pub async fn follower_count(uid: i64) -> Result<i64> {
    let url = format!("https://api.bilibili.com/x/web-interface/card?mid={uid}");
    let v = http_get(&url, &[]).await?;
    Ok(v["data"]["follower"].as_i64().unwrap_or(0))
}
```

---

## 2. 弹幕服务器发现：`getDanmuInfo`

### 2.1 URL 与参数

```
GET https://api.live.bilibili.com/xlive/web-room/v1/index/getDanmuInfo?id={real_room_id}&type=0
```

- 参数：`id`（**真实房间号**，非短号）、`type=0`（`api/info.go:81`）。
- **必须做 WBI 签名**：源码无条件走 `WbiKeysSignString`，签名失败即整体失败（`info.go:81-84`）；签名后 query 追加 `wts`、`w_rid`（`api/sign.go:106-112`）。
- header：`cookie`（可为空串，但源码仍设置该头）、`User-Agent`（Firefox/137）（`info.go:77-79`）。

### 2.2 WBI 签名算法（可直接照抄）

依据 `api/sign.go:52-61,97-114,122-177`：

1. 取密钥：`GET https://api.bilibili.com/x/web-interface/nav`，允许 `code == 0` 或 `code == -101`（未登录也返回 key；`sign.go:144`）。
2. `img_key` = `data.wbi_img.img_url` 去掉目录与 `.png` 后缀；`sub_key` 同理（`sign.go:147-163`）。
3. `mixin_key` = 取 `img_key + sub_key`，按 64 项置换表取前 32 个字符（`sign.go:170-177`）。置换表（`sign.go:52-61`）：

```
46, 47, 18,  2, 53,  8, 23, 32, 15, 50, 10, 31, 58,  3, 45, 35,
27, 43,  5, 49, 33,  9, 42, 19, 29, 28, 14, 39, 12, 38, 41, 13,
37, 48,  7, 16, 24, 55, 40, 61, 26, 17,  0,  1, 60, 51, 30,  4,
22, 25, 54, 21, 56, 59,  6, 63, 57, 62, 11, 36, 20, 34, 44, 52,
```

4. 对 query 做 `url.Values.Encode()`（**按 key 排序 + percent-encode**），删掉字符 `! ' ( ) *`，重新解析，然后 `wts = 当前 unix 秒`（`sign.go:104-106`）。
5. `w_rid = md5(sorted_query + mixin_key)` 的十六进制小写（`sign.go:110-111`）。
6. 最终 URL 的 query 是 `values.Encode()`（再次排序：`id, type, w_rid, wts` 的字典序）。
7. 密钥缓存 1 小时（`sign.go:123`）。

```rust
const MIXIN_KEY_ENC_TAB: [usize; 64] = [
    46,47,18,2,53,8,23,32,15,50,10,31,58,3,45,35,
    27,43,5,49,33,9,42,19,29,28,14,39,12,38,41,13,
    37,48,7,16,24,55,40,61,26,17,0,1,60,51,30,4,
    22,25,54,21,56,59,6,63,57,62,11,36,20,34,44,52,
];

fn wbi_sign(mut params: BTreeMap<String, String>, mixin_key: &str) -> BTreeMap<String, String> {
    // 源码是对 Encode 后的串删字符再 parse（sign.go:63-73），
    // 因此严格等价于「percent-encode 之后再做字符删除」
    let mut filtered: BTreeMap<String, String> = params
        .drain()
        .map(|(k, v)| (k, v.replace(['!', '\'', '(', ')', '*'], "")))
        .collect();
    filtered.insert("wts".into(), chrono::Utc::now().timestamp().to_string());
    let query = serde_urlencoded::to_string(&filtered).unwrap(); // BTreeMap → 已排序
    let rid = format!("{:x}", md5::compute(format!("{query}{mixin_key}")));
    filtered.insert("w_rid".into(), rid);
    filtered
}
```

> 坑：第 4 步的「删字符」在 Go 里作用于**已编码**的 query 串（`sign.go:63-73` 先 `v.Encode()` 再 `bytes.ReplaceAll`）。若 Rust 侧先删原值再 encode，对含这些字符的 value 会与 Go 结果不同。

### 2.3 返回结构（`api/info.go:40-58`）

| JSON 路径 | 类型 | 说明 |
| --- | --- | --- |
| `code` | int | 0 成功；`352` 等为风控/校验失败（源码在 `client/client.go:88` 注释里点名 352） |
| `message` | string | 错误描述 |
| `ttl` | int | token 有效期（单位源码未标注，**未验证**是秒还是分） |
| `data.group` | string | 分组 |
| `data.business_id` | int | 业务 id |
| `data.refresh_row_factor` | float64 | 弹幕行刷新因子 |
| `data.refresh_rate` | int | 刷新率 |
| `data.max_delay` | int | 最大延迟 |
| `data.token` | string | **auth 包的 `key` 字段** |
| `data.host_list` | array | 每项：`host`(string)、`port`(int)、`wss_port`(int)、`ws_port`(int) |

### 2.4 失败与降级（照抄前先看清）

`client/client.go:86-101`：

```go
if c.host == "" {
    info, err := api.GetDanmuInfo(c.RoomID, c.Cookie)
    if err != nil || info.Code != 0 {
        c.hostList = []string{"broadcastlv.chat.bilibili.com"}   // 兜底 host
    } else { for _, h := range info.Data.HostList { c.hostList = append(c.hostList, h.Host) } }
    c.token = info.Data.Token                                    // ← err != nil 时 info 为 nil
}
if c.token == "" { return errors.New("token 获取失败") }
```

- 【缺陷】`err != nil` 分支后仍无条件读 `info.Data.Token`（`client.go:96`）→ 空指针 panic。Rust 侧应写成 `let info = get_danmu_info(...).await.context("getDanmuInfo")?` 并在 `code != 0` 时报错。
- 【验证】没有 token 就无法开始监听：`Start()` 返回错误（`client.go:98-101`、`client.go:170-173`、`UniBarrage/bilibili/bilibili.go:304-309`）。
- 【验证】只取 `host`，**`wss_port` 被丢弃**：连接固定 `wss://{host}/sub`（默认 443，`client.go:111`）。若某 host 必须走非标端口，本例连不上（host_list 当前是否返回非标端口：**未验证**）。
- 【缺陷】token 只在 `init()` 取一次，重连复用旧 token；`ttl` 从不检查。

---

## 3. WebSocket 连接

### 3.1 地址与握手

- 地址：`wss://{host}/sub`，`host` 来自 `data.host_list[].host`，缺省 `broadcastlv.chat.bilibili.com`（`client/client.go:111`、`client.go:90,193`）。
- 握手 header：**只有 `User-Agent`**（Chrome/102，`client.go:107`）；`Upgrade` / `Sec-WebSocket-Key` / `Sec-WebSocket-Version: 13` 由 gorilla 自动补（`client.go:111`）。**不带 Cookie、不带 Origin、不带 Referer**。
- 子协议 / 压缩扩展：无（`websocket.DefaultDialer`，未设置任何 options）。
- 连接建立后**立刻**发认证包（`client.go:119`）。

### 3.2 二进制包格式（16 字节头，全大端）

依据 `packet/packet.go:104-122`（打包）与 `packet/packet.go:56-74`（解包）：

| 偏移 | 长度 | 字段 | 类型/字节序 | 源码处理 |
| --- | --- | --- | --- | --- |
| 0 | 4 | `packet_len` | uint32 **BigEndian** | 整个包长（含 16 字节头）；解包时校验 `== len(data)`，不等即坏包（`packet.go:62-67`） |
| 4 | 2 | `header_len` | uint16 **BigEndian** | **固定 16**（打包写 `0x00,0x10`，`packet.go:108`）；解包时**完全忽略**，直接从 16 取 body（`packet.go:71`） |
| 6 | 2 | `protover` | uint16 **BigEndian** | 0/1/2/3，见 §3.4 |
| 8 | 4 | `operation` | uint32 **BigEndian** | 见 §3.3 |
| 12 | 4 | `sequence` | uint32 **BigEndian** | 发送时恒为 **1**（`packet.go:108` 末字节）；解包时忽略 |
| 16 | … | `body` | bytes | JSON 明文，或 zlib/brotli 压缩流 |

```rust
pub const HEADER_LEN: usize = 16;

#[derive(Debug, Clone)]
pub struct Packet { pub protover: u16, pub operation: u32, pub body: Vec<u8> }

pub fn encode(protover: u16, operation: u32, body: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(HEADER_LEN + body.len());
    buf.extend_from_slice(&[0u8; 4]);              // packet_len 占位
    buf.extend_from_slice(&16u16.to_be_bytes());   // header_len = 16
    buf.extend_from_slice(&protover.to_be_bytes());
    buf.extend_from_slice(&operation.to_be_bytes());
    buf.extend_from_slice(&1u32.to_be_bytes());    // sequence = 1
    buf.extend_from_slice(body);
    let n = buf.len() as u32;
    buf[0..4].copy_from_slice(&n.to_be_bytes());
    buf
}

pub fn decode(data: &[u8]) -> Result<Packet> {
    if data.len() < HEADER_LEN { bail!("short packet: {}", data.len()); }
    let plen = u32::from_be_bytes(data[0..4].try_into()?) as usize;
    if plen != data.len() { bail!("packet_len {plen} != actual {}", data.len()); }
    Ok(Packet {
        protover: u16::from_be_bytes(data[6..8].try_into()?),
        operation: u32::from_be_bytes(data[8..12].try_into()?),
        body: data[HEADER_LEN..plen].to_vec(),
    })
}
```

> `header_len` 建议真读出来再用（取 body 时用 `header_len` 而非硬编码 16）。源码硬编码 16（`packet.go:71`），若 B 站将来改头长会碎。

### 3.3 `operation` 取值（`packet/packet.go:21-31`）

| 值 | 常量名 | 方向 | 含义 | 源码行为 |
| --- | --- | --- | --- | --- |
| 0 | `HandShake` | C→S | 握手（历史遗留） | 未使用 |
| 1 | `HandShakeResponse` | S→C | 握手回复 | 未处理 |
| 2 | `HeartBeat` | C→S | **心跳** | 每 30s 发一次（`client.go:152-167`） |
| 3 | `HeartBeatResponse` | S→C | **心跳回复** | 收到即丢弃（`client/handler.go:195` 空 case） |
| 4 | `_` | — | 未使用 | — |
| 5 | `Notification` | S→C | **业务消息（弹幕/礼物/…）** | 解析 `cmd` 后分发（`handler.go:124-194`） |
| 6 | `_` | — | 未使用 | — |
| 7 | `RoomEnter` | C→S | **认证（auth）** | 连接后立即发送（`client.go:119,196-205`） |
| 8 | `RoomEnterResponse` | S→C | **认证回复** | 收到即丢弃（`handler.go:196` 空 case） |

注意：`operation` 是 uint32、`protover` 是 uint16、`sequence` 是 uint32 —— 三者宽度不同。

### 3.4 `protover` 与解压顺序

| protover | 常量（`packet.go:14-19`） | body 形态 | 处理 |
| --- | --- | --- | --- |
| 0 | `Plain` | 明文 JSON（单条） | 直接解析（`packet.go:78-81` 与 1 同分支） |
| 1 | `Popularity` | 明文 JSON（单条；心跳/认证回复都是 1） | 直接解析 |
| 2 | `Zlib` | **zlib** 流（RFC1950，`compress/zlib`，`packet.go:146-154`） | 解压 → 按包切片 → 逐包解析 |
| 3 | `Brotli` | **brotli** 流（`andybalholm/brotli`，`packet.go:156-163`） | 解压 → 按包切片 → 逐包解析 |
| 其它 | — | — | 打日志丢弃（`packet.go:94-96`、`handler.go:197-200`） |

**解压顺序**（`packet.go:76-98` + `packet.go:134-144`）：

1. 读 16 字节头，取 `protover`；
2. `protover ∈ {0,1}` → 该包本身就是一条消息，`body` 即 JSON；
3. `protover ∈ {2,3}` → 解压 `body`，得到一段**由多个完整包首尾相接**的字节流，再从头循环：读 4 字节 `packet_len` → 切出整包 → 解 16 字节头 → 取 body 当 JSON；
4. 子包**不再递归解压**。

```rust
pub fn unpack(pkt: Packet) -> Result<Vec<Packet>> {
    match pkt.protover {
        0 | 1 => Ok(vec![pkt]),
        2 => Ok(slice_packets(&zlib_decompress(&pkt.body)?)?),
        3 => Ok(slice_packets(&brotli_decompress(&pkt.body)?)?),
        other => bail!("unknown protover {other}"),
    }
}

fn slice_packets(buf: &[u8]) -> Result<Vec<Packet>> {
    let mut out = Vec::new();
    let mut cur = 0usize;
    while cur < buf.len() {
        if buf.len() - cur < 4 { bail!("truncated sub-packet"); }
        let len = u32::from_be_bytes(buf[cur..cur + 4].try_into()?) as usize;
        if len == 0 || cur + len > buf.len() { bail!("bad sub-packet len {len}"); }
        out.push(decode(&buf[cur..cur + len])?);
        cur += len;
    }
    Ok(out)
}
```

> 【缺陷】Go 版 `Slice` 不检查 `len == 0` 与越界（`packet.go:138-142`）：`packLen == 0` 会**死循环**，缓冲区被截断会 panic。Rust 侧务必按上面校验。
> 【未验证】嵌套压缩（brotli 里再套 zlib）是否存在：源码不处理，按「不嵌套」实现。

### 3.5 认证包（op=7）JSON 字段

依据 `packet/enter.go:9-36`：

| JSON key | 值 | 说明 |
| --- | --- | --- |
| `uid` | int | 登录 uid；**未登录填 0**（`enter.go:20` 注释「uid 可以为 0」） |
| `buvid` | string | 见 §4；未登录为空串 |
| `roomid` | int | **真实房间号** |
| `protover` | int | **固定 3**（不是包头里的 1，`enter.go:26`） |
| `platform` | string | **固定 `"web"`**（`enter.go:27`） |
| `type` | int | **固定 2**（`enter.go:28`） |
| `key` | string | getDanmuInfo 返回的 `data.token` |

实际发出的包：包头 `protover = 1`（`NewPlainPacket` 把包头 protover 硬设为 1，`packet.go:52-54`）、`operation = 7`、`sequence = 1`、body 为上面的 JSON（无空格紧凑格式）。

```json
{"uid":0,"buvid":"","roomid":2233,"protover":3,"platform":"web","type":2,"key":"<token>"}
```

```rust
#[derive(serde::Serialize)]
struct AuthBody<'a> {
    uid: i64, buvid: &'a str, roomid: i64, protover: u8,
    platform: &'a str, #[serde(rename = "type")] kind: u8, key: &'a str,
}

fn auth_packet(uid: i64, buvid: &str, roomid: i64, token: &str) -> Vec<u8> {
    let body = serde_json::to_vec(&AuthBody {
        uid, buvid, roomid, protover: 3, platform: "web", kind: 2, key: token,
    }).unwrap();
    encode(1, 7, &body)          // 包头 protover = 1, op = 7
}
```

> 认证回复（op=8）的 body 是 `{"code":0}` 形态 —— **源码从不读取它**（`handler.go:196` 空 case），因此「认证失败」在 Go 版里是**静默的**。Rust 必须自己解析（§6）。

### 3.6 心跳

- 周期：**30 秒**（`client.go:158` 的 `time.After(30 * time.Second)`）。
- 包：`protover = 1`、`operation = 2`、`sequence = 1`、**body 为空（0 字节）**（`packet/heartbeat.go:4-7`：`NewPacket(1, HeartBeat, nil)`）。
- 因此心跳包是**恰好 16 字节**：`00 00 00 10 00 10 00 01 00 00 00 02 00 00 00 01`。
- 心跳回复（op=3）不校验、不超时、不掉线（`handler.go:195`）。

```rust
fn heartbeat_packet() -> [u8; 16] {
    let mut b = [0u8; 16];
    b[0..4].copy_from_slice(&16u32.to_be_bytes());   // packet_len = 16
    b[5] = 16;                                       // header_len = 16
    b[6..8].copy_from_slice(&1u16.to_be_bytes());    // protover = 1
    b[8..12].copy_from_slice(&2u32.to_be_bytes());   // operation = 2 (HeartBeat)
    b[12..16].copy_from_slice(&1u32.to_be_bytes());  // sequence = 1
    b
}
```

### 3.7 收发循环

```rust
// 连接 → 认证 → 循环读；每 30s 发心跳（独立 task）
async fn run_connection(ws: &mut WebSocket, ctx: &ConnCtx) -> Result<()> {
    ws.send(Message::Binary(auth_packet(ctx.uid, &ctx.buvid, ctx.room_id, &ctx.token))).await?;

    let mut hb = tokio::time::interval(Duration::from_secs(30));
    loop {
        tokio::select! {
            _ = hb.tick() => ws.send(Message::Binary(heartbeat_packet().to_vec())).await?,
            msg = ws.next() => match msg {
                Some(Ok(Message::Binary(buf))) => {
                    for pkt in unpack(decode(&buf)?)? {
                        dispatch(pkt)?;              // 串行处理，见 §7.3
                    }
                }
                Some(Ok(Message::Text(_))) => { /* 源码：非 binary 直接丢弃，client.go:141-144 */ }
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(e.into()), // → 重连
                None => bail!("ws closed"),
            },
        }
    }
}
```

> 【缺陷，务必不要照抄】Go 版对**每个子包**各起一个 goroutine（`client.go:145-147`），handler 内部再起 goroutine（`handler.go:131,139`）→ **同一批消息顺序不保证**。Rust 侧应串行 `dispatch`（或按到达序号入队）。

---

## 4. buvid 的获取

源码里**没有任何「申请 buvid」的 HTTP 调用**。在 blivedm-go v1.7.4 里 `grep -rn buvid` 共 12 处，全部是「从 Cookie 取值」或「塞进 auth 包」：`packet/enter.go:11,21,24`、`client/client.go:23,66-78,197,203`；UniBarrage 仓库内 0 处。取值逻辑（`client/client.go:66-78`）：

1. 有 Cookie 时：从 Cookie 头串里按 `;` 切分，先找 **`buvid3`**（`cookieValue(cookie, "buvid3")`，实现见 `client.go:207-223`）；
2. 找不到 → 退 `_uuid`（`client.go:69-71`）；
3. 再找不到 → 旧正则 `_uuid=([^;]+)` 兜底（`client.go:72-78`）；
4. 都没有 → 空串，auth 包 `buvid: ""`。

行为差异：**有 Cookie** 时会先调 `nav` 取 uid（`client.go:61-65`，失败只记日志、uid 保持 0），**且校验 Cookie 必须同时含 `bili_jct` 与 `SESSDATA`**，否则直接报「账号未登录」让 `Start()` 失败（`client.go:56-60`）。

→ Rust 实现建议：`buvid = cookie.buvid3 ?? cookie._uuid ?? ""`；无 cookie 就走匿名（uid=0、buvid 空）。
→ **未验证**：是否存在不依赖 Cookie 的 buvid 生成接口（本源码未使用，不做断言）。

---

## 5. 消息 `cmd` 清单与字段映射

### 5.1 分发机制（决定 Rust 侧解析入口）

`client/handler.go:122-210`：

1. 只处理 `operation == 5`（Notification）；
2. `cmd` 用正则 `"cmd":"([^"]+)"` 从 body 原文抠出（`handler.go:55,204-210`）→ **要求 JSON 里 `"cmd":"` 之间无空格**；
3. 若 `cmd` 含 `:` 则截断（如 `DANMU_MSG:4:0:2:2:2:0` → `DANMU_MSG`，`handler.go:127-129`）；
4. **先查自定义 handler 表**，命中就调用并 `return`（`handler.go:130-133`，参数是**整个 body 字符串**）；
5. 再走内置 switch；未命中但落在 `knownCMDMap` 白名单里则**静默丢弃**，不在白名单则打 debug 日志（`handler.go:189-193`）。

这就是 UniBarrage 的扩展原语（`RegisterCustomEventHandler`，`handler.go:80-82`；用例见 `UniBarrage/bilibili/bilibili.go:289-292,299-302`）。**Rust 版应保留「未知 cmd 透传原始 JSON」的通道。**

### 5.2 已解析的 cmd（逐个）

#### 5.2.1 `DANMU_MSG` —— 普通弹幕

解析：`message/danmu.go:86-142`；补字段：`UniBarrage/bilibili/extractor.go:101-130`。

| 语义 | JSON 路径 | 源码类型 | 出处 |
| --- | --- | --- | --- |
| 弹幕文本 | `info[1]` | string | danmu.go:104 |
| 发送者 uid | `info[2][0]` | int | danmu.go:107 |
| 昵称 | `info[2][1]` | string | danmu.go:108 |
| 是否房管 | `info[2][2]` | bool | danmu.go:110 |
| urank（老字段） | `info[2][5]` | int | danmu.go:111 |
| 是否绑定手机 | `info[2][6]` | bool | danmu.go:112 |
| **用户等级** | `info[2][16][0]` | int（**嵌在第二层数组里**） | danmu.go:109 |
| **发送者舰队等级** | `info[7]` | int（0 无 / 1 总督 / 2 提督 / 3 舰长） | danmu.go:113 |
| 勋章等级 | `info[3][0]` | int | danmu.go:116 |
| 勋章名 | `info[3][1]` | string | danmu.go:117 |
| 勋章主播昵称 | `info[3][2]` | string | danmu.go:118 |
| 勋章主播房间号 | `info[3][3]` | int | danmu.go:119 |
| **勋章主色** | `info[3][4]` | int（十进制 RGB，6067854 → `#5C968E`） | danmu.go:120 / extractor.go:60-68 |
| 勋章主播 uid | `info[3][12]` | int | danmu.go:121 |
| 勋章描边色 | `info[3][7]` | string（`#RRGGBB`） | extractor.go:109 / extractor_test.go:59 |
| 勋章渐变起色 | `info[3][8]` | string | extractor.go:107 |
| 勋章渐变止色 | `info[3][9]` | string | extractor.go:108 |
| 勋章文字色 | `info[3][10]` | string | extractor.go:110 |
| 弹幕类型 | `info[0][12]` | int（0 文本 / 1 表情，danmu.go:11-14） | danmu.go:126 |
| 发送时间戳 | `info[0][4]` | int64（秒） | danmu.go:127 |
| 扩展字段 | `info[0][15].extra` | **JSON 字符串**（需二次 `serde_json::from_str`） | danmu.go:94 |
| 表情 | `info[0][13]` | **JSON 对象被字符串化** | danmu.go:98 |
| 表情图 URL | `info[0][13].url` | string | danmu.go:72 |
| 表情唯一名 | `info[0][13].emoticon_unique` | string | danmu.go:68 |
| dm 字符串 id | `info[0][15].extra.id_str` | string | danmu.go:55 |
| **`dm_v2`** | 根级 `dm_v2`（**与 `info` 同级**，不是 info 子字段） | base64 字符串 → protobuf `pb.Dm` | danmu.go:130-140 |
| dm_v2 文本 | `dm_v2` → Dm.content（proto 字段 6） | string，**会覆盖 `info[1]`** | danmu.go:139 / dmv2.proto:123 |
| dm_v2 字符串 id | `dm_v2` → Dm.id_str（proto 字段 1） | string | dmv2.proto:118 |

`info[3]` 的完整下标含义（源码注释 + 测试固定下来）：`[level, name, anchor_uname, anchor_roomid, medal_color, special, icon_id, color_border, color_start, color_end, color_text, guard_level, anchor_uid]`（extractor.go:98-99；测试 raw 见 extractor_test.go:49-51）。

**坑（全部来自源码）：**

1. **勋章指针永不为 null**：`d.Sender.Medal = &Medal{...}` 无条件构造（danmu.go:115-122），未佩戴时是全 0 结构体 → 判「有没有勋章」必须用 `name == "" || level <= 0`（extractor.go:73-75），不能判 nil。
2. **`info[3]` 可能缺失或长度不足 11**：extractor 先检查 `IsArray() && len >= 11` 才取色（extractor.go:103-105）。
3. **颜色类型不统一**：`DANMU_MSG` 的 `medal_color` 是 int，`SUPER_CHAT_MESSAGE` 的 `medal_color` 是 string（extractor.go:76-83 注释 + extractor_test.go:37-45），字符串还可能是 `"#FF86B2"` 形式（extractor.go:133-149）。Rust 统一 `enum ColorRaw { Int(i64), Str(String) }`。
4. **数字可能是字符串、字段可能缺失**：Go 侧全走 gjson `.Int()/.String()/.Bool()`（类型不符/缺失一律零值）→ Rust 必须全 `Option`，不要 `unwrap`。
5. **`info[2]` 是数组**不是对象，不能写 `info[2].uname`。
6. **`dm_v2` 会覆盖文本**：protobuf 解成功则 `d.Content` 变成 dm_v2 的 content（danmu.go:139）；解失败保留 `info[1]`（danmu.go:135-138 直接 return）。Rust 建议**只用 `info[1]` 显示、dm_v2 仅取 `id_str` 去重**（两者是否等价：**未验证**）。
7. `info[0][15]` 不存在时 extra 解析失败只打日志（danmu.go:94-97），不影响其它字段。

```rust
pub struct Chat {
    pub uid: i64, pub uname: String, pub text: String, pub user_level: i64,
    pub guard_level: i64, pub ts: i64, pub is_admin: bool,
    pub medal: Option<Medal>, pub emoticon: Option<String>, pub id_str: Option<String>,
}

pub fn parse_danmu(v: &serde_json::Value) -> Option<Chat> {
    let info = v.get("info")?.as_array()?;
    let i2 = info.get(2).and_then(|x| x.as_array());          // 可能为 null
    let i3 = info.get(3).and_then(|x| x.as_array());
    let medal = i3.and_then(|a| {
        if a.len() < 11 { return None; }
        let name = a[1].as_str()?;
        let level = a[0].as_i64()?;
        if name.is_empty() || level <= 0 { return None; }      // extractor.go:73-75
        Some(Medal {
            name: name.into(), level,
            color: a[4].as_i64().unwrap_or(0),
            border: a.get(7).and_then(|x| x.as_str()).map(Into::into),
            start:  a.get(8).and_then(|x| x.as_str()).map(Into::into),
            end:    a.get(9).and_then(|x| x.as_str()).map(Into::into),
            text:   a.get(10).and_then(|x| x.as_str()).map(Into::into),
        })
    });
    Some(Chat {
        uid:   i2.and_then(|a| a.first()).and_then(|x| x.as_i64()).unwrap_or(0),
        uname: i2.and_then(|a| a.get(1)).and_then(|x| x.as_str()).unwrap_or("").to_string(),
        text:  info.get(1).and_then(|x| x.as_str()).unwrap_or("").to_string(),
        user_level: i2.and_then(|a| a.get(16)).and_then(|x| x.get(0))
                       .and_then(|x| x.as_i64()).unwrap_or(0),
        guard_level: info.get(7).and_then(|x| x.as_i64()).unwrap_or(0),
        ts: info.first().and_then(|x| x.get(4)).and_then(|x| x.as_i64()).unwrap_or(0),
        is_admin: i2.and_then(|a| a.get(2)).and_then(|x| x.as_bool()).unwrap_or(false),
        medal,
        emoticon: info.first().and_then(|x| x.get(13)).and_then(|x| x.get("url"))
                      .and_then(|x| x.as_str()).map(Into::into),
        id_str: v.get("dm_v2").and_then(|s| s.as_str())
                 .and_then(|s| BASE64.decode(s).ok())
                 .and_then(|b| pb::Dm::decode(&*b).ok())
                 .map(|d| d.id_str).filter(|s| !s.is_empty()),
    })
}
```

#### 5.2.2 `SEND_GIFT` —— 礼物

解析：`message/gift.go:115-122`（取 `data` 整块反序列化）。消费方：`UniBarrage/bilibili/bilibili.go:87-108`。

| 语义 | JSON 路径 | 类型 | 出处 |
| --- | --- | --- | --- |
| 动作 | `data.action` | string（如 `"投喂"`） | gift.go:11 |
| 送礼人 uid | `data.uid` | int | gift.go:73 |
| 送礼人昵称 | `data.uname` | string | gift.go:74 |
| 送礼人头像 | `data.face` | string | gift.go:30 |
| 礼物 id | `data.giftId` | int（**camelCase**） | gift.go:32 |
| 礼物名 | `data.giftName` | string（**camelCase**） | gift.go:33 |
| 礼物类型 | `data.giftType` | int | gift.go:34 |
| 数量 | `data.num` | int | gift.go:56 |
| 单价 | `data.price` | int（金瓜子；UniBarrage 用 `num*price/1000` 换算成元，bilibili.go:101） | gift.go:58 |
| 总价 | `data.total_coin` | int | gift.go:72 |
| 币种 | `data.coin_type` | string | gift.go:18 |
| 折扣价 | `data.discount_price` | int | gift.go:25 |
| 实际花费 | `data.rcost` | int | gift.go:59 |
| 送礼人舰队 | `data.guard_level` | int | gift.go:36 |
| 昵称颜色 | `data.name_color` | string（`#RRGGBB`） | gift.go:55 |
| 勋章名 / 等级 | `data.medal_info.medal_name` / `medal_level` | string / int | gift.go:51,50 |
| 勋章色 | `data.medal_info.medal_color` / `medal_color_start` / `_end` / `_border` | **int**（与 SC 不同） | gift.go:46-49 |
| 勋章主播 | `data.medal_info.anchor_uname` / `anchor_roomid` | string / int | gift.go:41-42 |
| 盲盒 | `data.blind_gift` | object（源码 `interface{}`） | gift.go:16 |
| 连击批次 | `data.batch_combo_id` | string | gift.go:12 |
| 连击信息 | `data.combo_send` / `data.batch_combo_send` | object | gift.go:20,13 |
| 连击总币 / 停留 | `data.combo_total_coin` / `combo_stay_time` | int | gift.go:22,21 |
| 超级礼物数 | `data.super_gift_num` / `super_batch_gift_num` | int | gift.go:66,65 |
| 是否首赠 | `data.is_first` | bool | gift.go:37 |
| 放大倍数 | `data.magnification` | float64 | gift.go:39 |
| 时间戳 | `data.timestamp` | int（秒） | gift.go:70 |
| tid | `data.tid` | **string** | gift.go:69 |
| 剩余 / 随机数 | `data.remain` / `data.rnd` | int / string | gift.go:60,61 |

坑：

- **命名风格混用**：`giftId/giftName/giftType` 驼峰，其余下划线；写成 `gift_id` 拿不到。
- **连击不会自己累加**：单条 `SEND_GIFT` 的 `num` 是这批数量，`batch_combo_id` 用于聚合（gift.go:12）；`COMBO_SEND` 是**另一条 cmd 且未解析**（见 §5.4）。是否按 `batch_combo_id` 合并：**未验证**（源码逐条广播，bilibili.go:96-107）。
- **盲盒礼物**：`giftName` / `price` 可能不是观众看到的那个（源码不处理 `blind_gift` 内容）→ 显示口径**未验证**。
- 礼物图标不来自消息：UniBarrage 用 `giftConfig` 建「名字→图标」表（`bilibili/gifts/gifts.go:35-84`，接口见 §1.1）。

#### 5.2.3 `SUPER_CHAT_MESSAGE` —— 醒目留言

解析：`message/superchat.go:73-80`（取 `data`）。消费方：`bilibili.go:140-167`。

| 语义 | JSON 路径 | 类型 | 出处 |
| --- | --- | --- | --- |
| 消息 id | `data.id` | int（**删除时按它匹配**） | superchat.go:28 |
| 内容 | `data.message` | string | superchat.go:46 |
| 金额 | `data.price` | int（**单位元**，源码直接 `float64(price)`，bilibili.go:152） | superchat.go:49 |
| 发送者 uid | `data.uid` | int | superchat.go:56 |
| 昵称 | `data.user_info.uname` | string | superchat.go:68 |
| 头像 | `data.user_info.face` | string | superchat.go:58 |
| 用户等级 | `data.user_info.user_level` | int | superchat.go:69 |
| 舰队等级 | `data.user_info.guard_level` | int | superchat.go:60 |
| 昵称色 | `data.user_info.name_color` | string | superchat.go:66 |
| 勋章名 / 等级 | `data.medal_info.medal_name` / `medal_level` | string / int | superchat.go:42,41 |
| 勋章色 | `data.medal_info.medal_color` | **string**（坑） | superchat.go:37 |
| 卡片渐变起/止 | `data.background_color_start` / `background_color_end` | string（`#RRGGBB`） | superchat.go:16,15 |
| 金额文字色 | `data.background_price_color` | string | superchat.go:19 |
| 持续时长 | `data.time` | int（秒） | superchat.go:52 |
| 起止时间 | `data.start_time` / `end_time` | int（秒） | superchat.go:51,22 |
| 翻译文本 / 标记 | `data.message_trans` / `trans_mark` | string / int | superchat.go:48,54 |
| 审核中 | `data.is_send_audit` | int | superchat.go:30 |
| 价格倍率 | `data.rate` | int | superchat.go:50 |
| 令牌 | `data.token` | string | superchat.go:53 |
| 占位礼物 | `data.gift.gift_id` / `gift_name` / `num` | int / string / int | superchat.go:23-27 |

坑：`price` 是整数元（不是金瓜子）；`medal_color` 是字符串；`SUPER_CHAT_MESSAGE_JPN` 只进白名单不解析（`handler.go:21`）。

#### 5.2.4 `SUPER_CHAT_MESSAGE_DELETE` —— 撤回 SC

解析：`message/superchat.go:83-97`。

| 语义 | JSON 路径 | 类型 | 出处 |
| --- | --- | --- | --- |
| 被删的 SC id 列表 | `data.ids` | **数组 int** | superchat.go:88 |

坑：`data.ids` 非数组时只打日志、`Ids` 为空（superchat.go:89-92）；UniBarrage **没有**订阅 `OnSuperChatDelete`（对比 `bilibili.go:263-271` 的 handler 表）→ 删除事件在业务侧被丢弃。Rust 侧要自己按 `id` 移除 SC 卡片。

#### 5.2.5 `GUARD_BUY` —— 上舰

解析：`message/guard.go:22-29`。消费方：`bilibili.go:111-137`。

| 语义 | JSON 路径 | 类型 | 出处 |
| --- | --- | --- | --- |
| uid | `data.uid` | int | guard.go:11 |
| 昵称 | `data.username` | string（**不是 `uname`**） | guard.go:12 |
| 舰队等级 | `data.guard_level` | int（1 总督 / 2 提督 / 3 舰长） | guard.go:13 |
| 数量 | `data.num` | int | guard.go:14 |
| 价格 | `data.price` | int（金瓜子，`/1000` = 元，bilibili.go:128） | guard.go:15 |
| 礼物 id / 名 | `data.gift_id` / `gift_name` | int / string | guard.go:16-17 |
| 开始 / 结束 | `data.start_time` / `end_time` | int（秒） | guard.go:18-19 |

坑：

- 字段是 `username`（`data.username`），写 `uname` 为空。
- 等级枚举反直觉：**3 = 舰长**、1 = 总督（`extractor.go:186-196`）。
- 头像不在消息里 → UniBarrage 额外打一次 `card`（`bilibili.go:113-117`）；Rust 建议懒加载 + 缓存。
- **与 `USER_TOAST_MSG_V2` 是同一次购买的两条消息**（见下条），两条都播会重复。

#### 5.2.6 `USER_TOAST_MSG_V2` —— 上舰/续费提示条

解析：`message/toast.go:24-41`，字段全在 `data` 下且层级很深：

| 语义 | JSON 路径 | 类型 | 出处 |
| --- | --- | --- | --- |
| uid | `data.sender_uinfo.uid` | int | toast.go:30 |
| 昵称 | `data.sender_uinfo.base.name` | string | toast.go:31 |
| 舰队等级 | `data.guard_info.guard_level` | int | toast.go:32 |
| 开始 / 结束 | `data.guard_info.start_time` / `end_time` | int | toast.go:37-38 |
| 月数/数量 | `data.pay_info.num` | int | toast.go:33 |
| 价格 | `data.pay_info.price` | int（金瓜子） | toast.go:34 |
| 单位 | `data.pay_info.unit` | string（如 `"月"`） | toast.go:35 |
| 礼物 id | `data.gift_info.gift_id` | int | toast.go:36 |
| 来源 | `data.option.source` | int（0 付费 / 2 赠送或转让，toast.go:19） | toast.go:39 |
| 提示文案 | `data.toast_msg` | string | toast.go:40 |

测试样例（可直接当 fixture）：`message/interact_test.go:36`。

坑：`data` 缺失时日志报错并返回零值（toast.go:26-29）；`sender_uinfo.base.name` 缺失 → 昵称为空，需回落 uid；**UniBarrage 根本没订阅** `OnUserToast`（`bilibili.go:263-271`），所以这条在现网输出里不存在。

#### 5.2.7 `INTERACT_WORD_V2` —— 进房/关注/分享/互粉/点赞

解析：`message/interact.go:31-56`。**不是 JSON 字段，是 base64 protobuf**：

| 语义 | JSON 路径 | 类型 | 出处 |
| --- | --- | --- | --- |
| 原始数据 | `data.pb` | base64 string → `pb.InteractWordV2` | interact.go:32-46 |
| uid | proto 字段 1（int64） | int64 | dmv2.proto:153 |
| uname | proto 字段 2 | string | dmv2.proto:154 |
| msg_type | proto 字段 5（int32） | int | dmv2.proto:155 |
| timestamp | proto 字段 7（int64） | int64 | dmv2.proto:156 |
| 头像 | proto 22 → 2 → 2（`uinfo.base.face`） | string | dmv2.proto:144-150 / interact.go:51-55 |

`msg_type` 取值（`interact.go:12-20`，与 `bilibili.go:204` 注释一致）：

| 值 | 含义 | 常量 |
| --- | --- | --- |
| 1 | 进入直播间 | `InteractMsgTypeEnter` |
| 2 | 关注主播 | `InteractMsgTypeFollow` |
| 3 | 分享直播间 | `InteractMsgTypeShare` |
| 4 | 特别关注 | `InteractMsgTypeSpecialFollow` |
| 5 | 互粉 | `InteractMsgTypeMutualFollow` |
| 6 | 点赞 | `InteractMsgTypeLike` |

坑：

- `data.pb` 缺失或解码失败 → 只打日志、结构体全 0（interact.go:33-46），但 handler **仍会被调用**（`handler.go:183-188`）→ 上层必须丢掉 `msg_type == 0` 的包（UniBarrage 靠 `!= InteractMsgTypeEnter` 过滤，bilibili.go:205-220）。
- 老格式 `INTERACT_WORD`（纯 JSON）**在白名单里但不解析**（handler.go:52 注释「legacy CMDs still appear occasionally; silence without handlers」）→ 老格式进房事件在 Go 版彻底丢失。
- 该消息**没有消息 id**（interact.go:23-29）→ 去重只能靠 `(uid, msg_type, timestamp)`。

#### 5.2.8 `LIVE` —— 开播

解析：`message/live.go:42-47`，是**整包反序列化**，字段在**根级**（不是 `data`）：

| 语义 | JSON 路径 | 类型 | 出处 |
| --- | --- | --- | --- |
| cmd | `cmd` | string | live.go:15 |
| 直播 key | `live_key` | string | live.go:16 |
| 语音背景 | `voice_background` | string | live.go:17 |
| 子会话 key | `sub_session_key` | string | live.go:18 |
| 直播平台 | `live_platform` | string | live.go:19 |
| 直播模式 | `live_model` | int | live.go:20 |
| 开播时长 | `live_time` | int | live.go:21 |
| 房间号 | `roomid` | **int** | live.go:22 |

坑：UniBarrage **没有** `OnLiveStart` 订阅（handler 表 `bilibili.go:263-271`）→「开播」不产生任何对外消息；`roomid` 在根级，写 `data.roomid` 拿不到。

#### 5.2.9 `PREPARING` —— 下播/轮播

两套实现，**UniBarrage 用自己的那套**：

- 库内 `message.LiveStop`（`live.go:26-34,48-53`，整包解析）：`cmd`、`msg_id`(string)、`p_is_ack`(bool)、`p_msg_type`(int)、`roomid`(**string**)、`round`(int，1 轮播中/0 未轮播)、`send_time`(int64)。
- UniBarrage 侧：注册**自定义 handler**，先 `gjson.Get(s, "data")` 取 `data` 再反序列化到 `message.Preparing{Cmd, Roomid string}`（`bilibili.go:299-302,242-255`；结构见 `live.go:37-40`）→ 因自定义 handler 优先（`handler.go:130-133`），库内 `OnLiveStop` 永不触发。

坑：自定义 handler 收到**整个 body 字符串**，`data` 可能是对象也可能是被转义过的 JSON 字符串 —— `gjson .String()` 两种都能拿到可反序列化文本，Rust 侧要兼容两种。

```rust
pub fn parse_preparing(body: &serde_json::Value) -> Option<String> {
    let data = body.get("data")?;
    // 兼容 data 为对象 / 被转义的 JSON 字符串两种形态
    let obj: serde_json::Value = match data {
        serde_json::Value::String(s) => serde_json::from_str(s).ok()?,
        v => v.clone(),
    };
    obj.get("roomid")?.as_str().map(|s| s.to_string())   // 字符串
}
```

### 5.3 UniBarrage 额外接的两条（自定义 handler）

#### 5.3.1 `LIKE_INFO_V3_CLICK` —— 点赞

`bilibili.go:289-292` 注册，先 `gjson.Get(s, "data")` 再交给 `handleLike`（`bilibili.go:170-196`）：

| 语义 | JSON 路径 | 类型 | 出处 |
| --- | --- | --- | --- |
| 昵称 | `data.uname` | string | bilibili.go:175 |
| uid | `data.uid` | int | bilibili.go:176 |
| 头像 | `data.uinfo.base.face` | string（空则回落 `card` 接口） | bilibili.go:178-182 |

坑：计数字段被**硬编码为 1**（`bilibili.go:190`）；消息里是否带累计点赞数：**未验证**。`LIKE_INFO_V3_UPDATE` 只在白名单（`handler.go:50`）。

#### 5.3.2 `PREPARING`
见 §5.2.9。

### 5.4 用户点名、但本源码**未解析**的 cmd

下表每条都能在 `knownCMD` 里找到（`client/handler.go:15-53`），但 `Handle` 的 switch（`handler.go:134-194`）没有对应分支，也**没有任何 struct 定义**（除注明者）。字段结构一律 **【未验证】**：实现前请先抓一段原始 JSON 再补表。

| cmd | 白名单位置 | 源码状态 | 建议 |
| --- | --- | --- | --- |
| `COMBO_SEND` | handler.go:36 | 未解析、无 struct | 连击聚合；与 `SEND_GIFT.batch_combo_id` 的关联**未验证** |
| `ENTRY_EFFECT` | handler.go:28 | 未解析、无 struct | 进场特效（舰长进场）；另有 `ENTRY_EFFECT_MUST_RECEIVE`（:42） |
| `NOTICE_MSG` | handler.go:31 | 未解析、无 struct | 全区/房间广播 |
| `ONLINE_RANK_COUNT` | handler.go:27 | 未解析；仅有 `OnlineRankCount{Count int}`（`message/utils.go:72-74`，未绑定） | 高能榜人数；字段路径**未验证** |
| `ONLINE_RANK_V2` | handler.go:21 | 未解析、无 struct | 高能榜前列 |
| `ONLINE_RANK_TOP3` | handler.go:25 | 未解析 | — |
| `WATCHED_CHANGE` | handler.go:49 | 未解析、无 struct | 「x 人看过」 |
| `ROOM_REAL_TIME_MESSAGE_UPDATE` | handler.go:32 | 未解析、无 struct | 粉丝数/粉丝团数实时变化 |
| `STOP_LIVE_ROOM_LIST` | handler.go:29 | 未解析；`StopLiveRoomList{RoomIdList []int}`（`live.go:9-11`，未绑定） | 批量停播列表 —— 注意**没有** `STOP_LIVE_ROOM` 这个 cmd |
| `INTERACT_WORD` | handler.go:52 | 未解析（legacy） | 老格式互动 |
| `USER_TOAST_MSG` | handler.go:52 | 未解析（legacy） | 老格式上舰提示 |
| `SUPER_CHAT_MESSAGE_JPN` | handler.go:21 | 未解析 | 日文翻译版 SC |
| `DANMU_AGGREGATION` | handler.go:49 | 未解析 | 聚合弹幕 |
| `COMMON_NOTICE_DANMAKU` | handler.go:27 | 未解析；`CommonNoticeDanmaku` struct 存在（`danmu.go:75-83`，未绑定） | 系统提示弹幕 |
| `WELCOME` / `WELCOME_GUARD` | handler.go:22 / handler.go:19 | 未解析 | 欢迎 / 欢迎舰长 |
| `ROOM_KICKOUT` | handler.go:23 | 未解析 | **房间内**被踢（与 WS 连接被踢无关） |
| `ROOM_BLOCK_MSG` / `ROOM_BLOCK_INTO` | handler.go:23,28 | 未解析 | 禁言 |
| `ROOM_CHANGE` | handler.go:32 | 未解析 | 标题/分区变更 |
| `ROOM_RANK` | handler.go:35 | 未解析 | 房间排行 |
| `SUPER_CHAT_ENTRANCE` | handler.go:23 | 未解析 | SC 入场动画 |
| `HOT_RANK_SETTLEMENT` / `HOT_RANK_CHANGED` / `HOT_RANK_SETTLEMENT_V2` / `HOT_RANK_CHANGED_V2` | handler.go:19,28,37 | 未解析；struct 存在（`utils.go:30-70`，未绑定） | 热门榜 |
| `LIVE_INTERACTIVE_GAME` | handler.go:30 | 未解析；struct 存在（`utils.go:76-97`，未绑定） | 互动玩法 |
| `WIDGET_BANNER` | handler.go:25 | 未解析；struct 存在（`utils.go:5-28`，未绑定） | 挂件横幅 |

【结构体】标注的几条可拿 struct tag 当字段路径的**作者意图**参考，但没跑过，不能当规格。

**白名单全量清单**：`client/handler.go:15-53` 共 100+ 项，逐字抄进 Rust 的 `KNOWN_CMDS: &[&str]` 即可；不在表里的 cmd 建议打 debug 日志并丢弃（源码行为，handler.go:189-193）。

### 5.5 输出映射（若要复刻 UniBarrage 的对外契约）

`universal/type.go:24-32` 只有 7 种消息类型：`Chat / Gift / Subscribe / SuperChat / Like / EnterRoom / EndLive`，`CreateUniMessage` 会校验平台与类型（`type.go:245-261`）。映射关系：

| cmd | uni type | 出处 |
| --- | --- | --- |
| `DANMU_MSG` | Chat（带 `UserMeta`：uid/等级/舰队/昵称色/勋章） | bilibili.go:70-83；type.go:71-78 |
| `SEND_GIFT` | Gift（price = `num*price/1000`） | bilibili.go:92-107 |
| `GUARD_BUY` | Subscribe（`Item = ExtractGuardLevel`） | bilibili.go:119-136 |
| `SUPER_CHAT_MESSAGE` | SuperChat（含背景渐变三色） | bilibili.go:144-166 |
| `LIKE_INFO_V3_CLICK` / `INTERACT_WORD_V2(msg_type=6)` | Like（Count 固定 1） | bilibili.go:184-195,207-218 |
| `INTERACT_WORD_V2(msg_type=1)` | EnterRoom | bilibili.go:228-238 |
| `PREPARING` | EndLive | bilibili.go:246-254 |
| `LIVE`、`USER_TOAST_MSG_V2`、`SUPER_CHAT_MESSAGE_DELETE` | **无**（未订阅） | bilibili.go:263-302 |

`UserMeta` 字段：`uid / user_level / guard_level / name_color / medal{name,level,color_start,color_end,color_border,color_text,guard_level,is_light}`（`type.go:48-66`）。注意 `medal.guard_level` 取的是**发送者**的舰队等级而非勋章主播的（`extractor.go:161-163` 注释与实现）。

---

## 6. 断线重连 / 心跳超时 / 被踢

### 6.1 源码现状（照抄会踩的坑）

| 场景 | 源码行为 | 出处 | 评价 |
| --- | --- | --- | --- |
| 读错误（连接断） | 打日志 → 睡 **3 毫秒** → `connect()` → 继续循环 | `client.go:135-140` | 【缺陷】3ms 显然是想要 3s 的笔误 |
| 拨号失败 | `time.Sleep(2s)` → `goto retry`（**无限重试、无上限、无退避**） | `client.go:112-116` | 【缺陷】 |
| host 选择 | `hostList[retryCount % len]`，每次尝试自增 | `client.go:109-110` | 轮换可用，但无失败计数上限 |
| 重连后认证 | 每次 `connect()` 都重发 auth 包（**旧 token**） | `client.go:119,196-205` | 【缺陷】token 可能已过期 |
| 心跳超时 | **完全不检测**：心跳只发不收，无看门狗 | `client.go:152-167` | 【缺陷】半死 TCP 会一直挂着 |
| 认证失败 | 忽略 `op=8` 的 body（空 case） | `handler.go:196` | 【缺陷】静默 |
| 心跳回复 `code != 0`（token 失效等） | 忽略 `op=3` 的 body（空 case） | `handler.go:195` | 【缺陷】静默 |
| 服务端主动断开 | 走「读错误」路径 | `client.go:134-140` | 会重连，但不刷新 token/host |
| 主动停止 | `Stop()` → `cancel()`；心跳循环退出，读循环**要等下一次 read 返回**才看到 `done` | `client.go:183-185,127-133` | 【缺陷】卡在 `ReadMessage` 时不退出 |
| 并发 | `wsLoop` 读 `c.conn` 无锁，`connect()` 会替换它；心跳写有锁 | `client.go:134,117,159-163` | 【缺陷】数据竞争 |

### 6.2 建议的 Rust 实现要点（在源码事实之上补齐）

```rust
enum Phase { Resolve, Connect, Authed, Backoff { attempt: u32 } }

// 1) 认证成功判定：op=8 且 body.code == 0
fn on_packet(pkt: Packet, st: &mut State) -> Result<()> {
    match pkt.operation {
        3 => {                                       // HeartBeatResponse
            st.last_inbound = Instant::now();
            let code = serde_json::from_slice::<Ack>(&pkt.body).map(|a| a.code).unwrap_or(0);
            if code != 0 { bail!("heartbeat code={code}"); }        // 视为被踢
        }
        8 => {                                       // RoomEnterResponse
            let code = serde_json::from_slice::<Ack>(&pkt.body).map(|a| a.code).unwrap_or(0);
            if code != 0 { bail!("auth rejected code={code}"); }
            st.phase = Phase::Authed;
        }
        5 => { st.last_inbound = Instant::now(); dispatch(pkt)?; }
        _ => {}
    }
    Ok(())
}

// 2) 看门狗（源码没有，必须自己加）：超过 ~2 个心跳周期没有任何入站包 → 判死重连
//    心跳 30s（client.go:158），阈值取 60~75s
tokio::spawn(async move {
    loop {
        sleep(Duration::from_secs(10)).await;
        if st.last_inbound.elapsed() > Duration::from_secs(70) {
            let _ = session_tx.send(SessionEvent::Stale).await;   // → 关连接、重连
        }
    }
});

// 3) 重连必须重新走 HTTP：room_init（可选）→ getDanmuInfo（新 token + host_list）
//    源码只在 init 取一次 token（client.go:86-97），这是"连上但没弹幕"的常见原因
// 4) 退避：1s, 2s, 4s, 8s, 16s, 30s(封顶)；区分原因
//    - 网络/IO 错误            → 退避重连
//    - auth code != 0 / 心跳 code == -101 → 立刻重取 token 再连（不等待长退避）
```

【未验证】`code` 的完整枚举（除 `0` 成功、`-101` 未登录见于 `sign.go:144` 的注释之外）。源码从不读取这些 code，无法从源码确认。

---

## 7. 弹幕长度 / 频率限制 / 去重

### 7.1 发送接口（唯一相关源码）

`blivedm-go/api/send.go:40-71`：

```
POST https://api.live.bilibili.com/msg/send
Content-Type: application/x-www-form-urlencoded
Cookie: bili_jct={csrf};SESSDATA={sessdata}
form: bubble, color, fontsize, mode, msg, roomid, csrf, csrf_token, rnd, [dm_type]
```

- `csrf` 与 `csrf_token` 同值，都取 Cookie 里的 `bili_jct`（`send.go:50-51,63`）。
- `SendDefaultDanmaku` 默认值：`bubble=0, color=16777215, fontsize=25, mode=1, dm_type=1`（`send.go:73-84`）。
- **【缺陷/矛盾】** `send.go:54` 注释写「`dm_type` 为 1 时发送的是表情弹幕」，而 `SendDefaultDanmaku`（默认纯文本路径）却填 `dm_type=1`（`send.go:81`）。Rust 发纯文本建议**不传 `dm_type`**，但该行为**未验证**。
- 返回：`code`（非 0 即失败）、`data.mode_info.{mode,show_player_type,extra}`、`message` / `msg`（`send.go:21-32`）。

### 7.2 长度 / 频率

- **UniBarrage 与 blivedm-go 都没有任何长度或频率校验**：`bilibili/` 目录只有 `bilibili.go / fetcher.go / extractor.go / extractor_test.go / gifts/`（`ls` 已确认），`api/send.go` 也没有 `len(msg)` 判断。
- 因此「多少字、多久一条」在本源码里**没有事实依据** → **【未验证】**。以 B 站返回的错误码为准。
- 接收侧同理：源码对入站消息**不做任何限流/截断**。

### 7.3 消息去重

源码里**没有任何去重逻辑**（无 msg_id 去重、无 LRU、无「已见集合」）。可用的去重键（按可靠度排序）：

| 场景 | 可用键 | 出处 | 备注 |
| --- | --- | --- | --- |
| 弹幕 | `dm_v2` → `Dm.id_str`（proto 字段 1） | `message/danmu.go:130-140`、`pb/dmv2.proto:118` | 最稳；但 `dm_v2` 可能缺失 |
| 弹幕 | `info[0][15].extra.id_str` | `danmu.go:55` | 与上面同源，二选一 |
| 弹幕 | `(uid, info[0][4], info[1])` 组合 | `danmu.go:107,127,104` | 兜底；同人同秒同文会误判（概率极低） |
| 礼物 | `data.batch_combo_id`（+ `num`） | `gift.go:12,56` | 连击聚合用 |
| 礼物 | `data.tid`（string） | `gift.go:69` | 是否唯一：**未验证** |
| SC | `data.id` | `superchat.go:28` | 删除事件也用它（`data.ids`） |
| 上舰 | `(uid, guard_level, start_time)` | `guard.go:11-18`、`toast.go:30-38` | **同一动作两条消息**（GUARD_BUY 与 USER_TOAST_MSG_V2），必须合并或只取其一 |
| 进房/点赞 | `(uid, msg_type, timestamp)` | `interact.go:47-50` | 该消息无 id |
| 点赞 | `(uid, ...)` | `bilibili.go:175-176` | 源码只取 uname/uid，timestamp **未验证** |

**顺序问题（比去重更致命）**：源码对每个包、每个 handler 都开 goroutine（`client.go:146`、`handler.go:131,139`）→ 同一批数据内消息可能乱序。Rust 侧应保证：单连接内**串行**按到达顺序派发。

**自回显去重**（若做发送）：自己发的弹幕会被服务端回推，用 `dm_v2` → `Dm.user.uid`（proto 字段 20，`dmv2.proto:137`）或 `info[2][0]` 与自己的 uid 比较即可识别（源码未实现，属建议）。

---

## 8. 附录 A：类型/缺失陷阱速查

| # | 陷阱 | 证据 |
| --- | --- | --- |
| 1 | `info[2]` / `info[3]` 是**数组**不是对象 | danmu.go:102-122 |
| 2 | 用户等级在 `info[2][16][0]`（两层嵌套） | danmu.go:109 |
| 3 | `info[3][4]` 是 int，SC 的 `medal_info.medal_color` 是 string | danmu.go:120 / superchat.go:37 / extractor.go:76-83 |
| 4 | 勋章色还可能是 `"#RRGGBB"` 字符串 | extractor.go:133-149 |
| 5 | 未佩戴勋章时 `Medal` 指针存在但全 0 | danmu.go:115-122 / extractor.go:73-75 |
| 6 | `info[3]` 长度可能 < 11 | extractor.go:103-105 |
| 7 | 礼物字段用驼峰 `giftId/giftName/giftType` | gift.go:32-34 |
| 8 | 上舰昵称字段是 `data.username` | guard.go:12 |
| 9 | 舰队枚举 1 总督 / 2 提督 / 3 舰长（3 是舰长） | extractor.go:186-196 |
| 10 | `LIVE` / 库内 `LiveStop` 字段在**根级** | live.go:14-34 |
| 11 | `info[0][15].extra` / `info[0][13]` 是**被字符串化的 JSON**，要二次解析 | danmu.go:94,98 |
| 12 | `INTERACT_WORD_V2` 正文在 `data.pb`（base64 protobuf） | interact.go:32-46 |
| 13 | `dm_v2` 在**根级**且会覆盖 `info[1]` | danmu.go:130-140 |
| 14 | `data.tid` 是 string | gift.go:69 |
| 15 | `PREPARING.roomid` 是 string，`LIVE.roomid` 是 int | live.go:22,31,39 |
| 16 | 数字可能是字符串、字段可能缺失（gjson 全静默转 0/""） | handler.go:204-210 与各 `.Int()` 调用 |

## 9. 附录 B：实现清单（照做即可）

1. `room_init` 拿真实房间号 + 主播 uid + 开播状态（§1.3）。
2. `getDanmuInfo`（WBI 签名）拿 token + host_list（§2）。
3. `wss://{host}/sub`，握手只带 UA，连上立刻发 auth（op=7、包头 protover=1、body protover=3）（§3.1、§3.5）。
4. 解析 op=8 判认证成功；解析 op=3 判心跳健康（§6.2）。
5. 每 30s 发 16 字节空 body 心跳（§3.6）。
6. 收 op=5 → 摘 cmd（截 `:` 后缀）→ 已解析的走结构体，未知的透传 `raw` JSON（§5.1）。
7. protover 2/3 解压 → 切包 → 逐包处理，串行派发（§3.4、§7.3）。
8. 断线重连时**重新走 HTTP 取 token**，退避 1→30s，加 70s 看门狗（§6.2）。

## 10. 附录 C：工作区内另一实现的已验证房间信息接口（**不属于 UniBarrage**，仅作 §1.5 缺口的可用来源）

来源：`bili-dm/src/bilidm_overlay/roomstats.py`（Python，生产在用）。与 UniBarrage 无关，但字段路径有代码与注释背书：

| 需求 | 接口 | 字段 | 出处 |
| --- | --- | --- | --- |
| 真实房间号/主播 uid/开播状态/标题/分区/人气/粉丝数（可批量） | `GET https://api.live.bilibili.com/xlive/web-room/v1/index/getRoomBaseInfo`，query 为**重复的** `room_ids` + `req_biz=web_room_componet` | `data.by_room_ids[room_id].{room_id, short_id, uid, uname, title, parent_area_name, area_name, live_status, online, attention}` —— 依次为 真实号/短号/主播 uid/主播昵称/标题/一级分区/二级分区/开播状态/**人气（热度）**/**粉丝数** | roomstats.py:170-182、219-233（`online` 注释明确为「热度，不是在线人数」，roomstats.py:3-4,60,231） |
| 在线人数 | `GET https://api.live.bilibili.com/xlive/general-interface/v1/rank/getOnlineGoldRank?ruid={uid}&roomId={real_room}&page=1&pageSize=1` | `data.onlineNum`(int)、`data.onlineNumText`(string，如 `"13万+"`) | roomstats.py:184-196 |
| 请求头 | — | `User-Agent: Mozilla/5.0 (X11; Linux x86_64) bilidm-overlay`、`Referer: https://live.bilibili.com/` | roomstats.py:30-31,199 |
| 短号→真实号（发送侧） | `GET .../room/v1/Room/get_info?room_id={rid}` | `data.room_id` | sender.py:296-305 |

注意：`room_ids` **必须重复传参**，逗号拼接会返回 `-400`（roomstats.py:171 注释，2026-10 实测）。

## 11. 附录 D：未验证清单（实现时自己抓包/实测）

1. `getDanmuInfo` 的 `data.ttl` 单位与 token 实际过期时间（§2.3）。
2. host_list 是否可能返回非 443 端口（源码丢弃 `wss_port`，§2.4）。
3. 嵌套压缩（brotli 内含 zlib 包）是否存在（§3.4）。
4. `dm_v2` 与 `info[1]` 文本是否等价（§5.2.1 坑 6）。
5. 盲盒礼物 `giftName` / `price` 的展示口径（§5.2.2）。
6. `COMBO_SEND`、`ENTRY_EFFECT`、`NOTICE_MSG`、`ONLINE_RANK_*`、`WATCHED_CHANGE`、`ROOM_REAL_TIME_MESSAGE_UPDATE`、`STOP_LIVE_ROOM_LIST` 的**全部字段路径**（§5.4）。
7. `LIKE_INFO_V3_CLICK` 是否带累计点赞数（§5.3.1）。
8. 认证/心跳回复里 `code` 的完整枚举（§6.2）。
9. 弹幕长度上限、发送频率上限（§7.2）。
10. 不依赖 Cookie 的 buvid 获取接口（§4）。
11. `data.tid` 是否全局唯一（§7.3）。
12. 是否存在 `STOP_LIVE_ROOM`（无 `_LIST`）这个 cmd —— 源码里只有 `STOP_LIVE_ROOM_LIST`（handler.go:29）。
