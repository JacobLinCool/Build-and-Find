use crate::error::{NanowebError, Result};
use serde::de::DeserializeOwned;

pub fn parse_form<T: DeserializeOwned>(body: &[u8]) -> Result<T> {
    let s = std::str::from_utf8(body)
        .map_err(|e| NanowebError::Parse(format!("Invalid UTF-8: {e}")))?;
    let map = urldecode(s);
    let json_val = serde_json::to_value(&map)
        .map_err(|e| NanowebError::Parse(format!("Conversion error: {e}")))?;
    serde_json::from_value(json_val).map_err(|e| NanowebError::Parse(e.to_string()))
}

fn urldecode(s: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for pair in s.split('&') {
        if let Some((key, value)) = pair.split_once('=') {
            map.insert(percent_decode(key), percent_decode(value));
        }
    }
    map
}

fn percent_decode(s: &str) -> String {
    let mut result = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => result.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = &s[i + 1..i + 3];
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    result.push(byte);
                    i += 2;
                } else {
                    result.push(bytes[i]);
                }
            }
            b => result.push(b),
        }
        i += 1;
    }
    String::from_utf8(result).unwrap_or_default()
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
    fn parse_form_data() {
        let data = b"username=admin&password=secret";
        let result: LoginForm = parse_form(data).unwrap();
        assert_eq!(result.username, "admin");
        assert_eq!(result.password, "secret");
    }

    #[test]
    fn parse_encoded_values() {
        let data = b"username=admin&message=hello+world%21";
        let map = urldecode(std::str::from_utf8(data).unwrap());
        assert_eq!(map.get("message").unwrap(), "hello world!");
    }
}
