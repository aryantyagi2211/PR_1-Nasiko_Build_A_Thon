use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ToolDef {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CompactTools {
    pub text: String,
    pub instructions: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Decoded {
    pub text: String,
    pub calls: Vec<ToolCall>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("unknown tool")]
    UnknownTool,
    #[error("invalid arguments")]
    InvalidArguments,
    #[error("malformed call")]
    MalformedCall,
    #[error("unsupported schema")]
    UnsupportedSchema,
}

impl Error {
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownTool => "unknown_tool",
            Self::InvalidArguments => "invalid_arguments",
            Self::MalformedCall => "malformed_call",
            Self::UnsupportedSchema => "unsupported_schema",
        }
    }
}
