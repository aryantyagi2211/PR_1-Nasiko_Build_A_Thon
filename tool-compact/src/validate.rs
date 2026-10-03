use serde_json::{Map, Value};

use crate::types::{Error, ToolCall, ToolDef};

pub fn validate_call(name: &str, args: &Map<String, Value>, tools: &[ToolDef]) -> Result<ToolCall, Error> {
    let Some(tool) = tools.iter().find(|t| t.name == name) else {
        return Err(Error::UnknownTool);
    };
    let schema = tool.parameters.clone().unwrap_or(Value::Object(Map::new()));
    validate_against_schema(&Value::Object(args.clone()), &schema, true)?;
    let arguments = serde_json::to_string(&Value::Object(args.clone())).map_err(|_| Error::MalformedCall)?;
    Ok(ToolCall {
        name: name.to_string(),
        arguments,
    })
}

pub(crate) fn validate_against_schema(value: &Value, schema: &Value, _root: bool) -> Result<(), Error> {
    let Some(schema_obj) = schema.as_object() else {
        return Err(Error::UnsupportedSchema);
    };

    if let Some(enum_values) = schema_obj.get("enum") {
        let Value::Array(items) = enum_values else { return Err(Error::UnsupportedSchema); };
        if !items.iter().any(|candidate| candidate == value) {
            return Err(Error::InvalidArguments);
        }
    }

    if matches!(schema_obj.get("format"), Some(Value::String(s)) if s == "date-time") {
        if !matches!(value, Value::String(s) if is_rfc3339(s)) {
            return Err(Error::InvalidArguments);
        }
    }

    match schema_obj.get("type") {
        Some(Value::String(kind)) => match kind.as_str() {
            "string" => {
                if !matches!(value, Value::String(_)) {
                    return Err(Error::InvalidArguments);
                }
                Ok(())
            }
            "integer" => {
                if !matches!(value, Value::Number(n) if n.is_i64() || n.is_u64()) {
                    return Err(Error::InvalidArguments);
                }
                Ok(())
            }
            "number" => {
                if !matches!(value, Value::Number(_)) {
                    return Err(Error::InvalidArguments);
                }
                Ok(())
            }
            "boolean" => {
                if !matches!(value, Value::Bool(_)) {
                    return Err(Error::InvalidArguments);
                }
                Ok(())
            }
            "array" => {
                let Value::Array(items) = value else { return Err(Error::InvalidArguments); };
                if let Some(item_schema) = schema_obj.get("items") {
                    for item in items {
                        validate_against_schema(item, item_schema, false)?;
                    }
                }
                Ok(())
            }
            "object" => {
                let Value::Object(obj) = value else { return Err(Error::InvalidArguments); };
                if let Some(required) = schema_obj.get("required") {
                    let Value::Array(reqs) = required else { return Err(Error::UnsupportedSchema); };
                    for field in reqs {
                        let Value::String(name) = field else { return Err(Error::UnsupportedSchema); };
                        if !obj.contains_key(name) {
                            return Err(Error::InvalidArguments);
                        }
                    }
                }

                let properties = match schema_obj.get("properties") {
                    Some(Value::Object(props)) => props,
                    Some(_) => return Err(Error::UnsupportedSchema),
                    None => &Map::new(),
                };
                for (key, val) in obj {
                    let Some(prop_schema) = properties.get(key) else {
                        return Err(Error::InvalidArguments);
                    };
                    validate_against_schema(val, prop_schema, false)?;
                }
                Ok(())
            }
            _ => Err(Error::UnsupportedSchema),
        },
        None => {
            if schema_obj.contains_key("properties") {
                let properties = match schema_obj.get("properties") {
                    Some(Value::Object(props)) => props,
                    Some(_) => return Err(Error::UnsupportedSchema),
                    None => &Map::new(),
                };
                let Value::Object(obj) = value else { return Err(Error::InvalidArguments); };
                for (key, val) in obj {
                    let Some(prop_schema) = properties.get(key) else {
                        return Err(Error::InvalidArguments);
                    };
                    validate_against_schema(val, prop_schema, false)?;
                }
                Ok(())
            } else {
                Ok(())
            }
        }
        Some(_) => Err(Error::UnsupportedSchema),
    }
}

fn is_rfc3339(value: &str) -> bool {
    let Some((date, time)) = value.split_once('T') else { return false; };
    let mut tz = time;
    if tz.ends_with('Z') || tz.ends_with('z') {
        tz = &tz[..tz.len() - 1];
    } else if let Some((base, offset)) = tz.rsplit_once('+') {
        if !offset.contains(':') || offset.len() != 5 {
            return false;
        }
        tz = base;
    } else if let Some((base, offset)) = tz.rsplit_once('-') {
        if time.matches('-').count() != 1 || !offset.contains(':') || offset.len() != 5 {
            return false;
        }
        tz = base;
    }
    let Some((hour, minute_sec)) = tz.split_once(':') else { return false; };
    let Some((minute, second)) = minute_sec.split_once(':') else { return false; };
    let Ok(hour) = hour.parse::<u32>() else { return false; };
    let Ok(minute) = minute.parse::<u32>() else { return false; };
    let Ok(second) = second.parse::<u32>() else { return false; };
    if !(0..=23).contains(&hour) || !(0..=59).contains(&minute) || !(0..=60).contains(&second) {
        return false;
    }
    let date_parts: Vec<_> = date.split('-').collect();
    if date_parts.len() != 3 { return false; }
    let Ok(year) = date_parts[0].parse::<u32>() else { return false; };
    let Ok(month) = date_parts[1].parse::<u32>() else { return false; };
    let Ok(day) = date_parts[2].parse::<u32>() else { return false; };
    if !(1..=12).contains(&month) { return false; }
    let max_days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 { 29 } else { 28 }
        }
        _ => return false,
    };
    (1..=max_days).contains(&day)
}
