pub mod json;
pub mod multipart;
pub mod urlencoded;

use crate::error::NanowebError;
use crate::http::Request;
use serde::de::DeserializeOwned;

pub trait BodyParser {
    fn parse<T: DeserializeOwned>(&self, req: &mut Request) -> Result<T, NanowebError>;
}

pub fn parse_body<T: DeserializeOwned>(req: &mut Request) -> Result<T, NanowebError> {
    let content_type = req.header("content-type").unwrap_or("").to_string();

    if content_type.contains("application/json") {
        json::parse_json(req)
    } else if content_type.contains("application/x-www-form-urlencoded") {
        urlencoded::parse_urlencoded(req)
    } else if content_type.contains("multipart/form-data") {
        Err(NanowebError::Body(
            "Multipart form data must be parsed with MultipartParser".into(),
        ))
    } else {
        Err(NanowebError::Body(format!(
            "Unsupported content type: {content_type}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct TestData {
        name: String,
        age: u32,
    }

    #[test]
    fn test_parse_json_body() {
        let mut req = Request::new(crate::http::Method::Post, "/".into(), "HTTP/1.1".into());
        req.headers
            .insert("content-type".into(), "application/json".into());
        req.set_body(b"{\"name\":\"Alice\",\"age\":30}".to_vec());

        let data: TestData = parse_body(&mut req).unwrap();
        assert_eq!(data.name, "Alice");
        assert_eq!(data.age, 30);
    }

    #[test]
    fn test_parse_urlencoded_body() {
        let mut req = Request::new(crate::http::Method::Post, "/".into(), "HTTP/1.1".into());
        req.headers.insert(
            "content-type".into(),
            "application/x-www-form-urlencoded".into(),
        );
        req.set_body(b"name=Bob&age=25".to_vec());

        let data: TestData = parse_body(&mut req).unwrap();
        assert_eq!(data.name, "Bob");
        assert_eq!(data.age, 25);
    }
}
