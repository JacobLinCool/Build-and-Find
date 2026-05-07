use crate::error::{NanowebError, Result};
use serde::de::DeserializeOwned;

pub struct JsonParser;

impl JsonParser {
    pub fn parse<T: DeserializeOwned>(body: &[u8]) -> Result<T> {
        serde_json::from_slice(body)
            .map_err(|e| NanowebError::Body(format!("JSON parse error: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize, Debug, PartialEq)]
    struct TestPayload {
        name: String,
        age: u32,
    }

    #[test]
    fn test_json_parse() {
        let data = br#"{"name":"Alice","age":30}"#;
        let payload: TestPayload = JsonParser::parse(data).unwrap();
        assert_eq!(payload.name, "Alice");
        assert_eq!(payload.age, 30);
    }

    #[test]
    fn test_json_parse_error() {
        let data = b"not json";
        let result: std::result::Result<TestPayload, _> = JsonParser::parse(data);
        assert!(result.is_err());
    }
}
