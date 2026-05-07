use crate::error::NanowebError;
use percent_encoding::percent_decode_str;
use serde::de::DeserializeOwned;
use std::collections::HashMap;

pub fn parse_json<T: DeserializeOwned>(body: &[u8]) -> Result<T, NanowebError> {
    serde_json::from_slice(body).map_err(|e| NanowebError::Parse(e.to_string()))
}

pub fn parse_form<T: DeserializeOwned>(body: &[u8]) -> Result<T, NanowebError> {
    let s = std::str::from_utf8(body).map_err(|e| NanowebError::Parse(e.to_string()))?;
    let mut map = HashMap::new();
    for pair in s.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        map.insert(
            percent_decode_str(k).decode_utf8_lossy().to_string(),
            percent_decode_str(v).decode_utf8_lossy().to_string(),
        );
    }
    let json = serde_json::to_vec(&map).map_err(|e| NanowebError::Parse(e.to_string()))?;
    serde_json::from_slice(&json).map_err(|e| NanowebError::Parse(e.to_string()))
}

pub fn parse_multipart(
    body: &[u8],
    boundary: &str,
) -> Result<HashMap<String, Vec<u8>>, NanowebError> {
    let mut out = HashMap::new();
    let marker = format!("--{}", boundary);
    let text = std::str::from_utf8(body).map_err(|e| NanowebError::Parse(e.to_string()))?;
    for part in text.split(&marker).skip(1) {
        if part.starts_with("--") {
            break;
        }
        let part = part.trim_start_matches("\r\n");
        let (headers, content) = part
            .split_once("\r\n\r\n")
            .ok_or_else(|| NanowebError::parse("invalid multipart"))?;
        let name = headers
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .contains("content-disposition")
                    .then(|| {
                        line.split(';').find_map(|kv| {
                            let kv = kv.trim();
                            kv.strip_prefix("name=")
                                .map(|n| n.trim_matches('"').to_string())
                        })
                    })
                    .flatten()
            })
            .ok_or_else(|| NanowebError::parse("missing multipart name"))?;
        out.insert(name, content.trim_end_matches("\r\n").as_bytes().to_vec());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(serde::Deserialize, PartialEq, Debug)]
    struct X {
        a: String,
    }
    #[test]
    fn json_parses() {
        let x: X = parse_json(br#"{"a":"b"}"#).unwrap();
        assert_eq!(x.a, "b");
    }
}
