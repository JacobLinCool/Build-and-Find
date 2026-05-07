use crate::error::NanowebError;
use serde::de::DeserializeOwned;

pub fn parse_json<T: DeserializeOwned>(body: &[u8]) -> Result<T, NanowebError> {
    serde_json::from_slice(body).map_err(|e| NanowebError::Body(format!("JSON parse error: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_json() {
        let body = b"{\"key\":\"value\"}";
        let result: serde_json::Value = parse_json(body).unwrap();
        assert_eq!(result["key"], "value");
    }

    #[test]
    fn parse_invalid_json() {
        let body = b"not json";
        let result: Result<serde_json::Value, _> = parse_json(body);
        assert!(result.is_err());
    }
}
