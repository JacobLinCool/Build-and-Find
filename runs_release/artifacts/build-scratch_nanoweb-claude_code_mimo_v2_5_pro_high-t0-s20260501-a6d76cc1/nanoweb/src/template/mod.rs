use crate::error::NanowebError;
use serde_json::Value;

pub struct Renderer;

impl Renderer {
    pub fn new() -> Self {
        Self
    }

    pub fn render(&self, template: &str, context: &Value) -> Result<String, NanowebError> {
        let mut output = String::new();
        self.render_into(template, context, &mut output)?;
        Ok(output)
    }

    fn render_into(
        &self,
        template: &str,
        context: &Value,
        output: &mut String,
    ) -> Result<(), NanowebError> {
        let bytes = template.as_bytes();
        let len = bytes.len();
        let mut pos = 0;
        let mut literal_start = 0;

        while pos < len {
            if pos + 1 < len && bytes[pos] == b'{' && bytes[pos + 1] == b'{' {
                // Flush literal
                if literal_start < pos {
                    output.push_str(&template[literal_start..pos]);
                }

                // Find closing }}
                let close = template[pos + 2..]
                    .find("}}")
                    .ok_or_else(|| NanowebError::Template("unclosed {{".into()))?
                    + pos
                    + 2;

                let tag = template[pos + 2..close].trim();

                if tag.starts_with('#') {
                    // Section: iterate or truthy
                    let name = tag[1..].trim();
                    let val = resolve(context, name);
                    pos = close + 2;
                    literal_start = pos;

                    // Find matching {{/name}}
                    let end_tag = format!("{{{{{}/{}}}}}", "", name);
                    let section_end = find_section_end(template, pos, name)?;

                    let section_body = &template[pos..section_end];
                    pos = section_end + end_tag.len();
                    literal_start = pos;

                    match val {
                        Some(Value::Array(arr)) => {
                            for item in arr {
                                self.render_into(section_body, item, output)?;
                            }
                        }
                        Some(Value::Bool(true))
                        | Some(Value::Object(_))
                        | Some(Value::String(_))
                        | Some(Value::Number(_)) => {
                            self.render_into(section_body, &val.unwrap(), output)?;
                        }
                        _ => {} // falsy, skip section
                    }
                } else if tag.starts_with('^') {
                    // Inverted section: render if absent or falsy
                    let name = tag[1..].trim();
                    let val = resolve(context, name);
                    pos = close + 2;
                    literal_start = pos;

                    let section_end = find_section_end(template, pos, name)?;
                    let section_body = &template[pos..section_end];
                    let end_tag = format!("{{{{{}/{}}}}}", "", name);
                    pos = section_end + end_tag.len();
                    literal_start = pos;

                    let is_falsy = match val {
                        None => true,
                        Some(Value::Null) => true,
                        Some(Value::Bool(b)) => !b,
                        Some(Value::Array(a)) => a.is_empty(),
                        _ => false,
                    };

                    if is_falsy {
                        self.render_into(section_body, context, output)?;
                    }
                } else {
                    // Variable: {{name}}
                    let name = tag;
                    if let Some(val) = resolve(context, name) {
                        match val {
                            Value::String(s) => output.push_str(&html_escape(&s)),
                            Value::Number(n) => output.push_str(&n.to_string()),
                            Value::Bool(b) => output.push_str(&b.to_string()),
                            Value::Null => {}
                            _ => output.push_str(&html_escape(&val.to_string())),
                        }
                    }
                    pos = close + 2;
                    literal_start = pos;
                }
            } else {
                pos += 1;
            }
        }

        if literal_start < len {
            output.push_str(&template[literal_start..]);
        }

        Ok(())
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

fn resolve<'a>(context: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = context;
    for part in path.split('.') {
        current = match current {
            Value::Object(map) => map.get(part)?,
            _ => return None,
        };
    }
    Some(current)
}

fn find_section_end(template: &str, start: usize, name: &str) -> Result<usize, NanowebError> {
    let end_tag = format!("{{{{{}/{}}}}}", "", name);
    // Simple approach: find the first occurrence of the closing tag
    // This doesn't handle nested sections with the same name, but is sufficient for a minimal engine
    template[start..]
        .find(&end_tag)
        .map(|i| i + start)
        .ok_or_else(|| NanowebError::Template(format!("unclosed section: {name}")))
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
    use serde_json::json;

    #[test]
    fn render_variable() {
        let r = Renderer::new();
        let result = r
            .render("Hello, {{name}}!", &json!({"name": "World"}))
            .unwrap();
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn render_section_list() {
        let r = Renderer::new();
        let tpl = "<ul>{{#items}}<li>{{name}}</li>{{/items}}</ul>";
        let ctx = json!({"items": [{"name": "A"}, {"name": "B"}]});
        let result = r.render(tpl, &ctx).unwrap();
        assert_eq!(result, "<ul><li>A</li><li>B</li></ul>");
    }

    #[test]
    fn render_inverted_section() {
        let r = Renderer::new();
        let tpl = "{{^items}}No items{{/items}}";
        let ctx = json!({"items": []});
        let result = r.render(tpl, &ctx).unwrap();
        assert_eq!(result, "No items");
    }

    #[test]
    fn render_html_escaped() {
        let r = Renderer::new();
        let result = r
            .render("{{val}}", &json!({"val": "<script>alert('xss')</script>"}))
            .unwrap();
        assert_eq!(
            result,
            "&lt;script&gt;alert(&#x27;xss&#x27;)&lt;/script&gt;"
        );
    }

    #[test]
    fn render_nested_path() {
        let r = Renderer::new();
        let result = r
            .render("{{user.name}}", &json!({"user": {"name": "Alice"}}))
            .unwrap();
        assert_eq!(result, "Alice");
    }
}
