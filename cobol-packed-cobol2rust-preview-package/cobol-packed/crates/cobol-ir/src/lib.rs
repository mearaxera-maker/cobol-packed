use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub file: String,
    pub line: usize,
    pub column: usize,
}

impl SourceSpan {
    pub fn generated() -> Self {
        Self {
            file: "<generated>".to_string(),
            line: 0,
            column: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: String,
    pub severity: Severity,
    pub message: String,
    pub span: SourceSpan,
}

impl Diagnostic {
    pub fn error(code: impl Into<String>, message: impl Into<String>, span: SourceSpan) -> Self {
        Self {
            code: code.into(),
            severity: Severity::Error,
            message: message.into(),
            span,
        }
    }

    pub fn warning(code: impl Into<String>, message: impl Into<String>, span: SourceSpan) -> Self {
        Self {
            code: code.into(),
            severity: Severity::Warning,
            message: message.into(),
            span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramIr {
    pub name: String,
    pub dialect: CobolDialect,
    pub data_items: Vec<DataItemIr>,
    pub paragraphs: Vec<ParagraphIr>,
    pub files: Vec<FileIr>,
    pub diagnostics: Vec<Diagnostic>,
}

impl ProgramIr {
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CobolDialect {
    Ibm,
    GnuCobol,
    MicroFocus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataItemIr {
    pub level: u8,
    pub name: String,
    pub rust_name: String,
    pub picture: Option<String>,
    pub usage: UsageIr,
    pub occurs: Option<OccursIr>,
    pub redefines: Option<String>,
    pub parent: Option<String>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsageIr {
    Display,
    PackedDecimal,
    Binary,
    NativeBinary,
    Float32,
    Float64,
    Alphanumeric,
    Group,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OccursIr {
    pub min: usize,
    pub max: usize,
    pub depending_on: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileIr {
    pub name: String,
    pub record_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParagraphIr {
    pub name: String,
    pub rust_name: String,
    pub statements: Vec<StatementIr>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatementIr {
    Display(Vec<OperandIr>),
    Move {
        source: OperandIr,
        target: String,
    },
    Add {
        source: OperandIr,
        target: String,
    },
    Subtract {
        source: OperandIr,
        target: String,
    },
    Multiply {
        source: OperandIr,
        target: String,
    },
    Divide {
        source: OperandIr,
        target: String,
    },
    Compute {
        target: String,
        expression: String,
    },
    If {
        condition: String,
        then_statements: Vec<StatementIr>,
        else_statements: Vec<StatementIr>,
    },
    Evaluate {
        expression: String,
        arms: Vec<String>,
    },
    Perform {
        target: String,
        through: Option<String>,
    },
    GoTo(String),
    Open(String),
    Read(String),
    Write(String),
    Close(String),
    StopRun,
    Unsupported {
        keyword: String,
        raw: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperandIr {
    Identifier(String),
    Literal(String),
    Number(String),
}
