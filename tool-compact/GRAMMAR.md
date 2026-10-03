# Compact tool grammar

Tool line:  name(param, param, ...) - description
  param   := key ["?"] ":" type        ("?" = optional, otherwise required)
  type    := "str" | "int" | "num" | "bool" | "datetime" | "any"
           | "[" type "]"                      (array)
           | enumvalue ("|" enumvalue)+        (string enum)
           | "{" param, ... "}"                (nested object)
  datetime = string with format "date-time".

Parameter descriptions are omitted when their normalized words only repeat the
parameter name or type. For names, split snake/camel case, normalize common
plurals, ignore articles and generic fillers (event, line, plain text), map
"to" to "recipient", and ignore "email(s)" for recipient/attendee/cc/bcc names.
For types, omit descriptions containing only the type name (string/str,
integer/int, number/num, boolean/bool, array, or object). Otherwise append
" /short text" after the type, truncating to 60 characters. These rules are
deterministic. Example:
  create_calendar_event(title:str, start:datetime /Start time, ISO 8601, duration_min?:int /Duration in minutes, attendees?:[str], visibility?:public|private) - Create event in user's calendar.

Instructions text:
  Tools: name(arg:type, arg?:type) - desc. ? optional. Call: <<call NAME {JSON}>> per call (valid JSON, listed tools only; multiple calls allowed). If no tool is needed, answer normally.
Call syntax: "<<call" SP name SP json ">>". Name = [A-Za-z0-9_.-]+.
The JSON value is found by a string-aware scanner (track quotes, escapes, brace/bracket depth), so ">>" inside a string is fine; the call ends at the first ">>" after the JSON value closes (whitespace allowed before it).
Text before/after/between calls is preserved as text.

Unsupported schema features (encode_tools returns unsupported_schema):
$ref, oneOf, anyOf, allOf, not, patternProperties, additionalProperties other than false/absent, mixed-type enums, any keyword outside:
type, properties, required, items, enum, description, format("date-time" only),
additionalProperties:false. Non-object top-level parameters also unsupported
(a tool with parameters None or empty properties is fine: "name() - desc").
