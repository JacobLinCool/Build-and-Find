use crate::error::NanowebError;
use crate::http::Request;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct MultipartField {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub data: Vec<u8>,
}

pub struct MultipartParser {
    boundary: String,
}

impl MultipartParser {
    pub fn from_request(req: &Request) -> Result<Self, NanowebError> {
        let content_type = req
            .header("content-type")
            .ok_or_else(|| NanowebError::Body("Missing content-type header".into()))?;

        let boundary = content_type
            .split(';')
            .find(|part| part.trim().starts_with("boundary="))
            .map(|part| {
                part.trim()
                    .strip_prefix("boundary=")
                    .unwrap_or("")
                    .trim_matches('"')
            })
            .ok_or_else(|| NanowebError::Body("Missing boundary in content-type".into()))?;

        Ok(Self {
            boundary: boundary.to_string(),
        })
    }

    pub fn parse(&self, body: &[u8]) -> Result<Vec<MultipartField>, NanowebError> {
        let delimiter = format!("--{}", self.boundary);
        let delimiter_bytes = delimiter.as_bytes();
        let mut fields = Vec::new();

        // Find all boundary positions
        let mut positions = Vec::new();
        let mut search_start = 0;
        while let Some(pos) = find_subsequence(&body[search_start..], delimiter_bytes) {
            positions.push(search_start + pos);
            search_start = search_start + pos + delimiter_bytes.len();
        }

        if positions.len() < 2 {
            return Err(NanowebError::Body("Invalid multipart format".into()));
        }

        // Parse each part between boundaries
        for i in 0..positions.len() - 1 {
            let part_start = positions[i] + delimiter_bytes.len();
            let part_end = positions[i + 1];

            if part_start >= part_end {
                continue;
            }

            let part = &body[part_start..part_end];
            // Skip leading CRLF
            let part = if part.starts_with(b"\r\n") {
                &part[2..]
            } else {
                part
            };

            // Find the end of headers (double CRLF)
            if let Some(header_end) = find_subsequence(part, b"\r\n\r\n") {
                let headers_bytes = &part[..header_end];
                let body_bytes = &part[header_end + 4..];

                // Remove trailing CRLF from body
                let body_bytes = if body_bytes.ends_with(b"\r\n") {
                    &body_bytes[..body_bytes.len() - 2]
                } else {
                    body_bytes
                };

                let mut name = String::new();
                let mut filename = None;
                let mut content_type = None;

                // Parse headers
                let headers_str = String::from_utf8_lossy(headers_bytes);
                for line in headers_str.lines() {
                    if let Some(colon_pos) = line.find(':') {
                        let header_name = line[..colon_pos].trim().to_ascii_lowercase();
                        let header_value = line[colon_pos + 1..].trim();

                        if header_name == "content-disposition" {
                            for part in header_value.split(';') {
                                let part = part.trim();
                                if let Some(val) = part.strip_prefix("name=") {
                                    name = val.trim_matches('"').to_string();
                                } else if let Some(val) = part.strip_prefix("filename=") {
                                    filename = Some(val.trim_matches('"').to_string());
                                }
                            }
                        } else if header_name == "content-type" {
                            content_type = Some(header_value.to_string());
                        }
                    }
                }

                if !name.is_empty() {
                    fields.push(MultipartField {
                        name,
                        filename,
                        content_type,
                        data: body_bytes.to_vec(),
                    });
                }
            }
        }

        Ok(fields)
    }

    pub fn parse_as_map(&self, body: &[u8]) -> Result<HashMap<String, Vec<u8>>, NanowebError> {
        let fields = self.parse(body)?;
        let mut map = HashMap::new();
        for field in fields {
            map.insert(field.name, field.data);
        }
        Ok(map)
    }
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_multipart() {
        let boundary = "----WebKitFormBoundary7MA4YWxkTrZu0gW";
        let body = format!(
            "------WebKitFormBoundary7MA4YWxkTrZu0gW\r\n\
             Content-Disposition: form-data; name=\"field1\"\r\n\r\n\
             value1\r\n\
             ------WebKitFormBoundary7MA4YWxkTrZu0gW\r\n\
             Content-Disposition: form-data; name=\"file\"; filename=\"test.txt\"\r\n\
             Content-Type: text/plain\r\n\r\n\
             file content\r\n\
             ------WebKitFormBoundary7MA4YWxkTrZu0gW--\r\n"
        );

        let parser = MultipartParser {
            boundary: boundary.to_string(),
        };
        let fields = parser.parse(body.as_bytes()).unwrap();

        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "field1");
        assert_eq!(fields[0].data, b"value1");
        assert_eq!(fields[1].name, "file");
        assert_eq!(fields[1].filename.as_deref(), Some("test.txt"));
        assert_eq!(fields[1].data, b"file content");
    }
}
