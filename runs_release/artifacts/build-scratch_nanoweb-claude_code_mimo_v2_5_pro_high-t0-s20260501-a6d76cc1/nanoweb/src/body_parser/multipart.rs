use crate::error::NanowebError;

#[derive(Debug, Clone)]
pub struct MultipartField {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub data: Vec<u8>,
}

pub fn parse_multipart(body: &[u8], boundary: &str) -> Result<Vec<MultipartField>, NanowebError> {
    let delimiter = format!("--{boundary}");
    let mut fields = Vec::new();

    let mut pos = 0;

    // Find first delimiter
    pos = find_subsequence(body, delimiter.as_bytes(), pos)
        .ok_or_else(|| NanowebError::Body("missing boundary".into()))?
        + delimiter.len();

    loop {
        // Skip \r\n after delimiter
        if pos + 2 > body.len() {
            break;
        }
        if &body[pos..pos + 2] == b"\r\n" {
            pos += 2;
        } else {
            // End delimiter has no \r\n after, just "--"
            break;
        }

        // Parse headers until \r\n\r\n
        let header_end = find_subsequence(body, b"\r\n\r\n", pos)
            .ok_or_else(|| NanowebError::Body("missing header terminator".into()))?;
        let headers_raw = std::str::from_utf8(&body[pos..header_end])
            .map_err(|_| NanowebError::Body("invalid UTF-8 in multipart headers".into()))?;

        let mut name = String::new();
        let mut filename = None;
        let mut content_type = None;

        for line in headers_raw.split("\r\n") {
            let lower = line.to_ascii_lowercase();
            if lower.starts_with("content-disposition:") {
                if let Some(n) = extract_param(line, "name") {
                    name = n;
                }
                filename = extract_param(line, "filename");
            } else if lower.starts_with("content-type:") {
                content_type = line.splitn(2, ':').nth(1).map(|s| s.trim().to_string());
            }
        }

        pos = header_end + 4; // skip \r\n\r\n

        // Find next delimiter
        let next = find_subsequence(body, delimiter.as_bytes(), pos)
            .ok_or_else(|| NanowebError::Body("missing closing boundary".into()))?;

        // Body is between pos and next-2 (\r\n before delimiter)
        let data = if next >= 2 && &body[next - 2..next] == b"\r\n" {
            body[pos..next - 2].to_vec()
        } else {
            body[pos..next].to_vec()
        };

        fields.push(MultipartField {
            name,
            filename,
            content_type,
            data,
        });

        pos = next + delimiter.len();

        // Check for end delimiter (-- after delimiter)
        if pos + 2 <= body.len() && &body[pos..pos + 2] == b"--" {
            break;
        }
    }

    Ok(fields)
}

fn find_subsequence(haystack: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    haystack[start..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + start)
}

fn extract_param(header: &str, param: &str) -> Option<String> {
    let pattern = format!("{param}=\"");
    if let Some(start) = header.find(&pattern) {
        let val_start = start + pattern.len();
        if let Some(end) = header[val_start..].find('"') {
            return Some(header[val_start..val_start + end].to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_multipart() {
        let boundary = "----testboundary";
        let body = format!(
            "------testboundary\r\n\
             Content-Disposition: form-data; name=\"field1\"\r\n\r\n\
             value1\r\n\
             ------testboundary\r\n\
             Content-Disposition: form-data; name=\"file\"; filename=\"test.txt\"\r\n\
             Content-Type: text/plain\r\n\r\n\
             file content\r\n\
             ------testboundary--\r\n"
        );
        let fields = parse_multipart(body.as_bytes(), boundary).unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "field1");
        assert_eq!(fields[0].data, b"value1");
        assert_eq!(fields[1].name, "file");
        assert_eq!(fields[1].filename.as_deref(), Some("test.txt"));
        assert_eq!(fields[1].data, b"file content");
    }
}
