use std::collections::HashSet;

use serde_json::Value;

use crate::types::ToolDef;

pub fn is_compactable(tools: &[ToolDef]) -> bool {
    tools.iter().all(|tool| match &tool.parameters {
        None => true,
        Some(value) => schema_is_supported(value),
    })
}

pub(crate) fn schema_is_supported(value: &Value) -> bool {
    match value {
        Value::Object(obj) => {
            if obj.contains_key("$ref")
                || obj.contains_key("oneOf")
                || obj.contains_key("anyOf")
                || obj.contains_key("allOf")
                || obj.contains_key("not")
                || obj.contains_key("patternProperties")
            {
                return false;
            }

            let allowed = [
                "type",
                "properties",
                "required",
                "items",
                "enum",
                "description",
                "format",
                "additionalProperties",
            ];
            for key in obj.keys() {
                if !allowed.iter().any(|allowed_key| *allowed_key == key.as_str()) {
                    return false;
                }
            }

            if let Some(typ) = obj.get("type") {
                let Value::String(kind) = typ else { return false; };
                if !matches!(
                    kind.as_str(),
                    "object" | "string" | "integer" | "number" | "boolean" | "array"
                ) {
                    return false;
                }
            }

            if let Some(additional) = obj.get("additionalProperties") {
                if !matches!(additional, Value::Bool(false) | Value::Null) {
                    return false;
                }
            }

            if let Some(format) = obj.get("format") {
                if !matches!(format, Value::String(s) if s == "date-time") {
                    return false;
                }
            }

            if let Some(enum_values) = obj.get("enum") {
                let Value::Array(values) = enum_values else { return false; };
                if !values.is_empty() {
                    let kinds: HashSet<_> = values.iter().map(kind_of).collect();
                    if kinds.len() > 1 {
                        return false;
                    }
                    if matches!(kinds.iter().next(), Some(kind) if *kind == "array" || *kind == "object" || *kind == "null") {
                        return false;
                    }
                }
            }

            if let Some(properties) = obj.get("properties") {
                let Value::Object(props) = properties else { return false; };
                for child in props.values() {
                    if !schema_is_supported(child) {
                        return false;
                    }
                }
            }

            if let Some(items) = obj.get("items") {
                if !schema_is_supported(items) {
                    return false;
                }
            }

            if let Some(required) = obj.get("required") {
                let Value::Array(items) = required else { return false; };
                if !items.iter().all(Value::is_string) {
                    return false;
                }
            }

            true
        }
        _ => false,
    }
}

fn kind_of(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}
