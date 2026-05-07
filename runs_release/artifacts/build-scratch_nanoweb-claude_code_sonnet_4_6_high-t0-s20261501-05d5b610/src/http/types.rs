use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
    pub fn from_str(s: &str) -> Self {
        match s.to_uppercase().as_str() {
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

    pub fn as_str(&self) -> &str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Patch => "PATCH",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
            Self::Other(s) => s.as_str(),
        }
    }

    pub fn is_safe(&self) -> bool {
        matches!(self, Self::Get | Self::Head | Self::Options)
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Version {
    Http10,
    Http11,
}

impl Version {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "HTTP/1.0" => Some(Self::Http10),
            "HTTP/1.1" => Some(Self::Http11),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Http10 => "HTTP/1.0",
            Self::Http11 => "HTTP/1.1",
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct Headers(pub Vec<(String, String)>);

impl Headers {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn insert(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        let value = value.into();
        // Replace existing header with same name (case-insensitive)
        let name_lower = name.to_lowercase();
        if let Some(pos) = self
            .0
            .iter()
            .position(|(k, _)| k.to_lowercase() == name_lower)
        {
            self.0[pos] = (name, value);
        } else {
            self.0.push((name, value));
        }
    }

    pub fn append(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.0.push((name.into(), value.into()));
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        let name_lower = name.to_lowercase();
        self.0
            .iter()
            .find(|(k, _)| k.to_lowercase() == name_lower)
            .map(|(_, v)| v.as_str())
    }

    pub fn get_all(&self, name: &str) -> Vec<&str> {
        let name_lower = name.to_lowercase();
        self.0
            .iter()
            .filter(|(k, _)| k.to_lowercase() == name_lower)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    pub fn remove(&mut self, name: &str) {
        let name_lower = name.to_lowercase();
        self.0.retain(|(k, _)| k.to_lowercase() != name_lower);
    }

    pub fn iter(&self) -> impl Iterator<Item = &(String, String)> {
        self.0.iter()
    }
}

impl Default for Headers {
    fn default() -> Self {
        Self::new()
    }
}

pub struct StatusCode;

impl StatusCode {
    pub fn reason(code: u16) -> &'static str {
        match code {
            100 => "Continue",
            101 => "Switching Protocols",
            200 => "OK",
            201 => "Created",
            202 => "Accepted",
            204 => "No Content",
            206 => "Partial Content",
            301 => "Moved Permanently",
            302 => "Found",
            304 => "Not Modified",
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            408 => "Request Timeout",
            409 => "Conflict",
            410 => "Gone",
            413 => "Payload Too Large",
            415 => "Unsupported Media Type",
            422 => "Unprocessable Entity",
            429 => "Too Many Requests",
            500 => "Internal Server Error",
            501 => "Not Implemented",
            502 => "Bad Gateway",
            503 => "Service Unavailable",
            _ => "Unknown",
        }
    }

    pub fn class(code: u16) -> &'static str {
        match code / 100 {
            1 => "1xx",
            2 => "2xx",
            3 => "3xx",
            4 => "4xx",
            5 => "5xx",
            _ => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_parsing() {
        assert_eq!(Method::from_str("GET"), Method::Get);
        assert_eq!(Method::from_str("post"), Method::Post);
        assert_eq!(Method::from_str("UNKNOWN"), Method::Other("UNKNOWN".into()));
    }

    #[test]
    fn test_headers_case_insensitive() {
        let mut h = Headers::new();
        h.insert("Content-Type", "text/html");
        assert_eq!(h.get("content-type"), Some("text/html"));
        assert_eq!(h.get("CONTENT-TYPE"), Some("text/html"));
    }

    #[test]
    fn test_headers_replace() {
        let mut h = Headers::new();
        h.insert("X-Foo", "bar");
        h.insert("X-Foo", "baz");
        assert_eq!(h.get("X-Foo"), Some("baz"));
        assert_eq!(h.0.len(), 1);
    }

    #[test]
    fn test_status_reason() {
        assert_eq!(StatusCode::reason(200), "OK");
        assert_eq!(StatusCode::reason(404), "Not Found");
        assert_eq!(StatusCode::class(200), "2xx");
        assert_eq!(StatusCode::class(404), "4xx");
    }
}
