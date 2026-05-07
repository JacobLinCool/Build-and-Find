use crate::http::Request;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use std::collections::HashMap;

pub fn parse<T: DeserializeOwned>(req: &Request) -> std::result::Result<T, String> {
    let ct = req.header("content-type").unwrap_or("");
    if ct.starts_with("application/json") {
        serde_json::from_slice(&req.body).map_err(|e| e.to_string())
    } else if ct.starts_with("application/x-www-form-urlencoded") {
        let pairs: HashMap<String, String> = url::form_urlencoded::parse(&req.body)
            .into_owned()
            .collect();
        let value = serde_json::to_value(pairs).map_err(|e| e.to_string())?;
        serde_json::from_value(value).map_err(|e| e.to_string())
    } else if ct.starts_with("multipart/form-data") {
        let boundary = ct
            .split(';')
            .find_map(|p| p.trim().strip_prefix("boundary="))
            .ok_or_else(|| "multipart boundary missing".to_string())?;
        let value = parse_multipart(&req.body, boundary)?;
        serde_json::from_value(value).map_err(|e| e.to_string())
    } else {
        Err("unsupported content type".into())
    }
}

fn parse_multipart(body: &[u8], boundary: &str) -> std::result::Result<Value, String> {
    let marker = format!("--{boundary}");
    let s = String::from_utf8_lossy(body);
    let mut map = Map::new();
    for part in s.split(&marker).skip(1) {
        let part = part.trim_start_matches("\r\n").trim_end_matches("\r\n");
        if part == "--" || part.is_empty() {
            continue;
        }
        let (headers, value) = part
            .split_once("\r\n\r\n")
            .ok_or_else(|| "malformed multipart part".to_string())?;
        let name = headers.lines().find_map(|line| {
            let lower = line.to_ascii_lowercase();
            if !lower.starts_with("content-disposition:") {
                return None;
            }
            line.split(';').find_map(|seg| {
                seg.trim()
                    .strip_prefix("name=\"")
                    .and_then(|v| v.strip_suffix('"'))
                    .map(ToOwned::to_owned)
            })
        });
        if let Some(name) = name {
            map.insert(
                name,
                Value::String(value.trim_end_matches("\r\n").to_string()),
            );
        }
    }
    Ok(Value::Object(map))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{Method, Request};
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Form {
        a: String,
    }

    #[test]
    fn parses_form() {
        let mut req = Request {
            method: Method::Post,
            target: "/".into(),
            path: "/".into(),
            query: None,
            version: "HTTP/1.1".into(),
            headers: HashMap::new(),
            body: b"a=b".to_vec(),
            peer_addr: None,
            request_id: "r".into(),
            params: HashMap::new(),
        };
        req.headers.insert(
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        );
        let form: Form = parse(&req).unwrap();
        assert_eq!(form.a, "b");
    }
}
