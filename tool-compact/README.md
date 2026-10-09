# tool-compact

`nasiko-tool-compact` is a standalone library that turns supported JSON tool
schemas into compact tool lines and decodes model tool-call markers. It does
not depend on `nasiko-llm-router`; convert router types at the integration seam.

## Format

Tool lines use `name(param:type, optional?:type) - description`. Types include
strings, numbers, booleans, date-times, enums, arrays, nested objects, and
`any`. Calls use `<<call NAME {JSON_ARGS}>>`. See [GRAMMAR.md](GRAMMAR.md).

## Public API

- `encode_tools` and `is_compactable` encode tools and check schema support.
- `decode_tools` rebuilds tool definitions from compact lines.
- `decode_calls` and `decode_with_text` decode and validate calls.
- `StreamDecoder` decodes calls and text incrementally.
- `ToolDef`, `ToolCall`, `CompactTools`, `Decoded`, `Event`, and `Error` are
  the public data types.

Unsupported schemas return `unsupported_schema`, allowing callers to bypass
compaction. Unsupported features include `$ref`, `oneOf`, `anyOf`, `allOf`,
`not`, `patternProperties`, mixed-type enums, unknown keywords, and
`additionalProperties` values other than `false` or absent.

Decoding fails closed: malformed calls are not repaired; tool names, required
and unknown arguments, types, enum values, nested values, and date-times are
validated against the original schema. Any invalid call returns an error
instead of partial results.

## Tests and offline evaluation

Run crate tests with:

```powershell
cargo test -p nasiko-tool-compact
```

The offline evaluator reads `EVAL_SET`, writes JSONL to `OUT`, and optionally
prints token measurements when `MEASURE=1`:

```powershell
$env:EVAL_SET = 'C:\dev\compact-tools-eval.json'
$env:OUT = 'C:\dev\out.jsonl'
$env:MEASURE = '1' # optional
cargo run --offline -p nasiko-llm-router --example compact_tools_eval
```

The evaluator can also run in live mode when both `PROVIDER_BASE_URL` and
`MODEL` are set; `PROVIDER_API_KEY` is optional. Leave those variables unset
for offline evaluation.
