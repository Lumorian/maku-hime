//! 直播间设置：标题、分区、封面、公告。
//!
//! 这些接口和开播一样按直播姬客户端签名（见 [crate::live] 的 `livehime_form`），
//! 并且都要求**主播本人**登录：Cookie 里必须有 SESSDATA 与 bili_jct。
//!
//! 几个容易踩的点：
//! - 改分区可能把直播间换到「仅直播姬」的分区，之后第三方推流会被 60034 挡掉；
//! - 封面是两步：先传到 B 站图床拿 location，再提交到直播间，提交后还要过审；
//! - 公告按已登录账号提交，不带 room_id（与直播姬网页端一致）。

use base64::Engine as _;
use image::ImageEncoder as _;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::http::{HttpClient, LIVEHIME_UA, UA, UploadPart};
use crate::live::{LIVEHIME_BUILD, encode_form, livehime_form, now_secs, raw_message};
use crate::sender::Credentials;

const PRELIVE_URL: &str = "https://api.live.bilibili.com/xlive/app-blink/v1/preLive/PreLive";
const UPDATE_PRELIVE_URL: &str =
    "https://api.live.bilibili.com/xlive/app-blink/v1/preLive/UpdatePreLiveInfo";
const ANCHOR_AREA_URL: &str =
    "https://api.live.bilibili.com/xlive/app-blink/v2/room/AnchorChangeRoomArea";
const AREA_LIST_URL: &str =
    "https://api.live.bilibili.com/xlive/app-blink/v1/preLive/GetAreaListForLive";
const MY_AREA_URL: &str = "https://api.live.bilibili.com/room/v1/Area/getMyChooseArea";
const ROOM_INFO_URL: &str = "https://api.live.bilibili.com/xlive/app-blink/v1/room/GetInfo";
const ANNOUNCE_INFO_URL: &str =
    "https://api.live.bilibili.com/xlive/app-blink/v1/room/AnnounceInfo";
const ANNOUNCE_COMMIT_URL: &str =
    "https://api.live.bilibili.com/xlive/app-blink/v1/room/AnnounceCommit";
const IMAGE_UPLOAD_URL: &str = "https://api.bilibili.com/x/upload/web/image";

/// 封面源图上限：与 B 站图床的常规限制一致，超过就不必上传了。
const COVER_MAX_BYTES: usize = 20 << 20;
/// 封面链接允许跟随的跳转层数（逐跳校验，跳太多说明在绕圈子）。
const MAX_COVER_REDIRECTS: usize = 3;
/// 上传前统一裁成 16:9 并缩到这个尺寸，兼顾清晰度与上传体积。
const COVER_WIDTH: u32 = 1280;
const COVER_HEIGHT: u32 = 720;
/// 上传体积上限：超过就降质量重编码。
const COVER_TARGET_BYTES: usize = 2 << 20;

/// 一个可选的小分区。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Area {
    pub id: u64,
    pub name: String,
    /// 大分区名（游戏 / 虚拟主播 …）
    pub parent: String,
}

/// 分区选择需要的两套数据：完整目录 + 最近用过。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AreaOptions {
    pub areas: Vec<Area>,
    pub recent: Vec<Area>,
}

/// 直播间当前的设置（能读到的部分）。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoomSettings {
    pub room_id: u64,
    pub title: String,
    pub area_id: u64,
    pub area_name: String,
    pub parent_name: String,
    /// 当前封面地址
    pub cover: String,
    /// 封面审核状态：1 通过 / 0 审核中 / -1 被拒 / 其它为未知
    pub cover_status: i64,
    pub cover_reason: String,
    pub announcement: String,
    pub live_status: i64,
}

/// 上传封面的结果。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CoverOutcome {
    /// 图床返回的地址（提交成功后即为直播间封面）
    pub cover: String,
    pub status: i64,
    pub reason: String,
}

fn require_login(credentials: &Credentials) -> Result<()> {
    if credentials.is_valid() {
        Ok(())
    } else {
        Err(Error::NotLoggedIn)
    }
}

/// 按直播姬规则签名后拼成带查询串的地址。
fn signed_url(url: &str, pairs: &[(&str, String)]) -> String {
    let params = livehime_form(pairs, now_secs());
    format!("{url}?{}", encode_form(&params))
}

async fn get_signed(http: &HttpClient, url: &str, pairs: &[(&str, String)]) -> Result<Value> {
    http.get_json_as(&signed_url(url, pairs), LIVEHIME_UA).await
}

async fn post_signed(
    http: &HttpClient,
    url: &str,
    pairs: &[(&str, String)],
    form: Vec<(String, String)>,
) -> Result<Value> {
    http.post_form_as(&signed_url(url, pairs), &form, LIVEHIME_UA)
        .await
}

fn csrf_pairs(credentials: &Credentials) -> Vec<(String, String)> {
    vec![
        ("csrf".into(), credentials.bili_jct.clone()),
        ("csrf_token".into(), credentials.bili_jct.clone()),
    ]
}

/// 拉取直播间当前设置：PreLive 给标题与封面，GetInfo 给分区与开播状态。
pub async fn fetch(
    http: &HttpClient,
    credentials: &Credentials,
    room_id: u64,
) -> Result<RoomSettings> {
    require_login(credentials)?;
    let uid = credentials.dede_user_id.trim();
    if uid.is_empty() {
        return Err(Error::other("登录信息里没有 UID，请重新扫码登录"));
    }
    http.set_cookie(credentials.cookie());

    let pre = get_signed(
        http,
        PRELIVE_URL,
        &[
            ("area", "true".into()),
            ("cover", "true".into()),
            ("coverVertical", "true".into()),
            ("liveDirectionType", "0".into()),
            ("mobi_app", "pc_link".into()),
            ("schedule", "true".into()),
            ("title", "true".into()),
        ],
    )
    .await?;
    check(&pre)?;
    let pre = pre.get("data").cloned().unwrap_or(Value::Null);

    let info = get_signed(http, ROOM_INFO_URL, &[("uId", uid.to_string())]).await?;
    check(&info)?;
    let info = info.get("data").cloned().unwrap_or(Value::Null);

    // 公告读不到不影响设置面板，其余项能改就行
    let announcement = match get_signed(http, ANNOUNCE_INFO_URL, &[]).await {
        Ok(value) => value
            .get("data")
            .and_then(|data| data.get("announces"))
            .and_then(|announces| announces.get("1"))
            .and_then(|item| item.get("content"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        Err(err) => {
            tracing::debug!(%err, "读取直播公告失败");
            String::new()
        }
    };

    let cover = pre.get("cover").cloned().unwrap_or(Value::Null);
    Ok(RoomSettings {
        room_id: info
            .get("room_id")
            .and_then(Value::as_u64)
            .unwrap_or(room_id),
        title: pre
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        area_id: info.get("area_v2_id").and_then(Value::as_u64).unwrap_or(0),
        area_name: info
            .get("area_v2_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        parent_name: info
            .get("parent_name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        cover: cover
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        cover_status: cover
            .get("auditStatus")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        cover_reason: cover
            .get("auditReason")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        announcement,
        live_status: info.get("live_status").and_then(Value::as_i64).unwrap_or(0),
    })
}

/// 改直播间标题。
pub async fn set_title(
    http: &HttpClient,
    credentials: &Credentials,
    room_id: u64,
    title: &str,
) -> Result<()> {
    require_login(credentials)?;
    let title = title.trim();
    if title.is_empty() {
        return Err(Error::Manage {
            code: -1,
            message: "直播间标题不能为空".into(),
        });
    }
    if title.chars().count() > 40 {
        return Err(Error::Manage {
            code: -1,
            message: "直播间标题最多 40 个字".into(),
        });
    }
    http.set_cookie(credentials.cookie());
    let mut form = csrf_pairs(credentials);
    form.push(("mobi_app".into(), "pc_link".into()));
    form.push(("room_id".into(), room_id.to_string()));
    form.push(("title".into(), title.to_string()));
    let value = post_signed(http, UPDATE_PRELIVE_URL, &[], form).await?;
    check(&value)
}

/// 改直播分区。
pub async fn set_area(
    http: &HttpClient,
    credentials: &Credentials,
    room_id: u64,
    area_id: u64,
) -> Result<()> {
    require_login(credentials)?;
    if area_id == 0 {
        return Err(Error::Manage {
            code: -1,
            message: "请先选择一个直播分区".into(),
        });
    }
    http.set_cookie(credentials.cookie());
    let mut form = csrf_pairs(credentials);
    form.push(("area_id".into(), area_id.to_string()));
    form.push(("build".into(), LIVEHIME_BUILD.into()));
    form.push(("platform".into(), "pc_link".into()));
    form.push(("room_id".into(), room_id.to_string()));
    let value = post_signed(http, ANCHOR_AREA_URL, &[], form).await?;
    check(&value)
}

/// 改直播间公告。空字符串等同于清空。
pub async fn set_announcement(
    http: &HttpClient,
    credentials: &Credentials,
    content: &str,
) -> Result<()> {
    require_login(credentials)?;
    if content.chars().count() > 500 {
        return Err(Error::Manage {
            code: -1,
            message: "公告最多 500 个字".into(),
        });
    }
    http.set_cookie(credentials.cookie());
    let mut form = csrf_pairs(credentials);
    form.push(("content".into(), content.trim().to_string()));
    form.push(("type".into(), "1".into()));
    let value = post_signed(http, ANNOUNCE_COMMIT_URL, &[], form).await?;
    check(&value)
}

/// 全部分区目录（大分区 → 小分区）。
pub async fn areas(http: &HttpClient, credentials: &Credentials) -> Result<Vec<Area>> {
    require_login(credentials)?;
    http.set_cookie(credentials.cookie());
    let value = get_signed(http, AREA_LIST_URL, &[]).await?;
    check(&value)?;
    let parents = value
        .get("data")
        .and_then(|data| data.get("area_v1_info"))
        .and_then(Value::as_array)
        .ok_or_else(|| Error::other("分区目录返回格式不认识"))?;

    let mut areas = Vec::new();
    for parent in parents {
        let parent_name = parent
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let Some(list) = parent.get("list").and_then(Value::as_array) else {
            continue;
        };
        for area in list {
            // B 站这里把 id 当字符串下发（"86"），不能只认数字
            let Some(id) = area.get("id").and_then(as_u64) else {
                continue;
            };
            let name = area
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if id == 0 || name.is_empty() || parent_name.is_empty() {
                continue;
            }
            areas.push(Area {
                id,
                name,
                parent: parent_name.to_string(),
            });
        }
    }
    Ok(areas)
}

/// 最近用过的分区。只保留能对上当前目录的条目，对不上就不猜。
pub async fn recent_areas(
    http: &HttpClient,
    credentials: &Credentials,
    room_id: u64,
    all: &[Area],
) -> Result<Vec<Area>> {
    require_login(credentials)?;
    http.set_cookie(credentials.cookie());
    let value = get_signed(http, MY_AREA_URL, &[("roomid", room_id.to_string())]).await?;
    // 这个接口偶尔返回空数组/空值，读不到就当没有历史
    if check(&value).is_err() {
        return Ok(Vec::new());
    }
    let Some(entries) = value.get("data").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };

    let mut result = Vec::new();
    for entry in entries {
        let id = entry.get("id").and_then(as_u64).unwrap_or(0);
        let name = entry
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let parent = entry
            .get("parent_name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let matched = if id > 0 {
            all.iter().find(|area| area.id == id)
        } else {
            let candidates: Vec<&Area> = all
                .iter()
                .filter(|area| area.name == name && area.parent == parent)
                .collect();
            // 同名分区有歧义时宁可不给快捷入口
            if candidates.len() == 1 {
                candidates.first().copied()
            } else {
                None
            }
        };
        if let Some(area) = matched
            && !result.iter().any(|item: &Area| item.id == area.id)
        {
            result.push(area.clone());
        }
    }
    Ok(result)
}

/// 上传并提交直播间封面：先传图床拿地址，再写进直播设置，最后读一次审核状态。
pub async fn upload_cover(
    http: &HttpClient,
    credentials: &Credentials,
    raw: &[u8],
) -> Result<CoverOutcome> {
    require_login(credentials)?;
    http.set_cookie(credentials.cookie());
    let jpeg = prepare_cover(raw)?;

    // 1) 传图床（网页端口径：csrf 走查询串，浏览器 UA）
    let upload_url = format!(
        "{IMAGE_UPLOAD_URL}?csrf={}",
        urlencoding::encode(&credentials.bili_jct)
    );
    let value = http
        .post_multipart_as(
            &upload_url,
            &[
                ("bucket", "live".to_string()),
                ("dir", "new_room_cover".to_string()),
            ],
            UploadPart {
                field: "file",
                file_name: "cover.jpg",
                mime: "image/jpeg",
                bytes: &jpeg,
            },
            UA,
        )
        .await?;
    check(&value)?;
    let location = value
        .get("data")
        .and_then(|data| data.get("location"))
        .and_then(Value::as_str)
        .filter(|location| !location.is_empty())
        .ok_or_else(|| Error::Manage {
            code: -1,
            message: "图床没有返回图片地址，请稍后再试".into(),
        })?
        .to_string();

    // 2) 提交到直播间设置
    let mut form = csrf_pairs(credentials);
    form.push(("build".into(), "1".into()));
    form.push(("cover".into(), location.clone()));
    form.push(("coverVertical".into(), String::new()));
    form.push(("liveDirectionType".into(), "1".into()));
    form.push(("mobi_app".into(), "pc_link".into()));
    form.push(("platform".into(), "pc_link".into()));
    form.push(("visit_id".into(), String::new()));
    let value = http.post_form_as(UPDATE_PRELIVE_URL, &form, UA).await?;
    check(&value)?;

    // 3) 审核状态：提交响应里带就先用，没有就当作「审核中」
    let audit = value
        .get("data")
        .and_then(|data| data.get("audit_info"))
        .cloned()
        .unwrap_or(Value::Null);
    let status = audit
        .get("audit_title_status")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    Ok(CoverOutcome {
        cover: location,
        status,
        reason: audit
            .get("audit_title_reason")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    })
}

/// 解码前端传来的封面数据：裸 base64 和 data URL 都认。
pub fn decode_cover_data(data: &str) -> Result<Vec<u8>> {
    let raw = data.trim();
    // data:image/png;base64,xxxx → 只留逗号后面那段
    let raw = raw.split_once(',').map_or(raw, |(_, rest)| rest);
    base64::engine::general_purpose::STANDARD
        .decode(raw.trim())
        .map_err(|_| Error::other("封面数据不是合法的 base64"))
}

/// 下载用户粘贴的封面地址。
///
/// 这里是唯一一处「请求由用户输入的 URL」，所以两道闸门缺一不可：
/// 1. [HttpClient::get_bytes_public] 不带任何登录 Cookie，重定向逐跳校验，
///    解析结果必须是公网地址（否则把 SESSDATA 发给别人就是账号接管）；
/// 2. 字面量形式的 localhost / 内网 IP 在这里先挡一次，给出更好懂的错误文案。
pub async fn download_cover(http: &HttpClient, raw_url: &str) -> Result<Vec<u8>> {
    let url = url::Url::parse(raw_url.trim()).map_err(|_| Error::other("封面链接不是合法地址"))?;
    if url.scheme() != "https" {
        return Err(Error::other("封面链接必须是 https 地址"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::other("封面链接不能带账号密码"));
    }
    let host = url
        .host_str()
        .ok_or_else(|| Error::other("封面链接缺少域名"))?;
    if is_private_host(host) {
        return Err(Error::other("封面链接不能指向本机或内网地址"));
    }
    http.get_bytes_public(url.as_str(), COVER_MAX_BYTES, MAX_COVER_REDIRECTS)
        .await
}

/// 数字或字符串形式的整数都认（B 站不少接口用字符串下发 id）。
fn as_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

fn is_private_host(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
        return true;
    }
    // 域名不做 DNS 解析；字面量 IP 直接判内网
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => {
            ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.is_unspecified()
        }
        Ok(std::net::IpAddr::V6(ip)) => {
            ip.is_loopback() || ip.is_unspecified() || ip.segments()[0] & 0xfe00 == 0xfc00
        }
        Err(_) => false,
    }
}

/// 把任意图片裁成 16:9 并压成 JPEG；B 站只收图片，转码这一步由我们兜住。
fn prepare_cover(raw: &[u8]) -> Result<Vec<u8>> {
    if raw.is_empty() {
        return Err(Error::other("封面图片是空的"));
    }
    if raw.len() > COVER_MAX_BYTES {
        return Err(Error::other("封面图片超过 20MB，请先压缩"));
    }
    let img = image::load_from_memory(raw)
        .map_err(|_| Error::other("封面图片解不开，请换 JPG / PNG / WebP"))?;

    let cropped = crop_to_ratio(&img, COVER_WIDTH, COVER_HEIGHT);
    // JPEG 只吃 RGB，先丢掉 alpha 再编码
    let scaled = cropped
        .resize_exact(
            COVER_WIDTH,
            COVER_HEIGHT,
            image::imageops::FilterType::Lanczos3,
        )
        .to_rgb8();

    for quality in [88u8, 78, 68, 58] {
        let mut out = Vec::new();
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
        encoder
            .write_image(
                scaled.as_raw(),
                COVER_WIDTH,
                COVER_HEIGHT,
                image::ExtendedColorType::Rgb8,
            )
            .map_err(|err| Error::other(format!("封面压缩失败: {err}")))?;
        if out.len() <= COVER_TARGET_BYTES {
            return Ok(out);
        }
    }
    Err(Error::other("封面压不到 2MB 以内，请换一张更简单的图"))
}

/// 居中裁成目标比例（不拉伸，多余的部分切掉）。
fn crop_to_ratio(img: &image::DynamicImage, width: u32, height: u32) -> image::DynamicImage {
    let (w, h) = (img.width(), img.height());
    let target = width as f64 / height as f64;
    let current = w as f64 / h as f64;
    if (current - target).abs() < 0.01 {
        return img.clone();
    }
    if current > target {
        // 太宽：切左右
        let new_w = (h as f64 * target).round() as u32;
        let x = (w - new_w) / 2;
        img.crop_imm(x, 0, new_w.max(1), h)
    } else {
        let new_h = (w as f64 / target).round() as u32;
        let y = (h - new_h) / 2;
        img.crop_imm(0, y, w, new_h.max(1))
    }
}

/// 统一校验业务码，并把错误码翻译成中文。
fn check(value: &Value) -> Result<()> {
    let code = value.get("code").and_then(Value::as_i64).unwrap_or(-1);
    if code == 0 {
        return Ok(());
    }
    Err(Error::Manage {
        code,
        message: explain(code, &raw_message(value)),
    })
}

fn explain(code: i64, raw: &str) -> String {
    match code {
        -101 => "登录已过期，请重新扫码登录".to_string(),
        -111 => "csrf 校验失败，请重新登录".to_string(),
        10030 => "内容命中敏感词，换一种说法再试".to_string(),
        60034 => "B 站把这个直播间划给了直播姬，网页端改不了，请在直播姬里设置".to_string(),
        _ => format!("{raw}（code={code}）"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_query_in_sorted_order() {
        let url = signed_url("https://x.test/a", &[("roomid", "1".into())]);
        assert!(url.starts_with("https://x.test/a?"));
        // 签名参数必须都在，且 roomid 被编码进来
        for key in [
            "access_key=",
            "appkey=",
            "build=",
            "platform=pc_link",
            "ts=",
            "sign=",
        ] {
            assert!(url.contains(key), "缺少 {key}: {url}");
        }
        assert!(url.contains("roomid=1"));
    }

    #[test]
    fn blocks_private_cover_hosts() {
        assert!(is_private_host("127.0.0.1"));
        assert!(is_private_host("localhost"));
        assert!(is_private_host("192.168.1.10"));
        assert!(is_private_host("::1"));
        assert!(!is_private_host("i0.hdslb.com"));
    }

    #[test]
    fn rejects_plain_http_cover_url() {
        let http = HttpClient::new().unwrap();
        let err = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(download_cover(&http, "http://i0.hdslb.com/a.jpg"))
            .unwrap_err();
        assert!(err.to_string().contains("https"));
    }

    #[test]
    fn crops_wide_and_tall_images_to_sixteen_by_nine() {
        let wide = image::DynamicImage::new_rgba8(1600, 600);
        let cropped = crop_to_ratio(&wide, COVER_WIDTH, COVER_HEIGHT);
        assert_eq!(cropped.width(), 1067);
        assert_eq!(cropped.height(), 600);

        let tall = image::DynamicImage::new_rgba8(600, 1600);
        let cropped = crop_to_ratio(&tall, COVER_WIDTH, COVER_HEIGHT);
        assert_eq!(cropped.width(), 600);
        assert_eq!(cropped.height(), 338);
    }

    #[test]
    fn encodes_cover_as_jpeg() {
        let img = image::DynamicImage::new_rgba8(64, 36);
        let mut raw = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut raw), image::ImageFormat::Png)
            .unwrap();
        let jpeg = prepare_cover(&raw).unwrap();
        assert_eq!(&jpeg[..2], &[0xff, 0xd8]);
        assert!(jpeg.len() < COVER_TARGET_BYTES);
    }
}
