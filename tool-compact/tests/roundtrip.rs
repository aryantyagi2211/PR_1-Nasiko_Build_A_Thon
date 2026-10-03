use nasiko_tool_compact::decode_calls;
use serde_json::Value;

#[test]
fn roundtrip_cases_decode_calls() {
    let cases = [
        (
            "ct-001",
            vec![
                ("create_calendar_event", r#"{"title":"Design review","start":"2026-10-05T15:00:00+05:30","attendees":["riya@example.com"]}"#),
            ],
        ),
        (
            "ct-002",
            vec![
                ("send_email", r#"{"to":["sam@example.com"],"subject":"Build status","body":"The build is green."}"#),
                ("create_calendar_event", r#"{"title":"Retro","start":"2026-10-04T10:00:00+05:30","duration_min":30,"visibility":"private"}"#),
            ],
        ),
        (
            "ct-003",
            vec![],
        ),
    ];

    for (id, expected_calls) in cases {
        let text = if expected_calls.is_empty() {
            "What is the weather?".to_string()
        } else {
            expected_calls
                .iter()
                .map(|(name, json)| format!("<<call {name} {json}>>"))
                .collect::<Vec<_>>()
                .join("\n")
        };

        let tools = [
            nasiko_tool_compact::ToolDef {
                name: "create_calendar_event".to_string(),
                description: Some("Create an event in the user's calendar.".to_string()),
                parameters: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "start": { "type": "string", "format": "date-time" },
                        "duration_min": { "type": "integer" },
                        "attendees": { "type": "array", "items": {"type": "string"} },
                        "visibility": { "type": "string", "enum": ["public", "private"] }
                    },
                    "required": ["title", "start"]
                })),
            },
            nasiko_tool_compact::ToolDef {
                name: "send_email".to_string(),
                description: Some("Send an email from the user's account.".to_string()),
                parameters: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "to": { "type": "array", "items": {"type": "string"} },
                        "subject": { "type": "string" },
                        "body": { "type": "string" },
                        "cc": { "type": "array", "items": {"type": "string"} }
                    },
                    "required": ["to", "subject", "body"]
                })),
            },
        ];

        let decoded = decode_calls(&text, &tools).unwrap();
        assert_eq!(decoded.len(), expected_calls.len(), "case {id} failed");
        for (idx, call) in decoded.iter().enumerate() {
            let expected = expected_calls[idx].clone();
            assert_eq!(call.name, expected.0);
            let actual: Value = serde_json::from_str(&call.arguments).unwrap();
            let expected_json: Value = serde_json::from_str(expected.1).unwrap();
            assert_eq!(actual, expected_json);
        }
    }
}
