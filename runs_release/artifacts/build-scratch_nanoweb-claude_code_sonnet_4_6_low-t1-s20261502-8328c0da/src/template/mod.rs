use crate::error::NanowebError;
use serde_json::Value;

pub struct Renderer;

impl Renderer {
    /// Render a mustache-like template with the given context
    pub fn render(template: &str, context: &Value) -> Result<String, NanowebError> {
        render_value(template, context)
    }
}

fn render_value(template: &str, context: &Value) -> Result<String, NanowebError> {
    let mut result = String::new();
    let mut chars = template.char_indices().peekable();

    while let Some((i, c)) = chars.next() {
        if c == '{' && template[i..].starts_with("{{") {
            // Find closing }}
            let rest = &template[i + 2..];
            if let Some(end) = rest.find("}}") {
                let tag = rest[..end].trim();

                // Skip past }}
                let skip_to = i + 2 + end + 2;
                // advance chars to skip_to
                // We need to consume chars up to skip_to
                while chars.peek().map(|(j, _)| *j).unwrap_or(usize::MAX) < skip_to {
                    chars.next();
                }

                if let Some(stripped) = tag.strip_prefix('#') {
                    // Section tag: {{#name}}...{{/name}}
                    let name = stripped.trim();
                    let closing_tag = format!("{{{{/{}}}}}", name);
                    let after_open = &template[skip_to..];
                    if let Some(close_pos) = after_open.find(&closing_tag) {
                        let section_content = &after_open[..close_pos];
                        let after_section = &after_open[close_pos + closing_tag.len()..];

                        // Advance chars past the section
                        let section_end = skip_to + close_pos + closing_tag.len();
                        while chars.peek().map(|(j, _)| *j).unwrap_or(usize::MAX) < section_end {
                            chars.next();
                        }

                        // Look up name in context
                        let value = lookup(context, name);
                        match &value {
                            Value::Array(arr) => {
                                for item in arr {
                                    result.push_str(&render_value(section_content, item)?);
                                }
                            }
                            Value::Bool(true) => {
                                result.push_str(&render_value(section_content, context)?);
                            }
                            Value::Bool(false) | Value::Null => {
                                // Don't render
                            }
                            _ => {
                                // Truthy: render once with merged context
                                result.push_str(&render_value(section_content, &value)?);
                            }
                        }

                        // Now render the remainder
                        result.push_str(&render_value(after_section, context)?);
                        return Ok(result);
                    }
                } else if let Some(stripped) = tag.strip_prefix('^') {
                    // Inverted section: {{^name}}...{{/name}}
                    let name = stripped.trim();
                    let closing_tag = format!("{{{{/{}}}}}", name);
                    let after_open = &template[skip_to..];
                    if let Some(close_pos) = after_open.find(&closing_tag) {
                        let section_content = &after_open[..close_pos];
                        let after_section = &after_open[close_pos + closing_tag.len()..];

                        let section_end = skip_to + close_pos + closing_tag.len();
                        while chars.peek().map(|(j, _)| *j).unwrap_or(usize::MAX) < section_end {
                            chars.next();
                        }

                        let value = lookup(context, name);
                        let is_falsy = match &value {
                            Value::Bool(false) | Value::Null => true,
                            Value::Array(arr) => arr.is_empty(),
                            Value::String(s) => s.is_empty(),
                            _ => false,
                        };

                        if is_falsy {
                            result.push_str(&render_value(section_content, context)?);
                        }

                        result.push_str(&render_value(after_section, context)?);
                        return Ok(result);
                    }
                } else if tag.starts_with('/') {
                    // Closing tag - should be handled by section, skip
                } else {
                    // Variable: {{name}}
                    let value = lookup(context, tag);
                    result.push_str(&value_to_string(&value));
                }
            } else {
                result.push(c);
            }
        } else {
            result.push(c);
        }
    }

    Ok(result)
}

fn lookup(context: &Value, key: &str) -> Value {
    if key == "." {
        return context.clone();
    }

    // Support nested keys: "a.b.c"
    let parts: Vec<&str> = key.split('.').collect();
    let mut current = context.clone();

    for part in &parts {
        match current {
            Value::Object(ref map) => {
                if let Some(v) = map.get(*part) {
                    current = v.clone();
                } else {
                    return Value::Null;
                }
            }
            _ => return Value::Null,
        }
    }

    current
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        _ => serde_json::to_string(value).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_simple_variable() {
        let result = Renderer::render("Hello, {{name}}!", &json!({"name": "World"})).unwrap();
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn test_section() {
        let result = Renderer::render(
            "{{#show}}visible{{/show}}",
            &json!({"show": true}),
        )
        .unwrap();
        assert_eq!(result, "visible");
    }

    #[test]
    fn test_inverted_section() {
        let result = Renderer::render(
            "{{^hidden}}shown{{/hidden}}",
            &json!({"hidden": false}),
        )
        .unwrap();
        assert_eq!(result, "shown");
    }

    #[test]
    fn test_list() {
        let result = Renderer::render(
            "{{#items}}{{.}},{{/items}}",
            &json!({"items": ["a", "b", "c"]}),
        )
        .unwrap();
        assert_eq!(result, "a,b,c,");
    }
}
