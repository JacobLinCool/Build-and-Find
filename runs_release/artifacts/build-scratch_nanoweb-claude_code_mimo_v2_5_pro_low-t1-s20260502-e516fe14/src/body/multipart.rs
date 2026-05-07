use crate::error::NanowebError;
use serde_json::Value;
use std::collections::HashMap;

/// A single part in a multipart form.
#[derive(Debug, Clone)]
pub struct MultipartPart {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub data: Vec<u8>,
}

/// Parse multipart/form-data body.
pub fn parse_multipart(body: &[u8], boundary: &str) -> Result<Vec<MultipartPart>, NanowebError> {
    let delimiter = format!("--{}", boundary);
    let end_delimiter = format!("--{}--", boundary);

    let mut parts = Vec::new();
    let mut remaining = body;

    loop {
        // Find the next delimiter
        let delim_pos = find_in_bytes(remaining, delimiter.as_bytes())
            .ok_or_else(|| NanowebError::HttpParse("Invalid multipart format".into()))?;

        remaining = &remaining[delim_pos + delimiter.len()..];

        // Skip \r\n after delimiter
        if remaining.starts_with(b"\r\n") {
            remaining = &remaining[2..];
        } else if remaining.starts_with(b"--") {
            // End delimiter
            break;
        } else {
            continue;
        }

        // Check for end delimiter
        if remaining.starts_with(end_delimiter.as_bytes()) {
            break;
        }

        // Find the end of headers (double CRLF)
        let header_end = find_in_bytes(remaining, b"\r\n\r\n")
            .ok_or_else(|| NanowebError::HttpParse("Invalid multipart part headers".into()))?;

        let header_bytes = &remaining[..header_end];
        let header_str = std::str::from_utf8(header_bytes)
            .map_err(|_| NanowebError::HttpParse("Invalid UTF-8 in multipart headers".into()))?;

        let mut name = String::new();
        let mut filename = None;
        let mut content_type = None;

        for line in header_str.split("\r\n") {
            let lower = line.to_lowercase();
            if lower.starts_with("content-disposition:") {
                // Parse name and filename from Content-Disposition
                for param in line[line.find(':').unwrap() + 1..].split(';') {
                    let param = param.trim();
                    if param.starts_with("name=") {
                        name = param[5..].trim_matches('"').to_string();
                    } else if param.starts_with("filename=") {
                        filename = Some(param[9..].trim_matches('"').to_string());
                    }
                }
            } else if lower.starts_with("content-type:") {
                content_type = Some(line[line.find(':').unwrap() + 1..].trim().to_string());
            }
        }

        // Find the body (up to next delimiter)
        let body_start = header_end + 4;
        let next_delim = find_in_bytes(&remaining[body_start..], delimiter.as_bytes());

        let data = if let Some(pos) = next_delim {
            let mut part_body = &remaining[body_start..body_start + pos];
            // Remove trailing \r\n before delimiter
            if part_body.ends_with(b"\r\n") {
                part_body = &part_body[..part_body.len() - 2];
            }
            part_body.to_vec()
        } else {
            remaining[body_start..].to_vec()
        };

        parts.push(MultipartPart {
            name,
            filename,
            content_type,
            data,
        });

        if let Some(pos) = next_delim {
            remaining = &remaining[body_start + pos..];
        } else {
            break;
        }
    }

    Ok(parts)
}

/// Convert multipart parts to a JSON map (for text fields only).
pub fn parts_to_map(parts: &[MultipartPart]) -> HashMap<String, Value> {
    let mut map = serde_json::Map::new();
    for part in parts {
        if part.filename.is_some() {
            // File part - store as object
            let mut file_obj = serde_json::Map::new();
            file_obj.insert(
                "filename".into(),
                Value::String(part.filename.clone().unwrap_or_default()),
            );
            file_obj.insert(
                "content_type".into(),
                Value::String(part.content_type.clone().unwrap_or_default()),
            );
            file_obj.insert("size".into(), Value::Number(part.data.len().into()));
            map.insert(part.name.clone(), Value::Object(file_obj));
        } else {
            // Text field
            let value = String::from_utf8_lossy(&part.data).to_string();
            map.insert(part.name.clone(), Value::String(value));
        }
    }
    map.into_iter().collect()
}

fn find_in_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_multipart_text_fields() {
        let body = b"--boundary\r\n\
                     Content-Disposition: form-data; name=\"field1\"\r\n\r\n\
                     value1\r\n\
                     --boundary\r\n\
                     Content-Disposition: form-data; name=\"field2\"\r\n\r\n\
                     value2\r\n\
                     --boundary--\r\n";

        let parts = parse_multipart(body, "boundary").unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].name, "field1");
        assert_eq!(parts[0].data, b"value1");
        assert_eq!(parts[1].name, "field2");
        assert_eq!(parts[1].data, b"value2");
    }

    #[test]
    fn test_parse_multipart_file_upload() {
        let body = b"--boundary\r\n\
                     Content-Disposition: form-data; name=\"file\"; filename=\"test.txt\"\r\n\
                     Content-Type: text/plain\r\n\r\n\
                     file content here\r\n\
                     --boundary--\r\n";

        let parts = parse_multipart(body, "boundary").unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].name, "file");
        assert_eq!(parts[0].filename, Some("test.txt".into()));
        assert_eq!(parts[0].content_type, Some("text/plain".into()));
        assert_eq!(parts[0].data, b"file content here");
    }

    #[test]
    fn test_parts_to_map() {
        let parts = vec![
            MultipartPart {
                name: "name".into(),
                filename: None,
                content_type: None,
                data: b"hello".to_vec(),
            },
            MultipartPart {
                name: "file".into(),
                filename: Some("test.txt".into()),
                content_type: Some("text/plain".into()),
                data: b"data".to_vec(),
            },
        ];

        let map = parts_to_map(&parts);
        assert_eq!(map.get("name").unwrap(), &Value::String("hello".into()));
    }
}
