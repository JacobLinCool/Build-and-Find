use crate::error::NanowebError;
use serde::de::DeserializeOwned;

pub fn parse<T: DeserializeOwned>(body: &[u8]) -> Result<T, NanowebError> {
    serde_urlencoded::from_bytes(body).map_err(|e| NanowebError::Parse(e.to_string()))
}

pub fn parse_pairs(body: &[u8]) -> Vec<(String, String)> {
    let s = std::str::from_utf8(body).unwrap_or("");
    s.split('&')
        .filter_map(|pair| {
            if let Some(eq) = pair.find('=') {
                let key = percent_decode(&pair[..eq]);
                let value = percent_decode(&pair[eq + 1..]);
                Some((key, value))
            } else if !pair.is_empty() {
                Some((percent_decode(pair), String::new()))
            } else {
                None
            }
        })
        .collect()
}

fn percent_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                if let Ok(c) = std::str::from_utf8(&[(h * 16 + l)]) {
                    result.push_str(c);
                } else {
                    result.push(h as char);
                }
                i += 3;
                continue;
            }
        } else if bytes[i] == b'+' {
            result.push(' ');
            i += 1;
            continue;
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_form() {
        #[derive(serde::Deserialize, Debug, PartialEq)]
        struct Form {
            name: String,
            age: String,
        }
        let body = b"name=alice&age=30";
        let form: Form = parse(body).unwrap();
        assert_eq!(form.name, "alice");
        assert_eq!(form.age, "30");
    }

    #[test]
    fn test_parse_pairs() {
        let pairs = parse_pairs(b"foo=bar&baz=qux");
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0], ("foo".to_string(), "bar".to_string()));
        assert_eq!(pairs[1], ("baz".to_string(), "qux".to_string()));
    }
}
