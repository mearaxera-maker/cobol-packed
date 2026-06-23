use rust_decimal::Decimal;
use std::collections::BTreeMap;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Default)]
pub enum CobolValue {
    Text(String),
    Decimal(Decimal),
    Bytes(Vec<u8>),
    #[default]
    Null,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CobolFieldKind {
    Group,
    Alphanumeric,
    Display,
    NumericDisplay,
    NumericEdited,
    PackedDecimal,
    Binary,
    NativeBinary,
    Float32,
    Float64,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CobolFieldLayout {
    pub name: String,
    pub offset: usize,
    pub byte_len: usize,
    pub kind: CobolFieldKind,
}

impl CobolValue {
    pub fn display_string(&self) -> String {
        match self {
            Self::Text(value) => value.clone(),
            Self::Decimal(value) => value.to_string(),
            Self::Bytes(bytes) => bytes
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<Vec<_>>()
                .join(""),
            Self::Null => String::new(),
        }
    }

    pub fn decimal(&self) -> Result<Decimal, RuntimeError> {
        match self {
            Self::Decimal(value) => Ok(*value),
            Self::Text(value) => {
                Decimal::from_str(value.trim()).map_err(|_| RuntimeError::InvalidDecimal {
                    value: value.clone(),
                })
            }
            Self::Bytes(bytes) => Err(RuntimeError::InvalidDecimal {
                value: format!("{bytes:02X?}"),
            }),
            Self::Null => Ok(Decimal::ZERO),
        }
    }
}

impl From<&str> for CobolValue {
    fn from(value: &str) -> Self {
        Self::Text(value.to_string())
    }
}

impl From<String> for CobolValue {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<Decimal> for CobolValue {
    fn from(value: Decimal) -> Self {
        Self::Decimal(value)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("unknown COBOL field {name}")]
    UnknownField { name: String },
    #[error("value cannot be interpreted as Decimal: {value}")]
    InvalidDecimal { value: String },
    #[error("division by zero")]
    DivisionByZero,
    #[error("file operation is not bound in generated runtime: {operation}")]
    UnboundFileOperation { operation: String },
    #[error("field {name} range {offset}..{end} exceeds storage length {len}")]
    FieldOutOfBounds {
        name: String,
        offset: usize,
        end: usize,
        len: usize,
    },
    #[error(
        "field {name} with kind {kind:?} is not supported by this generated runtime operation"
    )]
    UnsupportedFieldKind { name: String, kind: CobolFieldKind },
    #[error("COBOL field codec error: {message}")]
    Codec { message: String },
}

impl RuntimeError {
    pub fn code(&self) -> &'static str {
        match self {
            RuntimeError::UnknownField { .. } => "CBL-RT-FIELD-UNKNOWN",
            RuntimeError::InvalidDecimal { .. } => "CBL-RT-DECIMAL",
            RuntimeError::DivisionByZero => "CBL-RT-DIVIDE-BY-ZERO",
            RuntimeError::UnboundFileOperation { .. } => "CBL-RT-FILE-UNBOUND",
            RuntimeError::FieldOutOfBounds { .. } => "CBL-RT-FIELD-BOUNDS",
            RuntimeError::UnsupportedFieldKind { .. } => "CBL-RT-FIELD-UNSUPPORTED",
            RuntimeError::Codec { .. } => "CBL-RT-CODEC",
        }
    }

    pub fn runtime_message(&self) -> &'static str {
        match self {
            RuntimeError::UnknownField { .. } => "Unknown COBOL field reference at runtime",
            RuntimeError::InvalidDecimal { .. } => "COBOL numeric conversion failed",
            RuntimeError::DivisionByZero => "COBOL arithmetic division by zero",
            RuntimeError::UnboundFileOperation { .. } => {
                "Generated COBOL file operation has no bound runtime implementation"
            }
            RuntimeError::FieldOutOfBounds { .. } => {
                "COBOL field access exceeded generated storage bounds"
            }
            RuntimeError::UnsupportedFieldKind { .. } => {
                "Generated runtime operation does not support this COBOL field category"
            }
            RuntimeError::Codec { .. } => "COBOL data encoding or decoding failed",
        }
    }

    pub fn suggested_action(&self) -> &'static str {
        match self {
            RuntimeError::UnknownField { .. } => {
                "Check generated field names, qualification, and Data Division storage definitions."
            }
            RuntimeError::InvalidDecimal { .. } | RuntimeError::DivisionByZero => {
                "Validate numeric input bytes, PIC/USAGE metadata, and arithmetic operands before this statement."
            }
            RuntimeError::UnboundFileOperation { .. } => {
                "Run with the VM backend or bind this file operation through generated file/runtime configuration."
            }
            RuntimeError::FieldOutOfBounds { .. } => {
                "Check generated storage offsets, record length, REDEFINES, OCCURS, and SAME RECORD AREA definitions."
            }
            RuntimeError::UnsupportedFieldKind { .. } => {
                "Use a supported generated runtime operation for this field or route execution through the COBOL VM backend."
            }
            RuntimeError::Codec { .. } => {
                "Inspect the source bytes and generated field codec metadata for incompatible COBOL data encoding."
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlFlow {
    Next,
    Perform(usize),
    GoTo(usize),
    StopRun,
}

#[derive(Debug, Default)]
pub struct CobolRuntime {
    pub display: Vec<String>,
}

impl CobolRuntime {
    pub fn display_line(&mut self, value: impl Into<String>) {
        let value = value.into();
        println!("{value}");
        self.display.push(value);
    }
}

#[cfg(test)]
mod tests {
    use super::{CobolFieldKind, RuntimeError};

    #[test]
    fn runtime_error_exposes_stable_code_and_action_metadata() {
        let cases = [
            (
                RuntimeError::UnknownField {
                    name: "WS_MISSING".to_string(),
                },
                "CBL-RT-FIELD-UNKNOWN",
                "field names",
            ),
            (
                RuntimeError::InvalidDecimal {
                    value: "AB".to_string(),
                },
                "CBL-RT-DECIMAL",
                "numeric input bytes",
            ),
            (
                RuntimeError::DivisionByZero,
                "CBL-RT-DIVIDE-BY-ZERO",
                "arithmetic operands",
            ),
            (
                RuntimeError::UnboundFileOperation {
                    operation: "READ MASTER".to_string(),
                },
                "CBL-RT-FILE-UNBOUND",
                "file/runtime configuration",
            ),
            (
                RuntimeError::FieldOutOfBounds {
                    name: "REC".to_string(),
                    offset: 4,
                    end: 8,
                    len: 6,
                },
                "CBL-RT-FIELD-BOUNDS",
                "storage offsets",
            ),
            (
                RuntimeError::UnsupportedFieldKind {
                    name: "PACKED".to_string(),
                    kind: CobolFieldKind::PackedDecimal,
                },
                "CBL-RT-FIELD-UNSUPPORTED",
                "COBOL VM backend",
            ),
            (
                RuntimeError::Codec {
                    message: "bad overpunch".to_string(),
                },
                "CBL-RT-CODEC",
                "field codec metadata",
            ),
        ];

        for (error, code, action_fragment) in cases {
            assert_eq!(error.code(), code);
            assert!(
                !error.runtime_message().is_empty(),
                "missing runtime message for {error:?}"
            );
            assert!(
                error.suggested_action().contains(action_fragment),
                "missing actionable guidance {action_fragment:?} for {error:?}: {}",
                error.suggested_action()
            );
        }
    }
}

#[derive(Debug, Default)]
pub struct CobolStorage {
    values: BTreeMap<String, CobolValue>,
}

#[derive(Debug, Default)]
pub struct CobolByteStorage {
    bytes: Vec<u8>,
    layouts: BTreeMap<String, CobolFieldLayout>,
}

impl CobolByteStorage {
    pub fn define_field(
        &mut self,
        name: impl Into<String>,
        offset: usize,
        byte_len: usize,
        kind: CobolFieldKind,
    ) {
        let name = name.into();
        let end = offset.saturating_add(byte_len);
        if self.bytes.len() < end {
            self.bytes.resize(end, b' ');
        }
        self.layouts.insert(
            name.clone(),
            CobolFieldLayout {
                name,
                offset,
                byte_len,
                kind,
            },
        );
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn bytes_mut(&mut self) -> &mut [u8] {
        &mut self.bytes
    }

    pub fn get(&self, name: &str) -> Result<CobolValue, RuntimeError> {
        let layout = self.layout(name)?;
        let bytes = self.field_bytes(layout)?;
        match layout.kind {
            CobolFieldKind::Group | CobolFieldKind::Alphanumeric | CobolFieldKind::Display => Ok(
                CobolValue::Text(String::from_utf8_lossy(bytes).trim_end().to_string()),
            ),
            CobolFieldKind::NumericDisplay
            | CobolFieldKind::NumericEdited
            | CobolFieldKind::PackedDecimal
            | CobolFieldKind::Binary
            | CobolFieldKind::NativeBinary
            | CobolFieldKind::Float32
            | CobolFieldKind::Float64
            | CobolFieldKind::Unknown => Ok(CobolValue::Bytes(bytes.to_vec())),
        }
    }

    pub fn move_value(&mut self, source: CobolValue, target: &str) -> Result<(), RuntimeError> {
        let layout = self.layout(target)?.clone();
        match layout.kind {
            CobolFieldKind::Group | CobolFieldKind::Alphanumeric | CobolFieldKind::Display => {
                let display = source.display_string();
                let end = layout.offset.saturating_add(layout.byte_len);
                if end > self.bytes.len() {
                    return Err(RuntimeError::FieldOutOfBounds {
                        name: layout.name,
                        offset: layout.offset,
                        end,
                        len: self.bytes.len(),
                    });
                }
                let dst = &mut self.bytes[layout.offset..end];
                dst.fill(b' ');
                for (idx, byte) in display.as_bytes().iter().take(layout.byte_len).enumerate() {
                    dst[idx] = *byte;
                }
                Ok(())
            }
            _ => Err(RuntimeError::UnsupportedFieldKind {
                name: layout.name,
                kind: layout.kind,
            }),
        }
    }

    pub fn add(&mut self, source: CobolValue, target: &str) -> Result<(), RuntimeError> {
        self.numeric_update(source, target, |left, right| left + right)
    }

    pub fn subtract(&mut self, source: CobolValue, target: &str) -> Result<(), RuntimeError> {
        self.numeric_update(source, target, |left, right| left - right)
    }

    pub fn multiply(&mut self, source: CobolValue, target: &str) -> Result<(), RuntimeError> {
        self.numeric_update(source, target, |left, right| left * right)
    }

    pub fn divide(&mut self, source: CobolValue, target: &str) -> Result<(), RuntimeError> {
        let divisor = source.decimal()?;
        if divisor.is_zero() {
            return Err(RuntimeError::DivisionByZero);
        }
        self.numeric_update(CobolValue::Decimal(divisor), target, |left, right| {
            left / right
        })
    }

    fn numeric_update<F>(
        &mut self,
        source: CobolValue,
        target: &str,
        update: F,
    ) -> Result<(), RuntimeError>
    where
        F: FnOnce(Decimal, Decimal) -> Decimal,
    {
        let current = self.get(target)?.decimal()?;
        let next = update(current, source.decimal()?);
        self.move_value(CobolValue::Decimal(next), target)
    }

    fn layout(&self, name: &str) -> Result<&CobolFieldLayout, RuntimeError> {
        self.layouts
            .get(name)
            .ok_or_else(|| RuntimeError::UnknownField {
                name: name.to_string(),
            })
    }

    fn field_bytes<'a>(&'a self, layout: &CobolFieldLayout) -> Result<&'a [u8], RuntimeError> {
        let end = layout.offset.saturating_add(layout.byte_len);
        self.bytes
            .get(layout.offset..end)
            .ok_or_else(|| RuntimeError::FieldOutOfBounds {
                name: layout.name.clone(),
                offset: layout.offset,
                end,
                len: self.bytes.len(),
            })
    }
}

impl CobolStorage {
    pub fn define(&mut self, name: impl Into<String>, value: CobolValue) {
        self.values.insert(name.into(), value);
    }

    pub fn get(&self, name: &str) -> Result<&CobolValue, RuntimeError> {
        self.values
            .get(name)
            .ok_or_else(|| RuntimeError::UnknownField {
                name: name.to_string(),
            })
    }

    pub fn move_value(&mut self, source: CobolValue, target: &str) -> Result<(), RuntimeError> {
        if !self.values.contains_key(target) {
            return Err(RuntimeError::UnknownField {
                name: target.to_string(),
            });
        }
        self.values.insert(target.to_string(), source);
        Ok(())
    }

    pub fn add(&mut self, source: CobolValue, target: &str) -> Result<(), RuntimeError> {
        let current = self.get(target)?.decimal()?;
        let next = current + source.decimal()?;
        self.values
            .insert(target.to_string(), CobolValue::Decimal(next));
        Ok(())
    }

    pub fn subtract(&mut self, source: CobolValue, target: &str) -> Result<(), RuntimeError> {
        let current = self.get(target)?.decimal()?;
        let next = current - source.decimal()?;
        self.values
            .insert(target.to_string(), CobolValue::Decimal(next));
        Ok(())
    }

    pub fn multiply(&mut self, source: CobolValue, target: &str) -> Result<(), RuntimeError> {
        let current = self.get(target)?.decimal()?;
        let next = current * source.decimal()?;
        self.values
            .insert(target.to_string(), CobolValue::Decimal(next));
        Ok(())
    }

    pub fn divide(&mut self, source: CobolValue, target: &str) -> Result<(), RuntimeError> {
        let divisor = source.decimal()?;
        if divisor.is_zero() {
            return Err(RuntimeError::DivisionByZero);
        }
        let current = self.get(target)?.decimal()?;
        let next = current / divisor;
        self.values
            .insert(target.to_string(), CobolValue::Decimal(next));
        Ok(())
    }
}

pub trait CobolFileSystem {
    fn open(&mut self, operation: &str) -> Result<(), RuntimeError>;
    fn read(&mut self, operation: &str) -> Result<(), RuntimeError>;
    fn write(&mut self, operation: &str) -> Result<(), RuntimeError>;
    fn close(&mut self, operation: &str) -> Result<(), RuntimeError>;
}

#[derive(Debug, Default)]
pub struct UnboundFileSystem;

impl CobolFileSystem for UnboundFileSystem {
    fn open(&mut self, operation: &str) -> Result<(), RuntimeError> {
        Err(RuntimeError::UnboundFileOperation {
            operation: operation.to_string(),
        })
    }

    fn read(&mut self, operation: &str) -> Result<(), RuntimeError> {
        Err(RuntimeError::UnboundFileOperation {
            operation: operation.to_string(),
        })
    }

    fn write(&mut self, operation: &str) -> Result<(), RuntimeError> {
        Err(RuntimeError::UnboundFileOperation {
            operation: operation.to_string(),
        })
    }

    fn close(&mut self, operation: &str) -> Result<(), RuntimeError> {
        Err(RuntimeError::UnboundFileOperation {
            operation: operation.to_string(),
        })
    }
}
