use percent_encoding::percent_decode_str;

pub fn parse_urlencoded(data: &[u8]) -> Vec<(String, String)> {
    let s = std::str::from_utf8(data).unwrap_or("");
    if s.is_empty() {
        return Vec::new();
    }
    s.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = match p.split_once('=') {
                Some((k, v)) => (k, v),
                None => (p, ""),
            };
            let k = percent_decode_str(&k.replace('+', " "))
                .decode_utf8_lossy()
                .into_owned();
            let v = percent_decode_str(&v.replace('+', " "))
                .decode_utf8_lossy()
                .into_owned();
            (k, v)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse() {
        let p = parse_urlencoded(b"a=1&b=hello+world&c=%2F");
        assert_eq!(p[0], ("a".into(), "1".into()));
        assert_eq!(p[1], ("b".into(), "hello world".into()));
        assert_eq!(p[2], ("c".into(), "/".into()));
    }
}
