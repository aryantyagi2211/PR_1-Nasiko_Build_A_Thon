# Compact tool grammar

Tool line:  name(param, param, ...) - description
  param   := key ["?"] ":" type        ("?" = optional, otherwise required)
  type    := "str" | "int" | "num" | "bool" | "datetime" | "any"
           | "[" type "]"                      (array)
           | enumvalue ("|" enumvalue)+        (string enum)
           | "{" param, ... "}"                (nested object)
  datetime = string with format "date-time".

Param descriptions: append " /short text" after the type ONLY when the schema description exists and adds meaning (skip if it only repeats the param name; truncate to 60 chars, deterministic). Example:
  create_calendar_event(title:str, start:datetime /Start time ISO 8601, duration_min?:int /Duration in minutes, attendees?:[str], visibility?:public|private) - Create an event in the user's calendar.

Instructions text: tell the model to call a tool by emitting exactly
  <<call NAME {JSON_ARGS}>>
on its own, JSON args must be valid JSON with double-quoted keys, omit optional args it doesn't know, never invent tools, multiple calls allowed, plain answer with no marker when no tool is needed.
Call syntax: "<<call" SP name SP json ">>". Name = [A-Za-z0-9_.-]+.
The JSON value is found by a string-aware scanner (track quotes, escapes, brace/bracket depth), so ">>" inside a string is fine; the call ends at the first ">>" after the JSON value closes (whitespace allowed before it).
Text before/after/between calls is preserved as text.

Unsupported schema features (encode_tools returns unsupported_schema):
$ref, oneOf, anyOf, allOf, not, patternProperties, additionalProperties other than false/absent, mixed-type enums, any keyword outside:
type, properties, required, items, enum, description, format("date-time" only),
additionalProperties:false. Non-object top-level parameters also unsupported
(a tool with parameters None or empty properties is fine: "name() - desc").
