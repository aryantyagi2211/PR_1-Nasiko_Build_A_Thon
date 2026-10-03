use std::fs;

use nasiko_tool_compact::{decode_calls, decode_with_text, Event, StreamDecoder, ToolDef};
use serde_json::{Map, Value};

fn load_eval() -> serde_json::Value {
    let data = fs::read_to_string("tests/fixtures/eval.json").expect("eval fixture exists");
    serde_json::from_str(&data).expect("eval fixture is valid JSON")
}

fn openai_tool_to_tooldef(tool: &Value) -> ToolDef {
    let function = tool["function"].clone();
    ToolDef {
        name: function["name"].as_str().unwrap().to_string(),
        description: function["description"].as_str().map(ToString::to_string),
        parameters: function.get("parameters").cloned(),
    }
}

fn tool_list_for(case: &Value) -> Vec<ToolDef> {
    let eval = load_eval();
    let tools = case["tools"].as_array().unwrap();
    let all = eval["tools"].as_array().unwrap();
    let names: std::collections::HashSet<_> = tools.iter().filter_map(|v| v.as_str()).collect();
    all.iter()
        .filter(|tool| names.contains(tool["function"]["name"].as_str().unwrap()))
        .map(openai_tool_to_tooldef)
        .collect()
}

fn normalize_call(call: &serde_json::Value) -> Value {
    let obj = call.as_object().unwrap();
    let mut out = Map::new();
    out.insert("name".to_string(), Value::String(obj["name"].as_str().unwrap().to_string()));
    out.insert("arguments".to_string(), obj["arguments"].clone());
    Value::Object(out)
}

#[test]
fn decoder_cases_match_expected() {
    let eval = load_eval();
    let decoder_cases = eval["decoder_cases"].as_array().unwrap();

    for case in decoder_cases {
        let tools = tool_list_for(case);
        let mut decoder = StreamDecoder::new(&tools);
        let mut events = Vec::new();
        let result = (|| {
            for chunk in case["chunks"].as_array().unwrap() {
                events.extend(decoder.push(chunk.as_str().unwrap())?);
            }
            events.extend(decoder.finish()?);
            Ok(events)
        })();

        let expected = case["expected"].as_object().unwrap();
        if expected.contains_key("error") {
            let err: nasiko_tool_compact::Error = result.expect_err("expected decoder error");
            assert_eq!(err.code(), expected["error"].as_str().unwrap());
            continue;
        }

        let events = result.expect("decoder should succeed");
        let actual_calls: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Event::Call(call) => Some(call),
                _ => None,
            })
            .collect();

        let expected_calls = expected["calls"].as_array().unwrap();
        assert_eq!(actual_calls.len(), expected_calls.len());
        for (actual, expected_call) in actual_calls.iter().zip(expected_calls.iter()) {
            assert_eq!(actual.name, expected_call["name"].as_str().unwrap());
            let actual_args = serde_json::from_str::<Value>(&actual.arguments).unwrap();
            assert_eq!(actual_args, expected_call["arguments"]);
        }
    }
}
