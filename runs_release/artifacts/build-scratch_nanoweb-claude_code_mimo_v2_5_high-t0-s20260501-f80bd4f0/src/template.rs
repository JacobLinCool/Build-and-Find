use serde_json::Value;

pub struct Renderer;

impl Renderer {
    pub fn new() -> Self {
        Self
    }

    pub fn render(&self, template: &str, context: &Value) -> String {
        render(template, context)
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

fn render(template: &str, context: &Value) -> String {
    let mut output = String::new();
    let bytes = template.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i] == b'{' && i + 1 < len && bytes[i + 1] == b'{' {
            // Find the closing }}
            let tag_start = i;
            i += 2;
            let mut tag = String::new();
            while i + 1 < len && !(bytes[i] == b'}' && bytes[i + 1] == b'}') {
                tag.push(bytes[i] as char);
                i += 1;
            }
            if i + 1 < len {
                i += 2; // skip }}
            }
            let tag = tag.trim();

            if let Some(section_name) = tag.strip_prefix('#') {
                // Find the matching closing tag, accounting for nesting
                let (section_body, after) = find_section_body(template, tag_start, section_name);
                let value = get_value(context, section_name);
                match &value {
                    Value::Array(arr) => {
                        for item in arr {
                            output.push_str(&render(&section_body, item));
                        }
                    }
                    Value::Bool(true) => {
                        output.push_str(&render(&section_body, context));
                    }
                    Value::Null | Value::Bool(false) => {}
                    _ => {
                        output.push_str(&render(&section_body, context));
                    }
                }
                return output + &render(&after, context);
            } else if let Some(section_name) = tag.strip_prefix('^') {
                let (section_body, after) = find_section_body(template, tag_start, section_name);
                let value = get_value(context, section_name);
                match &value {
                    Value::Null | Value::Bool(false) => {
                        output.push_str(&render(&section_body, context));
                    }
                    _ => {}
                }
                return output + &render(&after, context);
            } else if tag.starts_with('/') {
                // Closing tag reached at top level - shouldn't happen in well-formed template
                // Return what we have so far
                return output;
            } else {
                // Variable
                let value = get_value(context, tag);
                let rendered = match &value {
                    Value::String(s) => s.clone(),
                    Value::Null => String::new(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    Value::Array(arr) => arr
                        .iter()
                        .map(|v| match v {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        })
                        .collect::<Vec<_>>()
                        .join(" "),
                    Value::Object(map) => serde_json::to_string(map).unwrap_or_default(),
                };
                output.push_str(&rendered);
            }
        } else {
            output.push(bytes[i] as char);
            i += 1;
        }
    }

    output
}

/// Find the body of a section and the remainder after the closing tag.
/// Handles nested sections with the same name.
fn find_section_body(template: &str, tag_start: usize, section_name: &str) -> (String, String) {
    let open_tag_with_hash = format!("{{{{#{section_name}}}}}");
    let open_tag_with_caret = format!("{{{{^{section_name}}}}}");
    let close_tag = format!("{{{{/{section_name}}}}}");

    // Find the end of the opening tag (could be # or ^)
    let open_tag = if template[tag_start..].starts_with(&open_tag_with_hash) {
        &open_tag_with_hash
    } else {
        &open_tag_with_caret
    };
    let open_pos = tag_start + open_tag.len();

    // Find the matching close tag, counting nesting
    let mut depth = 1;
    let mut pos = open_pos;
    let body_end;
    let after;

    loop {
        let next_open_hash = template[pos..].find(&open_tag_with_hash);
        let next_open_caret = template[pos..].find(&open_tag_with_caret);
        let next_close = template[pos..].find(&close_tag);

        let next_open = match (next_open_hash, next_open_caret) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, None) => a,
            (None, b) => b,
        };

        match (next_open, next_close) {
            (Some(o), Some(c)) if o < c => {
                depth += 1;
                pos += o + open_tag_with_hash.len();
            }
            (Some(_), Some(c)) => {
                depth -= 1;
                if depth == 0 {
                    body_end = pos + c;
                    after = template[body_end + close_tag.len()..].to_string();
                    let body = template[open_pos..body_end].to_string();
                    return (body, after);
                }
                pos += c + close_tag.len();
            }
            (None, Some(c)) => {
                depth -= 1;
                if depth == 0 {
                    body_end = pos + c;
                    after = template[body_end + close_tag.len()..].to_string();
                    let body = template[open_pos..body_end].to_string();
                    return (body, after);
                }
                pos += c + close_tag.len();
            }
            _ => {
                break;
            }
        }
    }

    // No matching close tag
    let body = template[open_pos..].to_string();
    (body, String::new())
}

fn get_value(context: &Value, key: &str) -> Value {
    // Support {{.}} for current context
    if key == "." {
        return context.clone();
    }
    match context {
        Value::Object(map) => map.get(key).cloned().unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn simple_variable() {
        let renderer = Renderer::new();
        let ctx = json!({"name": "World"});
        let result = renderer.render("Hello {{name}}!", &ctx);
        assert_eq!(result, "Hello World!");
    }

    #[test]
    fn missing_variable() {
        let renderer = Renderer::new();
        let ctx = json!({});
        let result = renderer.render("Hello {{name}}!", &ctx);
        assert_eq!(result, "Hello !");
    }

    #[test]
    fn section_array() {
        let renderer = Renderer::new();
        let ctx = json!({"items": ["a", "b", "c"]});
        let result = renderer.render("{{#items}}{{.}} {{/items}}", &ctx);
        assert!(result.contains("a"), "got: {result}");
        assert!(result.contains("b"), "got: {result}");
        assert!(result.contains("c"), "got: {result}");
    }

    #[test]
    fn inverted_section() {
        let renderer = Renderer::new();
        let ctx = json!({});
        let result = renderer.render("{{^missing}}default{{/missing}}", &ctx);
        assert_eq!(result, "default");
    }

    #[test]
    fn inverted_section_present() {
        let renderer = Renderer::new();
        let ctx = json!({"present": true});
        let result = renderer.render("{{^present}}hidden{{/present}}", &ctx);
        assert_eq!(result, "");
    }

    #[test]
    fn number_variable() {
        let renderer = Renderer::new();
        let ctx = json!({"count": 42});
        let result = renderer.render("Count: {{count}}", &ctx);
        assert_eq!(result, "Count: 42");
    }

    #[test]
    fn multiple_variables() {
        let renderer = Renderer::new();
        let ctx = json!({"greeting": "Hello", "name": "World"});
        let result = renderer.render("{{greeting}}, {{name}}!", &ctx);
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn no_template_tags() {
        let renderer = Renderer::new();
        let ctx = json!({});
        let result = renderer.render("plain text", &ctx);
        assert_eq!(result, "plain text");
    }

    #[test]
    fn bool_variable() {
        let renderer = Renderer::new();
        let ctx = json!({"active": true});
        let result = renderer.render("Active: {{active}}", &ctx);
        assert_eq!(result, "Active: true");
    }

    #[test]
    fn section_with_surrounding_text() {
        let renderer = Renderer::new();
        let ctx = json!({"items": ["x", "y"]});
        let result = renderer.render("Before {{#items}}[{{.}}] {{/items}}After", &ctx);
        assert_eq!(result, "Before [x] [y] After");
    }

    #[test]
    fn nested_sections() {
        let renderer = Renderer::new();
        let ctx = json!({"show": true, "name": "test"});
        let result = renderer.render("{{#show}}{{name}}{{/show}}", &ctx);
        assert_eq!(result, "test");
    }
}
