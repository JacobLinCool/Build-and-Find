use crate::error::NanowebError;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum Value {
    String(String),
    Bool(bool),
    List(Vec<HashMap<String, Value>>),
    Map(HashMap<String, Value>),
}

impl Value {
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::String(s) => !s.is_empty(),
            Value::Bool(b) => *b,
            Value::List(l) => !l.is_empty(),
            Value::Map(m) => !m.is_empty(),
        }
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::String(s.to_string())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::String(s)
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl From<Vec<HashMap<String, Value>>> for Value {
    fn from(v: Vec<HashMap<String, Value>>) -> Self {
        Value::List(v)
    }
}

pub struct Template {
    source: String,
}

impl Template {
    pub fn new(source: impl Into<String>) -> Self {
        Template {
            source: source.into(),
        }
    }

    pub fn render(&self, context: &HashMap<String, Value>) -> Result<String, NanowebError> {
        render_template(&self.source, context)
    }
}

fn render_template(
    template: &str,
    context: &HashMap<String, Value>,
) -> Result<String, NanowebError> {
    let mut output = String::new();
    let mut chars = template.char_indices().peekable();

    while let Some((i, c)) = chars.next() {
        if c == '{' {
            if let Some(&(_, '{')) = chars.peek() {
                chars.next(); // consume second {

                // Read tag
                let rest = &template[i + 2..];
                let close = rest
                    .find("}}")
                    .ok_or_else(|| NanowebError::Template("unclosed template tag".to_string()))?;
                let tag = rest[..close].trim();

                // Skip past the tag in main iterator
                for _ in 0..close + 4 - 2 {
                    chars.next();
                }

                if let Some(name) = tag.strip_prefix('#') {
                    // Block section {{#name}}...{{/name}}
                    let name = name.trim();
                    let section_end = format!("{{{{/{}}}}}", name);
                    let current_pos = i + 2 + close + 2;
                    let remaining = &template[current_pos..];
                    let end = remaining.find(&section_end).ok_or_else(|| {
                        NanowebError::Template(format!("unclosed section: {}", name))
                    })?;
                    let section_body = &remaining[..end];

                    // Skip the section in the main iterator
                    for _ in 0..end + section_end.len() {
                        chars.next();
                    }

                    if let Some(val) = context.get(name) {
                        if let Value::List(items) = val {
                            for item in items {
                                let rendered = render_template(section_body, item)?;
                                output.push_str(&rendered);
                            }
                        } else if val.is_truthy() {
                            let rendered = render_template(section_body, context)?;
                            output.push_str(&rendered);
                        }
                    }
                } else if let Some(name) = tag.strip_prefix('^') {
                    // Inverted section {{^name}}...{{/name}}
                    let name = name.trim();
                    let section_end = format!("{{{{/{}}}}}", name);
                    let current_pos = i + 2 + close + 2;
                    let remaining = &template[current_pos..];
                    let end = remaining.find(&section_end).ok_or_else(|| {
                        NanowebError::Template(format!("unclosed inverted section: {}", name))
                    })?;
                    let section_body = &remaining[..end];

                    for _ in 0..end + section_end.len() {
                        chars.next();
                    }

                    let render = context.get(name).map(|v| !v.is_truthy()).unwrap_or(true);
                    if render {
                        let rendered = render_template(section_body, context)?;
                        output.push_str(&rendered);
                    }
                } else if tag.starts_with('/') {
                    // End tag - should have been consumed above
                    return Err(NanowebError::Template(format!(
                        "unexpected end tag: {}",
                        tag
                    )));
                } else {
                    // Variable substitution {{name}}
                    if let Some(val) = context.get(tag) {
                        match val {
                            Value::String(s) => output.push_str(s),
                            Value::Bool(b) => output.push_str(&b.to_string()),
                            _ => output.push_str("[object]"),
                        }
                    }
                }
                continue;
            }
        }
        output.push(c);
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(pairs: &[(&str, Value)]) -> HashMap<String, Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn test_variable_substitution() {
        let tmpl = Template::new("Hello, {{name}}!");
        let ctx = ctx(&[("name", Value::from("World"))]);
        assert_eq!(tmpl.render(&ctx).unwrap(), "Hello, World!");
    }

    #[test]
    fn test_missing_variable() {
        let tmpl = Template::new("Hello, {{name}}!");
        let ctx = HashMap::new();
        assert_eq!(tmpl.render(&ctx).unwrap(), "Hello, !");
    }

    #[test]
    fn test_section_truthy() {
        let tmpl = Template::new("{{#show}}visible{{/show}}");
        let ctx = ctx(&[("show", Value::Bool(true))]);
        assert_eq!(tmpl.render(&ctx).unwrap(), "visible");
    }

    #[test]
    fn test_section_falsy() {
        let tmpl = Template::new("{{#show}}visible{{/show}}");
        let ctx = ctx(&[("show", Value::Bool(false))]);
        assert_eq!(tmpl.render(&ctx).unwrap(), "");
    }

    #[test]
    fn test_inverted_section() {
        let tmpl = Template::new("{{^absent}}fallback{{/absent}}");
        let ctx = HashMap::new();
        assert_eq!(tmpl.render(&ctx).unwrap(), "fallback");
    }

    #[test]
    fn test_list_section() {
        let tmpl = Template::new("{{#items}}{{name}} {{/items}}");
        let items = vec![
            [("name".to_string(), Value::from("Alice"))]
                .iter()
                .cloned()
                .collect(),
            [("name".to_string(), Value::from("Bob"))]
                .iter()
                .cloned()
                .collect(),
        ];
        let ctx = ctx(&[("items", Value::List(items))]);
        assert_eq!(tmpl.render(&ctx).unwrap(), "Alice Bob ");
    }
}
