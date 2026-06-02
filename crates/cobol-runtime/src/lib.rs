use rust_decimal::Decimal;
use std::collections::BTreeMap;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq)]
pub enum CobolValue {
    Text(String),
    Decimal(Decimal),
    Bytes(Vec<u8>),
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

impl Default for CobolValue {
    fn default() -> Self {
        Self::Null
    }
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
