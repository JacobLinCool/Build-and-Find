use crate::error::{NanowebError, Result};
use crate::http::Request;
use serde::de::DeserializeOwned;
use std::collections::HashMap;

pub fn parse_typed<T: DeserializeOwned>(req: &Request) -> Result<T> {
    let ct = req.header("content-type").unwrap_or("");
    if ct.starts_with("application/json") {
        Ok(serde_json::from_slice(&req.body)?)
    } else if ct.starts_with("application/x-www-form-urlencoded") {
        Ok(serde_urlencoded::from_bytes(&req.body)?)
    } else if ct.starts_with("multipart/form-data") {
        let fields = parse_multipart(req)?;
        let value = serde_json::to_value(fields)?;
        Ok(serde_json::from_value(value)?)
    } else {
        Err(NanowebError::Other("unsupported content type".into()))
    }
}

pub fn parse_multipart(req: &Request) -> Result<HashMap<String, String>> {
    let ct = req.header("content-type").unwrap_or("");
    let boundary = ct
        .split(';')
        .find_map(|p| p.trim().strip_prefix("boundary="))
        .ok_or_else(|| NanowebError::Parse("missing multipart boundary".into()))?;
    let marker = format!("--{boundary}");
    let body = String::from_utf8_lossy(&req.body);
    let mut out = HashMap::new();
    for part in body.split(&marker).skip(1) {
        if part.starts_with("--") {
            break;
        }
        let Some((raw_headers, data)) = part.trim_start_matches("\r\n").split_once("\r\n\r\n")
        else {
            continue;
        };
        let name = raw_headers.lines().find_map(|l| {
            l.split(';').find_map(|p| {
                p.trim()
                    .strip_prefix("name=\"")
                    .and_then(|x| x.strip_suffix('"'))
            })
        });
        if let Some(name) = name {
            out.insert(name.to_string(), data.trim_end_matches("\r\n").to_string());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::Method;

    #[test]
    fn parses_urlencoded() {
        let mut h = HashMap::new();
        h.insert(
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        );
        let req = Request {
            method: Method::Post,
            target: "/".into(),
            version: "HTTP/1.1".into(),
            headers: h,
            body: b"a=b".to_vec(),
            params: HashMap::new(),
            client_ip: None,
            request_id: "r".into(),
        };
        let v: HashMap<String, String> = parse_typed(&req).unwrap();
        assert_eq!(v["a"], "b");
    }
}
