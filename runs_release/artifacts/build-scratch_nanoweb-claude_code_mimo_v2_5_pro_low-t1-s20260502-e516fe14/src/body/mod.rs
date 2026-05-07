pub mod json;
pub mod multipart;
pub mod urlencoded;

use crate::error::NanowebError;
use serde::de::DeserializeOwned;

/// Parse body based on content-type.
pub fn parse_body<T: DeserializeOwned>(content_type: &str, body: &[u8]) -> Result<T, NanowebError> {
    if content_type.contains("application/json") {
        json::parse_json(body)
    } else if content_type.contains("application/x-www-form-urlencoded") {
        urlencoded::parse_form(body)
    } else {
        // Try JSON as default
        json::parse_json(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize, Debug, PartialEq)]
    struct TestData {
        name: String,
        age: u32,
    }

    #[test]
    fn test_parse_json_body() {
        let body = br#"{"name":"Alice","age":30}"#;
        let result: TestData = parse_body("application/json", body).unwrap();
        assert_eq!(result.name, "Alice");
        assert_eq!(result.age, 30);
    }

    #[test]
    fn test_parse_urlencoded_body() {
        #[derive(Deserialize, Debug, PartialEq)]
        struct FormData {
            name: String,
            value: String,
        }
        let body = b"name=Alice&value=30";
        let result: FormData = parse_body("application/x-www-form-urlencoded", body).unwrap();
        assert_eq!(result.name, "Alice");
        assert_eq!(result.value, "30");
    }
}
