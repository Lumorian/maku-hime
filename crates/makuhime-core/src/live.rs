//! 开播 / 停播：拿 B 站推流地址，交给 OBS 等推流软件。
//!
//! 走直播间 Web 接口（Cookie + csrf），与发送弹幕同一套登录凭据：
//! - POST /room/v1/Room/startLive   room_id + area_v2 + type=2 + csrf
//! - POST /room/v1/Room/stopLive    room_id + csrf
//!
//! 这两个接口按客户端投白名单：浏览器身份会被 60034（系统维护，仅支持直播姬开播）
//! 挡掉，因此参数按直播姬客户端的规则签名（appkey / build / version / ts + md5），
//! UA 也换成 LiveHime。协议细节对照 arcana-world 的实现。
//!
//! 推流密钥是敏感信息：只返回给本地前端，不写日志、不进快照。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use base64::Engine as _;
use hmac::{Hmac, Mac};
use md5::{Digest, Md5};
use sha2::Sha256;

use crate::error::{Error, Result};
use crate::http::HttpClient;
use crate::sender::Credentials;

const START_URL: &str = "https://api.live.bilibili.com/room/v1/Room/startLive";
const STOP_URL: &str = "https://api.live.bilibili.com/room/v1/Room/stopLive";

/// 直播姬客户端的 appkey / appsecret：开播参数要按这套值做 md5 签名。
pub(crate) const LIVEHIME_APPKEY: &str = "aae92bc66f3edfab";
const LIVEHIME_APPSECRET: &str = "af125a0d5279fd576c1b4418a3e8276d";
pub(crate) const LIVEHIME_BUILD: &str = "10783";
pub(crate) const LIVEHIME_VERSION: &str = "7.63.0.10783";

/// 换设备票据的接口；直播姬开播前会先拿一张塞进 Cookie。
const TICKET_URL: &str =
    "https://api.bilibili.com/bapis/bilibili.api.ticket.v1.Ticket/GenWebTicket";

/// 用 v_voucher 登记身份验证挑战，换出解密的 content。
const CHALLENGE_URL: &str = "https://api.bilibili.com/x/gaia-vgate/v2/register";

/// 一种可用的推流协议（RTMP / SRT）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StreamEndpoint {
    pub protocol: String,
    pub addr: String,
    /// 推流密钥；前端展示时需要遮挡
    pub code: String,
}

/// 开播结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveStream {
    pub room_id: u64,
    pub addr: String,
    pub code: String,
    /// 主推流协议（通常是 rtmp）
    pub protocol: String,
    /// 服务端返回的全部可用协议，含主协议
    pub protocols: Vec<StreamEndpoint>,
}

/// 开播请求的结果：拿到推流信息，或者被要求当场完成身份验证。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum StartOutcome {
    Stream { stream: LiveStream },
    Verify { verify: FaceChallenge },
}

/// 60024 / 60043：B 站要求这一次开播先做人脸 / 身份认证（不是"没实名"）。
///
/// 认证页链接由 B 站返回的 qr 或 risk_extra.v_voucher 换出来；换不出来时 url 为空，
/// 前端只能提示用户去 App 里认证一次再重试。
#[derive(Debug, Clone, Serialize)]
pub struct FaceChallenge {
    pub code: i64,
    pub message: String,
    pub url: Option<String>,
}

/// 开播：返回推流地址与密钥。area_id 传 0 时直接报错，避免开出没有分区的直播。
pub async fn start(
    http: &HttpClient,
    credentials: &Credentials,
    room_id: u64,
    area_id: u64,
) -> Result<StartOutcome> {
    if !credentials.is_valid() {
        return Err(Error::NotLoggedIn);
    }
    if area_id == 0 {
        return Err(Error::Live {
            code: -1,
            message: "这个直播间还没有分区：请先在 B 站直播设置里选好分区再开播".into(),
        });
    }
    http.set_cookie(credentials.cookie());
    let form = livehime_form(
        &[
            ("room_id", room_id.to_string()),
            ("area_v2", area_id.to_string()),
            ("type", "2".to_string()),
            ("csrf", credentials.bili_jct.clone()),
            ("csrf_token", credentials.bili_jct.clone()),
        ],
        now_secs(),
    );
    let value = http
        .post_form_as_livehime(
            START_URL,
            &form,
            &livehime_cookies(http, &credentials.bili_jct).await,
        )
        .await?;
    let code = value.get("code").and_then(Value::as_i64).unwrap_or(-1);
    if code == 60024 || code == 60043 {
        let url = resolve_face_url(http, &credentials.bili_jct, &value).await;
        return Ok(StartOutcome::Verify {
            verify: FaceChallenge {
                code,
                message: explain(code, &raw_message(&value)),
                url,
            },
        });
    }
    Ok(StartOutcome::Stream {
        stream: parse_start_response(&value, room_id)?,
    })
}

/// 停播。已经在未开播状态时 B 站会返回错误，由调用方展示。
pub async fn stop(http: &HttpClient, credentials: &Credentials, room_id: u64) -> Result<()> {
    if !credentials.is_valid() {
        return Err(Error::NotLoggedIn);
    }
    http.set_cookie(credentials.cookie());
    let form = livehime_form(
        &[
            ("room_id", room_id.to_string()),
            ("csrf", credentials.bili_jct.clone()),
            ("csrf_token", credentials.bili_jct.clone()),
        ],
        now_secs(),
    );
    let value = http
        .post_form_as_livehime(
            STOP_URL,
            &form,
            &livehime_cookies(http, &credentials.bili_jct).await,
        )
        .await?;
    check_response(&value)
}

/// 把 60024 / 60043 的响应换成可以直接打开的认证页。
///
/// B 站有两种给法：`data.qr` 直接就是链接；只给 `risk_extra.v_voucher` 时要先
/// 用它在 gaia-vgate 注册，再把返回的密文解出 token、混淆后拼成实名页。
async fn resolve_face_url(http: &HttpClient, csrf: &str, value: &Value) -> Option<String> {
    let data = value.get("data")?;
    if let Some(qr) = data
        .get("qr")
        .and_then(Value::as_str)
        .filter(|qr| !qr.is_empty())
        && trusted_face_link(qr)
    {
        return Some(qr.to_string());
    }
    let voucher = data
        .get("risk_extra")
        .and_then(|extra| extra.get("v_voucher"))
        .and_then(Value::as_str)
        .filter(|voucher| !voucher.is_empty())?;
    let content = register_challenge(http, csrf, voucher).await?;
    let token = decode_risk(voucher, &content).ok()?;
    let token = obfuscate_token(&token).ok()?;
    Some(format!(
        "https://www.bilibili.com/h5/risk-control/realname?t={}",
        quote(&quote(&token))
    ))
}

/// 只放行 bilibili.com 的链接：认证页会把账号状态带进去，不能跟着 B 站返回的任意地址走。
fn trusted_face_link(url: &str) -> bool {
    url.starts_with("https://www.bilibili.com/")
        || url.starts_with("https://bilibili.com/")
        || url.starts_with("https://live.bilibili.com/")
}

async fn register_challenge(http: &HttpClient, csrf: &str, voucher: &str) -> Option<String> {
    let form = [
        ("v_voucher", voucher.to_string()),
        ("dm_track", "[]".to_string()),
        ("csrf", csrf.to_string()),
    ];
    let value = http.post_form(CHALLENGE_URL, &form).await.ok()?;
    value
        .get("data")
        .and_then(|data| data.get("content"))
        .and_then(Value::as_str)
        .filter(|content| !content.is_empty())
        .map(str::to_string)
}

/// 解开 gaia 返回的密文：base64 → 用 voucher 的一段做 sha256 密钥 → XOR。
fn decode_risk(voucher: &str, content: &str) -> Result<String> {
    let invalid = || Error::other("身份验证响应解不开");
    let raw = base64::engine::general_purpose::STANDARD
        .decode(content)
        .map_err(|_| invalid())?;
    let text = String::from_utf8(raw).map_err(|_| invalid())?;
    let parts: Vec<&str> = text.split('|').collect();
    if parts.len() != 4 || parts[1].is_empty() {
        return Err(invalid());
    }
    let index: u32 = parts[2].parse().map_err(|_| invalid())?;
    // index 里最低的两个置位决定用 voucher 的哪一段做密钥
    let mut start = None;
    let mut end = None;
    for bit in 0..32 {
        if index & (1 << bit) != 0 {
            if start.is_none() {
                start = Some(bit as usize);
            } else {
                end = Some(bit as usize);
                break;
            }
        }
    }
    let (Some(start), Some(end)) = (start, end) else {
        return Err(invalid());
    };
    let tail = if voucher.len() > 8 {
        &voucher[8..]
    } else {
        voucher
    };
    // key = sha256(voucher[start..end] + parts[0])，两端越界时截到长度
    let end = end.min(tail.len());
    let start = start.min(end);
    let key_input = format!("{}{}", &tail[start..end], parts[0]);
    let digest = Sha256::digest(key_input.as_bytes());

    let mut encrypted = base64::engine::general_purpose::STANDARD
        .decode(parts[3])
        .map_err(|_| invalid())?;
    for (i, byte) in encrypted.iter_mut().enumerate() {
        *byte ^= digest[i % 16];
    }
    let decoded: Value = serde_json::from_slice(&encrypted).map_err(|_| invalid())?;
    if decoded.get("type").and_then(Value::as_str) != Some("realname") {
        return Err(Error::other("B 站要求的验证类型暂不支持，请在 App 里完成"));
    }
    decoded
        .get("token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .ok_or_else(invalid)
}

/// 给 token 加一层随机混淆：首字节是随机数，其余是 token 逐字节异或，末尾补随机尾巴。
fn obfuscate_token(token: &str) -> Result<String> {
    let random: [u8; 9] = rand::random();
    let mut raw = Vec::with_capacity(token.len() + 9);
    raw.push(random[0]);
    for (i, byte) in token.bytes().enumerate() {
        raw.push(byte ^ random[0] ^ random[1 + i % 8]);
    }
    raw.extend_from_slice(&random[1..]);
    Ok(base64::engine::general_purpose::STANDARD.encode(raw))
}

/// 与 Python quote 对齐：空格转 %20，斜杠保持原样。
fn quote(value: &str) -> String {
    urlencoding::encode(value).replace("%2F", "/")
}
/// 按直播姬客户端的规则拼参数：固定带 access_key / appkey / build / platform / ts / version，
/// 排序后做 md5(query + appsecret) 作为 sign。
///
/// 参数值只有数字、字母和点，`~` 与空格都不会出现，不必像 Python 那样补转义处理。
pub(crate) fn livehime_form(pairs: &[(&str, String)], ts: u64) -> Vec<(String, String)> {
    let mut params: Vec<(String, String)> = vec![
        ("access_key".into(), String::new()),
        ("appkey".into(), LIVEHIME_APPKEY.into()),
        ("build".into(), LIVEHIME_BUILD.into()),
        ("platform".into(), "pc_link".into()),
        ("ts".into(), ts.to_string()),
        ("version".into(), LIVEHIME_VERSION.into()),
    ];
    params.extend(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone())),
    );
    params.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = Md5::new();
    hasher.update(encode_form(&params).as_bytes());
    hasher.update(LIVEHIME_APPSECRET.as_bytes());
    params.push(("sign".to_string(), format!("{:x}", hasher.finalize())));

    params.sort_by(|a, b| a.0.cmp(&b.0));
    params
}

/// 直播姬客户端固定携带的 Cookie；buvid3 / bili_ticket 拿不到也不影响开播。
async fn livehime_cookies(http: &HttpClient, csrf: &str) -> Vec<String> {
    let mut cookies = vec![
        format!("appkey={LIVEHIME_APPKEY}"),
        "device_platform=Windows Version: 10.0 x86_64".to_string(),
    ];
    if let Some(buvid) = http.buvid3().await {
        cookies.push(format!("buvid3={buvid}"));
    }
    if let Some(ticket) = bili_ticket(http, csrf).await {
        cookies.push(format!("bili_ticket={ticket}"));
    }
    cookies
}

/// 设备票据：hexsign 是 HMAC-SHA256(key = "XgwSnGZ1", msg = "ts" + ts)。
/// 只影响风控强度，取不到就跳过，不阻塞开播。
async fn bili_ticket(http: &HttpClient, csrf: &str) -> Option<String> {
    let ts = now_secs().to_string();
    let mut mac = Hmac::<Sha256>::new_from_slice(b"XgwSnGZ1").ok()?;
    mac.update(format!("ts{ts}").as_bytes());
    let hexsign: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let form = [
        ("key_id", "ec02".to_string()),
        ("hexsign", hexsign),
        ("context[ts]", ts),
        ("csrf", csrf.to_string()),
    ];
    let value = http.post_form(TICKET_URL, &form).await.ok()?;
    value
        .get("data")
        .and_then(|data| data.get("ticket"))
        .and_then(Value::as_str)
        .filter(|ticket| !ticket.is_empty())
        .map(str::to_string)
}

pub(crate) fn encode_form(params: &[(String, String)]) -> String {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    serializer.extend_pairs(
        params
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str())),
    );
    serializer.finish()
}

pub(crate) fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn check_response(value: &Value) -> Result<()> {
    let code = value.get("code").and_then(Value::as_i64).unwrap_or(-1);
    if code == 0 {
        return Ok(());
    }
    let raw = raw_message(value);
    Err(Error::Live {
        code,
        message: explain(code, &raw),
    })
}

fn parse_start_response(value: &Value, room_id: u64) -> Result<LiveStream> {
    check_response(value)?;
    let data = value
        .get("data")
        .filter(|data| !data.is_null())
        .ok_or_else(|| Error::Live {
            code: -1,
            message: "B 站没有返回推流信息，请稍后重试".into(),
        })?;

    let mut endpoints = Vec::new();
    for key in ["protocols", "protocols_backup"] {
        if let Some(list) = data.get(key).and_then(Value::as_array) {
            for entry in list {
                let protocol = entry
                    .get("protocol")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let addr = entry
                    .get("addr")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let code = entry
                    .get("code")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if addr.is_empty() || code.is_empty() {
                    continue;
                }
                let endpoint = StreamEndpoint {
                    protocol,
                    addr,
                    code,
                };
                if !endpoints.contains(&endpoint) {
                    endpoints.push(endpoint);
                }
            }
        }
    }

    let rtmp = data.get("rtmp");
    let rtmp_addr = rtmp
        .and_then(|rtmp| rtmp.get("addr"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let rtmp_code = rtmp
        .and_then(|rtmp| rtmp.get("code"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let primary = if !rtmp_addr.is_empty() && !rtmp_code.is_empty() {
        StreamEndpoint {
            protocol: "rtmp".into(),
            addr: rtmp_addr,
            code: rtmp_code,
        }
    } else {
        endpoints.first().cloned().ok_or_else(|| Error::Live {
            code: -1,
            message: "B 站没有返回可用的推流地址，请稍后重试".into(),
        })?
    };
    if !endpoints.contains(&primary) {
        endpoints.insert(0, primary.clone());
    }

    Ok(LiveStream {
        room_id,
        addr: primary.addr,
        code: primary.code,
        protocol: primary.protocol,
        protocols: endpoints,
    })
}

pub(crate) fn raw_message(value: &Value) -> String {
    value
        .get("message")
        .or_else(|| value.get("msg"))
        .and_then(Value::as_str)
        .unwrap_or("未知错误")
        .to_string()
}

/// 把 B 站开播错误码翻译成人话；未知错误保留原始 message。
fn explain(code: i64, raw: &str) -> String {
    let known = match code {
        -101 => "账号未登录或登录已过期，请重新扫码登录",
        -111 => "csrf 校验失败，请重新登录",
        -400 => "开播参数有问题（房间号或分区）",
        -403 => "被 B 站风控拦截，请稍后再试",
        -352 => "被 B 站风控拦截（-352），请稍后再试",
        60024 => "开播需要人脸验证：请先在 B 站 App / 网页完成认证后再试",
        60043 => "开播需要实名认证：请先在 B 站完成实名认证后再试",
        60009 => "直播分区不可用：请先在 B 站直播设置里重新选择分区",
        60034 => "B 站只允许直播姬开播：这个账号或分区被单独限制了",
        60061 => "直播间被封禁，暂时不能开播",
        60063 => "开播过于频繁，请稍后再试",
        60066 => "播流密钥无效或已过期，请重新开播获取",
        60069 => "直播实名认证未通过，请先在 B 站完成认证",
        _ => return format!("{raw}（code={code}）"),
    };
    known.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const START_RESPONSE: &str = r#"{
        "code": 0,
        "message": "ok",
        "data": {
            "rtmp": {
                "addr": "rtmp://live-push.bilivideo.com/live-bvc/",
                "code": "?streamname=live_123_456&key=abcdef&schedule=rtmp"
            },
            "protocols": [
                {
                    "protocol": "rtmp",
                    "addr": "rtmp://live-push.bilivideo.com/live-bvc/",
                    "code": "?streamname=live_123_456&key=abcdef&schedule=rtmp"
                },
                {
                    "protocol": "srt",
                    "addr": "srt://live-push.bilivideo.com:1935",
                    "code": "?streamname=live_123_456&key=abcdef&schedule=srt"
                }
            ],
            "protocols_backup": []
        }
    }"#;

    #[test]
    fn parses_rtmp_and_srt_endpoints() {
        let value: Value = serde_json::from_str(START_RESPONSE).unwrap();
        let stream = parse_start_response(&value, 123).unwrap();
        assert_eq!(stream.room_id, 123);
        assert_eq!(stream.protocol, "rtmp");
        assert!(stream.addr.starts_with("rtmp://"));
        assert!(stream.code.contains("key=abcdef"));
        assert_eq!(stream.protocols.len(), 2);
        assert_eq!(stream.protocols[1].protocol, "srt");
    }

    #[test]
    fn falls_back_to_first_protocol_when_rtmp_is_missing() {
        let value: Value = serde_json::json!({
            "code": 0,
            "data": {
                "rtmp": { "addr": "", "code": "" },
                "protocols": [
                    { "protocol": "srt", "addr": "srt://push.example:1935", "code": "?key=x" }
                ]
            }
        });
        let stream = parse_start_response(&value, 7).unwrap();
        assert_eq!(stream.protocol, "srt");
        assert_eq!(stream.addr, "srt://push.example:1935");
        assert_eq!(stream.protocols.len(), 1);
    }

    #[test]
    fn explains_start_errors() {
        let value: Value = serde_json::json!({ "code": 60024, "message": "face auth" });
        let error = parse_start_response(&value, 1).unwrap_err();
        match error {
            Error::Live { code, message } => {
                assert_eq!(code, 60024);
                assert!(message.contains("人脸"));
            }
            other => panic!("错误类型不对: {other:?}"),
        }

        // 60034：系统维护、只允许直播姬开播。要让人看出这是 B 站的限制。
        let hime: Value =
            serde_json::json!({ "code": 60034, "message": "系统维护仅支持直播姬开播" });
        match parse_start_response(&hime, 1).unwrap_err() {
            Error::Live { code, message } => {
                assert_eq!(code, 60034);
                assert!(
                    message.contains("直播姬"),
                    "错误文案没提到直播姬: {message}"
                );
            }
            other => panic!("错误类型不对: {other:?}"),
        }

        let unknown: Value = serde_json::json!({ "code": 12345, "message": "boom" });
        let error = parse_start_response(&unknown, 1).unwrap_err();
        assert!(error.to_string().contains("12345"));
        assert!(error.to_string().contains("boom"));
    }

    #[test]
    fn resolves_gaia_challenge_payload() {
        // 这串 content 与参数由 Node 独立实现按同一算法算出，用来对齐解码逻辑
        let voucher = "voucher_ce182669-1e03-4d98-a083-6584b070d4e6";
        let content = "c2FsdDB8b25lfDV8Q3FycFpRRlJFcUpJd2R5STZ1UFpVUktxc1NnZVRRemxVTm1LbGVUZDFGMFE1ZmdvRnc9PQ==";
        assert_eq!(decode_risk(voucher, content).unwrap(), "tok_abc");
    }

    #[test]
    fn rejects_challenge_with_foreign_type() {
        // type 不是 realname 的挑战（比如滑块）交给 App 处理，不要拼出假链接
        let err = decode_risk("voucher_x", "bm90fGE=|x").unwrap_err();
        assert!(err.to_string().contains("解不开"));
    }

    #[test]
    fn obfuscated_token_round_trips() {
        let token = "tok_abc123";
        let encoded = obfuscate_token(token).unwrap();
        let raw = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        let head = raw[0];
        let body_len = raw.len() - 9;
        let decoded: Vec<u8> = raw[1..=body_len]
            .iter()
            .enumerate()
            .map(|(i, byte)| byte ^ head ^ raw[body_len + 1 + i % 8])
            .collect();
        assert_eq!(String::from_utf8(decoded).unwrap(), token);
    }

    #[test]
    fn only_trusts_bilibili_links() {
        assert!(trusted_face_link(
            "https://www.bilibili.com/h5/risk-control/realname?t=x"
        ));
        assert!(!trusted_face_link("https://evil.test/bilibili.com"));
        assert!(!trusted_face_link("http://www.bilibili.com/x"));
    }
    #[test]
    fn signs_start_params_like_livehime() {
        // 同一组参数用 Node 的 md5 独立算出的签名；对不上说明排序或拼接变了。
        let form = livehime_form(
            &[
                ("room_id", "7734200".to_string()),
                ("area_v2", "371".to_string()),
                ("type", "2".to_string()),
                ("csrf", "abc123".to_string()),
                ("csrf_token", "abc123".to_string()),
            ],
            1_792_951_565,
        );
        let sign = form
            .iter()
            .find(|(key, _)| key == "sign")
            .map(|(_, value)| value.as_str())
            .expect("缺少 sign 参数");
        assert_eq!(sign, "9ced8db9aeb99b42d76a13d75fe4d5bb");
        assert!(
            form.iter()
                .any(|(key, value)| key == "platform" && value == "pc_link"),
            "直播姬用的是 pc_link，不是 pc"
        );
        assert!(form.iter().any(|(key, _)| key == "access_key"));
    }

    #[test]
    fn empty_stream_response_is_rejected() {
        let value: Value =
            serde_json::json!({ "code": 0, "data": { "rtmp": null, "protocols": [] } });
        assert!(parse_start_response(&value, 1).is_err());
    }
}
