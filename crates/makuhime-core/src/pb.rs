//! 极简 protobuf 线格式读取器。
//!
//! B 站新版弹幕（SEND_GIFT_V2、INTERACT_WORD_V2 等）的 data.pb 是 base64 过的
//! protobuf。为了不引入 prost + protoc 构建链，这里只实现够用的 wire 格式解析：
//! 按字段号取 varint / 字符串 / 嵌套消息。
//!
//! 好处是对协议漂移宽容：取不到的字段就当没有，不会 panic。

/// 单字段的值。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
    Fixed32(u32),
    Fixed64(u64),
}

impl<'a> Value<'a> {
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Varint(v) => Some(*v),
            Value::Fixed32(v) => Some(*v as u64),
            Value::Fixed64(v) => Some(*v),
            Value::Bytes(_) => None,
        }
    }

    pub fn as_bytes(&self) -> Option<&'a [u8]> {
        match self {
            Value::Bytes(b) => Some(b),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&'a str> {
        self.as_bytes().and_then(|b| std::str::from_utf8(b).ok())
    }
}

/// 顺序读取 protobuf 字段。
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub fn next_field(&mut self) -> Option<(u32, Value<'a>)> {
        let key = self.read_varint()?;
        let field = (key >> 3) as u32;
        let wire = (key & 0x07) as u8;
        match wire {
            0 => self.read_varint().map(|v| (field, Value::Varint(v))),
            1 => self.read_fixed64().map(|v| (field, Value::Fixed64(v))),
            2 => {
                let len = self.read_varint()? as usize;
                let end = self.pos.checked_add(len)?;
                if end > self.buf.len() {
                    return None;
                }
                let bytes = &self.buf[self.pos..end];
                self.pos = end;
                Some((field, Value::Bytes(bytes)))
            }
            5 => self.read_fixed32().map(|v| (field, Value::Fixed32(v))),
            // 3/4 是已废弃的 group 语义，遇到就放弃解析剩余部分
            _ => None,
        }
    }

    fn read_varint(&mut self) -> Option<u64> {
        let mut result = 0u64;
        let mut shift = 0;
        loop {
            let byte = *self.buf.get(self.pos)?;
            self.pos += 1;
            result |= ((byte & 0x7f) as u64) << shift;
            if byte & 0x80 == 0 {
                return Some(result);
            }
            shift += 7;
            if shift >= 64 {
                return None;
            }
        }
    }

    fn read_fixed32(&mut self) -> Option<u32> {
        let bytes = self.buf.get(self.pos..self.pos + 4)?;
        self.pos += 4;
        Some(u32::from_le_bytes(bytes.try_into().ok()?))
    }

    fn read_fixed64(&mut self) -> Option<u64> {
        let bytes = self.buf.get(self.pos..self.pos + 8)?;
        self.pos += 8;
        Some(u64::from_le_bytes(bytes.try_into().ok()?))
    }
}

/// 取第一个字段号匹配的 varint。
pub fn varint(buf: &[u8], field: u32) -> Option<u64> {
    let mut reader = Reader::new(buf);
    while let Some((number, value)) = reader.next_field() {
        if number == field
            && let Some(v) = value.as_u64()
        {
            return Some(v);
        }
    }
    None
}

/// 取第一个字段号匹配的字符串（非法 UTF-8 会被忽略）。
pub fn string(buf: &[u8], field: u32) -> Option<String> {
    let mut reader = Reader::new(buf);
    while let Some((number, value)) = reader.next_field() {
        if number == field
            && let Some(s) = value.as_str()
        {
            return Some(s.to_owned());
        }
    }
    None
}

/// 取第一个字段号匹配的嵌套消息字节。
pub fn message(buf: &[u8], field: u32) -> Option<&[u8]> {
    let mut reader = Reader::new(buf);
    while let Some((number, value)) = reader.next_field() {
        if number == field {
            return value.as_bytes();
        }
    }
    None
}

/// 取字段号匹配的所有嵌套消息。
pub fn messages(buf: &[u8], field: u32) -> Vec<&[u8]> {
    let mut reader = Reader::new(buf);
    let mut out = Vec::new();
    while let Some((number, value)) = reader.next_field() {
        if number == field
            && let Some(bytes) = value.as_bytes()
        {
            out.push(bytes);
        }
    }
    out
}

/// 按「字段路径」逐层取 varint，例如 path_varint(data, &[8, 5])。
pub fn path_varint(buf: &[u8], path: &[u32]) -> Option<u64> {
    let mut current = buf;
    for (index, field) in path.iter().enumerate() {
        if index + 1 == path.len() {
            return varint(current, *field);
        }
        current = message(current, *field)?;
    }
    None
}

/// 按「字段路径」逐层取字符串。
pub fn path_string(buf: &[u8], path: &[u32]) -> Option<String> {
    let mut current = buf;
    for (index, field) in path.iter().enumerate() {
        if index + 1 == path.len() {
            return string(current, *field);
        }
        current = message(current, *field)?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 手工拼一个 {1: 42, 2: "hi", 3: {1: 7}} 的消息。
    fn sample() -> Vec<u8> {
        let inner = [0x08, 0x07];
        let mut buf = vec![0x08, 0x2a, 0x12, 0x02, b'h', b'i', 0x1a, inner.len() as u8];
        buf.extend_from_slice(&inner);
        buf
    }

    #[test]
    fn reads_scalar_fields() {
        let buf = sample();
        assert_eq!(varint(&buf, 1), Some(42));
        assert_eq!(string(&buf, 2).as_deref(), Some("hi"));
        assert_eq!(message(&buf, 3).unwrap(), &[0x08, 0x07]);
        assert_eq!(path_varint(&buf, &[3, 1]), Some(7));
        assert_eq!(varint(&buf, 9), None);
    }

    #[test]
    fn tolerates_truncated_input() {
        let buf = sample();
        let _ = varint(&buf[..5], 1);
        let _ = string(&buf[..3], 2);
        assert!(Reader::new(&buf[..7]).next_field().is_some());
    }

    #[test]
    fn reads_64bit_varint() {
        let mut buf = vec![0x08];
        let mut value = u64::MAX;
        loop {
            let byte = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                buf.push(byte);
                break;
            }
            buf.push(byte | 0x80);
        }
        assert_eq!(varint(&buf, 1), Some(u64::MAX));
    }

    #[test]
    fn reads_repeated_messages() {
        // {4: {1:1}, 4: {1:2}}
        let buf = [0x22, 0x02, 0x08, 0x01, 0x22, 0x02, 0x08, 0x02];
        let items = messages(&buf, 4);
        assert_eq!(items.len(), 2);
        assert_eq!(varint(items[1], 1), Some(2));
    }
}
