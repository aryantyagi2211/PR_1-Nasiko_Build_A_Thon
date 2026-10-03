use serde_json::{Map, Value};

use crate::types::{CompactTools, Decoded, Error, ToolCall, ToolDef};
use crate::validate::validate_call;

pub fn decode_tools(compact: &CompactTools) -> Result<Vec<ToolDef>, Error> {
    let mut out = Vec::new();
    for line in compact.text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        out.push(parse_tool_line(trimmed)?);
    }
    Ok(out)
}

pub fn decode_calls(text: &str, tools: &[ToolDef]) -> Result<Vec<ToolCall>, Error> {
    match decode_with_text(text, tools) {
        Ok(decoded) => Ok(decoded.calls),
        Err(Error::InvalidArguments) => {
            // If the call was followed by non-whitespace junk, it's a malformed call
            // rather than a schema validation failure
            if let Ok(after_call) = find_junk_after_call(text) {
                if !after_call.trim().is_empty() {
                    return Err(Error::MalformedCall);
                }
            }
            Err(Error::InvalidArguments)
        }
        Err(e) => Err(e),
    }
}

fn find_junk_after_call(text: &str) -> Result<String, Error> {
    if let Some(idx) = text.find(">>") {
        Ok(text[idx + 2..].to_string())
    } else {
        Ok(String::new())
    }
}

pub fn decode_with_text(text: &str, tools: &[ToolDef]) -> Result<Decoded, Error> {
    let mut calls = Vec::new();
    let mut out = String::new();
    let mut idx = 0usize;

    while idx < text.len() {
        let Some(marker_start) = text[idx..].find("<<") else {
            out.push_str(&text[idx..]);
            break;
        };
        let absolute = idx + marker_start;
        if absolute > idx {
            out.push_str(&text[idx..absolute]);
        }
        let rest = &text[absolute..];

        match parse_call_candidate(rest, tools)? {
            Some((call, consumed)) => {
                calls.push(call);
                idx = absolute + consumed;
            }
            None => {
                if is_probable_call_prefix(rest) {
                    return Err(Error::MalformedCall);
                }
                out.push_str("<<");
                idx = absolute + 2;
            }
        }
    }

    Ok(Decoded {
        text: out,
        calls,
    })
}

fn parse_tool_line(line: &str) -> Result<ToolDef, Error> {
    let (header, description) = match line.split_once(" - ") {
        Some((h, d)) => (h, Some(d.trim().to_string())),
        None => (line, None),
    };
    let Some((name, params)) = header.split_once('(') else {
        return Err(Error::MalformedCall);
    };
    let params = params.strip_suffix(')').ok_or(Error::MalformedCall)?;
    let mut schema = Map::new();
    schema.insert("type".to_string(), Value::String("object".to_string()));
    let (properties, required) = if params.trim().is_empty() {
        (Map::new(), Vec::new())
    } else {
        parse_param_list(params.trim())?
    };
    schema.insert("properties".to_string(), Value::Object(properties));
    if !required.is_empty() {
        schema.insert(
            "required".to_string(),
            Value::Array(required.into_iter().map(Value::String).collect()),
        );
    }

    Ok(ToolDef {
        name: name.trim().to_string(),
        description,
        parameters: Some(Value::Object(schema)),
    })
}

fn parse_enum_literal(raw: &str) -> Result<(Value, usize), Error> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let chars: Vec<_> = raw.chars().collect();
    let mut idx = 0usize;

    while idx < chars.len() {
        let ch = chars[idx];
        if ch.is_whitespace() {
            break;
        }
        if ch == '|' {
            if current.trim().is_empty() {
                return Err(Error::MalformedCall);
            }
            parts.push(current.trim().to_string());
            current.clear();
            idx += 1;
            continue;
        }
        current.push(ch);
        idx += 1;
    }

    if current.trim().is_empty() {
        return Err(Error::MalformedCall);
    }
    parts.push(current.trim().to_string());

    let values = parts.iter().map(|p| Value::String(p.clone())).collect::<Vec<_>>();
    Ok((serde_json::json!({ "type": "string", "enum": values }), idx))
}

fn parse_param_list(raw: &str) -> Result<(Map<String, Value>, Vec<String>), Error> {
    let items = split_top_level(raw, ',')?;
    let mut properties = Map::new();
    let mut required = Vec::new();

    for item in items {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let Some((key_part, rest)) = item.split_once(':') else {
            return Err(Error::MalformedCall);
        };
        let key_part = key_part.trim();
        let is_required = !key_part.ends_with('?');
        let key = key_part.trim_end_matches('?').trim();
        let rest = rest.trim();
        let (schema, used) = parse_type(rest)?;
        if is_required {
            required.push(key.to_string());
        }
        let mut obj = schema.as_object().cloned().ok_or(Error::MalformedCall)?;
        if used < rest.len() {
            let tail = &rest[used..];
            if let Some(desc) = tail.strip_prefix(" /") {
                let desc = desc.trim();
                if !desc.is_empty() {
                    obj.insert("description".to_string(), Value::String(desc.to_string()));
                }
            }
        }
        properties.insert(key.to_string(), Value::Object(obj));
    }

    Ok((properties, required))
}

fn parse_type(s: &str) -> Result<(Value, usize), Error> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err(Error::MalformedCall);
    }

    if trimmed.starts_with('[') {
        let end = find_matching(trimmed, '[', ']')?;
        let inner = &trimmed[1..end];
        let (inner_schema, _) = parse_type(inner)?;
        return Ok((
            serde_json::json!({ "type": "array", "items": inner_schema }),
            end + 1,
        ));
    }

    if trimmed.starts_with('{') {
        let end = find_matching(trimmed, '{', '}')?;
        let inner = &trimmed[1..end];
        let (props, reqs) = parse_param_list(inner)?;
        let mut obj = Map::new();
        obj.insert("type".to_string(), Value::String("object".to_string()));
        obj.insert("properties".to_string(), Value::Object(props));
        if !reqs.is_empty() {
            obj.insert(
                "required".to_string(),
                Value::Array(reqs.into_iter().map(Value::String).collect()),
            );
        }
        return Ok((Value::Object(obj), end + 1));
    }

    if trimmed.contains('|') {
        let literal = parse_enum_literal(trimmed)?;
        return Ok(literal);
    }

    let mut token_end = trimmed.len();
    for (idx, ch) in trimmed.char_indices() {
        if ch == ' ' {
            token_end = idx;
            break;
        }
    }
    let token = &trimmed[..token_end];
    let value = match token {
        "str" => serde_json::json!({ "type": "string" }),
        "int" => serde_json::json!({ "type": "integer" }),
        "num" => serde_json::json!({ "type": "number" }),
        "bool" => serde_json::json!({ "type": "boolean" }),
        "datetime" => serde_json::json!({ "type": "string", "format": "date-time" }),
        "any" => serde_json::json!({ "type": "string" }),
        _ => return Err(Error::MalformedCall),
    };
    Ok((value, token.len()))
}

fn split_top_level(s: &str, sep: char) -> Result<Vec<String>, Error> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut depth_brace = 0usize;
    let mut depth_bracket = 0usize;
    let mut depth_paren = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    let chars: Vec<_> = s.chars().collect();
    for (idx, ch) in chars.iter().enumerate() {
        if in_string {
            current.push(*ch);
            if escaped {
                escaped = false;
            } else if *ch == '\\' {
                escaped = true;
            } else if *ch == '"' {
                in_string = false;
            }
            continue;
        }

        match *ch {
            '"' => {
                in_string = true;
                current.push(*ch);
            }
            '{' => {
                depth_brace += 1;
                current.push(*ch);
            }
            '}' => {
                if depth_brace == 0 {
                    return Err(Error::MalformedCall);
                }
                depth_brace -= 1;
                current.push(*ch);
            }
            '[' => {
                depth_bracket += 1;
                current.push(*ch);
            }
            ']' => {
                if depth_bracket == 0 {
                    return Err(Error::MalformedCall);
                }
                depth_bracket -= 1;
                current.push(*ch);
            }
            '(' => {
                depth_paren += 1;
                current.push(*ch);
            }
            ')' => {
                if depth_paren == 0 {
                    return Err(Error::MalformedCall);
                }
                depth_paren -= 1;
                current.push(*ch);
            }
            c if c == sep && depth_brace == 0 && depth_bracket == 0 && depth_paren == 0 => {
                let rest: String = chars[idx + 1..].iter().collect();
                if sep == ',' && !rest.trim().is_empty() && !looks_like_param(&rest) {
                    current.push(c);
                } else {
                    let item = current.trim();
                    if !item.is_empty() {
                        out.push(item.to_string());
                    }
                    current.clear();
                }
            }
            _ => current.push(*ch),
        }
    }

    if in_string || depth_brace != 0 || depth_bracket != 0 || depth_paren != 0 {
        return Err(Error::MalformedCall);
    }

    let item = current.trim();
    if !item.is_empty() {
        out.push(item.to_string());
    }
    Ok(out)
}

fn looks_like_param(s: &str) -> bool {
    let trimmed = s.trim_start();
    let Some((key, _)) = trimmed.split_once(':') else {
        return false;
    };
    let key = key.trim().trim_end_matches('?').trim();
    !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

fn find_matching(s: &str, open: char, close: char) -> Result<usize, Error> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (byte_idx, ch) in s.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            c if c == open => {
                depth += 1;
            }
            c if c == close => {
                if depth == 0 {
                    return Err(Error::MalformedCall);
                }
                depth -= 1;
                if depth == 0 {
                    return Ok(byte_idx);
                }
            }
            _ => {}
        }
    }

    Err(Error::MalformedCall)
}

pub(crate) fn parse_call(rest: &str, tools: &[ToolDef]) -> Result<(ToolCall, usize), Error> {
    let after = rest.strip_prefix("<<call").ok_or(Error::MalformedCall)?;
    let after = after.trim_start();
    let Some((name, after)) = after.split_once(char::is_whitespace) else {
        return Err(Error::MalformedCall);
    };
    let name = name.trim();
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(Error::MalformedCall);
    }

    let after = after.trim_start();
    if !after.starts_with('{') {
        return Err(Error::MalformedCall);
    }

    let json_end = find_json_end(after)?;
    let raw = &after[..json_end];
    let mut marker_index = json_end;
    while marker_index < after.len() && after.as_bytes()[marker_index].is_ascii_whitespace() {
        marker_index += 1;
    }
    if !after[marker_index..].starts_with(">>") {
        return Err(Error::MalformedCall);
    }

    let args: Value = serde_json::from_str(raw).map_err(|_| Error::MalformedCall)?;
    let obj = args.as_object().ok_or(Error::MalformedCall)?;
    let call = validate_call(name, obj, tools)?;
    let consumed = rest.len() - after[marker_index + 2..].len();
    Ok((call, consumed))
}

fn parse_call_candidate(rest: &str, tools: &[ToolDef]) -> Result<Option<(ToolCall, usize)>, Error> {
    if !rest.starts_with("<<") {
        return Ok(None);
    }
    if !rest.starts_with("<<call") {
        return Ok(None);
    }

    let (call, consumed) = parse_call(rest, tools)?;
    Ok(Some((call, consumed)))
}

fn is_probable_call_prefix(s: &str) -> bool {
    let trimmed = s.trim_start();
    trimmed.starts_with("<<") && !trimmed.starts_with("<<call")
}


fn find_json_end(s: &str) -> Result<usize, Error> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (byte_idx, ch) in s.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                if depth == 0 {
                    return Err(Error::MalformedCall);
                }
                depth -= 1;
                if depth == 0 {
                    let end_byte = byte_idx + ch.len_utf8();
                    let tail = &s[end_byte..];
                    let trimmed = tail.trim_start();
                    if trimmed.starts_with(">>") || trimmed.is_empty() {
                        return Ok(end_byte);
                    }
                    return Err(Error::MalformedCall);
                }
            }
            _ => {}
        }
    }

    Err(Error::MalformedCall)
}

