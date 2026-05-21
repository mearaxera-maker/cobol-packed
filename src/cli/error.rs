use super::constants::{ERROR_DOCS_BASE_URL, OUTPUT_VERSION};
use std::io;

#[derive(Debug, Clone, Copy)]
pub enum ExitCode {
    Success = 0,
    Data = 1,
    Config = 2,
    Io = 3,
    Internal = 4,
}

#[derive(Debug)]
pub struct CliError {
    pub(super) code: &'static str,
    pub(super) message: String,
    pub(super) exit: ExitCode,
}

impl CliError {
    pub(super) fn data(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            exit: ExitCode::Data,
        }
    }

    pub(super) fn config(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            exit: ExitCode::Config,
        }
    }

    pub(super) fn io(message: impl Into<String>) -> Self {
        Self {
            code: "E_IO",
            message: message.into(),
            exit: ExitCode::Io,
        }
    }

    pub(super) fn internal(message: impl Into<String>) -> Self {
        Self {
            code: "E_INTERNAL",
            message: message.into(),
            exit: ExitCode::Internal,
        }
    }

    pub fn exit_code(&self) -> ExitCode {
        self.exit
    }

    pub fn render(&self) -> String {
        serde_json::json!({
            "version": OUTPUT_VERSION,
            "error_code": self.code,
            "error_docs_url": ERROR_DOCS_BASE_URL,
            "message": self.message,
        })
        .to_string()
    }
}

impl From<io::Error> for CliError {
    fn from(err: io::Error) -> Self {
        CliError::io(err.to_string())
    }
}

impl From<csv::Error> for CliError {
    fn from(err: csv::Error) -> Self {
        CliError::data("E_CSV", err.to_string())
    }
}

impl From<serde_json::Error> for CliError {
    fn from(err: serde_json::Error) -> Self {
        CliError::data("E_JSON", err.to_string())
    }
}
