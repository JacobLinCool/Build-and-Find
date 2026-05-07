use crate::error::NanowebError;
use serde_json::Value;

/// Mustache-like template renderer.
pub struct Renderer;

impl Renderer {
    /// Render a template with the given JSON context.
    pub fn render(template: &str, context: &Value) -> Result<String, NanowebError> {
        render_template(template, context)
    }
}

fn render_template(template: &str, context: &Value) -> Result<String, NanowebError> {
    let mut output = String::new();
    let bytes = template.as_bytes();
    let mut pos = 0;

    while pos < bytes.len() {
        let start = match find_pattern(bytes, pos, b"{{") {
            Some(s) => s,
            None => {
                output.push_str(&template[pos..]);
                break;
            }
        };

        output.push_str(&template[pos..start]);

        let end = match find_pattern(bytes, start + 2, b"}}") {
            Some(e) => e,
            None => {
                output.push_str(&template[start..]);
                break;
            }
        };

        let tag = template[start + 2..end].trim();

        if tag.starts_with('#') {
            let section_name = tag[1..].trim();
            let section_end = find_section_end(template, end + 2, section_name)?;
            let section_body = &template[end + 2..section_end];

            if let Some(val) = context_value(context, section_name) {
                match val {
                    Value::Array(arr) => {
                        for item in arr {
                            output.push_str(&render_template(section_body, item)?);
                        }
                    }
                    Value::Bool(true) => {
                        output.push_str(&render_template(section_body, context)?);
                    }
                    Value::Null => {}
                    _ => {
                        output.push_str(&render_template(section_body, context)?);
                    }
                }
            }

            pos = advance_past_close_tag(bytes, section_end, section_name)?;
        } else if tag.starts_with('^') {
            let section_name = tag[1..].trim();
            let section_end = find_section_end(template, end + 2, section_name)?;
            let section_body = &template[end + 2..section_end];

            if let Some(val) = context_value(context, section_name) {
                match val {
                    Value::Null | Value::Bool(false) => {
                        output.push_str(&render_template(section_body, context)?);
                    }
                    Value::Array(arr) if arr.is_empty() => {
                        output.push_str(&render_template(section_body, context)?);
                    }
                    _ => {}
                }
            } else {
                output.push_str(&render_template(section_body, context)?);
            }

            pos = advance_past_close_tag(bytes, section_end, section_name)?;
        } else if tag.starts_with('!') {
            pos = end + 2;
        } else if tag.starts_with('{') && tag.ends_with('}') {
            let name = tag.trim_matches('{').trim_matches('}').trim();
            if let Some(val) = context_value(context, name) {
                output.push_str(&value_to_string(val));
            }
            pos = end + 2;
        } else {
            let name = tag;
            if let Some(val) = context_value(context, name) {
                output.push_str(&html_escape(&value_to_string(val)));
            }
            pos = end + 2;
        }
    }

    Ok(output)
}

/// After finding the start of {{/name}}, skip past its closing }}.
fn advance_past_close_tag(
    bytes: &[u8],
    section_end: usize,
    section_name: &str,
) -> Result<usize, NanowebError> {
    // section_end points to the {{ of the closing tag
    // Find the }} after it
    let close_body_start = section_end + 2;
    let close_end = find_pattern(bytes, close_body_start, b"}}").ok_or_else(|| {
        NanowebError::Template(format!("Unclosed close tag for {}", section_name))
    })?;
    Ok(close_end + 2)
}

fn find_section_end(
    template: &str,
    start: usize,
    section_name: &str,
) -> Result<usize, NanowebError> {
    let mut depth = 1;
    let mut pos = start;
    let bytes = template.as_bytes();

    while pos < bytes.len() {
        if let Some(tag_start) = find_pattern(bytes, pos, b"{{") {
            if let Some(tag_end) = find_pattern(bytes, tag_start + 2, b"}}") {
                let tag = template[tag_start + 2..tag_end].trim();

                if tag == format!("#{}", section_name) {
                    depth += 1;
                } else if tag == format!("/{}", section_name) {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(tag_start);
                    }
                }
                pos = tag_end + 2;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    Err(NanowebError::Template(format!(
        "Unclosed section: {}",
        section_name
    )))
}

fn context_value<'a>(context: &'a Value, key: &str) -> Option<&'a Value> {
    let parts: Vec<&str> = key.split('.').collect();
    let mut current = context;
    for part in parts {
        match current {
            Value::Object(map) => {
                current = map.get(part)?;
            }
            _ => return None,
        }
    }
    Some(current)
}

fn value_to_string(val: &Value) -> String {
    match val {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        _ => String::new(),
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn find_pattern(haystack: &[u8], start: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(start);
    }
    for i in start..haystack.len().saturating_sub(needle.len() - 1) {
        if &haystack[i..i + needle.len()] == needle {
            return Some(i);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_simple_variable() {
        let ctx = json!({"name": "World"});
        let result = render_template("Hello {{name}}!", &ctx).unwrap();
        assert_eq!(result, "Hello World!");
    }

    #[test]
    fn test_html_escaping() {
        let ctx = json!({"name": "<script>alert('xss')</script>"});
        let result = render_template("{{name}}", &ctx).unwrap();
        assert!(result.contains("&lt;script&gt;"));
    }

    #[test]
    fn test_section_list() {
        let ctx = json!({"items": [{"name": "a"}, {"name": "b"}]});
        let result = render_template("{{#items}}{{name}} {{/items}}", &ctx).unwrap();
        assert_eq!(result, "a b ");
    }

    #[test]
    fn test_inverted_section() {
        let ctx = json!({"items": []});
        let result = render_template("{{^items}}empty{{/items}}", &ctx).unwrap();
        assert_eq!(result, "empty");
    }

    #[test]
    fn test_inverted_section_absent() {
        let ctx = json!({});
        let result = render_template("{{^missing}}default{{/missing}}", &ctx).unwrap();
        assert_eq!(result, "default");
    }

    #[test]
    fn test_nested_variable() {
        let ctx = json!({"user": {"name": "Alice"}});
        let result = render_template("{{user.name}}", &ctx).unwrap();
        assert_eq!(result, "Alice");
    }

    #[test]
    fn test_boolean_section() {
        let ctx = json!({"show": true});
        let result = render_template("{{#show}}visible{{/show}}", &ctx).unwrap();
        assert_eq!(result, "visible");
    }

    #[test]
    fn test_comment() {
        let ctx = json!({});
        let result = render_template("before{{! this is a comment }}after", &ctx).unwrap();
        assert_eq!(result, "beforeafter");
    }

    #[test]
    fn test_renderer_api() {
        let ctx = json!({"title": "Test"});
        let result = Renderer::render("<h1>{{title}}</h1>", &ctx).unwrap();
        assert_eq!(result, "<h1>Test</h1>");
    }
}
