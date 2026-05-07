use crate::error::Result;
use serde::de::DeserializeOwned;

pub fn parse_json<T: DeserializeOwned>(body: &[u8]) -> Result<T> {
    serde_json::from_slice(body).map_err(|e| crate::error::NanowebError::Parse(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize, Debug, PartialEq)]
    struct Test {
        name: String,
        count: u32,
    }

    #[test]
    fn parse_valid_json() {
        let data = br#"{"name":"hello","count":42}"#;
        let result: Test = parse_json(data).unwrap();
        assert_eq!(result.name, "hello");
        assert_eq!(result.count, 42);
    }

    #[test]
    fn parse_invalid_json() {
        let data = b"not json";
        let result: std::result::Result<Test, _> = parse_json(data);
        assert!(result.is_err());
    }
}
