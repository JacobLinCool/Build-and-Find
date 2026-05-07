use serde_json::Value;

pub struct Renderer;

impl Renderer {
    pub fn new() -> Self {
        Renderer
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

pub fn render(template: &str, context: &Value) -> String {
    let mut output = String::new();
    let chars: Vec<char> = template.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        if i + 1 < len && chars[i] == '{' && chars[i + 1] == '{' {
            // Find the closing }}
            if let Some(end) = find_closing(&chars, i + 2) {
                let tag: String = chars[i + 2..end].iter().collect();
                let tag = tag.trim();

                if tag.starts_with('#') {
                    // Section tag: {{#name}}...{{/name}}
                    let name = tag[1..].trim();
                    let close_tag = format!("{{{{/{}}}}}", name);
                    if let Some(close_pos) = template[end + 2..].find(&close_tag) {
                        let section_content = &template[end + 2..end + 2 + close_pos];
                        let value = get_value(context, name);
                        match value {
                            Value::Bool(true) | Value::Number(_) => {
                                output.push_str(&render(section_content, context));
                            }
                            Value::Array(arr) => {
                                for item in arr {
                                    let mut item_ctx = context.clone();
                                    if let Value::Object(map) = &mut item_ctx {
                                        map.insert(".".into(), item.clone());
                                        if let Value::Object(ref item_map) = item {
                                            for (k, v) in item_map {
                                                map.insert(k.clone(), v.clone());
                                            }
                                        }
                                        if let Value::String(ref s) = item {
                                            map.insert(name.to_string(), Value::String(s.clone()));
                                        }
                                    }
                                    output.push_str(&render(section_content, &item_ctx));
                                }
                            }
                            Value::Null | Value::Bool(false) => {}
                            _ => {}
                        }
                        i = end + 2 + close_pos + close_tag.len();
                        continue;
                    }
                } else if tag.starts_with('^') {
                    // Inverted section: {{^name}}...{{/name}}
                    let name = tag[1..].trim();
                    let close_tag = format!("{{{{/{}}}}}", name);
                    if let Some(close_pos) = template[end + 2..].find(&close_tag) {
                        let section_content = &template[end + 2..end + 2 + close_pos];
                        let value = get_value(context, name);
                        match value {
                            Value::Null | Value::Bool(false) => {
                                output.push_str(&render(section_content, context));
                            }
                            _ => {}
                        }
                        i = end + 2 + close_pos + close_tag.len();
                        continue;
                    }
                } else {
                    // Variable tag: {{name}}
                    let value = get_value(context, tag);
                    match value {
                        Value::String(s) => output.push_str(&s),
                        Value::Number(n) => output.push_str(&n.to_string()),
                        Value::Bool(b) => output.push_str(&b.to_string()),
                        Value::Null => {}
                        Value::Array(_) => output.push_str("[array]"),
                        Value::Object(_) => output.push_str("[object]"),
                    }
                    i = end + 2;
                    continue;
                }
            }
        }

        output.push(chars[i]);
        i += 1;
    }

    output
}

fn find_closing(chars: &[char], start: usize) -> Option<usize> {
    for i in start..chars.len() - 1 {
        if chars[i] == '}' && chars[i + 1] == '}' {
            return Some(i);
        }
    }
    None
}

fn get_value(context: &Value, name: &str) -> Value {
    if name == "." {
        if let Some(val) = context.get(".") {
            return val.clone();
        }
    }
    if let Some(val) = context.get(name) {
        return val.clone();
    }
    // Support dotted paths like "user.name"
    let parts: Vec<&str> = name.split('.').collect();
    let mut current = context;
    for part in &parts {
        match current.get(*part) {
            Some(val) => current = val,
            None => return Value::Null,
        }
    }
    current.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn simple_variable() {
        let template = "Hello, {{name}}!";
        let context = json!({"name": "World"});
        assert_eq!(render(template, &context), "Hello, World!");
    }

    #[test]
    fn section_array() {
        let template = "{{#items}}{{.}} {{/items}}";
        let context = json!({"items": ["a", "b", "c"]});
        assert_eq!(render(template, &context), "a b c ");
    }

    #[test]
    fn inverted_section() {
        let template = "{{^empty}}Not empty{{/empty}}";
        let context = json!({"empty": false});
        assert_eq!(render(template, &context), "Not empty");
    }

    #[test]
    fn missing_variable() {
        let template = "Hello, {{name}}!";
        let context = json!({});
        assert_eq!(render(template, &context), "Hello, !");
    }

    #[test]
    fn section_object_array() {
        let template = "{{#users}}Name: {{name}} {{/users}}";
        let context = json!({"users": [{"name": "Alice"}, {"name": "Bob"}]});
        assert_eq!(render(template, &context), "Name: Alice Name: Bob ");
    }
}
