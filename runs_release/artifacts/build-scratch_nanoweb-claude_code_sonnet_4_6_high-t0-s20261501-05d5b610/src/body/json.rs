use crate::error::NanowebError;
use serde::de::DeserializeOwned;

pub fn parse<T: DeserializeOwned>(body: &[u8]) -> Result<T, NanowebError> {
    serde_json::from_slice(body).map_err(NanowebError::Json)
}

pub fn serialize(value: &impl serde::Serialize) -> Result<Vec<u8>, NanowebError> {
    serde_json::to_vec(value).map_err(NanowebError::Json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_json() {
        #[derive(serde::Deserialize, Debug, PartialEq)]
        struct User {
            name: String,
            age: u32,
        }

        let body = br#"{"name":"alice","age":30}"#;
        let user: User = parse(body).unwrap();
        assert_eq!(user.name, "alice");
        assert_eq!(user.age, 30);
    }

    #[test]
    fn test_invalid_json() {
        let body = b"not json";
        let result: Result<serde_json::Value, _> = parse(body);
        assert!(result.is_err());
    }
}
