//! 直播表情面板：拉取当前账号可用的表情包。
//!
//! 面板里的「全部表情」其实是两类东西拼起来的，接口各不一样：
//! - 直播表情（通用表情 / 大表情 / 粉丝团 / 大航海）：
//!   `xlive/web-ucenter/v2|v1/emoticon/GetEmoticons`，带 `emoticon_unique`，发送时要走 `dm_type=1`；
//! - 评论区表情（小黄脸、颜文字、联动表情包）：
//!   `x/emote/user/panel/web?business=reply`，只有 `text`（形如 `[笑哭]`），
//!   按普通文本发出去，B 站自己会渲染成图。
//!
//! 没解锁的表情（粉丝牌等级不够等）也照样列出来，发送时由 B 站拒绝并给出原因。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::http::{HttpClient, UA};
use crate::sender::Credentials;

const LIVE_EMOTICONS_V2: &str =
    "https://api.live.bilibili.com/xlive/web-ucenter/v2/emoticon/GetEmoticons";
const LIVE_EMOTICONS_V1: &str =
    "https://api.live.bilibili.com/xlive/web-ucenter/v1/emoticon/GetEmoticons";
const COMMENT_PANEL: &str = "https://api.bilibili.com/x/emote/user/panel/web?business=reply";

/// 一个表情。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Emoticon {
    pub id: u64,
    /// 发送时要带上的唯一标识；评论区表情没有这个字段
    pub unique_name: String,
    /// 表情对应的文本（贴图是表情名，评论区表情是 `[笑哭]` 这种）
    pub text: String,
    pub url: String,
    pub width: u32,
    pub height: u32,
    /// 0 表示没有额外限制；非 0 时可能需要粉丝牌等条件
    pub perm: i64,
}

/// 一组表情包（同一个页签下展示）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EmoticonPackage {
    pub id: u64,
    pub name: String,
    /// 包类型：官方 / 粉丝牌 / 房间自定义等，原样透传给前端分类
    pub kind: i64,
    pub emoticons: Vec<Emoticon>,
}

/// 拉取表情面板：直播表情 + 评论区表情，能拿到多少给多少。
pub async fn fetch(
    http: &HttpClient,
    credentials: &Credentials,
    room_id: u64,
) -> Result<Vec<EmoticonPackage>> {
    if !credentials.is_valid() {
        return Err(Error::NotLoggedIn);
    }
    http.set_cookie(credentials.cookie());

    let mut packages = Vec::new();
    let mut failures = Vec::new();

    // 直播表情：v2 是通用表情，v1 是粉丝团 / 大航海（没解锁时是空包）
    for base in [LIVE_EMOTICONS_V2, LIVE_EMOTICONS_V1] {
        let url = format!("{base}?platform=pc&room_id={room_id}");
        match get(http, &url).await.and_then(|value| parse_live(&value)) {
            Ok(list) => packages.extend(list),
            Err(err) => {
                tracing::warn!(%err, source = base, "拉取直播表情失败");
                failures.push(err);
            }
        }
    }

    // 评论区表情：小黄脸、颜文字、联动表情包都在这里
    match get(http, COMMENT_PANEL)
        .await
        .and_then(|value| parse_comment(&value))
    {
        Ok(list) => packages.extend(list),
        Err(err) => {
            tracing::warn!(%err, "拉取评论区表情失败");
            failures.push(err);
        }
    }

    dedupe(&mut packages);
    if packages.is_empty() {
        // 全挂了才报错；只挂一个来源时用剩下的照常展示
        return Err(failures
            .into_iter()
            .next()
            .unwrap_or_else(|| Error::other("表情面板没有返回任何表情")));
    }
    Ok(packages)
}

async fn get(http: &HttpClient, url: &str) -> Result<Value> {
    let value = http.get_json_as(url, UA).await?;
    let code = value.get("code").and_then(Value::as_i64).unwrap_or(-1);
    if code != 0 {
        let raw = value
            .get("message")
            .or_else(|| value.get("msg"))
            .and_then(Value::as_str)
            .unwrap_or("未知错误");
        return Err(Error::Api {
            code,
            message: raw.to_string(),
        });
    }
    Ok(value)
}

/// 直播表情：`data.data[]`，每个包里是 `emoticons[]`。
fn parse_live(value: &Value) -> Result<Vec<EmoticonPackage>> {
    let packages = value
        .get("data")
        .and_then(|data| data.get("data"))
        .and_then(Value::as_array)
        .ok_or_else(|| Error::other("直播表情返回格式不认识"))?;

    let mut result = Vec::new();
    for package in packages {
        let Some(list) = package.get("emoticons").and_then(Value::as_array) else {
            continue;
        };
        let mut emoticons = Vec::new();
        for item in list {
            let Some(url) = item
                .get("url")
                .and_then(Value::as_str)
                .filter(|url| !url.is_empty())
            else {
                continue;
            };
            let unique_name = item
                .get("emoticon_unique")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            // 直播表情没有 text 字段，文字在 emoji 里；id 一般藏在 unique 的尾巴上（official_331）
            let text = item
                .get("text")
                .or_else(|| item.get("emoji"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let id = item
                .get("emoticon_id")
                .and_then(Value::as_u64)
                .or_else(|| {
                    unique_name
                        .rsplit('_')
                        .next()
                        .and_then(|tail| tail.parse().ok())
                })
                .unwrap_or(0);
            emoticons.push(Emoticon {
                id,
                unique_name,
                text,
                // B 站有时下发 http 图床地址，统一升级成 https，避免 https 页面被混合内容拦住
                url: url.replacen("http://", "https://", 1),
                width: item.get("width").and_then(Value::as_u64).unwrap_or(0) as u32,
                height: item.get("height").and_then(Value::as_u64).unwrap_or(0) as u32,
                perm: item.get("perm").and_then(Value::as_i64).unwrap_or(0),
            });
        }
        if emoticons.is_empty() {
            continue;
        }
        result.push(EmoticonPackage {
            id: package.get("pkg_id").and_then(Value::as_u64).unwrap_or(0),
            name: package
                .get("pkg_name")
                .and_then(Value::as_str)
                .unwrap_or("表情")
                .to_string(),
            kind: package.get("pkg_type").and_then(Value::as_i64).unwrap_or(0),
            emoticons,
        });
    }
    Ok(result)
}

/// 评论区表情：`data.packages[]`，每个包里是 `emote[]`，文字就是发送内容。
fn parse_comment(value: &Value) -> Result<Vec<EmoticonPackage>> {
    let packages = value
        .get("data")
        .and_then(|data| data.get("packages"))
        .and_then(Value::as_array)
        .ok_or_else(|| Error::other("评论区表情返回格式不认识"))?;

    let mut result = Vec::new();
    for package in packages {
        let Some(list) = package.get("emote").and_then(Value::as_array) else {
            continue;
        };
        let mut emoticons = Vec::new();
        for item in list {
            let Some(url) = item
                .get("url")
                .and_then(Value::as_str)
                .filter(|url| !url.is_empty())
            else {
                continue;
            };
            let text = item
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if text.is_empty() {
                continue;
            }
            emoticons.push(Emoticon {
                // 评论区表情用文本本身标识，不需要 emoticon_unique
                id: item.get("id").and_then(Value::as_u64).unwrap_or(0),
                unique_name: String::new(),
                text,
                url: url.replacen("http://", "https://", 1),
                width: 0,
                height: 0,
                perm: 0,
            });
        }
        if emoticons.is_empty() {
            continue;
        }
        result.push(EmoticonPackage {
            id: package.get("id").and_then(Value::as_u64).unwrap_or(0),
            name: package
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("表情")
                .to_string(),
            kind: 100,
            emoticons,
        });
    }
    Ok(result)
}

/// 去掉同图的重复项（两个接口偶尔会下发同一套表情）。
fn dedupe(packages: &mut Vec<EmoticonPackage>) {
    let mut seen = std::collections::HashSet::new();
    for package in packages.iter_mut() {
        package
            .emoticons
            .retain(|item| seen.insert((item.url.clone(), item.text.clone())));
    }
    packages.retain(|package| !package.emoticons.is_empty());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_live_packages_and_skips_empty_ones() {
        let raw = serde_json::json!({
            "code": 0,
            "data": { "data": [
                {
                    "pkg_id": 1,
                    "pkg_name": "通用表情",
                    "pkg_type": 1,
                    "emoticons": [
                        { "emoticon_id": 331, "emoticon_unique": "official_331", "emoji": "啊",
                          "url": "http://i0.hdslb.com/a.png", "width": 200, "height": 60, "perm": 1 },
                        { "emoticon_id": 332, "url": "" }
                    ]
                },
                { "pkg_id": 4, "pkg_name": "粉丝团", "pkg_type": 1, "emoticons": [] }
            ] }
        });
        let packages = parse_live(&raw).unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].name, "通用表情");
        assert_eq!(packages[0].emoticons[0].unique_name, "official_331");
        // 文字回退到 emoji 字段，图片地址升到 https
        assert_eq!(packages[0].emoticons[0].text, "啊");
        assert_eq!(packages[0].emoticons[0].url, "https://i0.hdslb.com/a.png");
    }

    #[test]
    fn parses_comment_packages_as_text_emoticons() {
        let raw = serde_json::json!({
            "code": 0,
            "data": { "packages": [
                {
                    "id": 1,
                    "text": "小黄脸",
                    "emote": [
                        { "id": 509, "text": "[笑哭]", "url": "http://i0.hdslb.com/b.png" },
                        { "id": 510, "text": "", "url": "https://i0.hdslb.com/c.png" }
                    ]
                }
            ] }
        });
        let packages = parse_comment(&raw).unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].name, "小黄脸");
        let emote = &packages[0].emoticons[0];
        // 评论区表情没有 unique，发送时按纯文本走
        assert!(emote.unique_name.is_empty());
        assert_eq!(emote.text, "[笑哭]");
        assert_eq!(emote.url, "https://i0.hdslb.com/b.png");
    }

    #[test]
    fn dedupes_same_image_and_text() {
        let one = || EmoticonPackage {
            id: 1,
            name: "A".into(),
            kind: 1,
            emoticons: vec![Emoticon {
                id: 1,
                unique_name: "x_1".into(),
                text: "啊".into(),
                url: "https://a".into(),
                width: 0,
                height: 0,
                perm: 0,
            }],
        };
        let mut packages = vec![one(), one()];
        dedupe(&mut packages);
        assert_eq!(packages.len(), 1);
    }
}
