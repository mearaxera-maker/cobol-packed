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
