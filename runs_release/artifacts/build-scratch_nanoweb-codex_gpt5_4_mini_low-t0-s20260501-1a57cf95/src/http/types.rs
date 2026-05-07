use serde::de::DeserializeOwned;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Options,
    Other(String),
}

impl Method {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Patch => "PATCH",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
            Self::Other(s) => s,
        }
    }
}

impl From<&str> for Method {
    fn from(s: &str) -> Self {
        match s {
            "GET" => Self::Get,
            "POST" => Self::Post,
            "PUT" => Self::Put,
            "DELETE" => Self::Delete,
            "PATCH" => Self::Patch,
            "HEAD" => Self::Head,
            "OPTIONS" => Self::Options,
            other => Self::Other(other.to_string()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub method: Method,
    pub target: String,
    pub version: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub params: HashMap<String, String>,
    pub peer_addr: Option<std::net::SocketAddr>,
    pub request_id: String,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(|s| s.as_str())
    }
    pub fn body_as<T: DeserializeOwned>(&self) -> Result<T, crate::NanowebError> {
        let ctype = self.header("content-type").unwrap_or("");
        if ctype.contains("application/json") {
            crate::body::parse_json(&self.body)
        } else if ctype.contains("application/x-www-form-urlencoded") {
            crate::body::parse_form(&self.body)
        } else if ctype.contains("multipart/form-data") {
            Err(crate::NanowebError::Parse(
                "multipart requires boundary-aware access".into(),
            ))
        } else {
            Err(crate::NanowebError::Parse(
                "unsupported content type".into(),
            ))
        }
    }
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: HashMap::new(),
            body: body.into(),
        }
    }
    pub fn text(status: u16, s: impl Into<String>) -> Self {
        let mut r = Self::new(status, s.into().into_bytes());
        r.headers
            .insert("content-type".into(), "text/plain; charset=utf-8".into());
        r
    }
    pub fn html(status: u16, s: impl Into<String>) -> Self {
        let mut r = Self::new(status, s.into().into_bytes());
        r.headers
            .insert("content-type".into(), "text/html; charset=utf-8".into());
        r
    }
}

pub type Params = HashMap<String, String>;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn method_parse() {
        assert_eq!(Method::from("GET").as_str(), "GET");
    }
}
