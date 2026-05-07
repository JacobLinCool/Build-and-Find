use crate::error::NanowebError;
use serde_json::Value;
use std::collections::HashMap;

pub struct Renderer;

impl Renderer {
    pub fn new() -> Self {
        Self
    }

    pub fn render(
        &self,
        template: &str,
        context: &HashMap<String, Value>,
    ) -> Result<String, NanowebError> {
        let mut result = String::new();
        let chars: Vec<char> = template.chars().collect();
        let len = chars.len();
        let mut i = 0;

        while i < len {
            if i + 1 < len && chars[i] == '{' && chars[i + 1] == '{' {
                // Find closing }}
                let remaining: String = chars[i + 2..].iter().collect();
                if let Some(end) = remaining.find("}}") {
                    let key = remaining[..end].trim();

                    if key.starts_with('#') {
                        let section_name = key[1..].trim();
                        let section_start = i + 2 + end + 2;
                        if let Some(section_end) =
                            find_section_end(&chars, section_start, section_name)
                        {
                            let section_content: String =
                                chars[section_start..section_end.0].iter().collect();

                            if let Some(value) = context.get(section_name) {
                                match value {
                                    Value::Array(arr) => {
                                        for item in arr {
                                            let mut child_context = context.clone();
                                            if let Value::Object(obj) = item {
                                                for (k, v) in obj {
                                                    child_context.insert(k.clone(), v.clone());
                                                }
                                            }
                                            result.push_str(
                                                &self.render(&section_content, &child_context)?,
                                            );
                                        }
                                    }
                                    Value::Bool(true) => {
                                        result.push_str(&self.render(&section_content, context)?);
                                    }
                                    Value::Null | Value::Bool(false) => {}
                                    _ => {
                                        result.push_str(&self.render(&section_content, context)?);
                                    }
                                }
                            }
                            i = section_end.1;
                            if i + 1 < len && chars[i] == '}' && chars[i + 1] == '}' {
                                i += 2;
                            }
                            continue;
                        }
                    } else if key.starts_with('^') {
                        let section_name = key[1..].trim();
                        let section_start = i + 2 + end + 2;
                        if let Some(section_end) =
                            find_section_end(&chars, section_start, section_name)
                        {
                            let section_content: String =
                                chars[section_start..section_end.0].iter().collect();

                            let should_render = match context.get(section_name) {
                                None => true,
                                Some(Value::Null) => true,
                                Some(Value::Bool(false)) => true,
                                Some(Value::Array(arr)) if arr.is_empty() => true,
                                _ => false,
                            };

                            if should_render {
                                result.push_str(&self.render(&section_content, context)?);
                            }
                            i = section_end.1;
                            if i + 1 < len && chars[i] == '}' && chars[i + 1] == '}' {
                                i += 2;
                            }
                            continue;
                        }
                    } else {
                        if let Some(value) = context.get(key) {
                            match value {
                                Value::String(s) => result.push_str(&html_escape(s)),
                                Value::Number(n) => result.push_str(&n.to_string()),
                                Value::Bool(b) => result.push_str(&b.to_string()),
                                Value::Null => {}
                                _ => result.push_str(&html_escape(&value.to_string())),
                            }
                        }
                        i += 2 + end + 2;
                        continue;
                    }
                }
            }
            result.push(chars[i]);
            i += 1;
        }

        Ok(result)
    }
}

fn find_section_end(chars: &[char], start: usize, section_name: &str) -> Option<(usize, usize)> {
    let closing_tag = format!("{{{{/{}}}}}", section_name);
    let closing_chars: Vec<char> = closing_tag.chars().collect();
    let remaining: String = chars[start..].iter().collect();
    if let Some(pos) = remaining.find(&closing_chars.iter().collect::<String>()) {
        Some((start + pos, start + pos + closing_chars.len()))
    } else {
        None
    }
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_variable() {
        let renderer = Renderer::new();
        let mut ctx = HashMap::new();
        ctx.insert("name".to_string(), Value::String("World".to_string()));
        let result = renderer.render("Hello, {{name}}!", &ctx).unwrap();
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn test_html_escaping() {
        let renderer = Renderer::new();
        let mut ctx = HashMap::new();
        ctx.insert(
            "name".to_string(),
            Value::String("<script>alert('xss')</script>".to_string()),
        );
        let result = renderer.render("{{name}}", &ctx).unwrap();
        assert_eq!(
            result,
            "&lt;script&gt;alert(&#x27;xss&#x27;)&lt;/script&gt;"
        );
    }

    #[test]
    fn test_section_list() {
        let renderer = Renderer::new();
        let mut ctx = HashMap::new();
        ctx.insert(
            "items".to_string(),
            Value::Array(vec![
                Value::Object(serde_json::Map::from_iter(vec![(
                    "name".to_string(),
                    Value::String("A".to_string()),
                )])),
                Value::Object(serde_json::Map::from_iter(vec![(
                    "name".to_string(),
                    Value::String("B".to_string()),
                )])),
            ]),
        );
        let result = renderer
            .render("<ul>{{#items}}<li>{{name}}</li>{{/items}}</ul>", &ctx)
            .unwrap();
        assert_eq!(result, "<ul><li>A</li><li>B</li></ul>");
    }

    #[test]
    fn test_inverted_section() {
        let renderer = Renderer::new();
        let ctx = HashMap::new();
        let result = renderer
            .render("{{^items}}No items{{/items}}", &ctx)
            .unwrap();
        assert_eq!(result, "No items");
    }

    #[test]
    fn test_missing_variable() {
        let renderer = Renderer::new();
        let ctx = HashMap::new();
        let result = renderer.render("Hello, {{name}}!", &ctx).unwrap();
        assert_eq!(result, "Hello, !");
    }
}
