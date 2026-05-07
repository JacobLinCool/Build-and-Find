use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Default)]
pub struct Renderer;

impl Renderer {
    pub fn new() -> Self {
        Self
    }

    pub fn render<T: Serialize>(&self, template: &str, context: &T) -> crate::Result<String> {
        let value = serde_json::to_value(context)?;
        let scopes = vec![value];
        Ok(render_template(template, &scopes))
    }
}

fn render_template(template: &str, scopes: &[Value]) -> String {
    let mut out = String::new();
    let mut cursor = 0usize;
    while let Some(open) = template[cursor..].find("{{") {
        let open = cursor + open;
        out.push_str(&template[cursor..open]);
        let close = template[open + 2..]
            .find("}}")
            .map(|idx| open + 2 + idx)
            .unwrap_or_else(|| template.len());
        if close == template.len() {
            out.push_str(&template[open..]);
            return out;
        }
        let tag = template[open + 2..close].trim();
        if let Some(section) = tag.strip_prefix('#') {
            let (inner, end) = extract_section(template, close + 2, section.trim());
            if let Some(value) = lookup(scopes, section.trim()) {
                if is_truthy(value) {
                    match value {
                        Value::Array(items) => {
                            for item in items {
                                let mut next = scopes.to_vec();
                                next.push(item.clone());
                                out.push_str(&render_template(&inner, &next));
                            }
                        }
                        Value::Object(_) => {
                            let mut next = scopes.to_vec();
                            next.push(value.clone());
                            out.push_str(&render_template(&inner, &next));
                        }
                        _ => {
                            out.push_str(&render_template(&inner, scopes));
                        }
                    }
                }
            }
            cursor = end;
            continue;
        }
        if let Some(section) = tag.strip_prefix('^') {
            let (inner, end) = extract_section(template, close + 2, section.trim());
            let should_render = match lookup(scopes, section.trim()) {
                Some(value) => !is_truthy(value),
                None => true,
            };
            if should_render {
                out.push_str(&render_template(&inner, scopes));
            }
            cursor = end;
            continue;
        }
        if tag.starts_with('/') {
            cursor = close + 2;
            continue;
        }
        if let Some(value) = lookup(scopes, tag) {
            out.push_str(&stringify(value));
        }
        cursor = close + 2;
    }
    out.push_str(&template[cursor..]);
    out
}

fn extract_section(template: &str, start: usize, name: &str) -> (String, usize) {
    let mut depth = 1usize;
    let mut cursor = start;
    while let Some(open) = template[cursor..].find("{{") {
        let open = cursor + open;
        let close = template[open + 2..]
            .find("}}")
            .map(|idx| open + 2 + idx)
            .unwrap_or(template.len());
        if close == template.len() {
            break;
        }
        let tag = template[open + 2..close].trim();
        if tag == format!("#{}", name) || tag == format!("^{}", name) {
            depth += 1;
        } else if tag == format!("/{}", name) {
            depth -= 1;
            if depth == 0 {
                return (template[start..open].to_string(), close + 2);
            }
        }
        cursor = close + 2;
    }
    (template[start..].to_string(), template.len())
}

fn lookup<'a>(scopes: &'a [Value], name: &str) -> Option<&'a Value> {
    if name == "." {
        return scopes.last();
    }
    for scope in scopes.iter().rev() {
        if let Some(value) = lookup_in_value(scope, name) {
            return Some(value);
        }
    }
    None
}

fn lookup_in_value<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    let mut current = value;
    for part in name.split('.') {
        match current {
            Value::Object(map) => {
                current = map.get(part)?;
            }
            _ => return None,
        }
    }
    Some(current)
}

fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(v) => *v,
        Value::Number(_) => true,
        Value::String(v) => !v.is_empty(),
        Value::Array(v) => !v.is_empty(),
        Value::Object(v) => !v.is_empty(),
    }
}

fn stringify(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(v) => v.to_string(),
        Value::Number(v) => v.to_string(),
        Value::String(v) => v.clone(),
        _ => serde_json::to_string(value).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::Renderer;
    use serde_json::json;

    #[test]
    fn render_sections() {
        let renderer = Renderer::new();
        let output = renderer
            .render(
                "<ul>{{#items}}<li>{{name}}</li>{{/items}}</ul>{{^empty}}<p>none</p>{{/empty}}",
                &json!({"items": [{"name": "a"}, {"name": "b"}]}),
            )
            .unwrap();
        assert!(output.contains("<li>a</li>"));
        assert!(output.contains("<p>none</p>"));
    }
}
