use crate::error::{NanowebError, Result};
use serde::de::DeserializeOwned;
use std::collections::HashMap;

pub struct UrlencodedParser;

impl UrlencodedParser {
    pub fn parse<T: DeserializeOwned>(body: &[u8]) -> Result<T> {
        let s = std::str::from_utf8(body)
            .map_err(|e| NanowebError::Body(format!("invalid UTF-8: {e}")))?;

        let map: HashMap<String, String> = s
            .split('&')
            .filter_map(|pair| {
                let mut parts = pair.splitn(2, '=');
                let key = parts.next()?;
                let value = parts.next().unwrap_or("");
                Some((
                    percent_encoding::percent_decode_str(key)
                        .decode_utf8_lossy()
                        .to_string(),
                    percent_encoding::percent_decode_str(value)
                        .decode_utf8_lossy()
                        .to_string(),
                ))
            })
            .collect();

        let json_val = serde_json::to_value(&map)
            .map_err(|e| NanowebError::Body(format!("serialization error: {e}")))?;
        serde_json::from_value(json_val)
            .map_err(|e| NanowebError::Body(format!("deserialization error: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize, Debug, PartialEq)]
    struct LoginForm {
        username: String,
        password: String,
    }

    #[test]
    fn test_urlencoded_parse() {
        let data = b"username=admin&password=secret";
        let form: LoginForm = UrlencodedParser::parse(data).unwrap();
        assert_eq!(form.username, "admin");
        assert_eq!(form.password, "secret");
    }

    #[test]
    fn test_urlencoded_percent_encoded() {
        let data = b"name=hello%20world";
        let map: HashMap<String, String> = UrlencodedParser::parse(data).unwrap();
        assert_eq!(map.get("name").unwrap(), "hello world");
    }
}
