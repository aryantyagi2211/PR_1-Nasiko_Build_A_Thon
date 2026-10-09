use std::collections::HashSet;

use serde_json::Value;

use crate::schema::is_compactable;
use crate::types::{CompactTools, Error, ToolDef};

pub fn encode_tools(tools: &[ToolDef]) -> Result<CompactTools, Error> {
    if !is_compactable(tools) {
        return Err(Error::UnsupportedSchema);
    }

    let text = tools.iter().map(tool_line).collect::<Vec<_>>().join("\n");

    Ok(CompactTools {
        text,
        instructions: instructions(tools),
    })
}

fn instructions(tools: &[ToolDef]) -> String {
    let example = tools
        .first()
        .map(|tool| {
            let arguments = example_arguments(tool.parameters.as_ref());
            format!(
                "\n<<call {} {}>>\n",
                tool.name,
                serde_json::to_string(&arguments).unwrap_or_else(|_| "{}".to_string())
            )
        })
        .unwrap_or_else(|| "\n".to_string());

    format!(
        "Reply with ONLY calls in <<call NAME {{JSON}}>> format. Example:{example}Never claim an action is done without a call. ? = optional. If no tool fits, answer normally."
    )
}

fn example_arguments(parameters: Option<&Value>) -> Value {
    let Some(Value::Object(schema)) = parameters else {
        return Value::Object(serde_json::Map::new());
    };
    let Some(Value::Object(properties)) = schema.get("properties") else {
        return Value::Object(serde_json::Map::new());
    };
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();

    let mut arguments = serde_json::Map::new();
    for key in required {
        if let Some(property) = properties.get(key) {
            arguments.insert(key.to_string(), example_value(property));
        }
    }
    Value::Object(arguments)
}

fn example_value(schema: &Value) -> Value {
    let Value::Object(schema) = schema else {
        return Value::String("x".to_string());
    };
    if let Some(first) = schema.get("enum").and_then(Value::as_array).and_then(|e| e.first()) {
        return first.clone();
    }
    match schema.get("type").and_then(Value::as_str) {
        Some("string") => Value::String("x".to_string()),
        Some("integer") | Some("number") => Value::Number(1.into()),
        Some("boolean") => Value::Bool(true),
        Some("array") => Value::Array(vec![Value::String("x".to_string())]),
        Some("object") => {
            let Some(Value::Object(properties)) = schema.get("properties") else {
                return Value::Object(serde_json::Map::new());
            };
            let required: Vec<&str> = schema
                .get("required")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            let mut object = serde_json::Map::new();
            for key in required {
                if let Some(property) = properties.get(key) {
                    object.insert(key.to_string(), example_value(property));
                }
            }
            Value::Object(object)
        }
        _ => Value::Null,
    }
}

fn tool_line(tool: &ToolDef) -> String {
    let params = schema_params_text(tool.parameters.as_ref());
    let description = tool
        .description
        .as_deref()
        .map(short_tool_description)
        .unwrap_or_default();
    if description.is_empty() {
        format!("{}({})", tool.name, params)
    } else {
        format!("{}({}) - {}", tool.name, params, description)
    }
}

fn schema_params_text(schema: Option<&Value>) -> String {
    let Some(schema) = schema else {
        return String::new();
    };
    let Value::Object(obj) = schema else {
        return String::new();
    };
    let Some(properties) = obj.get("properties") else {
        return String::new();
    };
    let Value::Object(props) = properties else {
        return String::new();
    };
    let required_list: Vec<&str> = obj
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let required_set: HashSet<&str> = required_list.iter().copied().collect();

    let mut entries = Vec::new();

    // Required fields first in required array order to ensure roundtrip preservation
    for key in &required_list {
        if let Some(value) = props.get(*key) {
            let mut rendered = format!("{}:{}", key, schema_type(value));
            if let Some(desc) = short_description(value, key) {
                rendered.push_str(" /");
                rendered.push_str(&desc);
            }
            entries.push(rendered);
        }
    }

    // Optional fields in props order
    for (key, value) in props {
        if !required_set.contains(key.as_str()) {
            let mut rendered = format!("{}?:{}", key, schema_type(value));
            if let Some(desc) = short_description(value, key) {
                rendered.push_str(" /");
                rendered.push_str(&desc);
            }
            entries.push(rendered);
        }
    }

    entries.join(", ")
}

fn schema_type(value: &Value) -> String {
    match value {
        Value::Object(obj) => {
            if let Some(enum_values) = obj.get("enum") {
                let Value::Array(items) = enum_values else {
                    return "any".to_string();
                };
                let values = items
                    .iter()
                    .map(|v| match v {
                        Value::String(s) => s.clone(),
                        Value::Number(n) => n.to_string(),
                        Value::Bool(b) => b.to_string(),
                        _ => "any".to_string(),
                    })
                    .collect::<Vec<_>>();
                return values.join("|");
            }
            if let Some(items) = obj.get("items") {
                return format!("[{}]", schema_type(items));
            }
            if let Some(properties) = obj.get("properties") {
                let Value::Object(props) = properties else {
                    return "any".to_string();
                };
                let required: HashSet<_> = obj
                    .get("required")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                let mut entries = Vec::new();
                for (k, v) in props {
                    let is_required = required.contains(k.as_str());
                    if is_required {
                        entries.push(format!("{}:{}", k, schema_type(v)));
                    } else {
                        entries.push(format!("{}?:{}", k, schema_type(v)));
                    }
                }
                return format!("{{{}}}", entries.join(", "));
            }
            if matches!(obj.get("format"), Some(Value::String(s)) if s == "date-time") {
                return "datetime".to_string();
            }
            if let Some(kind) = obj.get("type") {
                match kind {
                    Value::String(s) => match s.as_str() {
                        "string" => return "str".to_string(),
                        "integer" => return "int".to_string(),
                        "number" => return "num".to_string(),
                        "boolean" => return "bool".to_string(),
                        "array" => return "[any]".to_string(),
                        "object" => return "any".to_string(),
                        _ => return "any".to_string(),
                    },
                    _ => return "any".to_string(),
                }
            }
            "any".to_string()
        }
        Value::String(s) => s.clone(),
        Value::Number(_) => "num".to_string(),
        Value::Bool(_) => "bool".to_string(),
        Value::Array(_) => "[any]".to_string(),
        Value::Null => "any".to_string(),
    }
}

fn short_description(value: &Value, key: &str) -> Option<String> {
    let Value::Object(obj) = value else {
        return None;
    };
    let desc = obj.get("description")?.as_str()?.trim();
    if desc.is_empty() || description_repeats_key_or_type(desc, key, value) {
        return None;
    }
    let mut out = desc.chars().take(60).collect::<String>();
    if out.len() >= 60 {
        out = out.trim_end().to_string();
    }
    Some(out)
}

fn short_tool_description(description: &str) -> String {
    description
        .split_whitespace()
        .filter(|word| {
            !matches!(
                word.trim_matches(|c: char| !c.is_alphanumeric())
                    .to_ascii_lowercase()
                    .as_str(),
                "a" | "an" | "the"
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn description_repeats_key_or_type(description: &str, key: &str, schema: &Value) -> bool {
    let description_words = description_words(description);
    let mut key_words = identifier_words(key);
    match key {
        "to" => key_words = vec!["recipient".to_string()],
        "attendees" => key_words = vec!["attendee".to_string()],
        _ => {}
    }

    let mut key_description_words = description_words
        .iter()
        .filter(|word| {
            !matches!(
                word.as_str(),
                "a" | "an" | "the" | "of" | "in" | "event" | "line" | "plain" | "text"
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    if matches!(key, "to" | "attendees" | "cc" | "bcc") {
        key_description_words.retain(|word| word != "email" && word != "emails");
    }
    if key_description_words == key_words {
        return true;
    }

    let type_words = description_words
        .iter()
        .filter(|word| !matches!(word.as_str(), "a" | "an" | "the"))
        .cloned()
        .collect::<Vec<_>>();
    type_description_words(schema).is_some_and(|types| types.contains(&type_words))
}

fn description_words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

fn identifier_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            if c.is_ascii_uppercase() && !word.is_empty() {
                words.push(normalize_word(&word));
                word.clear();
            }
            word.push(c.to_ascii_lowercase());
        } else if !word.is_empty() {
            words.push(normalize_word(&word));
            word.clear();
        }
    }
    if !word.is_empty() {
        words.push(normalize_word(&word));
    }
    words
}

fn normalize_word(word: &str) -> String {
    if let Some(stem) = word.strip_suffix("ies") {
        format!("{stem}y")
    } else if word.ends_with('s') && !word.ends_with("ss") {
        word[..word.len() - 1].to_string()
    } else {
        word.to_string()
    }
}

fn type_description_words(schema: &Value) -> Option<Vec<Vec<String>>> {
    let Value::Object(obj) = schema else {
        return None;
    };
    if matches!(obj.get("format"), Some(Value::String(format)) if format == "date-time") {
        return None;
    }
    let words = match obj.get("type").and_then(Value::as_str)? {
        "string" => vec![vec!["string".to_string()], vec!["str".to_string()]],
        "integer" => vec![vec!["integer".to_string()], vec!["int".to_string()]],
        "number" => vec![vec!["number".to_string()], vec!["num".to_string()]],
        "boolean" => vec![vec!["boolean".to_string()], vec!["bool".to_string()]],
        "array" => vec![vec!["array".to_string()]],
        "object" => vec![vec!["object".to_string()]],
        _ => return None,
    };
    Some(words)
}
