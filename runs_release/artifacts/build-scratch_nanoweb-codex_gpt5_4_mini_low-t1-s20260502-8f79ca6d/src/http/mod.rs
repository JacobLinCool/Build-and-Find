pub mod parser;

use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
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
    fn from(value: &str) -> Self {
        match value {
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Version {
    Http10,
    Http11,
}

#[derive(Clone, Debug)]
pub struct Request {
    pub method: Method,
    pub target: String,
    pub version: Version,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub path_params: HashMap<String, String>,
    pub query_params: HashMap<String, String>,
    pub remote_addr: Option<std::net::SocketAddr>,
    pub request_id: String,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn body_as<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_slice(&self.body)
    }

    pub fn body_as_form<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        let pairs: Vec<(String, String)> = url::form_urlencoded::parse(&self.body)
            .into_owned()
            .collect();
        let mut map = serde_json::Map::new();
        for (k, v) in pairs {
            map.insert(k, serde_json::Value::String(v));
        }
        serde_json::from_value::<T>(serde_json::Value::Object(map))
    }
}

#[derive(Clone, Debug)]
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

    pub fn text(status: u16, body: impl Into<String>) -> Self {
        let mut resp = Self::new(status, body.into().into_bytes());
        resp.headers
            .insert("content-type".into(), "text/plain; charset=utf-8".into());
        resp
    }

    pub fn json<T: Serialize>(status: u16, value: &T) -> Result<Self, serde_json::Error> {
        let mut resp = Self::new(status, serde_json::to_vec(value)?);
        resp.headers
            .insert("content-type".into(), "application/json".into());
        Ok(resp)
    }
}

pub type HeaderMap = HashMap<String, String>;
