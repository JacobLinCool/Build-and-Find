use crate::error::NanowebError;
use serde::de::DeserializeOwned;

/// Parse JSON body.
pub fn parse_json<T: DeserializeOwned>(body: &[u8]) -> Result<T, NanowebError> {
    serde_json::from_slice(body)
        .map_err(|e| NanowebError::HttpParse(format!("JSON parse error: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize, Debug)]
    struct Item {
        id: u32,
        name: String,
    }

    #[test]
    fn test_parse_valid_json() {
        let body = br#"{"id":1,"name":"test"}"#;
        let item: Item = parse_json(body).unwrap();
        assert_eq!(item.id, 1);
        assert_eq!(item.name, "test");
    }

    #[test]
    fn test_parse_invalid_json() {
        let body = b"not json";
        let result: Result<Item, _> = parse_json(body);
        assert!(result.is_err());
    }
}
