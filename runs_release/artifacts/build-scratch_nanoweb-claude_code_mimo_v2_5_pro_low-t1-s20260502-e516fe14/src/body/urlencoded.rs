use crate::error::NanowebError;
use crate::http::types::url_decode;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Parse URL-encoded form body into a deserializable type.
pub fn parse_form<T: DeserializeOwned>(body: &[u8]) -> Result<T, NanowebError> {
    let s = std::str::from_utf8(body)
        .map_err(|_| NanowebError::HttpParse("Invalid UTF-8 in form body".into()))?;

    let mut map = serde_json::Map::new();
    for pair in s.split('&') {
        if pair.is_empty() {
            continue;
        }
        let mut parts = pair.splitn(2, '=');
        let key = url_decode(parts.next().unwrap_or(""));
        let value = url_decode(parts.next().unwrap_or(""));
        map.insert(key, Value::String(value));
    }

    serde_json::from_value(Value::Object(map))
        .map_err(|e| NanowebError::HttpParse(format!("Form parse error: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize, Debug)]
    struct FormData {
        username: String,
        password: String,
    }

    #[test]
    fn test_parse_urlencoded_form() {
        let body = b"username=admin&password=secret123";
        let form: FormData = parse_form(body).unwrap();
        assert_eq!(form.username, "admin");
        assert_eq!(form.password, "secret123");
    }

    #[test]
    fn test_parse_urlencoded_with_special_chars() {
        let body = b"name=hello+world&msg=%26special%3D";
        let form: std::collections::HashMap<String, String> = parse_form(body).unwrap();
        assert_eq!(form.get("name").unwrap(), "hello world");
        assert_eq!(form.get("msg").unwrap(), "&special=");
    }
}
