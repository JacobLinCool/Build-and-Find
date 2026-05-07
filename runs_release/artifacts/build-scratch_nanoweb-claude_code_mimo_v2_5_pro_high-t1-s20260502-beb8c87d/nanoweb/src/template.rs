use crate::error::NanowebError;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum TemplateValue {
    String(String),
    Bool(bool),
    List(Vec<TemplateValue>),
    Map(HashMap<String, TemplateValue>),
    Null,
}

impl From<&str> for TemplateValue {
    fn from(s: &str) -> Self {
        TemplateValue::String(s.to_string())
    }
}

impl From<String> for TemplateValue {
    fn from(s: String) -> Self {
        TemplateValue::String(s)
    }
}

impl From<bool> for TemplateValue {
    fn from(b: bool) -> Self {
        TemplateValue::Bool(b)
    }
}

impl From<Vec<TemplateValue>> for TemplateValue {
    fn from(v: Vec<TemplateValue>) -> Self {
        TemplateValue::List(v)
    }
}

impl From<HashMap<String, TemplateValue>> for TemplateValue {
    fn from(m: HashMap<String, TemplateValue>) -> Self {
        TemplateValue::Map(m)
    }
}

pub type Context = HashMap<String, TemplateValue>;

pub struct Renderer;

impl Renderer {
    pub fn render(template: &str, context: &Context) -> Result<String, NanowebError> {
        let mut output = String::new();
        Self::render_internal(template, context, &mut output)?;
        Ok(output)
    }

    fn render_internal(
        template: &str,
        context: &Context,
        output: &mut String,
    ) -> Result<(), NanowebError> {
        let chars: Vec<char> = template.chars().collect();
        let len = chars.len();
        let mut pos = 0;

        while pos < len {
            if pos + 1 < len && chars[pos] == '{' && chars[pos + 1] == '{' {
                // Find closing }}
                if let Some(end) = find_closing(&chars, pos + 2, '}', '}') {
                    let tag: String = chars[pos + 2..end].iter().collect();
                    let tag = tag.trim();

                    if let Some(name) = tag.strip_prefix('#') {
                        // Section: {{#name}} ... {{/name}}
                        let section_name = name.trim();
                        let section_start = end + 2;
                        if let Some(section_end) =
                            find_section_end(&chars, section_start, section_name)
                        {
                            let section_body: String =
                                chars[section_start..section_end].iter().collect();
                            Self::render_section(section_name, &section_body, context, output)?;
                            pos = section_end + section_name.len() + 5; // skip {{/name}}
                        } else {
                            return Err(NanowebError::Template(format!(
                                "Unclosed section: {section_name}"
                            )));
                        }
                    } else if let Some(name) = tag.strip_prefix('^') {
                        // Inverted section: {{^name}} ... {{/name}}
                        let section_name = name.trim();
                        let section_start = end + 2;
                        if let Some(section_end) =
                            find_section_end(&chars, section_start, section_name)
                        {
                            let section_body: String =
                                chars[section_start..section_end].iter().collect();
                            Self::render_inverted_section(
                                section_name,
                                &section_body,
                                context,
                                output,
                            )?;
                            pos = section_end + section_name.len() + 5;
                        } else {
                            return Err(NanowebError::Template(format!(
                                "Unclosed inverted section: {section_name}"
                            )));
                        }
                    } else {
                        // Variable: {{name}}
                        if let Some(value) = context.get(tag) {
                            match value {
                                TemplateValue::String(s) => output.push_str(&html_escape(s)),
                                TemplateValue::Bool(b) => output.push_str(&b.to_string()),
                                TemplateValue::Null => {}
                                _ => {}
                            }
                        }
                        pos = end + 2;
                    }
                } else {
                    output.push(chars[pos]);
                    pos += 1;
                }
            } else {
                output.push(chars[pos]);
                pos += 1;
            }
        }

        Ok(())
    }

    fn render_section(
        name: &str,
        body: &str,
        context: &Context,
        output: &mut String,
    ) -> Result<(), NanowebError> {
        if let Some(value) = context.get(name) {
            match value {
                TemplateValue::Bool(true) => {
                    Self::render_internal(body, context, output)?;
                }
                TemplateValue::Bool(false) => {}
                TemplateValue::List(items) => {
                    for item in items {
                        if let TemplateValue::Map(item_ctx) = item {
                            Self::render_internal(body, item_ctx, output)?;
                        }
                    }
                }
                TemplateValue::Map(item_ctx) => {
                    Self::render_internal(body, item_ctx, output)?;
                }
                TemplateValue::String(s) if !s.is_empty() => {
                    Self::render_internal(body, context, output)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn render_inverted_section(
        name: &str,
        body: &str,
        context: &Context,
        output: &mut String,
    ) -> Result<(), NanowebError> {
        let should_render = match context.get(name) {
            None => true,
            Some(TemplateValue::Bool(false)) => true,
            Some(TemplateValue::List(items)) if items.is_empty() => true,
            Some(TemplateValue::String(s)) if s.is_empty() => true,
            Some(TemplateValue::Null) => true,
            _ => false,
        };
        if should_render {
            Self::render_internal(body, context, output)?;
        }
        Ok(())
    }
}

fn find_closing(chars: &[char], start: usize, c1: char, c2: char) -> Option<usize> {
    let len = chars.len();
    for i in start..len - 1 {
        if chars[i] == c1 && chars[i + 1] == c2 {
            return Some(i);
        }
    }
    None
}

fn find_section_end(chars: &[char], start: usize, name: &str) -> Option<usize> {
    let len = chars.len();
    let close_tag = format!("/{name}");
    let close_pattern: Vec<char> = close_tag.chars().collect();

    for i in start..len {
        if i + 3 < len && chars[i] == '{' && chars[i + 1] == '{' {
            let tag_start = i + 2;
            if let Some(tag_end) = find_closing(chars, tag_start, '}', '}') {
                let tag: String = chars[tag_start..tag_end].iter().collect();
                if tag.trim() == close_pattern.iter().collect::<String>() {
                    return Some(i);
                }
            }
        }
    }
    None
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
        let mut ctx = Context::new();
        ctx.insert("name".to_string(), "World".into());
        let result = Renderer::render("Hello, {{name}}!", &ctx).unwrap();
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn test_html_escaping() {
        let mut ctx = Context::new();
        ctx.insert("name".to_string(), "<script>alert('xss')</script>".into());
        let result = Renderer::render("{{name}}", &ctx).unwrap();
        assert!(result.contains("&lt;script&gt;"));
    }

    #[test]
    fn test_section_list() {
        let mut ctx = Context::new();
        let items = vec![
            TemplateValue::Map({
                let mut m = HashMap::new();
                m.insert("item".to_string(), "A".into());
                m
            }),
            TemplateValue::Map({
                let mut m = HashMap::new();
                m.insert("item".to_string(), "B".into());
                m
            }),
        ];
        ctx.insert("items".to_string(), TemplateValue::List(items));
        let result =
            Renderer::render("<ul>{{#items}}<li>{{item}}</li>{{/items}}</ul>", &ctx).unwrap();
        assert_eq!(result, "<ul><li>A</li><li>B</li></ul>");
    }

    #[test]
    fn test_inverted_section() {
        let ctx = Context::new();
        let result = Renderer::render("{{^items}}No items{{/items}}", &ctx).unwrap();
        assert_eq!(result, "No items");
    }

    #[test]
    fn test_section_boolean() {
        let mut ctx = Context::new();
        ctx.insert("show".to_string(), TemplateValue::Bool(true));
        let result = Renderer::render("{{#show}}visible{{/show}}", &ctx).unwrap();
        assert_eq!(result, "visible");
    }
}
