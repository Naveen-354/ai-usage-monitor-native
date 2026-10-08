//! A minimal, strict protobuf *wire format* reader (no schema, no codegen).
//!
//! Antigravity stores per-generation metadata as protobuf blobs. The field numbers used by the decoder
//! were read out of the descriptors embedded in `agy.exe`, never guessed (docs/COLLECTORS.md). This module
//! only knows the wire format. It is bounds-checked everywhere and never panics on arbitrary bytes.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
    Fixed64(u64),
    Fixed32(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field<'a> {
    pub num: u32,
    pub value: Value<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireError {
    Truncated,
    VarintTooLong,
    BadFieldNumber,
    UnsupportedWireType(u8),
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WireError::Truncated => write!(f, "truncated protobuf"),
            WireError::VarintTooLong => write!(f, "varint longer than 10 bytes"),
            WireError::BadFieldNumber => write!(f, "invalid field number"),
            WireError::UnsupportedWireType(w) => write!(f, "unsupported wire type {w}"),
        }
    }
}

impl std::error::Error for WireError {}

/// A decoded message: its fields in order of appearance (repeated fields appear repeatedly).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Message<'a> {
    pub fields: Vec<Field<'a>>,
}

fn read_varint(buf: &[u8], pos: &mut usize) -> Result<u64, WireError> {
    let mut result: u64 = 0;
    for i in 0..10 {
        let b = *buf.get(*pos).ok_or(WireError::Truncated)?;
        *pos += 1;
        // The 10th byte may only contribute the top bit of a u64.
        if i == 9 && b > 1 {
            return Err(WireError::VarintTooLong);
        }
        result |= u64::from(b & 0x7f) << (7 * i);
        if b & 0x80 == 0 {
            return Ok(result);
        }
    }
    Err(WireError::VarintTooLong)
}

/// Parse a whole buffer as one message. Fails if any byte is left over or malformed.
pub fn parse(buf: &[u8]) -> Result<Message<'_>, WireError> {
    let mut fields = Vec::new();
    let mut pos = 0usize;
    while pos < buf.len() {
        let tag = read_varint(buf, &mut pos)?;
        let num = u32::try_from(tag >> 3).map_err(|_| WireError::BadFieldNumber)?;
        if num == 0 {
            return Err(WireError::BadFieldNumber);
        }
        let value = match (tag & 7) as u8 {
            0 => Value::Varint(read_varint(buf, &mut pos)?),
            1 => {
                let end = pos.checked_add(8).filter(|e| *e <= buf.len()).ok_or(WireError::Truncated)?;
                let v = u64::from_le_bytes(buf[pos..end].try_into().expect("8 bytes"));
                pos = end;
                Value::Fixed64(v)
            }
            2 => {
                let len = usize::try_from(read_varint(buf, &mut pos)?).map_err(|_| WireError::Truncated)?;
                let end = pos.checked_add(len).filter(|e| *e <= buf.len()).ok_or(WireError::Truncated)?;
                let v = &buf[pos..end];
                pos = end;
                Value::Bytes(v)
            }
            5 => {
                let end = pos.checked_add(4).filter(|e| *e <= buf.len()).ok_or(WireError::Truncated)?;
                let v = u32::from_le_bytes(buf[pos..end].try_into().expect("4 bytes"));
                pos = end;
                Value::Fixed32(v)
            }
            w => return Err(WireError::UnsupportedWireType(w)),
        };
        fields.push(Field { num, value });
    }
    Ok(Message { fields })
}

impl<'a> Message<'a> {
    /// First varint field `num`.
    pub fn varint(&self, num: u32) -> Option<u64> {
        self.fields.iter().find_map(|f| match (f.num == num, f.value) {
            (true, Value::Varint(v)) => Some(v),
            _ => None,
        })
    }

    pub fn bytes(&self, num: u32) -> Option<&'a [u8]> {
        self.fields.iter().find_map(|f| match (f.num == num, f.value) {
            (true, Value::Bytes(b)) => Some(b),
            _ => None,
        })
    }

    /// First length-delimited field `num`, parsed as a nested message.
    pub fn message(&self, num: u32) -> Option<Message<'a>> {
        parse(self.bytes(num)?).ok()
    }

    /// First length-delimited field `num` as UTF-8 text.
    pub fn string(&self, num: u32) -> Option<&'a str> {
        std::str::from_utf8(self.bytes(num)?).ok()
    }

    /// `repeated uint32` in either encoding: packed (one bytes field of varints) or one varint per entry.
    pub fn repeated_varints(&self, num: u32) -> Vec<u64> {
        let mut out = Vec::new();
        for f in &self.fields {
            if f.num != num {
                continue;
            }
            match f.value {
                Value::Varint(v) => out.push(v),
                Value::Bytes(b) => {
                    let mut pos = 0;
                    while pos < b.len() {
                        match read_varint(b, &mut pos) {
                            Ok(v) => out.push(v),
                            Err(_) => break,
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }
}

/// Encoder used only by tests, to build fixtures in the documented layout.
#[cfg(test)]
pub mod enc {
    pub fn varint(mut v: u64) -> Vec<u8> {
        let mut out = Vec::new();
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                return out;
            }
            out.push(b | 0x80);
        }
    }

    pub fn tag(num: u32, wire: u8) -> Vec<u8> {
        varint((u64::from(num) << 3) | u64::from(wire))
    }

    pub fn v(num: u32, value: u64) -> Vec<u8> {
        [tag(num, 0), varint(value)].concat()
    }

    pub fn bytes(num: u32, data: &[u8]) -> Vec<u8> {
        [tag(num, 2), varint(data.len() as u64), data.to_vec()].concat()
    }

    pub fn s(num: u32, text: &str) -> Vec<u8> {
        bytes(num, text.as_bytes())
    }

    pub fn msg(num: u32, parts: &[Vec<u8>]) -> Vec<u8> {
        bytes(num, &parts.concat())
    }
}

#[cfg(test)]
mod tests {
    use super::enc::*;
    use super::*;

    #[test]
    fn parses_varints_strings_and_nested_messages() {
        let blob = [v(1, 300), s(2, "héllo"), msg(3, &[v(1, 7), s(2, "inner")])].concat();
        let m = parse(&blob).unwrap();
        assert_eq!(m.varint(1), Some(300));
        assert_eq!(m.string(2), Some("héllo"));
        let inner = m.message(3).unwrap();
        assert_eq!((inner.varint(1), inner.string(2)), (Some(7), Some("inner")));
        assert_eq!(m.varint(99), None);
    }

    #[test]
    fn varint_edge_values_round_trip() {
        for val in [0u64, 1, 127, 128, 16_383, 16_384, u32::MAX as u64, u64::MAX] {
            let buf = v(1, val);
            let m = parse(&buf).unwrap();
            assert_eq!(m.varint(1), Some(val), "{val}");
        }
    }

    #[test]
    fn repeated_fields_support_both_packed_and_unpacked() {
        let packed = bytes(2, &[varint(5), varint(300), varint(7)].concat());
        assert_eq!(parse(&packed).unwrap().repeated_varints(2), vec![5, 300, 7]);
        let unpacked = [v(2, 5), v(2, 300)].concat();
        assert_eq!(parse(&unpacked).unwrap().repeated_varints(2), vec![5, 300]);
    }

    #[test]
    fn fixed_width_fields_are_skipped_over_correctly() {
        let blob = [tag(1, 1), 1u64.to_le_bytes().to_vec(), tag(2, 5), 9u32.to_le_bytes().to_vec(), v(3, 42)].concat();
        let m = parse(&blob).unwrap();
        assert_eq!(m.varint(3), Some(42));
        assert_eq!(m.fields.len(), 3);
    }

    #[test]
    fn malformed_input_is_an_error_not_a_panic() {
        assert_eq!(parse(&[0x0a]).unwrap_err(), WireError::Truncated); // tag, no length
        assert_eq!(parse(&[0x0a, 0x05, 1, 2]).unwrap_err(), WireError::Truncated); // length past the end
        assert_eq!(parse(&[0x08]).unwrap_err(), WireError::Truncated); // varint value missing
        assert_eq!(parse(&[0x00, 0x01]).unwrap_err(), WireError::BadFieldNumber); // field number 0
        assert_eq!(parse(&[0x0b]).unwrap_err(), WireError::UnsupportedWireType(3)); // start-group
        assert_eq!(parse(&[0x08, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f]).unwrap_err(), WireError::VarintTooLong);
        assert!(parse(&[]).unwrap().fields.is_empty());
    }

    #[test]
    fn a_huge_declared_length_cannot_overflow_or_allocate() {
        let blob = [tag(1, 2), varint(u64::MAX)].concat();
        assert!(parse(&blob).is_err());
    }

    /// Property test: no input, however corrupt, may panic. 30k random buffers + mutations of a valid one.
    #[test]
    fn arbitrary_bytes_never_panic() {
        let mut seed = 0x1234_5678_9abc_def0u64;
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as u32
        };
        for _ in 0..20_000 {
            let len = (next() % 64) as usize;
            let buf: Vec<u8> = (0..len).map(|_| next() as u8).collect();
            let _ = parse(&buf).map(|m| (m.varint(1), m.string(2), m.message(3), m.repeated_varints(4)));
        }
        let valid = [v(1, 300), s(2, "x"), msg(3, &[v(1, 7), s(2, "y")]), bytes(4, &[1, 2, 3])].concat();
        for _ in 0..10_000 {
            let mut b = valid.clone();
            for _ in 0..(1 + next() % 4) {
                let i = (next() as usize) % b.len();
                b[i] = next() as u8;
            }
            if next() % 3 == 0 {
                b.truncate((next() as usize) % b.len());
            }
            let _ = parse(&b).map(|m| (m.varint(1), m.message(3)));
        }
    }
}
