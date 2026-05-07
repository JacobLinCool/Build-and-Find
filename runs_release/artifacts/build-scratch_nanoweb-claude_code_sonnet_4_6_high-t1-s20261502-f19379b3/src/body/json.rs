use crate::error::NanowebError;

pub fn parse_json<T: serde::de::DeserializeOwned>(data: &[u8]) -> Result<T, NanowebError> {
    serde_json::from_slice(data).map_err(|e| NanowebError::Parse(format!("JSON: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize, Debug, PartialEq)]
    struct Foo {
        name: String,
        value: i32,
    }

    #[test]
    fn test_parse_json_ok() {
        let data = br#"{"name":"hello","value":42}"#;
        let foo: Foo = parse_json(data).unwrap();
        assert_eq!(foo.name, "hello");
        assert_eq!(foo.value, 42);
    }

    #[test]
    fn test_parse_json_err() {
        let data = b"not json";
        let result: Result<Foo, _> = parse_json(data);
        assert!(result.is_err());
    }
}
