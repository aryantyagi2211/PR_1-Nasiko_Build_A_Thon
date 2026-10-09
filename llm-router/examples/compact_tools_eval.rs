//! Offline eval for the compact-tools encoding/decoding pipeline.
//!
//! Run:
//!   set EVAL_SET=C:\dev\compact-tools-eval.json
//!   set OUT=C:\dev\out1.jsonl
//!   cargo run --release -p nasiko-llm-router --example compact_tools_eval
//!
//! Optional token-savings measurement (prints to stderr only):
//!   set MEASURE=1
//!
//! Optional live provider evaluation (requires both base URL and model):
//!   set PROVIDER_BASE_URL=https://api.example.com/v1
//!   set MODEL=your-model
//!   PROVIDER_API_KEY is optional
//!
//! Exit code 0 if the eval ran. Output is deterministic.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::time::Duration;

use nasiko_tool_compact::{
    decode_calls, decode_with_text, encode_tools, is_compactable, Error, Event, StreamDecoder,
    ToolDef,
};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Eval file schema (what we read from EVAL_SET)
// ---------------------------------------------------------------------------

/// A tool entry in the eval JSON – OpenAI native format.
/// `{"type":"function","function":{"name":..,"description":..,"parameters":{..}}}`
fn openai_tool_to_def(v: &Value) -> ToolDef {
    let f = &v["function"];
    ToolDef {
        name: f["name"].as_str().unwrap_or("").to_string(),
        description: f["description"].as_str().map(str::to_string),
        parameters: f.get("parameters").cloned(),
    }
}


// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Fixed reference time line that goes at the top of every system message.
const TIME_LINE: &str =
    "Today is 2026-10-02. Timezone: Asia/Kolkata (+05:30). Resolve relative dates to ISO 8601 with offset.";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Look up tool defs from the global map by name list (preserving order).
fn resolve_tools<'a>(
    all_tools: &'a BTreeMap<String, ToolDef>,
    names: &[&str],
) -> Vec<&'a ToolDef> {
    names
        .iter()
        .filter_map(|n| all_tools.get(*n))
        .collect()
}

/// Build the compact system message content:
///   TIME_LINE + "\n" + instructions + "\n" + compact_text
fn compact_system_content(compact_text: &str, instructions: &str) -> String {
    format!("{TIME_LINE}\n{instructions}\n{compact_text}")
}

/// Build a native system message content (just the time line – tools go in the
/// `tools` field of the request, not the system prompt).
fn native_system_content() -> String {
    TIME_LINE.to_string()
}

/// Render expected calls as compact text lines.
/// `<<call NAME {json}>>` per call, joined by `\n`. Empty string when no calls.
fn render_calls(expected: &[Value]) -> String {
    if expected.is_empty() {
        return String::new();
    }
    expected
        .iter()
        .map(|c| {
            let name = c["name"].as_str().unwrap_or("");
            let args = serde_json::to_string(&c["arguments"]).unwrap_or_else(|_| "{}".to_string());
            format!("<<call {name} {args}>>")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Decode rendered_calls text back to a JSON array of {name, arguments:<object>}.
fn decode_rendered(rendered: &str, tools: &[ToolDef]) -> Value {
    if rendered.is_empty() {
        return Value::Array(vec![]);
    }
    match decode_calls(rendered, tools) {
        Ok(calls) => {
            let arr: Vec<Value> = calls
                .iter()
                .map(|c| {
                    let args: Value = serde_json::from_str(&c.arguments)
                        .unwrap_or(Value::Object(serde_json::Map::new()));
                    serde_json::json!({ "name": c.name, "arguments": args })
                })
                .collect();
            Value::Array(arr)
        }
        Err(e) => {
            eprintln!("WARN: decode_calls failed: {e}");
            Value::Array(vec![])
        }
    }
}

struct LiveMode {
    runtime: Option<tokio::runtime::Runtime>,
    client: Option<reqwest::Client>,
    endpoint: String,
    model: String,
    api_key: Option<String>,
}

impl LiveMode {
    fn from_env() -> Option<Self> {
        let base_url = std::env::var("PROVIDER_BASE_URL").ok()?;
        let model = std::env::var("MODEL").ok()?;
        if base_url.is_empty() || model.is_empty() {
            return None;
        }

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .ok();

        Some(Self {
            runtime,
            client,
            endpoint: format!("{}/chat/completions", base_url.trim_end_matches('/')),
            model,
            api_key: std::env::var("PROVIDER_API_KEY").ok(),
        })
    }

    fn call(&self, request: &Value) -> Result<String, ()> {
        let (Some(runtime), Some(client)) = (&self.runtime, &self.client) else {
            return Err(());
        };
        runtime.block_on(send_live_request(
            client,
            &self.endpoint,
            &self.model,
            self.api_key.as_deref(),
            request,
        ))
    }
}

async fn send_live_request(
    client: &reqwest::Client,
    endpoint: &str,
    model: &str,
    api_key: Option<&str>,
    request: &Value,
) -> Result<String, ()> {
    let mut body = request.clone();
    body["model"] = Value::String(model.to_string());

    for attempt in 0..=1 {
        let mut req = client.post(endpoint).json(&body);
        if let Some(api_key) = api_key {
            req = req.bearer_auth(api_key);
        }
        let response = match req.send().await {
            Ok(response) => response,
            Err(_) if attempt == 0 => {
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
            Err(_) => return Err(()),
        };

        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
            if attempt == 0 {
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
            return Err(());
        }
        if !status.is_success() {
            return Err(());
        }

        let response_bytes = match response.bytes().await {
            Ok(bytes) => bytes,
            Err(_) if attempt == 0 => {
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
            Err(_) => return Err(()),
        };
        let response: Value = serde_json::from_slice(&response_bytes).map_err(|_| ())?;
        return response["choices"][0]["message"]["content"]
            .as_str()
            .map(str::to_string)
            .ok_or(());
    }

    Err(())
}

fn decode_live_output(raw_output: &str, tools: &[ToolDef]) -> Value {
    match decode_with_text(raw_output, tools) {
        Ok(decoded) => Value::Array(
            decoded
                .calls
                .iter()
                .map(|call| {
                    let arguments: Value = serde_json::from_str(&call.arguments)
                        .unwrap_or_else(|_| Value::Object(serde_json::Map::new()));
                    serde_json::json!({ "name": call.name, "arguments": arguments })
                })
                .collect(),
        ),
        Err(error) => serde_json::json!({ "error": error.code() }),
    }
}

fn calls_match(expected: &[Value], actual: &Value, free_text_fields: &[String]) -> bool {
    let Some(actual) = actual.as_array() else {
        return false;
    };
    actual.len() == expected.len()
        && expected.iter().zip(actual).all(|(expected, actual)| {
            expected["name"] == actual["name"]
                && values_match(
                    &expected["arguments"],
                    &actual["arguments"],
                    free_text_fields,
                )
        })
}

fn values_match(expected: &Value, actual: &Value, free_text_fields: &[String]) -> bool {
    match (expected, actual) {
        (Value::Object(expected), Value::Object(actual)) => {
            expected.len() == actual.len()
                && expected.iter().all(|(key, expected_value)| {
                    actual.get(key).is_some_and(|actual_value| {
                        if free_text_fields.iter().any(|field| field == key) {
                            actual_value.is_string()
                        } else {
                            values_match(expected_value, actual_value, free_text_fields)
                        }
                    })
                })
        }
        (Value::Array(expected), Value::Array(actual)) => {
            expected.len() == actual.len()
                && expected
                    .iter()
                    .zip(actual)
                    .all(|(expected, actual)| values_match(expected, actual, free_text_fields))
        }
        _ => expected == actual,
    }
}

fn redact_secret(value: &mut Value, secret: &str) {
    if secret.is_empty() {
        return;
    }
    match value {
        Value::String(text) => *text = text.replace(secret, "[REDACTED]"),
        Value::Array(values) => {
            for value in values {
                redact_secret(value, secret);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                redact_secret(value, secret);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Token measurement (MEASURE=1 only, stderr)
// ---------------------------------------------------------------------------

fn try_count_tokens(req: &Value) -> Option<usize> {
    // Only attempt measurement when MEASURE=1
    if std::env::var("MEASURE").as_deref() != Ok("1") {
        return None;
    }
    let text = serde_json::to_string(req).ok()?;
    let bpe = tiktoken_rs::o200k_base().ok()?;
    Some(bpe.encode_with_special_tokens(&text).len())
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let eval_path = std::env::var("EVAL_SET").expect("EVAL_SET env var not set");
    let out_path = std::env::var("OUT").expect("OUT env var not set");

    let raw = std::fs::read_to_string(&eval_path)
        .unwrap_or_else(|e| panic!("Cannot read EVAL_SET={eval_path}: {e}"));
    let data: Value =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("Invalid JSON in EVAL_SET: {e}"));

    // Build global tool map (BTreeMap for determinism).
    let tools_array = data["tools"].as_array().expect("'tools' array in eval JSON");
    let all_tools: BTreeMap<String, ToolDef> = tools_array
        .iter()
        .map(|v| {
            let def = openai_tool_to_def(v);
            (def.name.clone(), def)
        })
        .collect();

    // Also keep OpenAI-native tool objects (for native baseline requests).
    let all_tools_openai: BTreeMap<String, Value> = tools_array
        .iter()
        .map(|v| {
            let name = v["function"]["name"].as_str().unwrap_or("").to_string();
            (name, v.clone())
        })
        .collect();

    let out_file =
        std::fs::File::create(&out_path).unwrap_or_else(|e| panic!("Cannot create OUT={out_path}: {e}"));
    let mut out = std::io::BufWriter::new(out_file);
    let live_mode = LiveMode::from_env();
    let live_enabled = live_mode.is_some();

    // --- Accumulate token stats (MEASURE=1) ---
    let measure = std::env::var("MEASURE").as_deref() == Ok("1");
    let mut total_native_tokens: usize = 0;
    let mut total_compact_tokens: usize = 0;

    // -----------------------------------------------------------------------
    // Process `cases`
    // -----------------------------------------------------------------------
    let cases = data["cases"].as_array().expect("'cases' array in eval JSON");
    let mut live_passed = 0;
    for case in cases {
        let id = case["id"].as_str().unwrap_or("");

        // Resolve tool names for this case.
        let tool_names: Vec<&str> = case["tools"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();

        let case_tool_defs: Vec<ToolDef> = resolve_tools(&all_tools, &tool_names)
            .into_iter()
            .cloned()
            .collect();

        // Case messages.
        let messages_val = case["messages"].as_array().expect("'messages' in case");

        // Expected calls.
        let expected: Vec<Value> = case["expected"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        // Try compact encoding.
        let compact_result = encode_tools(&case_tool_defs);
        let compacted = compact_result.is_ok() && is_compactable(&case_tool_defs);

        let (compact_request, rendered_calls, roundtrip_calls) = if compacted {
            let ct = compact_result.unwrap();
            let system_content = compact_system_content(&ct.text, &ct.instructions);

            // Build messages: system first, then case messages.
            let mut messages: Vec<Value> = vec![
                serde_json::json!({ "role": "system", "content": system_content }),
            ];
            for m in messages_val {
                messages.push(m.clone());
            }

            // Compact request has NO "tools" field.
            let request = serde_json::json!({
                "model": "eval",
                "temperature": 0,
                "messages": messages,
            });

            // Render expected calls and round-trip.
            let rendered = render_calls(&expected);
            let roundtrip = decode_rendered(&rendered, &case_tool_defs);

            (request, rendered, roundtrip)
        } else {
            // Fallback: native request (include "tools" array).
            let system_content = native_system_content();

            let mut messages: Vec<Value> = vec![
                serde_json::json!({ "role": "system", "content": system_content }),
            ];
            for m in messages_val {
                messages.push(m.clone());
            }

            let native_tools: Vec<Value> = tool_names
                .iter()
                .filter_map(|n| all_tools_openai.get(*n))
                .cloned()
                .collect();

            let request = serde_json::json!({
                "model": "eval",
                "temperature": 0,
                "messages": messages,
                "tools": native_tools,
            });

            (request, String::new(), Value::Array(vec![]))
        };

        // Token measurement (stderr only).
        if measure {
            // Baseline native request.
            let native_sys = native_system_content();
            let mut native_msgs: Vec<Value> = vec![
                serde_json::json!({ "role": "system", "content": native_sys }),
            ];
            for m in messages_val {
                native_msgs.push(m.clone());
            }
            let native_tools: Vec<Value> = tool_names
                .iter()
                .filter_map(|n| all_tools_openai.get(*n))
                .cloned()
                .collect();
            let native_req = serde_json::json!({
                "model": "eval",
                "temperature": 0,
                "messages": native_msgs,
                "tools": native_tools,
            });

            if let (Some(native_tok), Some(compact_tok)) =
                (try_count_tokens(&native_req), try_count_tokens(&compact_request))
            {
                total_native_tokens += native_tok;
                total_compact_tokens += compact_tok;
                eprintln!(
                    "[MEASURE] case={id} native={native_tok} compact={compact_tok} saved={}",
                    native_tok as isize - compact_tok as isize
                );
            }
        }

        // Write output line.
        let mut line = serde_json::json!({
            "id": id,
            "compact_request": compact_request,
            "compacted": compacted,
            "rendered_calls": rendered_calls,
            "roundtrip_calls": roundtrip_calls,
        });
        if let Some(live_mode) = &live_mode {
            let (mut raw_output, mut live_calls) = match live_mode.call(&compact_request) {
                Ok(raw_output) => {
                    let live_calls = decode_live_output(&raw_output, &case_tool_defs);
                    (raw_output, live_calls)
                }
                Err(()) => (
                    String::new(),
                    serde_json::json!({ "error": "request_failed" }),
                ),
            };
            let free_text_fields: Vec<String> = case["match"]["free_text_fields"]
                .as_array()
                .map(|fields| {
                    fields
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            let passed = calls_match(&expected, &live_calls, &free_text_fields);
            if passed {
                live_passed += 1;
            }
            if let Some(secret) = &live_mode.api_key {
                raw_output = raw_output.replace(secret, "[REDACTED]");
                redact_secret(&mut live_calls, secret);
            }
            eprintln!("{} {id}", if passed { "PASS" } else { "FAIL" });
            line["raw_output"] = Value::String(raw_output);
            line["live_calls"] = live_calls;
        }
        writeln!(out, "{line}").expect("write OUT");
    }
    if live_enabled {
        let total = cases.len();
        eprintln!("live pass: {live_passed}/{total}");
    }

    // -----------------------------------------------------------------------
    // Process `decoder_cases`
    // -----------------------------------------------------------------------
    let decoder_cases = data["decoder_cases"]
        .as_array()
        .expect("'decoder_cases' array in eval JSON");

    for dc in decoder_cases {
        let id = dc["id"].as_str().unwrap_or("");

        let dc_tool_names: Vec<&str> = dc["tools"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();

        let dc_tools: Vec<ToolDef> = resolve_tools(&all_tools, &dc_tool_names)
            .into_iter()
            .cloned()
            .collect();

        let chunks: Vec<&str> = dc["chunks"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();

        // Feed chunks one by one into StreamDecoder.
        let mut decoder = StreamDecoder::new(&dc_tools);
        let mut all_events: Vec<Event> = Vec::new();
        let mut stream_error: Option<Error> = None;

        'chunks: for chunk in &chunks {
            match decoder.push(chunk) {
                Ok(events) => all_events.extend(events),
                Err(e) => {
                    stream_error = Some(e);
                    break 'chunks;
                }
            }
        }

        if stream_error.is_none() {
            match decoder.finish() {
                Ok(events) => all_events.extend(events),
                Err(e) => stream_error = Some(e),
            }
        }

        let decoded_value = match stream_error {
            Some(e) => {
                serde_json::json!({ "error": e.code() })
            }
            None => {
                // Collect calls from events; any error during collection = error result.
                let calls: Vec<Value> = all_events
                    .iter()
                    .filter_map(|ev| {
                        if let Event::Call(c) = ev {
                            let args: Value = serde_json::from_str(&c.arguments)
                                .unwrap_or(Value::Object(serde_json::Map::new()));
                            Some(serde_json::json!({ "name": c.name, "arguments": args }))
                        } else {
                            None
                        }
                    })
                    .collect();
                serde_json::json!({ "calls": calls })
            }
        };

        let line = serde_json::json!({
            "id": id,
            "decoded": decoded_value,
        });
        writeln!(out, "{line}").expect("write OUT");
    }

    out.flush().expect("flush OUT");

    // Summary token stats.
    if measure && total_native_tokens > 0 {
        let savings = 1.0 - (total_compact_tokens as f64 / total_native_tokens as f64);
        eprintln!(
            "[MEASURE] TOTAL native={total_native_tokens} compact={total_compact_tokens} savings={:.1}%",
            savings * 100.0
        );
    }
}
