//! B 站直播弹幕 WebSocket 的二进制帧编解码。
//!
//! 一帧固定 16 字节头 + 负载：
//!
//! | 偏移 | 长度 | 字段     | 说明                                              |
//! |------|------|----------|---------------------------------------------------|
//! | 0    | 4    | 包总长度 | 大端 u32，含头                                    |
//! | 4    | 2    | 头长度   | 固定 16（大端 u16）                               |
//! | 6    | 2    | 协议版本 | 0 明文 / 1 人气值 / 2 zlib / 3 brotli             |
//! | 8    | 4    | 操作码   | 2 心跳 / 3 心跳回复 / 5 消息 / 7 认证 / 8 认证回复 |
//! | 12   | 4    | 序号     | 固定 1                                            |
//!
//! 协议版本 2/3 的负载是「压缩后的若干完整帧」，需要解压后递归解析。

use crate::error::{Error, Result};
use serde::Serialize;

/// 头部长度
pub const HEADER_LEN: usize = 16;

pub const OP_HEARTBEAT: u32 = 2;
pub const OP_HEARTBEAT_REPLY: u32 = 3;
pub const OP_MESSAGE: u32 = 5;
pub const OP_AUTH: u32 = 7;
pub const OP_AUTH_REPLY: u32 = 8;

pub const PROTO_JSON: u16 = 0;
pub const PROTO_POPULARITY: u16 = 1;
pub const PROTO_ZLIB: u16 = 2;
pub const PROTO_BROTLI: u16 = 3;

/// 解压后的一帧（协议版本 0/1，负载已是明文）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub op: u32,
    pub proto: u16,
    pub body: Vec<u8>,
}

impl Frame {
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T> {
        Ok(serde_json::from_slice(&self.body)?)
    }

    pub fn as_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.body).ok()
    }
}

/// 按头部构造一帧。
pub fn encode(op: u32, proto: u16, body: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(HEADER_LEN + body.len());
    buf.extend_from_slice(&((HEADER_LEN + body.len()) as u32).to_be_bytes());
    buf.extend_from_slice(&(HEADER_LEN as u16).to_be_bytes());
    buf.extend_from_slice(&proto.to_be_bytes());
    buf.extend_from_slice(&op.to_be_bytes());
    buf.extend_from_slice(&1u32.to_be_bytes());
    buf.extend_from_slice(body);
    buf
}

pub fn encode_json<T: Serialize>(op: u32, value: &T) -> Result<Vec<u8>> {
    Ok(encode(op, PROTO_JSON, &serde_json::to_vec(value)?))
}

/// 心跳包。负载用官方网页端那串 "[object Object]"，空负载同样被接受。
pub fn heartbeat() -> Vec<u8> {
    encode(OP_HEARTBEAT, PROTO_JSON, b"[object Object]")
}

/// 认证（进房）包。
#[derive(Debug, Serialize)]
pub struct AuthPayload<'a> {
    pub uid: u64,
    pub roomid: u64,
    pub protover: u16,
    pub buvid: &'a str,
    pub platform: &'a str,
    #[serde(rename = "type")]
    pub kind: u8,
    pub key: &'a str,
}

pub fn auth(uid: u64, room_id: u64, buvid: &str, token: &str) -> Result<Vec<u8>> {
    let payload = AuthPayload {
        uid,
        roomid: room_id,
        protover: PROTO_BROTLI,
        buvid,
        platform: "web",
        kind: 2,
        key: token,
    };
    encode_json(OP_AUTH, &payload)
}

/// 单帧解压后的最大明文长度。正常一帧几 KB，留 8MB 足够，
/// 同时挡住「小压缩包放大成几个 GB」的压缩炸弹。
pub const MAX_PLAIN_BYTES: usize = 8 * 1024 * 1024;

fn check_plain_size(len: usize) -> Result<()> {
    if len >= MAX_PLAIN_BYTES {
        return Err(Error::Packet(format!(
            "解压后的数据超过上限（{} 字节），疑似压缩炸弹",
            MAX_PLAIN_BYTES
        )));
    }
    Ok(())
}

/// 解出一段数据里的所有帧（自动处理 zlib / brotli 嵌套压缩）。
pub fn decode(buf: &[u8]) -> Result<Vec<Frame>> {
    let mut out = Vec::new();
    decode_into(buf, &mut out, 0)?;
    Ok(out)
}

fn decode_into(buf: &[u8], out: &mut Vec<Frame>, depth: usize) -> Result<()> {
    if depth > 4 {
        return Err(Error::Packet("压缩嵌套层数过多".into()));
    }
    let mut cursor = 0usize;
    while cursor + HEADER_LEN <= buf.len() {
        let total_len = u32::from_be_bytes(buf[cursor..cursor + 4].try_into().unwrap()) as usize;
        let header_len =
            u16::from_be_bytes(buf[cursor + 4..cursor + 6].try_into().unwrap()) as usize;
        let proto = u16::from_be_bytes(buf[cursor + 6..cursor + 8].try_into().unwrap());
        let op = u32::from_be_bytes(buf[cursor + 8..cursor + 12].try_into().unwrap());

        if total_len < HEADER_LEN
            || header_len < HEADER_LEN
            // header_len 超过 total_len 会让下面的切片 start > end 直接 panic
            || header_len > total_len
            || total_len > buf.len() - cursor
        {
            return Err(Error::Packet(format!(
                "帧长度非法: total={} header={} remain={}",
                total_len,
                header_len,
                buf.len() - cursor
            )));
        }
        let body = &buf[cursor + header_len..cursor + total_len];
        cursor += total_len;

        match proto {
            PROTO_ZLIB => {
                use std::io::Read;
                let mut decoder = flate2::read::ZlibDecoder::new(body).take(MAX_PLAIN_BYTES as u64);
                let mut plain = Vec::new();
                decoder
                    .read_to_end(&mut plain)
                    .map_err(|e| Error::Packet(format!("zlib 解压失败: {e}")))?;
                check_plain_size(plain.len())?;
                decode_into(&plain, out, depth + 1)?;
            }
            PROTO_BROTLI => {
                use std::io::Read;
                let mut plain = Vec::new();
                let mut decoder = brotli::Decompressor::new(body, 8192);
                (&mut decoder)
                    .take(MAX_PLAIN_BYTES as u64)
                    .read_to_end(&mut plain)
                    .map_err(|e| Error::Packet(format!("brotli 解压失败: {e}")))?;
                check_plain_size(plain.len())?;
                decode_into(&plain, out, depth + 1)?;
            }
            _ => out.push(Frame {
                op,
                proto,
                body: body.to_vec(),
            }),
        }
    }
    Ok(())
}

/// 心跳回复里的大端 u32 人气值。
pub fn parse_popularity(frame: &Frame) -> Option<u64> {
    if frame.body.len() >= 4 {
        Some(u32::from_be_bytes(frame.body[..4].try_into().ok()?) as u64)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_layout_matches_bilibili_spec() {
        let raw = encode(OP_AUTH, PROTO_JSON, b"{}");
        assert_eq!(raw.len(), 18);
        assert_eq!(&raw[0..4], &18u32.to_be_bytes());
        assert_eq!(&raw[4..6], &16u16.to_be_bytes());
        assert_eq!(&raw[6..8], &0u16.to_be_bytes());
        assert_eq!(&raw[8..12], &7u32.to_be_bytes());
        assert_eq!(&raw[12..16], &1u32.to_be_bytes());
        assert_eq!(&raw[16..], b"{}");
    }

    #[test]
    fn decodes_plain_frame() {
        let raw = encode_json(OP_AUTH_REPLY, &serde_json::json!({"code": 0})).unwrap();
        let frames = decode(&raw).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].op, OP_AUTH_REPLY);
        assert_eq!(frames[0].json::<serde_json::Value>().unwrap()["code"], 0);
    }

    #[test]
    fn decodes_zlib_nested_frames() {
        use flate2::Compression;
        use flate2::write::ZlibEncoder;
        use std::io::Write;

        let a = encode_json(OP_MESSAGE, &serde_json::json!({"cmd": "A"})).unwrap();
        let b = encode_json(OP_MESSAGE, &serde_json::json!({"cmd": "B"})).unwrap();
        let mut plain = a.clone();
        plain.extend_from_slice(&b);

        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&plain).unwrap();
        let compressed = encoder.finish().unwrap();

        let outer = encode(OP_MESSAGE, PROTO_ZLIB, &compressed);
        let frames = decode(&outer).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].json::<serde_json::Value>().unwrap()["cmd"], "A");
        assert_eq!(frames[1].json::<serde_json::Value>().unwrap()["cmd"], "B");
    }

    #[test]
    fn decodes_brotli_nested_frames() {
        use std::io::Write;
        let a = encode_json(OP_MESSAGE, &serde_json::json!({"cmd": "X"})).unwrap();
        let mut compressed = Vec::new();
        {
            let mut writer = brotli::CompressorWriter::new(&mut compressed, 4096, 5, 22);
            writer.write_all(&a).unwrap();
        }
        let outer = encode(OP_MESSAGE, PROTO_BROTLI, &compressed);
        let frames = decode(&outer).unwrap();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].json::<serde_json::Value>().unwrap()["cmd"], "X");
    }

    #[test]
    fn heartbeat_frame_roundtrip() {
        let raw = heartbeat();
        let frames = decode(&raw).unwrap();
        assert_eq!(frames[0].op, OP_HEARTBEAT);
    }

    #[test]
    fn auth_payload_has_expected_fields() {
        let raw = auth(0, 689422, "buvid", "token-abc").unwrap();
        let frames = decode(&raw).unwrap();
        let value: serde_json::Value = frames[0].json().unwrap();
        assert_eq!(value["uid"], 0);
        assert_eq!(value["roomid"], 689422);
        assert_eq!(value["protover"], 3);
        assert_eq!(value["platform"], "web");
        assert_eq!(value["type"], 2);
        assert_eq!(value["key"], "token-abc");
    }

    #[test]
    fn rejects_truncated_packet() {
        let raw = encode(OP_MESSAGE, PROTO_JSON, b"{}");
        assert!(decode(&raw[..10]).unwrap().is_empty());
        let mut broken = raw.clone();
        broken[0..4].copy_from_slice(&999u32.to_be_bytes());
        assert!(decode(&broken).is_err());
    }

    #[test]
    fn rejects_header_longer_than_packet() {
        // header_len=32、total_len=16：修复前这里会 start > end panic
        let mut raw = vec![0u8; 32];
        raw[0..4].copy_from_slice(&16u32.to_be_bytes());
        raw[4..6].copy_from_slice(&32u16.to_be_bytes());
        raw[6..8].copy_from_slice(&0u16.to_be_bytes());
        raw[8..12].copy_from_slice(&5u32.to_be_bytes());
        assert!(decode(&raw).is_err());
    }

    #[test]
    fn rejects_decompression_bomb() {
        use flate2::Compression;
        use flate2::write::ZlibEncoder;
        use std::io::Write;

        // 16MB 的 0，压缩后只有几十 KB
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::best());
        encoder.write_all(&vec![0u8; MAX_PLAIN_BYTES * 2]).unwrap();
        let compressed = encoder.finish().unwrap();
        let outer = encode(OP_MESSAGE, PROTO_ZLIB, &compressed);
        let err = decode(&outer).unwrap_err();
        assert!(
            err.to_string().contains("上限"),
            "错误信息应提示超限: {err}"
        );
    }

    #[test]
    fn popularity_is_big_endian() {
        let frame = Frame {
            op: OP_HEARTBEAT_REPLY,
            proto: PROTO_POPULARITY,
            body: 143_401_516u32.to_be_bytes().to_vec(),
        };
        assert_eq!(parse_popularity(&frame), Some(143_401_516));
    }
}
