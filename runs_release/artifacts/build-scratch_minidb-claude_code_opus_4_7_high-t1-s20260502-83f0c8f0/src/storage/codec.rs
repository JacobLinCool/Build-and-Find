//! Compact binary encoding for `Value` rows used by the heap and the WAL.
//!
//! Each row is encoded as `u8 num_columns` followed by `num_columns` tagged values.
//! The tags are: `0x00 NULL`, `0x01 INT (i64)`, `0x02 REAL (f64)`, `0x03 TEXT (u32 len + bytes)`,
//! `0x04 BOOL (u8)`.

use crate::error::{MiniDbError, Result};
use crate::types::Value;

const TAG_NULL: u8 = 0x00;
const TAG_INT: u8 = 0x01;
const TAG_REAL: u8 = 0x02;
const TAG_TEXT: u8 = 0x03;
const TAG_BOOL: u8 = 0x04;

pub fn encode_row(values: &[Value], out: &mut Vec<u8>) -> Result<()> {
    if values.len() > u8::MAX as usize {
        return Err(MiniDbError::storage("row too wide"));
    }
    out.push(values.len() as u8);
    for v in values {
        encode_value(v, out)?;
    }
    Ok(())
}

pub fn encode_value(v: &Value, out: &mut Vec<u8>) -> Result<()> {
    match v {
        Value::Null => out.push(TAG_NULL),
        Value::Int(n) => {
            out.push(TAG_INT);
            out.extend_from_slice(&n.to_le_bytes());
        }
        Value::Real(r) => {
            out.push(TAG_REAL);
            out.extend_from_slice(&r.to_le_bytes());
        }
        Value::Text(s) => {
            out.push(TAG_TEXT);
            let bytes = s.as_bytes();
            if bytes.len() > u32::MAX as usize {
                return Err(MiniDbError::storage("text value too long"));
            }
            out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            out.extend_from_slice(bytes);
        }
        Value::Bool(b) => {
            out.push(TAG_BOOL);
            out.push(if *b { 1 } else { 0 });
        }
    }
    Ok(())
}

pub fn decode_row(buf: &[u8]) -> Result<(Vec<Value>, usize)> {
    if buf.is_empty() {
        return Err(MiniDbError::storage("truncated row"));
    }
    let n = buf[0] as usize;
    let mut pos = 1;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let (v, c) = decode_value(&buf[pos..])?;
        out.push(v);
        pos += c;
    }
    Ok((out, pos))
}

pub fn decode_value(buf: &[u8]) -> Result<(Value, usize)> {
    if buf.is_empty() {
        return Err(MiniDbError::storage("truncated value"));
    }
    match buf[0] {
        TAG_NULL => Ok((Value::Null, 1)),
        TAG_INT => {
            if buf.len() < 9 {
                return Err(MiniDbError::storage("truncated int"));
            }
            let mut bs = [0u8; 8];
            bs.copy_from_slice(&buf[1..9]);
            Ok((Value::Int(i64::from_le_bytes(bs)), 9))
        }
        TAG_REAL => {
            if buf.len() < 9 {
                return Err(MiniDbError::storage("truncated real"));
            }
            let mut bs = [0u8; 8];
            bs.copy_from_slice(&buf[1..9]);
            Ok((Value::Real(f64::from_le_bytes(bs)), 9))
        }
        TAG_TEXT => {
            if buf.len() < 5 {
                return Err(MiniDbError::storage("truncated text"));
            }
            let mut ls = [0u8; 4];
            ls.copy_from_slice(&buf[1..5]);
            let l = u32::from_le_bytes(ls) as usize;
            if buf.len() < 5 + l {
                return Err(MiniDbError::storage("truncated text body"));
            }
            let s = std::str::from_utf8(&buf[5..5 + l])
                .map_err(|_| MiniDbError::storage("invalid utf8 in text"))?
                .to_string();
            Ok((Value::Text(s), 5 + l))
        }
        TAG_BOOL => {
            if buf.len() < 2 {
                return Err(MiniDbError::storage("truncated bool"));
            }
            Ok((Value::Bool(buf[1] != 0), 2))
        }
        other => Err(MiniDbError::storage(format!(
            "unknown value tag: {}",
            other
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(values: Vec<Value>) {
        let mut buf = Vec::new();
        encode_row(&values, &mut buf).unwrap();
        let (got, used) = decode_row(&buf).unwrap();
        assert_eq!(got, values);
        assert_eq!(used, buf.len());
    }

    #[test]
    fn round_trip_simple() {
        roundtrip(vec![Value::Int(42), Value::Text("hello".into())]);
        roundtrip(vec![Value::Null, Value::Bool(true), Value::Real(3.14)]);
        roundtrip(vec![]);
    }

    #[test]
    fn detect_truncation() {
        let mut buf = Vec::new();
        encode_row(&[Value::Int(1), Value::Text("ab".into())], &mut buf).unwrap();
        let truncated = &buf[..buf.len() - 1];
        assert!(decode_row(truncated).is_err());
    }
}
