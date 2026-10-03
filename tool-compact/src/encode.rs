use std::collections::HashSet;

use serde_json::Value;

use crate::schema::is_compactable;
use crate::types::{CompactTools, Error, ToolDef};

const INSTRUCTIONS: &str = "Call a tool by emitting exactly\n<<call NAME {JSON_ARGS}>>\non its own line, JSON args must be valid JSON with double-quoted keys, omit optional args it doesn't know, never invent tools, multiple calls allowed, plain answer with no marker when no tool is needed.";

pub fn encode_tools(tools: &[ToolDef]) -> Result<CompactTools, Error> {
    if !is_compactable(tools) {
        return Err(Error::UnsupportedSchema);
    }

    let text = tools
        .iter()
        .map(tool_line)
        .collect::<Vec<_>>()
        .join("\n");

    Ok(CompactTools {
        text,
        instructions: INSTRUCTIONS.to_string(),
    })
}

fn tool_line(tool: &ToolDef) -> String {
    let params = schema_params_text(tool.parameters.as_ref());
    let description = tool.description.as_deref().unwrap_or("");
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
    if desc.is_empty() || desc.eq_ignore_ascii_case(key) {
        return None;
    }
    let mut out = desc.chars().take(60).collect::<String>();
    if out.len() >= 60 {
        out = out.trim_end().to_string();
    }
    Some(out)
}
