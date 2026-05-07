use crate::error::{NanowebError, Result};

#[derive(Debug, Clone)]
pub struct Part {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub data: Vec<u8>,
}

pub fn parse(body: &[u8], boundary: &str) -> Result<Vec<Part>> {
    let delim = format!("--{}", boundary);
    let final_delim = format!("--{}--", boundary);
    let body_str = body;
    let delim_bytes = delim.as_bytes();
    let mut parts = Vec::new();
    // Find first boundary
    let mut idx = match find(body_str, delim_bytes) {
        Some(i) => i,
        None => {
            return Err(NanowebError::BadRequest(
                "multipart boundary missing".into(),
            ))
        }
    };
    // skip past first boundary line
    idx += delim_bytes.len();
    if body_str.get(idx..idx + 2) == Some(b"\r\n") {
        idx += 2;
    }
    loop {
        // Find next delimiter
        let next = find(&body_str[idx..], delim_bytes)
            .ok_or_else(|| NanowebError::BadRequest("multipart truncated".into()))?;
        let part_bytes = &body_str[idx..idx + next];
        // strip trailing CRLF before boundary
        let part_bytes = if part_bytes.ends_with(b"\r\n") {
            &part_bytes[..part_bytes.len() - 2]
        } else {
            part_bytes
        };
        // Split headers and body by \r\n\r\n
        let split = find(part_bytes, b"\r\n\r\n")
            .ok_or_else(|| NanowebError::BadRequest("multipart part header missing".into()))?;
        let header_bytes = &part_bytes[..split];
        let data_bytes = &part_bytes[split + 4..];
        let header_str = std::str::from_utf8(header_bytes)
            .map_err(|_| NanowebError::BadRequest("bad multipart header".into()))?;
        let mut name = None;
        let mut filename = None;
        let mut content_type = None;
        for line in header_str.split("\r\n") {
            if let Some((k, v)) = line.split_once(':') {
                let k = k.trim().to_ascii_lowercase();
                let v = v.trim();
                if k == "content-disposition" {
                    for token in v.split(';') {
                        let token = token.trim();
                        if let Some(rest) = token.strip_prefix("name=") {
                            name = Some(rest.trim_matches('"').to_string());
                        } else if let Some(rest) = token.strip_prefix("filename=") {
                            filename = Some(rest.trim_matches('"').to_string());
                        }
                    }
                } else if k == "content-type" {
                    content_type = Some(v.to_string());
                }
            }
        }
        let name =
            name.ok_or_else(|| NanowebError::BadRequest("multipart part missing name".into()))?;
        parts.push(Part {
            name,
            filename,
            content_type,
            data: data_bytes.to_vec(),
        });
        idx += next + delim_bytes.len();
        // Check if final boundary
        if body_str.get(idx..idx + 2) == Some(b"--") {
            // final
            let _ = final_delim;
            break;
        }
        if body_str.get(idx..idx + 2) == Some(b"\r\n") {
            idx += 2;
        }
    }
    Ok(parts)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_multipart() {
        let boundary = "X";
        let body = b"--X\r\nContent-Disposition: form-data; name=\"a\"\r\n\r\nhello\r\n--X\r\nContent-Disposition: form-data; name=\"b\"\r\n\r\nworld\r\n--X--\r\n";
        let parts = parse(body, boundary).unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].name, "a");
        assert_eq!(parts[0].data, b"hello");
        assert_eq!(parts[1].name, "b");
        assert_eq!(parts[1].data, b"world");
    }
}
