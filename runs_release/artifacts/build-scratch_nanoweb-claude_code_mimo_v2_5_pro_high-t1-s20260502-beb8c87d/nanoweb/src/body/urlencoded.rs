use crate::error::NanowebError;
use crate::http::types::Request;
use serde::de::DeserializeOwned;

pub fn parse_urlencoded<T: DeserializeOwned>(req: &Request) -> Result<T, NanowebError> {
    let body_str = std::str::from_utf8(&req.body)
        .map_err(|_| NanowebError::Body("Invalid UTF-8 in form body".to_string()))?;
    serde_urlencoded::from_str(body_str)
        .map_err(|e| NanowebError::Body(format!("URL-encoded parse error: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::collections::HashMap;

    #[derive(Deserialize, Debug)]
    struct FormData {
        username: String,
        password: String,
    }

    #[test]
    fn test_urlencoded_parse() {
        let req = Request {
            method: crate::http::types::Method::Post,
            target: "/".to_string(),
            path: "/".to_string(),
            query: HashMap::new(),
            version: "HTTP/1.1".to_string(),
            headers: HashMap::new(),
            params: HashMap::new(),
            body: b"username=admin&password=secret".to_vec(),
            request_id: "test".to_string(),
            remote_addr: None,
        };
        let form: FormData = parse_urlencoded(&req).unwrap();
        assert_eq!(form.username, "admin");
        assert_eq!(form.password, "secret");
    }
}
