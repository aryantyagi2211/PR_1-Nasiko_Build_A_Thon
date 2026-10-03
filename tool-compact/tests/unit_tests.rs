use proptest::prelude::*;
use serde_json::{json, Value};

use nasiko_tool_compact::{decode_calls, decode_tools, decode_with_text, encode_tools, Event, StreamDecoder, ToolDef};

fn calendar_tool() -> ToolDef {
    ToolDef {
        name: "create_calendar_event".to_string(),
        description: Some("Create an event in the user's calendar.".to_string()),
        parameters: Some(json!({
            "type": "object",
            "properties": {
                "title": { "type": "string", "description": "Event title" },
                "start": { "type": "string", "format": "date-time", "description": "Start time, ISO 8601" },
                "duration_min": { "type": "integer", "description": "Duration in minutes" },
                "attendees": { "type": "array", "items": { "type": "string" }, "description": "Attendee emails" },
                "visibility": { "type": "string", "enum": ["public", "private"] }
            },
            "required": ["title", "start"]
        })),
    }
}

fn email_tool() -> ToolDef {
    ToolDef {
        name: "send_email".to_string(),
        description: Some("Send an email from the user's account.".to_string()),
        parameters: Some(json!({
            "type": "object",
            "properties": {
                "to": { "type": "array", "items": { "type": "string" } },
                "subject": { "type": "string" },
                "body": { "type": "string" },
                "cc": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["to", "subject", "body"]
        })),
    }
}

#[test]
fn multiple_calls() {
    let tools = vec![calendar_tool()];
    let text = "before <<call create_calendar_event {\"title\":\"Design review\",\"start\":\"2026-10-05T15:00:00+05:30\"}>> middle <<call create_calendar_event {\"title\":\"Retro\",\"start\":\"2026-10-04T10:00:00+05:30\"}>> after";
    let result = decode_calls(text, &tools).unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].name, "create_calendar_event");
    assert_eq!(result[1].name, "create_calendar_event");
}

#[test]
fn text_before_and_after_call() {
    let tools = vec![calendar_tool()];
    let text = "Hi <<call create_calendar_event {\"title\":\"Design review\",\"start\":\"2026-10-05T15:00:00+05:30\"}>> bye";
    let decoded = decode_with_text(text, &tools).unwrap();
    assert_eq!(decoded.text, "Hi  bye");
    assert_eq!(decoded.calls.len(), 1);
}

#[test]
fn plain_answer_with_no_call() {
    let tools = vec![calendar_tool()];
    let decoded = decode_with_text("I am fine.", &tools).unwrap();
    assert!(decoded.calls.is_empty());
    assert_eq!(decoded.text, "I am fine.");
}

#[test]
fn escaped_quotes_and_unicode_in_strings() {
    let tools = vec![calendar_tool()];
    let text = "<<call create_calendar_event {\"title\":\"He said \\\"hi\\\" and café\",\"start\":\"2026-10-05T15:00:00+05:30\"}>>";
    let decoded = decode_calls(text, &tools).unwrap();
    let args: Value = serde_json::from_str(&decoded[0].arguments).unwrap();
    assert_eq!(args["title"], "He said \"hi\" and café");
}

#[test]
fn int_vs_num() {
    let tools = vec![calendar_tool()];
    let err = decode_calls("<<call create_calendar_event {\"title\":\"X\",\"start\":\"2026-10-05T15:00:00+05:30\",\"duration_min\":3.5}>>", &tools).unwrap_err();
    assert_eq!(err.code(), "invalid_arguments");
}

#[test]
fn missing_required_field() {
    let tools = vec![calendar_tool()];
    let err = decode_calls("<<call create_calendar_event {\"start\":\"2026-10-05T15:00:00+05:30\"}>>", &tools).unwrap_err();
    assert_eq!(err.code(), "invalid_arguments");
}

#[test]
fn bad_enum_value() {
    let tools = vec![calendar_tool()];
    let err = decode_calls("<<call create_calendar_event {\"title\":\"X\",\"start\":\"2026-10-05T15:00:00+05:30\",\"visibility\":\"secret\"}>>", &tools).unwrap_err();
    assert_eq!(err.code(), "invalid_arguments");
}

#[test]
fn unknown_key() {
    let tools = vec![calendar_tool()];
    let err = decode_calls("<<call create_calendar_event {\"title\":\"X\",\"start\":\"2026-10-05T15:00:00+05:30\",\"bogus\":1}>>", &tools).unwrap_err();
    assert_eq!(err.code(), "invalid_arguments");
}

#[test]
fn unsupported_schema_is_rejected() {
    let bad = ToolDef {
        name: "bad".to_string(),
        description: None,
        parameters: Some(json!({ "$ref": "#/defs/x" })),
    };
    let err = encode_tools(&[bad]).unwrap_err();
    assert_eq!(err.code(), "unsupported_schema");
}

#[test]
fn encode_decode_tools_roundtrip_keeps_schema_shape() {
    let tool = calendar_tool();
    let encoded = encode_tools(&[tool.clone()]).unwrap();
    let decoded = decode_tools(&encoded).unwrap();
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].name, tool.name);
    assert_eq!(decoded[0].description, tool.description);
    assert_eq!(decoded[0].parameters, tool.parameters);
}

#[test]
fn encode_is_deterministic() {
    let tool = calendar_tool();
    let a = encode_tools(&[tool.clone()]).unwrap();
    let b = encode_tools(&[tool]).unwrap();
    assert_eq!(a.text, b.text);
    assert_eq!(a.instructions, b.instructions);
}

#[test]
fn malformed_input() {
    let tools = vec![calendar_tool()];
    let cases = [
        "<<call create_calendar_event {\"title\":\"X\",\"start\":\"2026-10-05T15:00:00+05:30\"}",
        "<<call create_calendar_event {not-json}>>",
        "<<call create_calendar_event {\"title\":\"X\"}>> junk",
    ];
    for text in cases {
        let err = decode_calls(text, &tools).unwrap_err();
        assert_eq!(err.code(), "malformed_call");
    }
}

#[test]
fn stream_decoder_matches_decode_with_text_for_random_splits() {
    let tools = vec![calendar_tool()];
    let text = "before <<call create_calendar_event {\"title\":\"Design review\",\"start\":\"2026-10-05T15:00:00+05:30\"}>> after";
    let expected = decode_with_text(text, &tools).unwrap();
    let mut decoder = StreamDecoder::new(&tools);
    let mut events = Vec::new();

    for chunk in [
        "before ",
        "<<call create_calendar_event {\"title\":\"Design review\",\"start\":\"2026-10-05T15:00:00+05:30\"}",
        ">> after",
    ] {
        events.extend(decoder.push(chunk).unwrap());
    }
    events.extend(decoder.finish().unwrap());

    assert_eq!(events.len(), 3);
    assert!(matches!(events[0], Event::Text(ref s) if s == "before "));
    assert!(matches!(events[1], Event::Call(_)));
    assert!(matches!(events[2], Event::Text(ref s) if s == " after"));
    assert_eq!(expected.text, "before  after");
}
