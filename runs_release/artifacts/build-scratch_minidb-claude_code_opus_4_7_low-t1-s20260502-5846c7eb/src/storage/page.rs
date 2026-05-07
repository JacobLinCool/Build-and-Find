use crate::error::{MiniDbError, Result};
use crate::types::{DataType, Value};

pub const PAGE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PageId(pub u64);

impl PageId {
    pub const META: PageId = PageId(0);
    pub fn new(v: u64) -> Self {
        PageId(v)
    }
    pub fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct Page {
    pub id: PageId,
    pub data: Vec<u8>,
}

impl Page {
    pub fn empty(id: PageId) -> Self {
        Page {
            id,
            data: vec![0u8; PAGE_SIZE],
        }
    }
}

/// Encode a slice of typed values into a binary tuple representation.
pub fn encode_tuple(values: &[Value]) -> Vec<u8> {
    let mut out = Vec::with_capacity(64);
    out.extend_from_slice(&(values.len() as u16).to_le_bytes());
    for v in values {
        match v {
            Value::Null => out.push(0),
            Value::Int(i) => {
                out.push(1);
                out.extend_from_slice(&i.to_le_bytes());
            }
            Value::Real(r) => {
                out.push(2);
                out.extend_from_slice(&r.to_le_bytes());
            }
            Value::Text(s) => {
                out.push(3);
                let b = s.as_bytes();
                out.extend_from_slice(&(b.len() as u32).to_le_bytes());
                out.extend_from_slice(b);
            }
            Value::Bool(b) => {
                out.push(4);
                out.push(if *b { 1 } else { 0 });
            }
        }
    }
    out
}

pub fn decode_tuple(buf: &[u8]) -> Result<Vec<Value>> {
    if buf.len() < 2 {
        return Err(MiniDbError::Storage("tuple buffer too short".into()));
    }
    let n = u16::from_le_bytes([buf[0], buf[1]]) as usize;
    let mut pos = 2;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        if pos >= buf.len() {
            return Err(MiniDbError::Storage("tuple truncated".into()));
        }
        let tag = buf[pos];
        pos += 1;
        match tag {
            0 => out.push(Value::Null),
            1 => {
                if pos + 8 > buf.len() {
                    return Err(MiniDbError::Storage("int truncated".into()));
                }
                let mut a = [0u8; 8];
                a.copy_from_slice(&buf[pos..pos + 8]);
                pos += 8;
                out.push(Value::Int(i64::from_le_bytes(a)));
            }
            2 => {
                if pos + 8 > buf.len() {
                    return Err(MiniDbError::Storage("real truncated".into()));
                }
                let mut a = [0u8; 8];
                a.copy_from_slice(&buf[pos..pos + 8]);
                pos += 8;
                out.push(Value::Real(f64::from_le_bytes(a)));
            }
            3 => {
                if pos + 4 > buf.len() {
                    return Err(MiniDbError::Storage("text len truncated".into()));
                }
                let mut a = [0u8; 4];
                a.copy_from_slice(&buf[pos..pos + 4]);
                let len = u32::from_le_bytes(a) as usize;
                pos += 4;
                if pos + len > buf.len() {
                    return Err(MiniDbError::Storage("text payload truncated".into()));
                }
                let s = std::str::from_utf8(&buf[pos..pos + len])
                    .map_err(|_| MiniDbError::Storage("invalid utf8 in tuple".into()))?
                    .to_string();
                pos += len;
                out.push(Value::Text(s));
            }
            4 => {
                if pos >= buf.len() {
                    return Err(MiniDbError::Storage("bool truncated".into()));
                }
                let b = buf[pos] != 0;
                pos += 1;
                out.push(Value::Bool(b));
            }
            other => return Err(MiniDbError::Storage(format!("unknown tuple tag {}", other))),
        }
    }
    Ok(out)
}

/// Coerce decoded raw values to declared column types (mainly to align Int/Real).
pub fn coerce_row(values: Vec<Value>, types: &[DataType]) -> Result<Vec<Value>> {
    if values.len() != types.len() {
        return Err(MiniDbError::Type(format!(
            "row width {} does not match schema width {}",
            values.len(),
            types.len()
        )));
    }
    values
        .into_iter()
        .zip(types.iter())
        .map(|(v, t)| v.coerce(*t))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        let row = vec![
            Value::Int(42),
            Value::Real(3.14),
            Value::Text("hi".into()),
            Value::Null,
            Value::Bool(true),
        ];
        let bytes = encode_tuple(&row);
        let back = decode_tuple(&bytes).unwrap();
        assert_eq!(row, back);
    }

    #[test]
    fn page_size_constant() {
        assert_eq!(PAGE_SIZE, 4096);
    }
}
