use crate::body::{FieldValue, FormField, MultipartForm};
use crate::error::NanowebError;

/// Parses a multipart/form-data body given the boundary string.
pub fn parse(body: &[u8], boundary: &str) -> Result<MultipartForm, NanowebError> {
    let delimiter = format!("--{}", boundary);
    let body_str = std::str::from_utf8(body)
        .map_err(|_| NanowebError::BadRequest("Multipart body is not valid UTF-8".into()))?;

    let mut form = MultipartForm { fields: Vec::new() };
    let mut parts = body_str.split(delimiter.as_str());

    // Skip preamble
    let _ = parts.next();

    for part in parts {
        let part = part.trim_start_matches("\r\n");
        if part.starts_with("--") || part.starts_with("--\r\n") || part == "--" {
            break; // Final delimiter
        }
        let part = part.trim_end_matches("\r\n");
        if part == "--" {
            break;
        }

        let field = parse_part(part)?;
        if let Some(f) = field {
            form.fields.push(f);
        }
    }

    // Also handle the binary-safe version
    drop(form);
    let form = parse_binary(body, boundary)?;
    Ok(form)
}

fn parse_part(part: &str) -> Result<Option<FormField>, NanowebError> {
    // Find end of headers
    let header_end = part.find("\r\n\r\n");
    let (headers_str, body) = if let Some(pos) = header_end {
        (&part[..pos], &part[pos + 4..])
    } else {
        return Ok(None);
    };

    // Parse headers
    let mut name = String::new();
    let mut filename: Option<String> = None;
    let mut content_type: Option<String> = None;

    for line in headers_str.split("\r\n") {
        if line.to_lowercase().starts_with("content-disposition:") {
            let value = &line[line.find(':').unwrap() + 1..].trim().to_string();
            name = extract_param(value, "name").unwrap_or_default();
            filename = extract_param(value, "filename");
        } else if line.to_lowercase().starts_with("content-type:") {
            content_type = Some(line[line.find(':').unwrap() + 1..].trim().to_string());
        }
    }

    if name.is_empty() {
        return Ok(None);
    }

    let value = if filename.is_some()
        || content_type
            .as_deref()
            .map(|ct| ct != "text/plain")
            .unwrap_or(false)
    {
        FieldValue::File {
            filename,
            data: body.as_bytes().to_vec(),
        }
    } else {
        FieldValue::Text(body.to_string())
    };

    Ok(Some(FormField {
        name,
        value,
        content_type,
    }))
}

fn extract_param(header_value: &str, param: &str) -> Option<String> {
    let search = format!("{}=", param);
    let pos = header_value.find(&search)?;
    let rest = &header_value[pos + search.len()..];
    if rest.starts_with('"') {
        let end = rest[1..].find('"')? + 1;
        Some(rest[1..end].to_string())
    } else {
        let end = rest.find([';', ' ', '\r', '\n']).unwrap_or(rest.len());
        Some(rest[..end].to_string())
    }
}

/// Binary-safe multipart parser (handles file uploads with binary data).
fn parse_binary(body: &[u8], boundary: &str) -> Result<MultipartForm, NanowebError> {
    let delimiter = format!("--{}", boundary);
    let delim_bytes = delimiter.as_bytes();
    let _final_delim = format!("--{}--", boundary);

    let mut form = MultipartForm { fields: Vec::new() };
    let mut pos = 0;

    // Find first delimiter
    let first = find_bytes(body, delim_bytes, pos)
        .ok_or_else(|| NanowebError::BadRequest("Multipart delimiter not found".into()))?;
    pos = first + delim_bytes.len();

    // Skip CRLF after first delimiter
    if pos < body.len() && body[pos..].starts_with(b"\r\n") {
        pos += 2;
    } else {
        return Ok(form);
    }

    loop {
        // Find next delimiter
        let next_delim = match find_bytes(body, delim_bytes, pos) {
            Some(p) => p,
            None => break,
        };

        let part_data = &body[pos..next_delim.saturating_sub(2)]; // -2 for CRLF before delimiter

        if let Some(field) = parse_part_binary(part_data)? {
            form.fields.push(field);
        }

        pos = next_delim + delim_bytes.len();

        // Check if this is the final delimiter
        if pos + 2 <= body.len() && &body[pos..pos + 2] == b"--" {
            break;
        }

        // Skip CRLF
        if pos < body.len() && body[pos..].starts_with(b"\r\n") {
            pos += 2;
        } else {
            break;
        }
    }

    Ok(form)
}

fn find_bytes(haystack: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack[start..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + start)
}

fn parse_part_binary(part: &[u8]) -> Result<Option<FormField>, NanowebError> {
    // Find end of headers (CRLFCRLF)
    let header_end = part.windows(4).position(|w| w == b"\r\n\r\n");

    let (header_bytes, body_bytes) = match header_end {
        Some(pos) => (&part[..pos], &part[pos + 4..]),
        None => return Ok(None),
    };

    let headers_str = std::str::from_utf8(header_bytes)
        .map_err(|_| NanowebError::BadRequest("Multipart headers not UTF-8".into()))?;

    let mut name = String::new();
    let mut filename: Option<String> = None;
    let mut content_type: Option<String> = None;

    for line in headers_str.split("\r\n") {
        if line.to_lowercase().starts_with("content-disposition:") {
            let value = line[line.find(':').unwrap() + 1..].trim();
            name = extract_param(value, "name").unwrap_or_default();
            filename = extract_param(value, "filename");
        } else if line.to_lowercase().starts_with("content-type:") {
            content_type = Some(line[line.find(':').unwrap() + 1..].trim().to_string());
        }
    }

    if name.is_empty() {
        return Ok(None);
    }

    let value = if filename.is_some() {
        FieldValue::File {
            filename,
            data: body_bytes.to_vec(),
        }
    } else {
        let text = std::str::from_utf8(body_bytes)
            .map_err(|_| NanowebError::BadRequest("Multipart field value not UTF-8".into()))?;
        FieldValue::Text(text.to_string())
    };

    Ok(Some(FormField {
        name,
        value,
        content_type,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_form() {
        let boundary = "boundary123";
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"field1\"\r\n\r\nvalue1\r\n--{boundary}--\r\n",
            boundary = boundary
        );
        let form = parse(body.as_bytes(), boundary).unwrap();
        assert_eq!(form.get_text("field1"), Some("value1"));
    }

    #[test]
    fn test_parse_multiple_fields() {
        let boundary = "testboundary";
        let body = format!(
            "--{b}\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\nalice\r\n\
             --{b}\r\nContent-Disposition: form-data; name=\"age\"\r\n\r\n30\r\n\
             --{b}--\r\n",
            b = boundary
        );
        let form = parse(body.as_bytes(), boundary).unwrap();
        assert_eq!(form.get_text("name"), Some("alice"));
        assert_eq!(form.get_text("age"), Some("30"));
    }

    #[test]
    fn test_parse_file_upload() {
        let boundary = "fileboundary";
        let file_data = b"file content here";
        let header = format!(
            "--{b}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.txt\"\r\nContent-Type: text/plain\r\n\r\n",
            b = boundary
        );
        let footer = format!("\r\n--{}--\r\n", boundary);
        let mut body = header.into_bytes();
        body.extend_from_slice(file_data);
        body.extend_from_slice(footer.as_bytes());

        let form = parse(&body, boundary).unwrap();
        let (fname, data) = form.get_file("file").unwrap();
        assert_eq!(fname.as_deref(), Some("test.txt"));
        assert_eq!(data, file_data);
    }
}
