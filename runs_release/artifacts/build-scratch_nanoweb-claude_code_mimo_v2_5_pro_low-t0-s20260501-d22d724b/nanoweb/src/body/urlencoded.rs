use crate::error::NanowebError;
use crate::http::Request;
use serde::de::DeserializeOwned;

pub fn parse_urlencoded<T: DeserializeOwned>(req: &mut Request) -> Result<T, NanowebError> {
    let body = req.take_body();
    serde_urlencoded::from_bytes(&body)
        .map_err(|e| NanowebError::Body(format!("URL-encoded decode error: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct FormData {
        username: String,
        password: String,
    }

    #[test]
    fn test_parse_urlencoded() {
        let mut req = Request::new(crate::http::Method::Post, "/".into(), "HTTP/1.1".into());
        req.set_body(b"username=admin&password=secret".to_vec());
        let form: FormData = parse_urlencoded(&mut req).unwrap();
        assert_eq!(form.username, "admin");
        assert_eq!(form.password, "secret");
    }
}
