use crate::error::NanowebError;
use crate::http::Request;
use serde::de::DeserializeOwned;

pub fn parse_json<T: DeserializeOwned>(req: &mut Request) -> Result<T, NanowebError> {
    let body = req.take_body();
    serde_json::from_slice(&body).map_err(|e| NanowebError::Body(format!("JSON decode error: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct User {
        name: String,
        email: String,
    }

    #[test]
    fn test_parse_json() {
        let mut req = Request::new(crate::http::Method::Post, "/".into(), "HTTP/1.1".into());
        req.set_body(b"{\"name\":\"Alice\",\"email\":\"alice@example.com\"}".to_vec());
        let user: User = parse_json(&mut req).unwrap();
        assert_eq!(user.name, "Alice");
        assert_eq!(user.email, "alice@example.com");
    }

    #[test]
    fn test_parse_invalid_json() {
        let mut req = Request::new(crate::http::Method::Post, "/".into(), "HTTP/1.1".into());
        req.set_body(b"not json".to_vec());
        let result: Result<User, _> = parse_json(&mut req);
        assert!(result.is_err());
    }
}
