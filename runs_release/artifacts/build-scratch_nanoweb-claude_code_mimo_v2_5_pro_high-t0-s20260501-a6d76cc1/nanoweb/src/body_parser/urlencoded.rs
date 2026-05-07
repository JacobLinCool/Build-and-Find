use crate::error::NanowebError;
use serde::de::DeserializeOwned;

pub fn parse_urlencoded<T: DeserializeOwned>(body: &[u8]) -> Result<T, NanowebError> {
    let s = std::str::from_utf8(body)
        .map_err(|_| NanowebError::Body("invalid UTF-8 in form body".into()))?;
    serde_urlencoded::from_str(s)
        .map_err(|e| NanowebError::Body(format!("URL-encoded parse error: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn parse_form() {
        let body = b"name=hello&value=42";
        let result: HashMap<String, String> = parse_urlencoded(body).unwrap();
        assert_eq!(result.get("name").map(|s| s.as_str()), Some("hello"));
        assert_eq!(result.get("value").map(|s| s.as_str()), Some("42"));
    }
}
