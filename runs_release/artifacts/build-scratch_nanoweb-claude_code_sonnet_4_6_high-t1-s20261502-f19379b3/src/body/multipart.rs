use crate::error::NanowebError;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct MultipartField {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub headers: Vec<(String, String)>,
    pub data: Vec<u8>,
}

pub fn parse_multipart(data: &[u8], boundary: &str) -> Result<Vec<MultipartField>, NanowebError> {
    let delimiter = format!("--{}", boundary);
    let delimiter_bytes = delimiter.as_bytes();
    let end_delimiter = format!("--{}--", boundary);
    let end_bytes = end_delimiter.as_bytes();

    let mut fields = Vec::new();
    let mut pos = 0;

    // Find first boundary
    pos = find_bytes(data, delimiter_bytes, pos)
        .ok_or_else(|| NanowebError::Parse("Multipart: missing boundary".into()))?;
    pos += delimiter_bytes.len();

    // Skip CRLF after first boundary
    if data.get(pos..pos + 2) == Some(b"\r\n") {
        pos += 2;
    } else {
        return Ok(fields);
    }

    loop {
        // Parse headers for this part
        let mut part_headers: Vec<(String, String)> = Vec::new();
        loop {
            // Find end of header line
            let line_end = find_bytes(data, b"\r\n", pos)
                .ok_or_else(|| NanowebError::Parse("Multipart: truncated headers".into()))?;

            if line_end == pos {
                // Empty line = end of headers
                pos += 2;
                break;
            }

            let line = std::str::from_utf8(&data[pos..line_end])
                .map_err(|_| NanowebError::Parse("Multipart: invalid header encoding".into()))?;

            if let Some(colon) = line.find(':') {
                let name = line[..colon].trim().to_lowercase();
                let value = line[colon + 1..].trim().to_string();
                part_headers.push((name, value));
            }

            pos = line_end + 2;
        }

        // Extract name and filename from Content-Disposition
        let disposition = part_headers
            .iter()
            .find(|(k, _)| k == "content-disposition")
            .map(|(_, v)| v.as_str())
            .unwrap_or("");

        let name = extract_param(disposition, "name").unwrap_or_default();
        let filename = extract_param(disposition, "filename");

        let content_type = part_headers
            .iter()
            .find(|(k, _)| k == "content-type")
            .map(|(_, v)| v.clone());

        // Find next boundary
        let next_boundary_pos = find_bytes(data, delimiter_bytes, pos)
            .ok_or_else(|| NanowebError::Parse("Multipart: missing closing boundary".into()))?;

        // Part data is everything up to (boundary - CRLF)
        let part_end = if next_boundary_pos >= 2
            && &data[next_boundary_pos - 2..next_boundary_pos] == b"\r\n"
        {
            next_boundary_pos - 2
        } else {
            next_boundary_pos
        };

        let part_data = data[pos..part_end].to_vec();

        fields.push(MultipartField {
            name,
            filename,
            content_type,
            headers: part_headers,
            data: part_data,
        });

        pos = next_boundary_pos + delimiter_bytes.len();

        // Check if this is the end boundary
        if data.get(pos..pos + 2) == Some(b"--") {
            break;
        }

        // Skip CRLF after boundary
        if data.get(pos..pos + 2) == Some(b"\r\n") {
            pos += 2;
        } else {
            break;
        }
    }

    Ok(fields)
}

fn find_bytes(data: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    if needle.is_empty() || data.len() < needle.len() {
        return None;
    }
    data[start..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + start)
}

fn extract_param(header_value: &str, param: &str) -> Option<String> {
    let search = format!("{}=", param);
    let idx = header_value.find(&search)?;
    let rest = &header_value[idx + search.len()..];
    let value = if rest.starts_with('"') {
        let end = rest[1..].find('"')?;
        rest[1..end + 1].to_string()
    } else {
        rest.split(';').next().unwrap_or("").trim().to_string()
    };
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

pub fn extract_boundary(content_type: &str) -> Option<String> {
    content_type.split(';').map(|s| s.trim()).find_map(|part| {
        if let Some(b) = part.strip_prefix("boundary=") {
            Some(b.trim_matches('"').to_string())
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_multipart() {
        let boundary = "abcboundary";
        let body = format!(
            "--{}\r\nContent-Disposition: form-data; name=\"field1\"\r\n\r\nvalue1\r\n--{}--\r\n",
            boundary, boundary
        );
        let fields = parse_multipart(body.as_bytes(), boundary).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "field1");
        assert_eq!(fields[0].data, b"value1");
    }

    #[test]
    fn test_parse_multipart_with_file() {
        let boundary = "testbound";
        let body = format!(
            "--{}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.txt\"\r\nContent-Type: text/plain\r\n\r\nfile content\r\n--{}--\r\n",
            boundary, boundary
        );
        let fields = parse_multipart(body.as_bytes(), boundary).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "file");
        assert_eq!(fields[0].filename.as_deref(), Some("test.txt"));
        assert_eq!(fields[0].data, b"file content");
    }

    #[test]
    fn test_extract_boundary() {
        let ct = "multipart/form-data; boundary=abc123";
        assert_eq!(extract_boundary(ct), Some("abc123".to_string()));
    }
}
