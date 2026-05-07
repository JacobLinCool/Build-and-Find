use serde::de::DeserializeOwned;

pub fn parse_json<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, serde_json::Error> {
    serde_json::from_slice(bytes)
}

pub fn parse_urlencoded<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, serde_json::Error> {
    let mut map = serde_json::Map::new();
    for (k, v) in url::form_urlencoded::parse(bytes) {
        map.insert(k.into_owned(), serde_json::Value::String(v.into_owned()));
    }
    serde_json::from_value(serde_json::Value::Object(map))
}

pub fn parse_multipart(bytes: &[u8]) -> Vec<u8> {
    bytes.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    #[derive(Deserialize)]
    struct S {
        a: String,
    }
    #[test]
    fn json() {
        let s: S = parse_json(br#"{"a":"b"}"#).unwrap();
        assert_eq!(s.a, "b");
    }
}
