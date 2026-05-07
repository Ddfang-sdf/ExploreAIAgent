use std::fmt;
use serde::{Serialize, Deserialize, Serializer, Deserializer};
use serde::de;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorCode {
    PathOutsideRoot,
    PathNotFound,
    PathNotFile,
    PathNotDirectory,
    InvalidPattern,
    InvalidLineRange,
    EncodingError,
    PermissionDenied,
    ExecutionTimeout,
    OutputTooLarge,
    ShellCmdNotAllowed,
    ShellDangerousOperator,
    ShellExecutionFailed,
    InternalError,
}

impl ErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorCode::PathOutsideRoot => "PATH_OUTSIDE_ROOT",
            ErrorCode::PathNotFound => "PATH_NOT_FOUND",
            ErrorCode::PathNotFile => "PATH_NOT_FILE",
            ErrorCode::PathNotDirectory => "PATH_NOT_DIRECTORY",
            ErrorCode::InvalidPattern => "INVALID_PATTERN",
            ErrorCode::InvalidLineRange => "INVALID_LINE_RANGE",
            ErrorCode::EncodingError => "ENCODING_ERROR",
            ErrorCode::PermissionDenied => "PERMISSION_DENIED",
            ErrorCode::ExecutionTimeout => "EXECUTION_TIMEOUT",
            ErrorCode::OutputTooLarge => "OUTPUT_TOO_LARGE",
            ErrorCode::ShellCmdNotAllowed => "SHELL_CMD_NOT_ALLOWED",
            ErrorCode::ShellDangerousOperator => "SHELL_DANGEROUS_OPERATOR",
            ErrorCode::ShellExecutionFailed => "SHELL_EXECUTION_FAILED",
            ErrorCode::InternalError => "INTERNAL_ERROR",
        }
    }

    pub fn from_str(s: &str) -> Option<ErrorCode> {
        match s {
            "PATH_OUTSIDE_ROOT" => Some(ErrorCode::PathOutsideRoot),
            "PATH_NOT_FOUND" => Some(ErrorCode::PathNotFound),
            "PATH_NOT_FILE" => Some(ErrorCode::PathNotFile),
            "PATH_NOT_DIRECTORY" => Some(ErrorCode::PathNotDirectory),
            "INVALID_PATTERN" => Some(ErrorCode::InvalidPattern),
            "INVALID_LINE_RANGE" => Some(ErrorCode::InvalidLineRange),
            "ENCODING_ERROR" => Some(ErrorCode::EncodingError),
            "PERMISSION_DENIED" => Some(ErrorCode::PermissionDenied),
            "EXECUTION_TIMEOUT" => Some(ErrorCode::ExecutionTimeout),
            "OUTPUT_TOO_LARGE" => Some(ErrorCode::OutputTooLarge),
            "SHELL_CMD_NOT_ALLOWED" => Some(ErrorCode::ShellCmdNotAllowed),
            "SHELL_DANGEROUS_OPERATOR" => Some(ErrorCode::ShellDangerousOperator),
            "SHELL_EXECUTION_FAILED" => Some(ErrorCode::ShellExecutionFailed),
            "INTERNAL_ERROR" => Some(ErrorCode::InternalError),
            _ => None,
        }
    }
}

impl Serialize for ErrorCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ErrorCode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        ErrorCode::from_str(&s)
            .ok_or_else(|| de::Error::custom(format!("unknown error code: {}", s)))
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolError {
    pub success: bool,
    pub error: String,
    pub code: ErrorCode,
}

impl ToolError {
    pub fn new(code: ErrorCode, error: impl Into<String>) -> Self {
        ToolError {
            success: false,
            error: error.into(),
            code,
        }
    }
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.code, self.error)
    }
}

impl std::error::Error for ToolError {}
