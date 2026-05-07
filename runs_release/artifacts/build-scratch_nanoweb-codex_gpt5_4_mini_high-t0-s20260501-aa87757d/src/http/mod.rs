pub mod parser;
pub mod types;

#[cfg(test)]
mod tests {
    use super::parser::parse_request;

    #[test]
    fn parse_simple_request() {
        let raw =
            b"GET /hello?name=world HTTP/1.1\r\nHost: example\r\nContent-Length: 5\r\n\r\nabcde";
        let (req, consumed) = parse_request(raw).unwrap();
        assert_eq!(consumed, raw.len());
        assert_eq!(req.method.to_string(), "GET");
        assert_eq!(req.path, "/hello");
        assert_eq!(req.query.as_deref(), Some("name=world"));
        assert_eq!(req.body, b"abcde");
    }
}
