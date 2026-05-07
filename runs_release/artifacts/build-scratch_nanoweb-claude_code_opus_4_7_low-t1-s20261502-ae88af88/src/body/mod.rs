pub mod form;
pub mod multipart;

use crate::error::{NanowebError, Result};
use crate::http::Request;
use serde::de::DeserializeOwned;

pub fn parse_body<T: DeserializeOwned>(req: &Request) -> Result<T> {
    let ct = req
        .header("Content-Type")
        .unwrap_or("application/json")
        .to_ascii_lowercase();
    if ct.starts_with("application/json") {
        return Ok(serde_json::from_slice(&req.body)?);
    }
    if ct.starts_with("application/x-www-form-urlencoded") {
        let pairs = form::parse_urlencoded(&req.body);
        let map: std::collections::HashMap<String, String> = pairs.into_iter().collect();
        let v = serde_json::to_value(map)?;
        return Ok(serde_json::from_value(v)?);
    }
    if ct.starts_with("multipart/form-data") {
        let boundary = ct
            .split(';')
            .find_map(|p| {
                let p = p.trim();
                p.strip_prefix("boundary=")
                    .map(|s| s.trim_matches('"').to_string())
            })
            .ok_or_else(|| NanowebError::BadRequest("missing multipart boundary".into()))?;
        let parts = multipart::parse(&req.body, &boundary)?;
        let mut map = serde_json::Map::new();
        for p in parts {
            map.insert(
                p.name,
                serde_json::Value::String(String::from_utf8_lossy(&p.data).into_owned()),
            );
        }
        return Ok(serde_json::from_value(serde_json::Value::Object(map))?);
    }
    Err(NanowebError::BadRequest(format!(
        "unsupported content-type: {}",
        ct
    )))
}
