use crate::error::NanowebError;
use std::collections::HashMap;

pub fn parse_urlencoded<T: serde::de::DeserializeOwned>(data: &[u8]) -> Result<T, NanowebError> {
    serde_urlencoded::from_bytes(data)
        .map_err(|e| NanowebError::Parse(format!("URL-encoded: {}", e)))
}

pub fn parse_urlencoded_map(data: &[u8]) -> Result<HashMap<String, String>, NanowebError> {
    serde_urlencoded::from_bytes(data)
        .map_err(|e| NanowebError::Parse(format!("URL-encoded: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize, Debug, PartialEq)]
    struct Form {
        name: String,
        age: u32,
    }

    #[test]
    fn test_parse_urlencoded_struct() {
        let data = b"name=alice&age=30";
        let form: Form = parse_urlencoded(data).unwrap();
        assert_eq!(form.name, "alice");
        assert_eq!(form.age, 30);
    }

    #[test]
    fn test_parse_urlencoded_map() {
        let data = b"foo=bar&baz=qux";
        let map = parse_urlencoded_map(data).unwrap();
        assert_eq!(map.get("foo"), Some(&"bar".to_string()));
        assert_eq!(map.get("baz"), Some(&"qux".to_string()));
    }
}
