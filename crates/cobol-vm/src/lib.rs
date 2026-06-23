use cobol_dialect::{
    CollatingSequence, DialectName, DialectProfile, FloatFormat, InvalidNumericPolicy, Numproc,
};
use cobol_record::{
    decode_binary_integer, decode_ibm_float32, decode_ibm_float64, decode_ieee_float32,
    decode_ieee_float64, decode_packed_decimal, encode_binary_integer, encode_ibm_float32,
    encode_ibm_float64, encode_ieee_float32, encode_ieee_float64, encode_packed_decimal,
    DecodedValue, Endian,
};
use rust_decimal::{Decimal, RoundingStrategy};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::str::FromStr;

const TAPE_IMAGE_MAGIC: &[u8] = b"COBOLVM-TAPE-1\n";
const TAPE_DATA_RECORD: u8 = b'D';
const CHECKPOINT_MAGIC: &str = "COBOLVMCKPT1";
const PROGRAM_STATUS_REGISTER: &str = "PROGRAM_STATUS";
const PROGRAM_STATUS_DISPLAY_NAME: &str = "PROGRAM-STATUS";
const TALLY_REGISTER: &str = "TALLY";
const DEBUG_ITEM_REGISTER: &str = "DEBUG_ITEM";
const DEBUG_ITEM_DISPLAY_NAME: &str = "DEBUG-ITEM";
const DEBUG_CONTENTS_REGISTER: &str = "DEBUG_CONTENTS";
const DEBUG_CONTENTS_DISPLAY_NAME: &str = "DEBUG-CONTENTS";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmCategory {
    Group,
    Alphanumeric,
    Alphabetic,
    National,
    Dbcs,
    NumericDisplay,
    NumericEdited,
    PackedDecimal,
    Binary,
    NativeBinary,
    Float,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmUsage {
    Display,
    PackedDecimal,
    Binary,
    NativeBinary,
    Float32,
    Float64,
    National,
    Dbcs,
    Group,
    Alphanumeric,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmPicture {
    pub signed: bool,
    pub digits: usize,
    pub scale: u32,
    pub char_len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmField {
    pub name: String,
    pub offset: usize,
    pub byte_len: usize,
    pub category: VmCategory,
    pub usage: VmUsage,
    pub picture: Option<VmPicture>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmConditionValue {
    Single(String),
    Range { start: String, end: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmConditionName {
    pub name: String,
    pub parent: String,
    pub values: Vec<VmConditionValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmDeclaredView {
    pub condition: String,
    pub parent: String,
    pub children: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct VmProgram {
    dialect: DialectProfile,
    fields: Vec<VmField>,
    conditions: Vec<VmConditionName>,
    condition_views: BTreeMap<String, VmDeclaredView>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum VmValue {
    Decimal(Decimal),
    Integer(i64),
    UnsignedInteger(u64),
    Float(f64),
    Text(String),
    NationalText(String),
    DbcsText(Vec<u8>),
    Bytes(Vec<u8>),
    Bool(bool),
    Null,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VmEvaluatedValue {
    pub value: VmValue,
    pub category: VmCategory,
    pub byte_len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmExpr {
    Access(VmAccessPath),
    Identifier(String),
    Index(String),
    Literal(String),
    Number(String),
    Figurative(VmFigurative),
    AllLiteral(String),
    Function {
        function: VmFunction,
        args: Vec<VmExpr>,
    },
    Condition(Box<VmCondition>),
    Add(Box<VmExpr>, Box<VmExpr>),
    Subtract(Box<VmExpr>, Box<VmExpr>),
    Multiply(Box<VmExpr>, Box<VmExpr>),
    Divide(Box<VmExpr>, Box<VmExpr>),
    Bool(bool),
}

pub type VmOperand = VmExpr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmAccessPath {
    pub target: String,
    pub condition_name: Option<String>,
    pub subscripts: Vec<VmSubscript>,
    pub reference_modifier: Option<VmReferenceModifier>,
    pub result_len: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StorageKey {
    pub program: String,
    pub item: String,
    pub occurrence: Vec<usize>,
}

impl StorageKey {
    pub const EXTERNAL_PROGRAM: &'static str = "__EXTERNAL__";
    pub const SPECIAL_PROGRAM: &'static str = "__SPECIAL__";

    pub fn new(
        program: impl Into<String>,
        item: impl Into<String>,
        occurrence: Vec<usize>,
    ) -> Self {
        Self {
            program: program.into(),
            item: item.into(),
            occurrence,
        }
    }

    pub fn scalar(program: impl Into<String>, item: impl Into<String>) -> Self {
        Self::new(program, item, Vec::new())
    }

    pub fn occurrence(
        program: impl Into<String>,
        item: impl Into<String>,
        occurrence: Vec<usize>,
    ) -> Self {
        Self::new(program, item, occurrence)
    }

    pub fn external(item: impl Into<String>) -> Self {
        Self::new(Self::EXTERNAL_PROGRAM, item, Vec::new())
    }

    pub fn external_occurrence(item: impl Into<String>, occurrence: Vec<usize>) -> Self {
        Self::new(Self::EXTERNAL_PROGRAM, item, occurrence)
    }

    pub fn special(item: impl Into<String>) -> Self {
        Self::new(Self::SPECIAL_PROGRAM, item, Vec::new())
    }
}

impl fmt::Display for StorageKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.occurrence.is_empty() {
            write!(f, "{}::{}", self.program, self.item)
        } else {
            let subscripts = self
                .occurrence
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(",");
            write!(f, "{}::{}({})", self.program, self.item, subscripts)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageCell {
    bytes: Vec<u8>,
    generation: u64,
}

impl StorageCell {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            generation: 0,
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    fn write_exact(&mut self, bytes: &[u8]) -> Result<(), VmError> {
        if bytes.len() != self.bytes.len() {
            return Err(VmError::StoragePool {
                key: "<cell>".to_string(),
                message: format!(
                    "write length {} does not match cell length {}",
                    bytes.len(),
                    self.bytes.len()
                ),
            });
        }
        self.bytes.copy_from_slice(bytes);
        self.generation = self.generation.saturating_add(1);
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OdoCellDescriptor {
    pub program: String,
    pub table: String,
    pub depending_on: StorageKey,
    pub element_len: usize,
    pub min: usize,
    pub max: usize,
    pub active: usize,
    pub generation: u64,
    pub templates: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoragePool {
    cells: BTreeMap<StorageKey, StorageCell>,
    odo_tables: BTreeMap<(String, String), OdoCellDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmBinding {
    Cell {
        key: StorageKey,
    },
    Slice {
        key: StorageKey,
        offset: usize,
        len: usize,
    },
    OccursCell {
        program: String,
        item: String,
    },
    Group {
        children: Vec<String>,
    },
}

impl StoragePool {
    pub fn define_cell(&mut self, key: StorageKey, bytes: Vec<u8>) -> Result<(), VmError> {
        if self.cells.contains_key(&key) {
            return Err(VmError::StoragePool {
                key: key.to_string(),
                message: "cell is already defined".to_string(),
            });
        }
        self.cells.insert(key, StorageCell::new(bytes));
        Ok(())
    }

    pub fn cell(&self, key: &StorageKey) -> Result<&StorageCell, VmError> {
        self.cells.get(key).ok_or_else(|| VmError::StoragePool {
            key: key.to_string(),
            message: "cell is not defined".to_string(),
        })
    }

    pub fn bytes(&self, key: &StorageKey) -> Result<&[u8], VmError> {
        self.cell(key).map(StorageCell::bytes)
    }

    pub fn write_cell(&mut self, key: &StorageKey, bytes: &[u8]) -> Result<(), VmError> {
        let cell = self
            .cells
            .get_mut(key)
            .ok_or_else(|| VmError::StoragePool {
                key: key.to_string(),
                message: "cell is not defined".to_string(),
            })?;
        cell.write_exact(bytes).map_err(|err| match err {
            VmError::StoragePool { message, .. } => VmError::StoragePool {
                key: key.to_string(),
                message,
            },
            other => other,
        })
    }

    pub fn define_or_write_cell(&mut self, key: StorageKey, bytes: Vec<u8>) -> Result<(), VmError> {
        if self.cells.contains_key(&key) {
            self.write_cell(&key, &bytes)
        } else {
            self.define_cell(key, bytes)
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn define_odo_table(
        &mut self,
        program: impl Into<String>,
        table: impl Into<String>,
        depending_on: StorageKey,
        element_len: usize,
        min: usize,
        max: usize,
        active: usize,
    ) -> Result<(), VmError> {
        let table = table.into();
        let mut templates = BTreeMap::new();
        templates.insert(table.clone(), vec![b' '; element_len]);
        self.define_odo_table_with_templates(
            program,
            table,
            depending_on,
            element_len,
            min,
            max,
            active,
            templates,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn define_odo_table_with_templates(
        &mut self,
        program: impl Into<String>,
        table: impl Into<String>,
        depending_on: StorageKey,
        element_len: usize,
        min: usize,
        max: usize,
        active: usize,
        templates: BTreeMap<String, Vec<u8>>,
    ) -> Result<(), VmError> {
        let program = program.into();
        let table = table.into();
        if element_len == 0 {
            return Err(VmError::OdoRuntime {
                table,
                message: "ODO element length must be greater than zero".to_string(),
            });
        }
        if min > max {
            return Err(VmError::OdoRuntime {
                table,
                message: format!("ODO min {min} is greater than max {max}"),
            });
        }
        if active < min || active > max {
            return Err(VmError::OdoRuntime {
                table,
                message: format!("active count {active} is outside {min}..={max}"),
            });
        }
        let table_key = (program.clone(), table.clone());
        if self.odo_tables.contains_key(&table_key) {
            return Err(VmError::OdoRuntime {
                table,
                message: "ODO table is already defined".to_string(),
            });
        }
        if !self.cells.contains_key(&depending_on) {
            return Err(VmError::StoragePool {
                key: depending_on.to_string(),
                message: "DEPENDING ON cell is not defined".to_string(),
            });
        }
        self.odo_tables.insert(
            table_key,
            OdoCellDescriptor {
                program: program.clone(),
                table: table.clone(),
                depending_on,
                element_len,
                min,
                max,
                active: 0,
                generation: 0,
                templates,
            },
        );
        self.resize_odo_table(&program, &table, active)
    }

    pub fn odo_descriptor(
        &self,
        program: &str,
        table: &str,
    ) -> Result<&OdoCellDescriptor, VmError> {
        self.odo_tables
            .get(&(program.to_string(), table.to_string()))
            .ok_or_else(|| VmError::OdoRuntime {
                table: table.to_string(),
                message: "ODO descriptor is not defined".to_string(),
            })
    }

    pub fn active_count_for_occurs_item(&self, program: &str, item: &str) -> Option<usize> {
        self.odo_tables
            .iter()
            .find(|((descriptor_program, descriptor_table), descriptor)| {
                descriptor_program.eq_ignore_ascii_case(program)
                    && (descriptor_table.eq_ignore_ascii_case(item)
                        || descriptor
                            .templates
                            .keys()
                            .any(|template_item| template_item.eq_ignore_ascii_case(item)))
            })
            .map(|(_, descriptor)| descriptor.active)
    }

    pub fn resize_odo_tables_for_name(
        &mut self,
        table: &str,
        active: usize,
    ) -> Result<bool, VmError> {
        let keys = self
            .odo_tables
            .keys()
            .filter(|(_, descriptor_table)| descriptor_table.eq_ignore_ascii_case(table))
            .cloned()
            .collect::<Vec<_>>();
        let resized = !keys.is_empty();
        for (program, descriptor_table) in keys {
            self.resize_odo_table(&program, &descriptor_table, active)?;
        }
        Ok(resized)
    }

    pub fn resize_odo_table(
        &mut self,
        program: &str,
        table: &str,
        active: usize,
    ) -> Result<(), VmError> {
        let table_key = (program.to_string(), table.to_string());
        let descriptor =
            self.odo_tables
                .get_mut(&table_key)
                .ok_or_else(|| VmError::OdoRuntime {
                    table: table.to_string(),
                    message: "ODO descriptor is not defined".to_string(),
                })?;
        if active < descriptor.min || active > descriptor.max {
            return Err(VmError::OdoRuntime {
                table: table.to_string(),
                message: format!(
                    "active count {active} is outside {}..={}",
                    descriptor.min, descriptor.max
                ),
            });
        }

        let previous = descriptor.active;
        if active > previous {
            for occurrence in previous.saturating_add(1)..=active {
                for (item, template) in &descriptor.templates {
                    let key = StorageKey::occurrence(program, item, vec![occurrence]);
                    self.cells
                        .entry(key)
                        .or_insert_with(|| StorageCell::new(template.clone()));
                }
            }
        } else if active < previous {
            for occurrence in active.saturating_add(1)..=previous {
                for item in descriptor.templates.keys() {
                    let key = StorageKey::occurrence(program, item, vec![occurrence]);
                    self.cells.remove(&key);
                }
            }
        }
        descriptor.active = active;
        if active != previous {
            descriptor.generation = descriptor.generation.saturating_add(1);
        }
        Ok(())
    }

    pub fn occurrence_cell(
        &self,
        program: &str,
        table: &str,
        occurrence: usize,
    ) -> Result<&StorageCell, VmError> {
        let descriptor = self.odo_descriptor(program, table)?;
        if occurrence < 1 || occurrence > descriptor.active {
            return Err(VmError::InvalidSubscript {
                target: table.to_string(),
                value: occurrence as i128,
                min: 1,
                max: descriptor.active,
            });
        }
        let key = StorageKey::occurrence(program, table, vec![occurrence]);
        self.cell(&key)
    }

    pub fn occurrence_bytes(
        &self,
        program: &str,
        table: &str,
        occurrence: usize,
    ) -> Result<&[u8], VmError> {
        self.occurrence_cell(program, table, occurrence)
            .map(StorageCell::bytes)
    }

    pub fn write_occurrence(
        &mut self,
        program: &str,
        table: &str,
        occurrence: usize,
        bytes: &[u8],
    ) -> Result<(), VmError> {
        self.occurrence_cell(program, table, occurrence)?;
        let key = StorageKey::occurrence(program, table, vec![occurrence]);
        self.write_cell(&key, bytes)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmIndexState {
    pub name: String,
    pub table: String,
    pub occurrence: Option<usize>,
    pub min: usize,
    pub max: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmOdoState {
    pub program: Option<String>,
    pub table: String,
    pub depending_on: String,
    pub active: usize,
    pub min: usize,
    pub max: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmProcedure {
    pub entry: String,
    pub blocks: Vec<VmBasicBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmBasicBlock {
    pub name: String,
    pub ops: Vec<VmProcedureOp>,
    pub transfer: VmControlTransfer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmVaryingTarget {
    Access(VmAccessPath),
    Index(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmPerformVarying {
    pub target: VmVaryingTarget,
    pub from: VmExpr,
    pub by: VmExpr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmProcedureRange {
    pub target: String,
    pub through: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmCallTarget {
    Literal(String),
    Dynamic(VmExpr),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmInspectTally {
    pub target: VmAccessPath,
    pub pattern: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmInspectReplacing {
    pub pattern: String,
    pub replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmInspectConverting {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmStringPiece {
    pub source: VmExpr,
    pub delimiter: VmStringDelimiter,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmStringDelimiter {
    Size,
    Literal { value: String, all: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmUnstringTarget {
    pub target: VmAccessPath,
    pub count: Option<VmAccessPath>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmSourceSpan {
    pub file: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmProcedureOp {
    SetSourceSpan(VmSourceSpan),
    Display(Vec<VmExpr>),
    Move {
        source: VmExpr,
        target: VmAccessPath,
    },
    Add {
        source: VmExpr,
        target: VmAccessPath,
    },
    Subtract {
        source: VmExpr,
        target: VmAccessPath,
    },
    Multiply {
        source: VmExpr,
        target: VmAccessPath,
    },
    Divide {
        source: VmExpr,
        target: VmAccessPath,
    },
    Compute {
        target: VmAccessPath,
        expr: VmExpr,
        rounded: bool,
        on_size_error_ops: Vec<VmProcedureOp>,
        not_on_size_error_ops: Vec<VmProcedureOp>,
    },
    If {
        condition: VmCondition,
        then_ops: Vec<VmProcedureOp>,
        else_ops: Vec<VmProcedureOp>,
    },
    Evaluate {
        evaluate: VmEvaluate,
        branches: Vec<Vec<VmProcedureOp>>,
    },
    SetConditionName {
        name: String,
    },
    Perform {
        target: String,
        through: Option<String>,
        times: Option<VmExpr>,
    },
    DynamicPerform {
        target: VmExpr,
    },
    PerformLoop {
        target: String,
        through: Option<String>,
        varying: Option<VmPerformVarying>,
        until: Option<VmCondition>,
    },
    GoTo {
        target: String,
    },
    ComputedGoTo {
        targets: Vec<String>,
        depending_on: VmExpr,
    },
    Alter {
        paragraph: String,
        target: String,
    },
    Call {
        target: VmCallTarget,
        using: Vec<VmAccessPath>,
    },
    StopRun,
    OpenFile {
        name: String,
        mode: VmOpenMode,
    },
    ReadFile {
        name: String,
        target: VmAccessPath,
        at_end_ops: Vec<VmProcedureOp>,
        not_at_end_ops: Vec<VmProcedureOp>,
        on_exception_ops: Vec<VmProcedureOp>,
    },
    WriteFile {
        name: String,
        source: VmAccessPath,
        advancing: VmWriteAdvancing,
    },
    RewriteFile {
        name: String,
        source: VmAccessPath,
        invalid_key_ops: Vec<VmProcedureOp>,
        not_invalid_key_ops: Vec<VmProcedureOp>,
    },
    DeleteFile {
        name: String,
        invalid_key_ops: Vec<VmProcedureOp>,
        not_invalid_key_ops: Vec<VmProcedureOp>,
    },
    CloseFile {
        name: String,
    },
    SortProcedure {
        file: String,
        record: VmAccessPath,
        key: Option<VmSortKeyDescriptor>,
        input: Option<VmProcedureRange>,
        output: VmProcedureRange,
    },
    ReleaseSortRecord {
        record: VmAccessPath,
        source: Option<VmAccessPath>,
    },
    ReturnSortRecord {
        file: String,
        record: VmAccessPath,
        target: Option<VmAccessPath>,
        at_end_ops: Vec<VmProcedureOp>,
        not_at_end_ops: Vec<VmProcedureOp>,
    },
    InspectLike {
        subject: VmAccessPath,
        tally: Option<VmInspectTally>,
        replacing: Option<VmInspectReplacing>,
        converting: Option<VmInspectConverting>,
    },
    StringOp {
        pieces: Vec<VmStringPiece>,
        target: VmAccessPath,
        pointer: Option<VmAccessPath>,
        on_overflow_ops: Vec<VmProcedureOp>,
        not_on_overflow_ops: Vec<VmProcedureOp>,
    },
    UnstringOp {
        source: VmExpr,
        delimiter: VmStringDelimiter,
        targets: Vec<VmUnstringTarget>,
        pointer: Option<VmAccessPath>,
        tallying: Option<VmAccessPath>,
        on_overflow_ops: Vec<VmProcedureOp>,
        not_on_overflow_ops: Vec<VmProcedureOp>,
    },
    SetIndex {
        name: String,
        operation: VmSetIndexOperation,
    },
    SearchSerial {
        table: String,
        index_name: String,
        min: usize,
        max: usize,
        whens: Vec<VmSearchWhen>,
        at_end_ops: Vec<VmProcedureOp>,
    },
    SearchAll {
        table: String,
        index_name: String,
        min: usize,
        max: usize,
        direction: VmSearchDirection,
        key: VmExpr,
        target: VmExpr,
        found_ops: Vec<VmProcedureOp>,
        at_end_ops: Vec<VmProcedureOp>,
    },
    SetOdo {
        table: String,
        active: VmExpr,
    },
    TraceOn,
    TraceOff,
    UnsupportedTrap {
        message: String,
    },
    Noop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmSetIndexOperation {
    To(VmExpr),
    UpBy(VmExpr),
    DownBy(VmExpr),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmControlTransfer {
    FallThrough(Option<String>),
    Perform {
        target: String,
        through: Option<String>,
        times: Option<VmExpr>,
    },
    GoTo(String),
    AlteredGoTo {
        slot: String,
    },
    StopRun,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum VmProcedureSignal {
    Continue,
    GoTo(String),
    StopRun,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmFrame {
    pub program: String,
    pub current: String,
    pub return_to: Option<String>,
    pub source_span: Option<VmSourceSpan>,
    pub local_bindings: BTreeMap<String, VmBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmPerformFrame {
    pub target: String,
    pub through: String,
    pub return_to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmSearch {
    pub table: String,
    pub index_name: String,
    pub min: usize,
    pub max: usize,
    pub condition: VmCondition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmSearchWhen {
    pub condition: VmCondition,
    pub ops: Vec<VmProcedureOp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmSearchDirection {
    Ascending,
    Descending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmSortDirection {
    Ascending,
    Descending,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmSortKeyDescriptor {
    pub offset: usize,
    pub byte_len: usize,
    pub direction: VmSortDirection,
    pub encoding: VmSortKeyEncoding,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmSortKeyEncoding {
    Bytes,
    NumericDisplay {
        digits: usize,
        scale: u32,
        signed: bool,
    },
    PackedDecimal {
        digits: usize,
        scale: u32,
        signed: bool,
    },
}

impl VmSortKeyEncoding {
    fn checkpoint_parts(&self) -> (VmCategory, VmUsage, usize, u32, bool) {
        match self {
            VmSortKeyEncoding::Bytes => (VmCategory::Alphanumeric, VmUsage::Display, 0, 0, false),
            VmSortKeyEncoding::NumericDisplay {
                digits,
                scale,
                signed,
            } => (
                VmCategory::NumericDisplay,
                VmUsage::Display,
                *digits,
                *scale,
                *signed,
            ),
            VmSortKeyEncoding::PackedDecimal {
                digits,
                scale,
                signed,
            } => (
                VmCategory::PackedDecimal,
                VmUsage::PackedDecimal,
                *digits,
                *scale,
                *signed,
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmSortPhase {
    Input,
    Sorting,
    Output,
    Done,
}

impl VmSortPhase {
    fn label(self) -> &'static str {
        match self {
            VmSortPhase::Input => "Input",
            VmSortPhase::Sorting => "Sorting",
            VmSortPhase::Output => "Output",
            VmSortPhase::Done => "Done",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmSortState {
    pub file: String,
    pub phase: VmSortPhase,
    pub record: VmAccessPath,
    pub record_len: usize,
    pub released_records: Vec<Vec<u8>>,
    pub sorted_records: Vec<Vec<u8>>,
    pub cursor: usize,
    pub key: Option<VmSortKeyDescriptor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmWriteAdvancing {
    None,
    BeforeLines(usize),
    AfterLines(usize),
    BeforePage,
    AfterPage,
}

pub trait VmFileHandler {
    fn open(&mut self, name: &str, mode: VmOpenMode) -> Result<(), VmError>;
    fn read(&mut self, name: &str, record_len: usize) -> Result<Option<Vec<u8>>, VmError>;
    fn write(&mut self, name: &str, record: &[u8]) -> Result<(), VmError>;
    fn rewrite(&mut self, name: &str, record: &[u8]) -> Result<(), VmError>;
    fn delete(&mut self, name: &str) -> Result<(), VmError>;
    fn close(&mut self, name: &str) -> Result<(), VmError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmOpenMode {
    Input,
    Output,
    Io,
    Extend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmFileOrganization {
    Sequential,
}

#[derive(Debug)]
pub enum VmFileBacking {
    Memory {
        records: Vec<Vec<u8>>,
    },
    OsSequential {
        path: PathBuf,
        handle: Option<File>,
    },
    Tape {
        path: PathBuf,
        records: Vec<Vec<u8>>,
    },
}

impl VmFileBacking {
    fn clone_for_checkpoint_restore(&self) -> Self {
        match self {
            VmFileBacking::Memory { records } => VmFileBacking::Memory {
                records: records.clone(),
            },
            VmFileBacking::OsSequential { path, .. } => VmFileBacking::OsSequential {
                path: path.clone(),
                handle: None,
            },
            VmFileBacking::Tape { path, records } => VmFileBacking::Tape {
                path: path.clone(),
                records: records.clone(),
            },
        }
    }
}

#[derive(Debug)]
pub struct VmFile {
    pub name: String,
    pub organization: VmFileOrganization,
    pub backing: VmFileBacking,
    pub platform_disposition: Option<cobol_platform::FileDisposition>,
    pub cursor: usize,
    pub open_mode: Option<VmOpenMode>,
    pub last_status: Option<String>,
    pub last_record_index: Option<usize>,
    pub last_record_len: Option<usize>,
    pub fixed_record_len: Option<usize>,
    pub linage: Option<usize>,
    pub current_line: usize,
}

impl VmFile {
    fn clone_for_checkpoint_restore(&self) -> Self {
        Self {
            name: self.name.clone(),
            organization: self.organization,
            backing: self.backing.clone_for_checkpoint_restore(),
            platform_disposition: self.platform_disposition,
            cursor: self.cursor,
            open_mode: self.open_mode,
            last_status: self.last_status.clone(),
            last_record_index: self.last_record_index,
            last_record_len: self.last_record_len,
            fixed_record_len: self.fixed_record_len,
            linage: self.linage,
            current_line: self.current_line,
        }
    }
}

#[derive(Debug, Default)]
pub struct VmFileRuntime {
    files: BTreeMap<String, VmFile>,
    aliases: BTreeMap<String, String>,
}

impl VmFileRuntime {
    fn clone_for_checkpoint_restore(&self) -> Self {
        Self {
            files: self
                .files
                .iter()
                .map(|(name, file)| (name.clone(), file.clone_for_checkpoint_restore()))
                .collect(),
            aliases: self.aliases.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmRegisteredProgram {
    pub procedure: VmProcedure,
    pub linkage: Vec<VmLinkageParam>,
    pub is_initial: bool,
    pub initial_cells: Vec<(StorageKey, Vec<u8>)>,
    pub initial_odo: Vec<VmOdoInitialState>,
    pub initial_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmLinkageParam {
    pub name: String,
    pub children: Vec<VmLinkageChild>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmLinkageChild {
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmOdoInitialState {
    pub program: String,
    pub table: String,
    pub active: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VmProgramRegistry {
    programs: BTreeMap<String, VmRegisteredProgram>,
}

#[derive(Debug)]
pub struct VmRuntime {
    pub program: VmProgram,
    pub dialect: DialectProfile,
    pub storage_pool: StoragePool,
    pub storage_descriptors: BTreeMap<String, VmBinding>,
    pub indexes: BTreeMap<String, VmIndexState>,
    pub odo: BTreeMap<String, VmOdoState>,
    pub files: VmFileRuntime,
    pub file_status: BTreeMap<String, VmAccessPath>,
    pub registry: VmProgramRegistry,
    pub activation_stack: Vec<VmFrame>,
    last_abend_frame: Option<VmFrame>,
    pub alter_table: BTreeMap<String, String>,
    pub rerun_checkpoints: Vec<VmRerunCheckpoint>,
    pub file_error_declaratives: BTreeMap<String, Vec<VmProcedureOp>>,
    active_file_error_declaratives: BTreeSet<String>,
    pub debugging_declaratives: BTreeMap<String, Vec<VmProcedureOp>>,
    active_debugging_declaratives: BTreeSet<String>,
    trace_enabled: bool,
    checkpoint_in_progress: bool,
    pub sort_states: Vec<VmSortState>,
    pub display: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmRerunCheckpoint {
    pub checkpoint_file: String,
    pub watched_file: String,
    pub watched_key: String,
    pub every_records: usize,
    pub record_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmSubscript {
    pub expr: Box<VmExpr>,
    pub stride: usize,
    pub min: usize,
    pub max: usize,
    pub depending_on: Option<String>,
    pub index_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmReferenceModifier {
    pub start: Box<VmExpr>,
    pub length: Option<Box<VmExpr>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmFunction {
    Length,
    Ord,
    Numval,
    UserDefined,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmFigurative {
    Zero,
    Space,
    HighValue,
    LowValue,
    Quote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmRelOp {
    Equal,
    NotEqual,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmClassTest {
    Numeric,
    Alphabetic,
    AlphabeticUpper,
    AlphabeticLower,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmSignTest {
    Positive,
    Negative,
    Zero,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmCondition {
    Relation {
        left: VmOperand,
        op: VmRelOp,
        right: VmOperand,
    },
    ClassTest {
        operand: VmOperand,
        class: VmClassTest,
        negated: bool,
    },
    SignTest {
        operand: VmOperand,
        sign: VmSignTest,
        negated: bool,
    },
    ConditionName {
        reference: String,
    },
    Not(Box<VmCondition>),
    And(Box<VmCondition>, Box<VmCondition>),
    Or(Box<VmCondition>, Box<VmCondition>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VmEvaluatePattern {
    Any,
    Operand(VmOperand),
    Range { start: VmOperand, end: VmOperand },
    Condition(VmCondition),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmEvaluate {
    pub subjects: Vec<VmExpr>,
    pub branches: Vec<VmBranch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VmBranch {
    pub patterns: Vec<VmEvaluatePattern>,
}

#[derive(Debug, thiserror::Error)]
pub enum VmError {
    #[error("unknown COBOL data reference {name}")]
    UnknownReference { name: String },
    #[error("unknown COBOL condition name {name}")]
    UnknownConditionName { name: String },
    #[error("ambiguous COBOL condition name {name}; candidates: {candidates}")]
    AmbiguousConditionName { name: String, candidates: String },
    #[error("field {name} range {offset}..{end} exceeds storage length {len}")]
    FieldOutOfBounds {
        name: String,
        offset: usize,
        end: usize,
        len: usize,
    },
    #[error("COBOL codec error for {name}: {message}")]
    Codec { name: String, message: String },
    #[error("invalid decimal value {value}")]
    InvalidDecimal { value: String },
    #[error("unsupported condition operand {message}")]
    UnsupportedOperand { message: String },
    #[error("unsupported condition comparison between {left:?} and {right:?}")]
    UnsupportedComparison { left: VmCategory, right: VmCategory },
    #[error("unsupported dialect behavior: {message}")]
    UnsupportedDialect { message: String },
    #[error("invalid COBOL subscript {value} for {target}; expected {min}..={max}")]
    InvalidSubscript {
        target: String,
        value: i128,
        min: usize,
        max: usize,
    },
    #[error("invalid reference modification for {target}: {message}")]
    InvalidReferenceModification { target: String, message: String },
    #[error("unsupported COBOL function operand {name}")]
    UnsupportedFunction { name: String },
    #[error("unsupported or uninitialized index item {name}")]
    UnsupportedIndex { name: String },
    #[error("COBOL procedure runtime error in {block}: {message}")]
    ProcedureRuntime { block: String, message: String },
    #[error("COBOL ODO runtime error for {table}: {message}")]
    OdoRuntime { table: String, message: String },
    #[error("COBOL file runtime error for {name}: {message}")]
    FileRuntime { name: String, message: String },
    #[error("COBOL storage pool error for {key}: {message}")]
    StoragePool { key: String, message: String },
    #[error("unsupported nested program behavior: {message}")]
    NestedProgramRuntime { message: String },
}

impl VmError {
    pub fn code(&self) -> &'static str {
        match self {
            VmError::UnknownReference { .. } => "CBL-RT-REFERENCE",
            VmError::UnknownConditionName { .. } => "CBL-RT-CONDITION",
            VmError::AmbiguousConditionName { .. } => "CBL-RT-CONDITION",
            VmError::FieldOutOfBounds { .. } => "CBL-RT-FIELD",
            VmError::Codec { .. } => "CBL-RT-CODEC",
            VmError::InvalidDecimal { .. } => "CBL-RT-DECIMAL",
            VmError::UnsupportedOperand { .. } => "CBL-RT-UNSUPPORTED-OPERAND",
            VmError::UnsupportedComparison { .. } => "CBL-RT-UNSUPPORTED-COMPARISON",
            VmError::UnsupportedDialect { .. } => "CBL-RT-UNSUPPORTED-DIALECT",
            VmError::InvalidSubscript { .. } => "CBL-RT-SUBSCRIPT",
            VmError::InvalidReferenceModification { .. } => "CBL-RT-REFERENCE-MODIFICATION",
            VmError::UnsupportedFunction { .. } => "CBL-RT-UNSUPPORTED-FUNCTION",
            VmError::UnsupportedIndex { .. } => "CBL-RT-INDEX",
            VmError::ProcedureRuntime { .. } => "CBL-RT-PROCEDURE",
            VmError::OdoRuntime { .. } => "CBL-RT-ODO",
            VmError::FileRuntime { .. } => "CBL-RT-FILE",
            VmError::StoragePool { .. } => "CBL-RT-STORAGE",
            VmError::NestedProgramRuntime { .. } => "CBL-RT-NESTED-PROGRAM",
        }
    }
}

impl VmFileRuntime {
    pub fn apply_platform_config(
        &mut self,
        config: &cobol_platform::PlatformConfig,
    ) -> Result<(), VmError> {
        config.validate().map_err(platform_error_to_vm)?;
        for binding in &config.files {
            let record_len = match &binding.record_format {
                cobol_platform::RecordFormat::Fixed { record_len } => *record_len,
                cobol_platform::RecordFormat::Variable
                | cobol_platform::RecordFormat::LineSequential => {
                    return Err(VmError::FileRuntime {
                        name: binding.name.clone(),
                        message: format!(
                            "unsupported runtime platform record format {:?}",
                            binding.record_format
                        ),
                    });
                }
            };
            match binding.organization {
                cobol_platform::DatasetOrganization::Sequential => {}
                cobol_platform::DatasetOrganization::Indexed
                | cobol_platform::DatasetOrganization::Relative
                | cobol_platform::DatasetOrganization::Vsam => {
                    return Err(VmError::FileRuntime {
                        name: binding.name.clone(),
                        message: format!(
                            "unsupported runtime platform organization {:?}",
                            binding.organization
                        ),
                    });
                }
            }
            self.define_os_sequential_file_with_record_len(
                binding.name.clone(),
                binding.path.clone(),
                record_len,
            );
            let key = self.file_key(&binding.name);
            if let Some(file) = self.files.get_mut(&key) {
                file.platform_disposition = Some(binding.disposition);
            }
        }
        Ok(())
    }

    pub fn define_file(&mut self, name: impl Into<String>, records: Vec<Vec<u8>>) {
        let name = name.into();
        self.aliases.insert(normalize_vm_key(&name), name.clone());
        self.files.insert(
            name.clone(),
            VmFile {
                name,
                organization: VmFileOrganization::Sequential,
                backing: VmFileBacking::Memory { records },
                platform_disposition: None,
                cursor: 0,
                open_mode: None,
                last_status: None,
                last_record_index: None,
                last_record_len: None,
                fixed_record_len: None,
                linage: None,
                current_line: 1,
            },
        );
    }

    pub fn define_os_sequential_file(&mut self, name: impl Into<String>, path: impl Into<PathBuf>) {
        self.define_os_sequential_file_with_optional_record_len(name, path, None);
    }

    pub fn define_os_sequential_file_with_record_len(
        &mut self,
        name: impl Into<String>,
        path: impl Into<PathBuf>,
        record_len: usize,
    ) {
        self.define_os_sequential_file_with_optional_record_len(
            name,
            path,
            (record_len > 0).then_some(record_len),
        );
    }

    fn define_os_sequential_file_with_optional_record_len(
        &mut self,
        name: impl Into<String>,
        path: impl Into<PathBuf>,
        fixed_record_len: Option<usize>,
    ) {
        let name = name.into();
        let path = path.into();
        self.aliases.insert(normalize_vm_key(&name), name.clone());
        self.aliases
            .insert(normalize_vm_key(&path.to_string_lossy()), name.clone());
        self.files.insert(
            name.clone(),
            VmFile {
                name,
                organization: VmFileOrganization::Sequential,
                backing: VmFileBacking::OsSequential { path, handle: None },
                platform_disposition: None,
                cursor: 0,
                open_mode: None,
                last_status: None,
                last_record_index: None,
                last_record_len: None,
                fixed_record_len,
                linage: None,
                current_line: 1,
            },
        );
    }

    pub fn define_tape_file(&mut self, name: impl Into<String>, path: impl Into<PathBuf>) {
        let name = name.into();
        let path = path.into();
        self.aliases.insert(normalize_vm_key(&name), name.clone());
        self.aliases
            .insert(normalize_vm_key(&path.to_string_lossy()), name.clone());
        self.files.insert(
            name.clone(),
            VmFile {
                name,
                organization: VmFileOrganization::Sequential,
                backing: VmFileBacking::Tape {
                    path,
                    records: Vec::new(),
                },
                platform_disposition: None,
                cursor: 0,
                open_mode: None,
                last_status: None,
                last_record_index: None,
                last_record_len: None,
                fixed_record_len: None,
                linage: None,
                current_line: 1,
            },
        );
    }

    fn file_key(&self, name: &str) -> String {
        self.aliases
            .get(&normalize_vm_key(name))
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    fn file(&self, name: &str) -> Result<&VmFile, VmError> {
        let key = self.file_key(name);
        self.files.get(&key).ok_or_else(|| VmError::FileRuntime {
            name: name.to_string(),
            message: "file is not defined".to_string(),
        })
    }

    fn file_mut(&mut self, name: &str) -> Result<&mut VmFile, VmError> {
        let key = self.file_key(name);
        self.files
            .get_mut(&key)
            .ok_or_else(|| VmError::FileRuntime {
                name: name.to_string(),
                message: "file is not defined".to_string(),
            })
    }

    pub fn set_linage(&mut self, name: impl Into<String>, lines: usize) {
        let name = name.into();
        if let Ok(file) = self.file_mut(&name) {
            file.linage = Some(lines.max(1));
            file.current_line = 1;
        }
    }

    pub fn map_external_name(&mut self, name: impl Into<String>, path: impl Into<PathBuf>) {
        let name = name.into();
        let normalized = normalize_vm_key(&name);
        let key = if self.files.contains_key(&name) {
            name.clone()
        } else if self.files.contains_key(&normalized) {
            normalized.clone()
        } else if let Some(target) = self.aliases.get(&normalized) {
            target.clone()
        } else {
            normalized
        };
        let path = path.into();
        if let Some(file) = self.files.get_mut(&key) {
            if let VmFileBacking::OsSequential {
                handle: Some(handle),
                ..
            } = &mut file.backing
            {
                let _ = handle.flush();
            }
            file.backing = match &file.backing {
                VmFileBacking::Tape { .. } => VmFileBacking::Tape {
                    path,
                    records: Vec::new(),
                },
                VmFileBacking::Memory { .. } | VmFileBacking::OsSequential { .. } => {
                    VmFileBacking::OsSequential { path, handle: None }
                }
            };
            file.organization = VmFileOrganization::Sequential;
            file.cursor = 0;
            file.open_mode = None;
            file.last_status = None;
            file.last_record_index = None;
            file.last_record_len = None;
            file.current_line = 1;
            self.aliases.insert(normalize_vm_key(&name), key);
        } else {
            self.define_os_sequential_file(key.clone(), path);
            self.aliases.insert(normalize_vm_key(&name), key);
        }
    }

    pub fn records(&self, name: &str) -> Option<&[Vec<u8>]> {
        self.file(name).ok().and_then(|file| match &file.backing {
            VmFileBacking::Memory { records } => Some(records.as_slice()),
            VmFileBacking::OsSequential { .. } => None,
            VmFileBacking::Tape { records, .. } => Some(records.as_slice()),
        })
    }

    pub fn checkpoint_records(&mut self, name: &str) -> Result<Vec<Vec<u8>>, VmError> {
        let file = self.file_mut(name).map_err(|_| VmError::FileRuntime {
            name: name.to_string(),
            message: "checkpoint file is not defined".to_string(),
        })?;
        match &mut file.backing {
            VmFileBacking::Memory { records } => Ok(records.clone()),
            VmFileBacking::Tape { path, records } => {
                if records.is_empty() && path.exists() {
                    *records = read_tape_image(path)?;
                }
                Ok(records.clone())
            }
            VmFileBacking::OsSequential { path, .. } => {
                let bytes = fs::read(path).map_err(|error| VmError::FileRuntime {
                    name: name.to_string(),
                    message: format!("failed to read checkpoint bytes: {error}"),
                })?;
                Ok(vec![bytes])
            }
        }
    }

    pub fn last_status(&self, name: &str) -> Option<&str> {
        self.file(name)
            .ok()
            .and_then(|file| file.last_status.as_deref())
    }

    pub fn reset_lifecycle_file(&mut self, name: &str) -> Result<(), VmError> {
        let key = self.file_key(name);
        self.aliases.insert(normalize_vm_key(name), key.clone());
        let file = self.files.entry(key.clone()).or_insert_with(|| VmFile {
            name: key,
            organization: VmFileOrganization::Sequential,
            backing: VmFileBacking::Memory {
                records: Vec::new(),
            },
            platform_disposition: None,
            cursor: 0,
            open_mode: None,
            last_status: None,
            last_record_index: None,
            last_record_len: None,
            fixed_record_len: None,
            linage: None,
            current_line: 1,
        });
        if let VmFileBacking::OsSequential {
            handle: Some(handle),
            ..
        } = &mut file.backing
        {
            handle.flush().map_err(|error| VmError::FileRuntime {
                name: name.to_string(),
                message: format!("failed to flush file during lifecycle reset: {error}"),
            })?;
        }
        if let VmFileBacking::OsSequential { handle, .. } = &mut file.backing {
            *handle = None;
        }
        if let VmFileBacking::Tape { path, records } = &mut file.backing {
            write_tape_image(path, records)?;
        }
        file.cursor = 0;
        file.open_mode = None;
        file.last_record_index = None;
        file.last_record_len = None;
        file.current_line = 1;
        Ok(())
    }

    pub fn write_with_advancing(
        &mut self,
        name: &str,
        record: &[u8],
        advancing: VmWriteAdvancing,
    ) -> Result<(), VmError> {
        match advancing {
            VmWriteAdvancing::BeforePage => self.advance_page(name)?,
            VmWriteAdvancing::BeforeLines(lines) => self.advance_lines(name, lines)?,
            VmWriteAdvancing::None
            | VmWriteAdvancing::AfterLines(_)
            | VmWriteAdvancing::AfterPage => {}
        }
        self.write(name, record)?;
        match advancing {
            VmWriteAdvancing::AfterPage => self.advance_page(name)?,
            VmWriteAdvancing::AfterLines(lines) => self.advance_lines(name, lines)?,
            VmWriteAdvancing::None
            | VmWriteAdvancing::BeforeLines(_)
            | VmWriteAdvancing::BeforePage => {}
        }
        Ok(())
    }

    fn advance_page(&mut self, name: &str) -> Result<(), VmError> {
        self.write(name, b"\x0C")?;
        if let Ok(file) = self.file_mut(name) {
            file.current_line = 1;
        }
        Ok(())
    }

    fn advance_lines(&mut self, name: &str, lines: usize) -> Result<(), VmError> {
        let lines = lines.max(1);
        let needs_page = self
            .file(name)
            .ok()
            .and_then(|file| {
                file.linage
                    .map(|linage| file.current_line.saturating_add(lines) > linage)
            })
            .unwrap_or(false);
        if needs_page {
            self.advance_page(name)?;
            return Ok(());
        }
        self.write(name, &vec![b'\n'; lines])?;
        if let Ok(file) = self.file_mut(name) {
            file.current_line = file.current_line.saturating_add(lines);
        }
        Ok(())
    }
}

fn file_status_for_io_error(error: &std::io::Error) -> &'static str {
    match error.kind() {
        ErrorKind::NotFound => "35",
        ErrorKind::PermissionDenied => "37",
        _ => "30",
    }
}

fn file_logic_status() -> String {
    "90".to_string()
}

fn read_tape_image(path: &PathBuf) -> Result<Vec<Vec<u8>>, VmError> {
    let bytes = fs::read(path).map_err(|error| VmError::FileRuntime {
        name: path.to_string_lossy().to_string(),
        message: format!("failed to read tape image: {error}"),
    })?;
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if !bytes.starts_with(TAPE_IMAGE_MAGIC) {
        return Err(VmError::FileRuntime {
            name: path.to_string_lossy().to_string(),
            message: "invalid tape image header".to_string(),
        });
    }
    let mut idx = TAPE_IMAGE_MAGIC.len();
    let mut records = Vec::new();
    while idx < bytes.len() {
        let tag = bytes[idx];
        idx += 1;
        if tag != TAPE_DATA_RECORD {
            return Err(VmError::FileRuntime {
                name: path.to_string_lossy().to_string(),
                message: format!("unsupported tape block tag {tag}"),
            });
        }
        if idx + 8 > bytes.len() {
            return Err(VmError::FileRuntime {
                name: path.to_string_lossy().to_string(),
                message: "truncated tape record length".to_string(),
            });
        }
        let mut len_bytes = [0u8; 8];
        len_bytes.copy_from_slice(&bytes[idx..idx + 8]);
        idx += 8;
        let len = u64::from_le_bytes(len_bytes) as usize;
        if idx + len > bytes.len() {
            return Err(VmError::FileRuntime {
                name: path.to_string_lossy().to_string(),
                message: "truncated tape record payload".to_string(),
            });
        }
        records.push(bytes[idx..idx + len].to_vec());
        idx += len;
    }
    Ok(records)
}

fn write_tape_image(path: &PathBuf, records: &[Vec<u8>]) -> Result<(), VmError> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(TAPE_IMAGE_MAGIC);
    for record in records {
        bytes.push(TAPE_DATA_RECORD);
        bytes.extend_from_slice(&(record.len() as u64).to_le_bytes());
        bytes.extend_from_slice(record);
    }
    fs::write(path, bytes).map_err(|error| VmError::FileRuntime {
        name: path.to_string_lossy().to_string(),
        message: format!("failed to write tape image: {error}"),
    })
}

fn normalize_tape_read_record(record: &[u8], record_len: usize) -> Vec<u8> {
    if record_len == 0 || record.len() == record_len {
        return record.to_vec();
    }
    if record.len() > record_len {
        return record[..record_len].to_vec();
    }
    let mut out = vec![b' '; record_len];
    out[..record.len()].copy_from_slice(record);
    out
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0F) as usize] as char);
    }
    out
}

fn hex_decode(value: &str) -> Result<Vec<u8>, VmError> {
    if value.len() % 2 != 0 {
        return Err(VmError::ProcedureRuntime {
            block: "CHECKPOINT".to_string(),
            message: "hex field has odd length".to_string(),
        });
    }
    let mut out = Vec::with_capacity(value.len() / 2);
    for idx in (0..value.len()).step_by(2) {
        let byte = u8::from_str_radix(&value[idx..idx + 2], 16).map_err(|error| {
            VmError::ProcedureRuntime {
                block: "CHECKPOINT".to_string(),
                message: format!("invalid hex field: {error}"),
            }
        })?;
        out.push(byte);
    }
    Ok(out)
}

fn hex_string(value: &str) -> String {
    hex_encode(value.as_bytes())
}

fn unhex_string(value: &str) -> Result<String, VmError> {
    String::from_utf8(hex_decode(value)?).map_err(|error| VmError::ProcedureRuntime {
        block: "CHECKPOINT".to_string(),
        message: format!("checkpoint field is not UTF-8: {error}"),
    })
}

fn option_usize_token(value: Option<usize>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn parse_option_usize_token(value: &str) -> Result<Option<usize>, VmError> {
    if value == "-" {
        Ok(None)
    } else {
        value
            .parse::<usize>()
            .map(Some)
            .map_err(|error| VmError::ProcedureRuntime {
                block: "CHECKPOINT".to_string(),
                message: format!("invalid usize checkpoint field: {error}"),
            })
    }
}

fn open_mode_token(value: Option<VmOpenMode>) -> &'static str {
    match value {
        Some(VmOpenMode::Input) => "INPUT",
        Some(VmOpenMode::Output) => "OUTPUT",
        Some(VmOpenMode::Io) => "IO",
        Some(VmOpenMode::Extend) => "EXTEND",
        None => "-",
    }
}

fn parse_open_mode_token(value: &str) -> Result<Option<VmOpenMode>, VmError> {
    match value {
        "-" => Ok(None),
        "INPUT" => Ok(Some(VmOpenMode::Input)),
        "OUTPUT" => Ok(Some(VmOpenMode::Output)),
        "IO" => Ok(Some(VmOpenMode::Io)),
        "EXTEND" => Ok(Some(VmOpenMode::Extend)),
        other => Err(VmError::ProcedureRuntime {
            block: "CHECKPOINT".to_string(),
            message: format!("invalid open mode {other}"),
        }),
    }
}

fn sort_phase_token(value: VmSortPhase) -> &'static str {
    match value {
        VmSortPhase::Input => "INPUT",
        VmSortPhase::Sorting => "SORTING",
        VmSortPhase::Output => "OUTPUT",
        VmSortPhase::Done => "DONE",
    }
}

fn parse_sort_phase_token(value: &str) -> Result<VmSortPhase, VmError> {
    match value {
        "INPUT" => Ok(VmSortPhase::Input),
        "SORTING" => Ok(VmSortPhase::Sorting),
        "OUTPUT" => Ok(VmSortPhase::Output),
        "DONE" => Ok(VmSortPhase::Done),
        other => Err(checkpoint_error(format!("invalid SORT phase {other}"))),
    }
}

fn sort_direction_token(value: VmSortDirection) -> &'static str {
    match value {
        VmSortDirection::Ascending => "ASC",
        VmSortDirection::Descending => "DESC",
    }
}

fn parse_sort_direction_token(value: &str) -> Result<VmSortDirection, VmError> {
    match value {
        "ASC" => Ok(VmSortDirection::Ascending),
        "DESC" => Ok(VmSortDirection::Descending),
        other => Err(checkpoint_error(format!("invalid SORT direction {other}"))),
    }
}

fn category_token(value: VmCategory) -> &'static str {
    match value {
        VmCategory::Group => "GROUP",
        VmCategory::Alphanumeric => "ALPHANUMERIC",
        VmCategory::Alphabetic => "ALPHABETIC",
        VmCategory::National => "NATIONAL",
        VmCategory::Dbcs => "DBCS",
        VmCategory::NumericDisplay => "NUMERIC_DISPLAY",
        VmCategory::NumericEdited => "NUMERIC_EDITED",
        VmCategory::PackedDecimal => "PACKED_DECIMAL",
        VmCategory::Binary => "BINARY",
        VmCategory::NativeBinary => "NATIVE_BINARY",
        VmCategory::Float => "FLOAT",
        VmCategory::Unsupported => "UNSUPPORTED",
    }
}

fn parse_category_token(value: &str) -> Result<VmCategory, VmError> {
    match value {
        "GROUP" => Ok(VmCategory::Group),
        "ALPHANUMERIC" => Ok(VmCategory::Alphanumeric),
        "ALPHABETIC" => Ok(VmCategory::Alphabetic),
        "NATIONAL" => Ok(VmCategory::National),
        "DBCS" => Ok(VmCategory::Dbcs),
        "NUMERIC_DISPLAY" => Ok(VmCategory::NumericDisplay),
        "NUMERIC_EDITED" => Ok(VmCategory::NumericEdited),
        "PACKED_DECIMAL" => Ok(VmCategory::PackedDecimal),
        "BINARY" => Ok(VmCategory::Binary),
        "NATIVE_BINARY" => Ok(VmCategory::NativeBinary),
        "FLOAT" => Ok(VmCategory::Float),
        "UNSUPPORTED" => Ok(VmCategory::Unsupported),
        other => Err(checkpoint_error(format!("invalid category {other}"))),
    }
}

fn usage_token(value: VmUsage) -> &'static str {
    match value {
        VmUsage::Display => "DISPLAY",
        VmUsage::PackedDecimal => "PACKED_DECIMAL",
        VmUsage::Binary => "BINARY",
        VmUsage::NativeBinary => "NATIVE_BINARY",
        VmUsage::Float32 => "FLOAT32",
        VmUsage::Float64 => "FLOAT64",
        VmUsage::National => "NATIONAL",
        VmUsage::Dbcs => "DBCS",
        VmUsage::Group => "GROUP",
        VmUsage::Alphanumeric => "ALPHANUMERIC",
        VmUsage::Unknown => "UNKNOWN",
    }
}

fn parse_usage_token(value: &str) -> Result<VmUsage, VmError> {
    match value {
        "DISPLAY" => Ok(VmUsage::Display),
        "PACKED_DECIMAL" => Ok(VmUsage::PackedDecimal),
        "BINARY" => Ok(VmUsage::Binary),
        "NATIVE_BINARY" => Ok(VmUsage::NativeBinary),
        "FLOAT32" => Ok(VmUsage::Float32),
        "FLOAT64" => Ok(VmUsage::Float64),
        "NATIONAL" => Ok(VmUsage::National),
        "DBCS" => Ok(VmUsage::Dbcs),
        "GROUP" => Ok(VmUsage::Group),
        "ALPHANUMERIC" => Ok(VmUsage::Alphanumeric),
        "UNKNOWN" => Ok(VmUsage::Unknown),
        other => Err(checkpoint_error(format!("invalid usage {other}"))),
    }
}

fn bool_token(value: bool) -> &'static str {
    if value {
        "1"
    } else {
        "0"
    }
}

fn parse_bool_token(value: &str) -> Result<bool, VmError> {
    match value {
        "1" | "true" | "TRUE" => Ok(true),
        "0" | "false" | "FALSE" => Ok(false),
        other => Err(checkpoint_error(format!(
            "invalid checkpoint bool token {other}"
        ))),
    }
}

fn parse_sort_key_encoding_checkpoint(
    category: &str,
    usage: &str,
    digits: &str,
    scale: &str,
    signed: &str,
) -> Result<VmSortKeyEncoding, VmError> {
    let category = parse_category_token(category)?;
    let _usage = parse_usage_token(usage)?;
    let digits = parse_checkpoint_count(digits)?;
    let scale = u32::try_from(parse_checkpoint_count(scale)?)
        .map_err(|_| checkpoint_error("sort key scale exceeds u32"))?;
    let signed = parse_bool_token(signed)?;
    match category {
        VmCategory::NumericDisplay => Ok(VmSortKeyEncoding::NumericDisplay {
            digits,
            scale,
            signed,
        }),
        VmCategory::PackedDecimal => Ok(VmSortKeyEncoding::PackedDecimal {
            digits,
            scale,
            signed,
        }),
        _ => Ok(VmSortKeyEncoding::Bytes),
    }
}

fn dialect_profile_token(profile: &DialectProfile) -> &'static str {
    match profile.name {
        DialectName::IbmZos => "IBM_ZOS",
        DialectName::GnuCobol => "GNUCOBOL",
        DialectName::MicroFocus => "MICRO_FOCUS",
    }
}

fn parse_dialect_profile_token(value: &str) -> Result<DialectProfile, VmError> {
    match value {
        "IBM_ZOS" => Ok(DialectProfile::ibm_zos()),
        "GNUCOBOL" => Ok(DialectProfile::gnucobol()),
        "MICRO_FOCUS" => Ok(DialectProfile::micro_focus()),
        other => Err(checkpoint_error(format!("invalid dialect profile {other}"))),
    }
}

fn storage_key_occurrence_token(key: &StorageKey) -> String {
    if key.occurrence.is_empty() {
        "-".to_string()
    } else {
        key.occurrence
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(",")
    }
}

fn parse_storage_key_occurrence(value: &str) -> Result<Vec<usize>, VmError> {
    if value == "-" {
        return Ok(Vec::new());
    }
    value
        .split(',')
        .map(|part| {
            part.parse::<usize>()
                .map_err(|error| VmError::ProcedureRuntime {
                    block: "CHECKPOINT".to_string(),
                    message: format!("invalid storage occurrence: {error}"),
                })
        })
        .collect()
}

fn checkpoint_error(message: impl Into<String>) -> VmError {
    VmError::ProcedureRuntime {
        block: "CHECKPOINT".to_string(),
        message: message.into(),
    }
}

fn parse_checkpoint_count(value: &str) -> Result<usize, VmError> {
    value
        .parse::<usize>()
        .map_err(|error| checkpoint_error(format!("invalid checkpoint count: {error}")))
}

fn next_checkpoint_parts<'a>(lines: &mut std::str::Lines<'a>) -> Result<Vec<&'a str>, VmError> {
    lines
        .next()
        .map(|line| line.split_whitespace().collect::<Vec<_>>())
        .ok_or_else(|| checkpoint_error("truncated checkpoint"))
}

fn file_fingerprint_token(file: &VmFile) -> String {
    match &file.backing {
        VmFileBacking::OsSequential { path, .. } => match os_file_fingerprint(path) {
            Ok(fingerprint) => format!("{fingerprint:016X}"),
            Err(_) => "-".to_string(),
        },
        VmFileBacking::Memory { .. } | VmFileBacking::Tape { .. } => "-".to_string(),
    }
}

fn os_file_fingerprint(path: &PathBuf) -> Result<u64, VmError> {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut file = File::open(path).map_err(|error| {
        checkpoint_error(format!(
            "failed to read OS file fingerprint for {}: {error}",
            path.display()
        ))
    })?;
    let mut fingerprint = FNV_OFFSET;
    let mut buffer = [0u8; 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| {
            checkpoint_error(format!(
                "failed to read OS file fingerprint for {}: {error}",
                path.display()
            ))
        })?;
        if read == 0 {
            break;
        }
        for byte in &buffer[..read] {
            fingerprint ^= u64::from(*byte);
            fingerprint = fingerprint.wrapping_mul(FNV_PRIME);
        }
    }
    let total_len = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    fingerprint ^= total_len;
    fingerprint = fingerprint.wrapping_mul(FNV_PRIME);
    Ok(fingerprint)
}

fn validate_os_file_fingerprint(name: &str, path: &PathBuf, expected: &str) -> Result<(), VmError> {
    if expected == "-" {
        return Ok(());
    }
    let expected = u64::from_str_radix(expected, 16)
        .map_err(|error| checkpoint_error(format!("invalid OS file fingerprint: {error}")))?;
    let actual = os_file_fingerprint(path)?;
    if actual != expected {
        return Err(checkpoint_error(format!(
            "RERUN-MISMATCH for OS sequential file {name}: content fingerprint changed"
        )));
    }
    Ok(())
}

fn restored_os_file_offset(
    name: &str,
    path: &PathBuf,
    cursor: usize,
    fixed_record_len: Option<usize>,
    last_record_len: Option<usize>,
) -> Result<u64, VmError> {
    if cursor == 0 {
        return Ok(0);
    }
    let record_len = match fixed_record_len.or(last_record_len) {
        Some(record_len) => record_len,
        None => infer_fixed_record_len_from_cursor(name, path, cursor)?,
    };
    let cursor = u64::try_from(cursor)
        .map_err(|_| checkpoint_error(format!("file {name} cursor is too large")))?;
    let record_len = u64::try_from(record_len)
        .map_err(|_| checkpoint_error(format!("file {name} record length is too large")))?;
    cursor
        .checked_mul(record_len)
        .ok_or_else(|| checkpoint_error(format!("file {name} restore offset overflow")))
}

fn infer_fixed_record_len_from_cursor(
    name: &str,
    path: &PathBuf,
    cursor: usize,
) -> Result<usize, VmError> {
    let total_len = fs::metadata(path)
        .map_err(|error| {
            checkpoint_error(format!(
                "cannot infer OS sequential file {name} record length from {}: {error}",
                path.display()
            ))
        })?
        .len();
    let cursor = u64::try_from(cursor)
        .map_err(|_| checkpoint_error(format!("file {name} cursor is too large")))?;
    if cursor == 0 || total_len % cursor != 0 {
        return Err(checkpoint_error(format!(
            "cannot infer fixed record length for OS sequential file {name} at cursor {cursor}"
        )));
    }
    usize::try_from(total_len / cursor)
        .map_err(|_| checkpoint_error(format!("file {name} inferred record length is too large")))
}

fn platform_error_to_vm(error: cobol_platform::PlatformError) -> VmError {
    let name = match &error {
        cobol_platform::PlatformError::EmptyFileName { index } => {
            format!("platform file binding #{index}")
        }
        cobol_platform::PlatformError::InvalidFileName { name, .. }
        | cobol_platform::PlatformError::DuplicateFileName { name, .. }
        | cobol_platform::PlatformError::UnsupportedEncoding { name, .. }
        | cobol_platform::PlatformError::EmptyHostPath { name }
        | cobol_platform::PlatformError::InvalidFixedRecordLen { name }
        | cobol_platform::PlatformError::UnsupportedOrganization { name, .. }
        | cobol_platform::PlatformError::UnsupportedRecordFormat { name, .. } => name.clone(),
        cobol_platform::PlatformError::Io { .. } | cobol_platform::PlatformError::Json(_) => {
            "platform".to_string()
        }
    };
    VmError::FileRuntime {
        name,
        message: error.to_string(),
    }
}

fn validate_platform_open_mode(
    name: &str,
    disposition: Option<cobol_platform::FileDisposition>,
    mode: VmOpenMode,
) -> Result<(), VmError> {
    let Some(disposition) = disposition else {
        return Ok(());
    };
    let allowed = match disposition {
        cobol_platform::FileDisposition::Old | cobol_platform::FileDisposition::Shr => {
            matches!(mode, VmOpenMode::Input | VmOpenMode::Io)
        }
        cobol_platform::FileDisposition::New => matches!(mode, VmOpenMode::Output),
        cobol_platform::FileDisposition::Mod => matches!(mode, VmOpenMode::Extend),
    };
    if allowed {
        Ok(())
    } else {
        Err(VmError::FileRuntime {
            name: name.to_string(),
            message: format!(
                "platform disposition {:?} does not allow OPEN {}",
                disposition,
                open_mode_token(Some(mode))
            ),
        })
    }
}

fn platform_disposition_token(
    disposition: Option<cobol_platform::FileDisposition>,
) -> &'static str {
    match disposition {
        Some(cobol_platform::FileDisposition::Old) => "OLD",
        Some(cobol_platform::FileDisposition::Shr) => "SHR",
        Some(cobol_platform::FileDisposition::New) => "NEW",
        Some(cobol_platform::FileDisposition::Mod) => "MOD",
        None => "-",
    }
}

fn parse_platform_disposition_token(
    token: &str,
) -> Result<Option<cobol_platform::FileDisposition>, VmError> {
    match token {
        "-" => Ok(None),
        "OLD" => Ok(Some(cobol_platform::FileDisposition::Old)),
        "SHR" => Ok(Some(cobol_platform::FileDisposition::Shr)),
        "NEW" => Ok(Some(cobol_platform::FileDisposition::New)),
        "MOD" => Ok(Some(cobol_platform::FileDisposition::Mod)),
        other => Err(checkpoint_error(format!(
            "invalid platform disposition token {other}"
        ))),
    }
}

impl VmFileHandler for VmFileRuntime {
    fn open(&mut self, name: &str, mode: VmOpenMode) -> Result<(), VmError> {
        let key = self.file_key(name);
        self.aliases.insert(normalize_vm_key(name), key.clone());
        let file = self.files.entry(key.clone()).or_insert_with(|| VmFile {
            name: key,
            organization: VmFileOrganization::Sequential,
            backing: VmFileBacking::Memory {
                records: Vec::new(),
            },
            platform_disposition: None,
            cursor: 0,
            open_mode: None,
            last_status: None,
            last_record_index: None,
            last_record_len: None,
            fixed_record_len: None,
            linage: None,
            current_line: 1,
        });
        file.cursor = 0;
        file.last_record_index = None;
        file.last_record_len = None;
        file.current_line = 1;
        match &mut file.backing {
            VmFileBacking::Memory { records } => {
                if matches!(mode, VmOpenMode::Output) {
                    records.clear();
                }
                file.open_mode = Some(mode);
                file.last_status = Some("00".to_string());
            }
            VmFileBacking::OsSequential { path, handle } => {
                if let Err(error) =
                    validate_platform_open_mode(name, file.platform_disposition, mode)
                {
                    file.open_mode = None;
                    file.last_status = Some(file_logic_status());
                    return Err(error);
                }
                let opened = match mode {
                    VmOpenMode::Input => OpenOptions::new().read(true).open(path),
                    VmOpenMode::Output => OpenOptions::new()
                        .write(true)
                        .create(true)
                        .truncate(true)
                        .open(path),
                    VmOpenMode::Io => OpenOptions::new().read(true).write(true).open(path),
                    VmOpenMode::Extend => OpenOptions::new().append(true).create(true).open(path),
                };
                let mut opened = match opened {
                    Ok(opened) => opened,
                    Err(error) => {
                        file.open_mode = None;
                        file.last_status = Some(file_status_for_io_error(&error).to_string());
                        return Err(VmError::FileRuntime {
                            name: name.to_string(),
                            message: format!("failed to open OS sequential file: {error}"),
                        });
                    }
                };
                match mode {
                    VmOpenMode::Input | VmOpenMode::Io => {
                        if let Err(error) = opened.seek(SeekFrom::Start(0)) {
                            file.open_mode = None;
                            file.last_status = Some(file_status_for_io_error(&error).to_string());
                            return Err(VmError::FileRuntime {
                                name: name.to_string(),
                                message: format!("failed to rewind OS sequential file: {error}"),
                            });
                        }
                    }
                    VmOpenMode::Output | VmOpenMode::Extend => {
                        if let Err(error) = opened.seek(SeekFrom::End(0)) {
                            file.open_mode = None;
                            file.last_status = Some(file_status_for_io_error(&error).to_string());
                            return Err(VmError::FileRuntime {
                                name: name.to_string(),
                                message: format!("failed to seek OS sequential file: {error}"),
                            });
                        }
                    }
                }
                *handle = Some(opened);
                file.open_mode = Some(mode);
                file.last_status = Some("00".to_string());
            }
            VmFileBacking::Tape { path, records } => {
                if matches!(mode, VmOpenMode::Io) {
                    file.open_mode = None;
                    file.last_status = Some(file_logic_status());
                    return Err(VmError::FileRuntime {
                        name: name.to_string(),
                        message: "tape files do not support I-O mode".to_string(),
                    });
                }
                match mode {
                    VmOpenMode::Input => match read_tape_image(path) {
                        Ok(loaded) => {
                            *records = loaded;
                            file.cursor = 0;
                        }
                        Err(error) => {
                            file.open_mode = None;
                            file.last_status = Some("35".to_string());
                            return Err(error);
                        }
                    },
                    VmOpenMode::Output => {
                        records.clear();
                        write_tape_image(path, records)?;
                        file.cursor = 0;
                    }
                    VmOpenMode::Extend => {
                        *records = if path.exists() {
                            read_tape_image(path)?
                        } else {
                            Vec::new()
                        };
                        file.cursor = records.len();
                    }
                    VmOpenMode::Io => unreachable!(),
                }
                file.open_mode = Some(mode);
                file.last_status = Some("00".to_string());
            }
        }
        Ok(())
    }

    fn read(&mut self, name: &str, record_len: usize) -> Result<Option<Vec<u8>>, VmError> {
        let file = self.file_mut(name)?;
        if !matches!(file.open_mode, Some(VmOpenMode::Input | VmOpenMode::Io)) {
            file.last_status = Some(file_logic_status());
            return Err(VmError::FileRuntime {
                name: name.to_string(),
                message: "file is not open for input".to_string(),
            });
        }
        match &mut file.backing {
            VmFileBacking::Memory { records } => {
                let idx = file.cursor;
                let record = records.get(idx).cloned();
                if let Some(record) = &record {
                    file.cursor += 1;
                    file.last_record_index = Some(idx);
                    file.last_record_len = Some(record.len());
                    file.last_status = Some("00".to_string());
                } else {
                    file.last_record_index = None;
                    file.last_record_len = None;
                    file.last_status = Some("10".to_string());
                }
                Ok(record)
            }
            VmFileBacking::OsSequential { handle, .. } => {
                let record_len = match file.fixed_record_len {
                    Some(expected) if record_len == 0 || record_len == expected => expected,
                    Some(expected) => {
                        file.last_status = Some(file_logic_status());
                        return Err(VmError::FileRuntime {
                            name: name.to_string(),
                            message: format!(
                                "fixed-length OS sequential read requested {record_len} bytes but file metadata requires {expected}"
                            ),
                        });
                    }
                    None => record_len,
                };
                if record_len == 0 {
                    file.last_status = Some(file_logic_status());
                    return Err(VmError::FileRuntime {
                        name: name.to_string(),
                        message: "fixed-length OS sequential read requires a nonzero record length"
                            .to_string(),
                    });
                }
                let handle = handle.as_mut().ok_or_else(|| VmError::FileRuntime {
                    name: name.to_string(),
                    message: "OS sequential file handle is not open".to_string(),
                });
                let handle = match handle {
                    Ok(handle) => handle,
                    Err(error) => {
                        file.last_status = Some(file_logic_status());
                        return Err(error);
                    }
                };
                let mut record = vec![0; record_len];
                let mut read_total = 0;
                while read_total < record_len {
                    match handle.read(&mut record[read_total..]) {
                        Ok(0) if read_total == 0 => {
                            file.last_record_index = None;
                            file.last_record_len = None;
                            file.last_status = Some("10".to_string());
                            return Ok(None);
                        }
                        Ok(0) => {
                            file.last_record_index = None;
                            file.last_record_len = None;
                            file.last_status = Some("30".to_string());
                            return Err(VmError::FileRuntime {
                                name: name.to_string(),
                                message: format!(
                                    "partial fixed-length record: read {read_total} of {record_len} bytes"
                                ),
                            });
                        }
                        Ok(n) => read_total += n,
                        Err(error) => {
                            file.last_record_index = None;
                            file.last_record_len = None;
                            file.last_status = Some(file_status_for_io_error(&error).to_string());
                            return Err(VmError::FileRuntime {
                                name: name.to_string(),
                                message: format!("failed to read OS sequential file: {error}"),
                            });
                        }
                    }
                }
                file.cursor += 1;
                file.last_record_index = Some(file.cursor - 1);
                file.last_record_len = Some(record_len);
                file.fixed_record_len.get_or_insert(record_len);
                file.last_status = Some("00".to_string());
                Ok(Some(record))
            }
            VmFileBacking::Tape { records, .. } => {
                let idx = file.cursor;
                let Some(record) = records.get(idx) else {
                    file.last_record_index = None;
                    file.last_record_len = None;
                    file.last_status = Some("10".to_string());
                    return Ok(None);
                };
                file.cursor += 1;
                file.last_record_index = Some(idx);
                file.last_record_len = Some(record.len());
                file.last_status = Some("00".to_string());
                Ok(Some(normalize_tape_read_record(record, record_len)))
            }
        }
    }

    fn write(&mut self, name: &str, record: &[u8]) -> Result<(), VmError> {
        let file = self.file_mut(name)?;
        if !matches!(
            file.open_mode,
            Some(VmOpenMode::Output | VmOpenMode::Io | VmOpenMode::Extend)
        ) {
            file.last_status = Some(file_logic_status());
            return Err(VmError::FileRuntime {
                name: name.to_string(),
                message: "file is not open for output".to_string(),
            });
        }
        match &mut file.backing {
            VmFileBacking::Memory { records } => {
                records.push(record.to_vec());
                file.last_status = Some("00".to_string());
            }
            VmFileBacking::OsSequential { handle, .. } => {
                let handle = match handle.as_mut() {
                    Some(handle) => handle,
                    None => {
                        file.last_status = Some(file_logic_status());
                        return Err(VmError::FileRuntime {
                            name: name.to_string(),
                            message: "OS sequential file handle is not open".to_string(),
                        });
                    }
                };
                if let Err(error) = handle.seek(SeekFrom::End(0)) {
                    file.last_status = Some(file_status_for_io_error(&error).to_string());
                    return Err(VmError::FileRuntime {
                        name: name.to_string(),
                        message: format!("failed to seek OS sequential file before write: {error}"),
                    });
                }
                if let Err(error) = handle.write_all(record) {
                    file.last_status = Some(file_status_for_io_error(&error).to_string());
                    return Err(VmError::FileRuntime {
                        name: name.to_string(),
                        message: format!("failed to write OS sequential file: {error}"),
                    });
                }
                if let Err(error) = handle.flush() {
                    file.last_status = Some(file_status_for_io_error(&error).to_string());
                    return Err(VmError::FileRuntime {
                        name: name.to_string(),
                        message: format!("failed to flush OS sequential file: {error}"),
                    });
                }
                file.cursor += 1;
                file.last_status = Some("00".to_string());
            }
            VmFileBacking::Tape { path, records } => {
                records.push(record.to_vec());
                file.cursor = records.len();
                if let Err(error) = write_tape_image(path, records) {
                    file.last_status = Some("30".to_string());
                    return Err(error);
                }
                file.last_status = Some("00".to_string());
            }
        }
        Ok(())
    }

    fn rewrite(&mut self, name: &str, record: &[u8]) -> Result<(), VmError> {
        let file = self.file_mut(name)?;
        if !matches!(file.open_mode, Some(VmOpenMode::Io)) {
            file.last_status = Some(file_logic_status());
            return Err(VmError::FileRuntime {
                name: name.to_string(),
                message: "file is not open for I-O".to_string(),
            });
        }
        match &mut file.backing {
            VmFileBacking::Memory { records } => {
                let idx = file.last_record_index.ok_or_else(|| VmError::FileRuntime {
                    name: name.to_string(),
                    message: "REWRITE requires a previously read record".to_string(),
                });
                let idx = match idx {
                    Ok(idx) => idx,
                    Err(error) => {
                        file.last_status = Some(file_logic_status());
                        return Err(error);
                    }
                };
                let slot = records.get_mut(idx).ok_or_else(|| VmError::FileRuntime {
                    name: name.to_string(),
                    message: "REWRITE requires a previously read record".to_string(),
                });
                let slot = match slot {
                    Ok(slot) => slot,
                    Err(error) => {
                        file.last_status = Some(file_logic_status());
                        return Err(error);
                    }
                };
                *slot = record.to_vec();
                file.last_record_len = Some(record.len());
                file.last_status = Some("00".to_string());
            }
            VmFileBacking::OsSequential { handle, .. } => {
                let idx = match file.last_record_index {
                    Some(idx) => idx,
                    None => {
                        file.last_status = Some(file_logic_status());
                        return Err(VmError::FileRuntime {
                            name: name.to_string(),
                            message: "REWRITE requires a previously read record".to_string(),
                        });
                    }
                };
                let expected_len = file
                    .fixed_record_len
                    .or(file.last_record_len)
                    .unwrap_or(record.len());
                if record.len() != expected_len {
                    file.last_status = Some(file_logic_status());
                    return Err(VmError::FileRuntime {
                        name: name.to_string(),
                        message: format!(
                            "REWRITE record length {} does not match last read length {expected_len}",
                            record.len()
                        ),
                    });
                }
                let handle = match handle.as_mut() {
                    Some(handle) => handle,
                    None => {
                        file.last_status = Some(file_logic_status());
                        return Err(VmError::FileRuntime {
                            name: name.to_string(),
                            message: "OS sequential file handle is not open".to_string(),
                        });
                    }
                };
                let offset = (idx * expected_len) as u64;
                if let Err(error) = handle.seek(SeekFrom::Start(offset)) {
                    file.last_status = Some(file_status_for_io_error(&error).to_string());
                    return Err(VmError::FileRuntime {
                        name: name.to_string(),
                        message: format!(
                            "failed to seek OS sequential file before rewrite: {error}"
                        ),
                    });
                }
                if let Err(error) = handle.write_all(record) {
                    file.last_status = Some(file_status_for_io_error(&error).to_string());
                    return Err(VmError::FileRuntime {
                        name: name.to_string(),
                        message: format!("failed to rewrite OS sequential file: {error}"),
                    });
                }
                if let Err(error) = handle.flush() {
                    file.last_status = Some(file_status_for_io_error(&error).to_string());
                    return Err(VmError::FileRuntime {
                        name: name.to_string(),
                        message: format!(
                            "failed to flush OS sequential file after rewrite: {error}"
                        ),
                    });
                }
                file.last_status = Some("00".to_string());
            }
            VmFileBacking::Tape { .. } => {
                file.last_status = Some(file_logic_status());
                return Err(VmError::FileRuntime {
                    name: name.to_string(),
                    message: "REWRITE is not supported for tape files".to_string(),
                });
            }
        }
        file.last_record_index = None;
        file.last_record_len = None;
        Ok(())
    }

    fn delete(&mut self, name: &str) -> Result<(), VmError> {
        let file = self.file_mut(name)?;
        if !matches!(file.open_mode, Some(VmOpenMode::Io)) {
            file.last_status = Some(file_logic_status());
            return Err(VmError::FileRuntime {
                name: name.to_string(),
                message: "file is not open for I-O".to_string(),
            });
        }
        let idx = match file.last_record_index {
            Some(idx) => idx,
            None => {
                file.last_status = Some(file_logic_status());
                return Err(VmError::FileRuntime {
                    name: name.to_string(),
                    message: "DELETE requires a previously read record".to_string(),
                });
            }
        };
        let record_len = match file.last_record_len.or(file.fixed_record_len) {
            Some(record_len) if record_len > 0 => record_len,
            _ => {
                file.last_status = Some(file_logic_status());
                return Err(VmError::FileRuntime {
                    name: name.to_string(),
                    message: "DELETE requires a nonzero last-read record length".to_string(),
                });
            }
        };
        match &mut file.backing {
            VmFileBacking::Memory { records } => {
                if idx >= records.len() {
                    file.last_status = Some(file_logic_status());
                    return Err(VmError::FileRuntime {
                        name: name.to_string(),
                        message: "DELETE requires a previously read record".to_string(),
                    });
                }
                records.remove(idx);
                file.cursor = idx.min(records.len());
                file.last_status = Some("00".to_string());
            }
            VmFileBacking::OsSequential { handle, .. } => {
                let handle = match handle.as_mut() {
                    Some(handle) => handle,
                    None => {
                        file.last_status = Some(file_logic_status());
                        return Err(VmError::FileRuntime {
                            name: name.to_string(),
                            message: "OS sequential file handle is not open".to_string(),
                        });
                    }
                };
                let start = (idx * record_len) as u64;
                let end = start + record_len as u64;
                let total_len = match handle.seek(SeekFrom::End(0)) {
                    Ok(len) => len,
                    Err(error) => {
                        file.last_status = Some(file_status_for_io_error(&error).to_string());
                        return Err(VmError::FileRuntime {
                            name: name.to_string(),
                            message: format!(
                                "failed to size OS sequential file before delete: {error}"
                            ),
                        });
                    }
                };
                if end > total_len {
                    file.last_status = Some(file_logic_status());
                    return Err(VmError::FileRuntime {
                        name: name.to_string(),
                        message: "DELETE last-read record is outside the current file".to_string(),
                    });
                }
                if let Err(error) = handle.seek(SeekFrom::Start(end)) {
                    file.last_status = Some(file_status_for_io_error(&error).to_string());
                    return Err(VmError::FileRuntime {
                        name: name.to_string(),
                        message: format!(
                            "failed to seek OS sequential file tail before delete: {error}"
                        ),
                    });
                }
                let mut tail = Vec::new();
                if let Err(error) = handle.read_to_end(&mut tail) {
                    file.last_status = Some(file_status_for_io_error(&error).to_string());
                    return Err(VmError::FileRuntime {
                        name: name.to_string(),
                        message: format!(
                            "failed to read OS sequential file tail before delete: {error}"
                        ),
                    });
                }
                if let Err(error) = handle.seek(SeekFrom::Start(start)) {
                    file.last_status = Some(file_status_for_io_error(&error).to_string());
                    return Err(VmError::FileRuntime {
                        name: name.to_string(),
                        message: format!(
                            "failed to seek OS sequential file before delete: {error}"
                        ),
                    });
                }
                if let Err(error) = handle.write_all(&tail) {
                    file.last_status = Some(file_status_for_io_error(&error).to_string());
                    return Err(VmError::FileRuntime {
                        name: name.to_string(),
                        message: format!(
                            "failed to compact OS sequential file during delete: {error}"
                        ),
                    });
                }
                if let Err(error) = handle.set_len(total_len - record_len as u64) {
                    file.last_status = Some(file_status_for_io_error(&error).to_string());
                    return Err(VmError::FileRuntime {
                        name: name.to_string(),
                        message: format!(
                            "failed to truncate OS sequential file during delete: {error}"
                        ),
                    });
                }
                if let Err(error) = handle.flush() {
                    file.last_status = Some(file_status_for_io_error(&error).to_string());
                    return Err(VmError::FileRuntime {
                        name: name.to_string(),
                        message: format!(
                            "failed to flush OS sequential file after delete: {error}"
                        ),
                    });
                }
                file.cursor = idx;
                file.last_status = Some("00".to_string());
            }
            VmFileBacking::Tape { .. } => {
                file.last_status = Some(file_logic_status());
                return Err(VmError::FileRuntime {
                    name: name.to_string(),
                    message: "DELETE is not supported for tape files".to_string(),
                });
            }
        }
        file.last_record_index = None;
        file.last_record_len = None;
        Ok(())
    }

    fn close(&mut self, name: &str) -> Result<(), VmError> {
        let file = self.file_mut(name)?;
        if let VmFileBacking::OsSequential {
            handle: Some(handle),
            ..
        } = &mut file.backing
        {
            if let Err(error) = handle.flush() {
                file.last_status = Some(file_status_for_io_error(&error).to_string());
                return Err(VmError::FileRuntime {
                    name: name.to_string(),
                    message: format!("failed to flush OS sequential file during close: {error}"),
                });
            }
        }
        if let VmFileBacking::OsSequential { handle, .. } = &mut file.backing {
            *handle = None;
        }
        if let VmFileBacking::Tape { path, records } = &mut file.backing {
            write_tape_image(path, records)?;
        }
        file.open_mode = None;
        file.last_status = Some("00".to_string());
        Ok(())
    }
}

impl VmProgramRegistry {
    pub fn insert(&mut self, name: impl Into<String>, procedure: VmProcedure) {
        self.insert_with_linkage(name, procedure, Vec::new());
    }

    pub fn insert_with_linkage(
        &mut self,
        name: impl Into<String>,
        procedure: VmProcedure,
        linkage: Vec<String>,
    ) {
        let linkage = linkage
            .into_iter()
            .map(|name| VmLinkageParam {
                name,
                children: Vec::new(),
            })
            .collect();
        self.insert_with_linkage_descriptors(name, procedure, linkage);
    }

    pub fn insert_with_linkage_descriptors(
        &mut self,
        name: impl Into<String>,
        procedure: VmProcedure,
        linkage: Vec<VmLinkageParam>,
    ) {
        self.insert_with_lifecycle_descriptors(
            name,
            procedure,
            linkage,
            false,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn insert_with_lifecycle_descriptors(
        &mut self,
        name: impl Into<String>,
        procedure: VmProcedure,
        linkage: Vec<VmLinkageParam>,
        is_initial: bool,
        initial_cells: Vec<(StorageKey, Vec<u8>)>,
        initial_odo: Vec<VmOdoInitialState>,
        initial_files: Vec<String>,
    ) {
        self.programs.insert(
            normalize_vm_key(&name.into()),
            VmRegisteredProgram {
                procedure,
                linkage,
                is_initial,
                initial_cells,
                initial_odo,
                initial_files,
            },
        );
    }

    pub fn get(&self, name: &str) -> Option<&VmProcedure> {
        self.programs
            .get(&normalize_vm_key(name))
            .map(|program| &program.procedure)
    }

    pub fn registered(&self, name: &str) -> Option<&VmRegisteredProgram> {
        self.programs.get(&normalize_vm_key(name))
    }
}

impl VmRuntime {
    pub fn new(program: VmProgram, mut storage_pool: StoragePool) -> Self {
        let program_status_key = StorageKey::special(PROGRAM_STATUS_REGISTER);
        if !storage_pool.cells.contains_key(&program_status_key) {
            let _ = storage_pool.define_cell(program_status_key.clone(), b"00".to_vec());
        }
        let tally_key = StorageKey::special(TALLY_REGISTER);
        if !storage_pool.cells.contains_key(&tally_key) {
            let _ = storage_pool.define_cell(tally_key.clone(), b"000000000".to_vec());
        }
        let debug_item_key = StorageKey::special(DEBUG_ITEM_REGISTER);
        if !storage_pool.cells.contains_key(&debug_item_key) {
            let _ = storage_pool.define_cell(debug_item_key.clone(), vec![b' '; 64]);
        }
        let debug_contents_key = StorageKey::special(DEBUG_CONTENTS_REGISTER);
        if !storage_pool.cells.contains_key(&debug_contents_key) {
            let _ = storage_pool.define_cell(debug_contents_key.clone(), vec![b' '; 16]);
        }
        let dialect = program.dialect.clone();
        let mut runtime = Self {
            program,
            dialect,
            storage_pool,
            storage_descriptors: BTreeMap::new(),
            indexes: BTreeMap::new(),
            odo: BTreeMap::new(),
            files: VmFileRuntime::default(),
            file_status: BTreeMap::new(),
            registry: VmProgramRegistry::default(),
            activation_stack: Vec::new(),
            last_abend_frame: None,
            alter_table: BTreeMap::new(),
            rerun_checkpoints: Vec::new(),
            file_error_declaratives: BTreeMap::new(),
            active_file_error_declaratives: BTreeSet::new(),
            debugging_declaratives: BTreeMap::new(),
            active_debugging_declaratives: BTreeSet::new(),
            trace_enabled: false,
            checkpoint_in_progress: false,
            sort_states: Vec::new(),
            display: Vec::new(),
        };
        runtime.bind_storage_cell(PROGRAM_STATUS_REGISTER, program_status_key.clone());
        runtime.bind_storage_cell(PROGRAM_STATUS_DISPLAY_NAME, program_status_key);
        runtime.bind_storage_cell(TALLY_REGISTER, tally_key);
        runtime.bind_storage_cell(DEBUG_ITEM_REGISTER, debug_item_key.clone());
        runtime.bind_storage_cell(DEBUG_ITEM_DISPLAY_NAME, debug_item_key);
        runtime.bind_storage_cell(DEBUG_CONTENTS_REGISTER, debug_contents_key.clone());
        runtime.bind_storage_cell(DEBUG_CONTENTS_DISPLAY_NAME, debug_contents_key);
        runtime
    }

    pub fn dialect(&self) -> &DialectProfile {
        &self.dialect
    }

    pub fn set_dialect(&mut self, dialect: DialectProfile) {
        self.program.dialect = dialect.clone();
        self.dialect = dialect;
    }

    pub fn abend_report_json(&self, error: &VmError) -> String {
        serde_json::json!({
            "type": "ABEND",
            "error": {
                "code": error.code(),
                "message": error.to_string(),
            },
            "state": {
                "dialect": format!("{:?}", self.dialect.name),
                "current_frame": self.abend_current_frame_json(),
                "activation_depth": self.activation_stack.len(),
                "storage_cells": self.storage_pool.cells.len(),
                "odo_tables": self.storage_pool.odo_tables.len(),
                "indexes": self.indexes.len(),
                "runtime_odo": self.odo.len(),
                "files": self.abend_files_json(),
                "sort_states": self.sort_states.len(),
                "rerun_checkpoints": self.rerun_checkpoints.len(),
                "display_lines": self.display.len(),
                "trace_enabled": self.trace_enabled,
            }
        })
        .to_string()
    }

    fn abend_current_frame_json(&self) -> serde_json::Value {
        self.activation_stack
            .last()
            .or(self.last_abend_frame.as_ref())
            .map_or(serde_json::Value::Null, |frame| {
                serde_json::json!({
                    "program": frame.program,
                    "paragraph": frame.current,
                    "return_to": frame.return_to,
                    "source_span": frame.source_span.as_ref().map(|span| serde_json::json!({
                        "file": span.file,
                        "line": span.line,
                        "column": span.column,
                    })),
                    "local_bindings": frame.local_bindings.len(),
                })
            })
    }

    fn abend_files_json(&self) -> serde_json::Value {
        serde_json::Value::Array(
            self.files
                .files
                .values()
                .map(|file| {
                    let (backing, path, records) = match &file.backing {
                        VmFileBacking::Memory { records } => ("memory", None, Some(records.len())),
                        VmFileBacking::OsSequential { path, .. } => (
                            "os_sequential",
                            Some(path.to_string_lossy().to_string()),
                            None,
                        ),
                        VmFileBacking::Tape { path, records } => (
                            "tape",
                            Some(path.to_string_lossy().to_string()),
                            Some(records.len()),
                        ),
                    };
                    serde_json::json!({
                        "name": file.name,
                        "organization": format!("{:?}", file.organization),
                        "backing": backing,
                        "path": path,
                        "records": records,
                        "cursor": file.cursor,
                        "open_mode": file.open_mode.map(|mode| format!("{mode:?}")),
                        "last_status": file.last_status,
                        "last_record_index": file.last_record_index,
                        "last_record_len": file.last_record_len,
                        "fixed_record_len": file.fixed_record_len,
                        "linage": file.linage,
                        "current_line": file.current_line,
                    })
                })
                .collect(),
        )
    }

    pub fn bind_record_storage(&mut self, target: impl Into<String>, key: StorageKey) {
        self.storage_descriptors
            .insert(target.into(), VmBinding::Cell { key });
    }

    pub fn bind_storage_cell(&mut self, target: impl Into<String>, key: StorageKey) {
        self.storage_descriptors
            .insert(target.into(), VmBinding::Cell { key });
    }

    pub fn bind_storage_slice(
        &mut self,
        target: impl Into<String>,
        key: StorageKey,
        offset: usize,
        len: usize,
    ) {
        self.storage_descriptors
            .insert(target.into(), VmBinding::Slice { key, offset, len });
    }

    pub fn bind_occurs_storage_cell(
        &mut self,
        target: impl Into<String>,
        program: impl Into<String>,
        item: impl Into<String>,
    ) {
        self.storage_descriptors.insert(
            target.into(),
            VmBinding::OccursCell {
                program: program.into(),
                item: item.into(),
            },
        );
    }

    pub fn bind_group_storage(&mut self, target: impl Into<String>, children: Vec<String>) {
        self.storage_descriptors
            .insert(target.into(), VmBinding::Group { children });
    }

    pub fn bind_file_status(&mut self, file: impl Into<String>, target: VmAccessPath) {
        self.file_status
            .insert(normalize_vm_key(&file.into()), target);
    }

    pub fn register_file_error_declarative(
        &mut self,
        file: impl Into<String>,
        ops: Vec<VmProcedureOp>,
    ) {
        self.file_error_declaratives
            .insert(normalize_vm_key(&file.into()), ops);
    }

    pub fn register_debugging_declarative(
        &mut self,
        paragraph: impl Into<String>,
        ops: Vec<VmProcedureOp>,
    ) {
        self.debugging_declaratives
            .insert(normalize_vm_key(&paragraph.into()), ops);
    }

    pub fn register_rerun_checkpoint(
        &mut self,
        checkpoint_file: impl Into<String>,
        watched_file: impl Into<String>,
        every_records: usize,
    ) {
        if every_records == 0 {
            return;
        }
        let checkpoint_file = checkpoint_file.into();
        let watched_file = watched_file.into();
        self.rerun_checkpoints.push(VmRerunCheckpoint {
            checkpoint_file,
            watched_key: normalize_vm_key(&watched_file),
            watched_file,
            every_records,
            record_count: 0,
        });
    }

    pub fn checkpoint_snapshot_bytes(&self) -> Vec<u8> {
        let mut out = String::new();
        out.push_str(CHECKPOINT_MAGIC);
        out.push('\n');
        out.push_str(&format!(
            "DIALECT {}\n",
            dialect_profile_token(&self.dialect)
        ));

        out.push_str(&format!("STORAGE {}\n", self.storage_pool.cells.len()));
        for (key, cell) in &self.storage_pool.cells {
            out.push_str(&format!(
                "S {} {} {} {}\n",
                hex_string(&key.program),
                hex_string(&key.item),
                storage_key_occurrence_token(key),
                hex_encode(cell.bytes())
            ));
        }

        out.push_str(&format!("INDEX {}\n", self.indexes.len()));
        for (name, index) in &self.indexes {
            out.push_str(&format!(
                "I {} {} {} {} {}\n",
                hex_string(name),
                hex_string(&index.table),
                index.min,
                index.max,
                option_usize_token(index.occurrence)
            ));
        }

        out.push_str(&format!("ODO {}\n", self.odo.len()));
        for (name, odo) in &self.odo {
            out.push_str(&format!(
                "O {} {} {} {} {} {} {}\n",
                hex_string(name),
                odo.program
                    .as_deref()
                    .map(hex_string)
                    .unwrap_or_else(|| "-".to_string()),
                hex_string(&odo.table),
                hex_string(&odo.depending_on),
                odo.active,
                odo.min,
                odo.max
            ));
        }

        out.push_str(&format!("ALTER {}\n", self.alter_table.len()));
        for (slot, target) in &self.alter_table {
            out.push_str(&format!("A {} {}\n", hex_string(slot), hex_string(target)));
        }

        out.push_str(&format!("SORT {}\n", self.sort_states.len()));
        for state in &self.sort_states {
            let key = state.key.as_ref();
            let key_parts = key.map(|key| key.encoding.checkpoint_parts());
            out.push_str(&format!(
                "SORTSTATE {} {} {} {} {} {} {} {} {} {} {} {} {} {}\n",
                hex_string(&state.file),
                sort_phase_token(state.phase),
                hex_string(&state.record.target),
                state.record_len,
                state.cursor,
                key.map(|key| key.offset.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                key.map(|key| key.byte_len.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                key.map(|key| sort_direction_token(key.direction).to_string())
                    .unwrap_or_else(|| "-".to_string()),
                key_parts
                    .map(|(category, _, _, _, _)| category_token(category).to_string())
                    .unwrap_or_else(|| "-".to_string()),
                key_parts
                    .map(|(_, usage, _, _, _)| usage_token(usage).to_string())
                    .unwrap_or_else(|| "-".to_string()),
                key_parts
                    .map(|(_, _, digits, _, _)| digits.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                key_parts
                    .map(|(_, _, _, scale, _)| scale.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                key_parts
                    .map(|(_, _, _, _, signed)| bool_token(signed).to_string())
                    .unwrap_or_else(|| "-".to_string()),
                state.released_records.len()
            ));
            for record in &state.released_records {
                out.push_str(&format!("SR {}\n", hex_encode(record)));
            }
            out.push_str(&format!("SS {}\n", state.sorted_records.len()));
            for record in &state.sorted_records {
                out.push_str(&format!("SR {}\n", hex_encode(record)));
            }
        }

        out.push_str(&format!("RERUN {}\n", self.rerun_checkpoints.len()));
        for checkpoint in &self.rerun_checkpoints {
            out.push_str(&format!(
                "R {} {} {} {}\n",
                hex_string(&checkpoint.checkpoint_file),
                hex_string(&checkpoint.watched_file),
                checkpoint.every_records,
                checkpoint.record_count
            ));
        }

        out.push_str(&format!("FILE {}\n", self.files.files.len()));
        for (name, file) in &self.files.files {
            let (kind, path, records) = match &file.backing {
                VmFileBacking::Memory { records } => ("MEM", String::new(), Some(records)),
                VmFileBacking::OsSequential { path, .. } => {
                    ("OS", path.to_string_lossy().to_string(), None)
                }
                VmFileBacking::Tape { path, records } => {
                    ("TAPE", path.to_string_lossy().to_string(), Some(records))
                }
            };
            let record_count = records.map(|records| records.len()).unwrap_or(0);
            out.push_str(&format!(
                "F {} {} {} {} {} {} {} {} {} {} {} {} {} {}\n",
                hex_string(name),
                kind,
                if path.is_empty() {
                    "-".to_string()
                } else {
                    hex_string(&path)
                },
                file.cursor,
                open_mode_token(file.open_mode),
                file.last_status
                    .as_deref()
                    .map(hex_string)
                    .unwrap_or_else(|| "-".to_string()),
                option_usize_token(file.last_record_index),
                option_usize_token(file.last_record_len),
                file.current_line,
                option_usize_token(file.linage),
                record_count,
                option_usize_token(file.fixed_record_len),
                file_fingerprint_token(file),
                platform_disposition_token(file.platform_disposition)
            ));
            if let Some(records) = records {
                for record in records {
                    out.push_str(&format!("FR {}\n", hex_encode(record)));
                }
            }
        }

        out.push_str(&format!(
            "RUNTIME {} {} {} {}\n",
            usize::from(self.trace_enabled),
            self.display.len(),
            self.active_file_error_declaratives.len(),
            self.active_debugging_declaratives.len()
        ));
        for line in &self.display {
            out.push_str(&format!("D {}\n", hex_string(line)));
        }
        for key in &self.active_file_error_declaratives {
            out.push_str(&format!("AFE {}\n", hex_string(key)));
        }
        for key in &self.active_debugging_declaratives {
            out.push_str(&format!("ADBG {}\n", hex_string(key)));
        }

        out.push_str("END\n");
        out.into_bytes()
    }

    pub fn restore_checkpoint_snapshot(&mut self, bytes: &[u8]) -> Result<(), VmError> {
        let mut candidate = self.clone_for_checkpoint_restore();
        candidate.restore_checkpoint_snapshot_in_place(bytes)?;
        candidate.reopen_restored_os_files()?;
        *self = candidate;
        Ok(())
    }

    fn clone_for_checkpoint_restore(&self) -> Self {
        Self {
            program: self.program.clone(),
            dialect: self.dialect.clone(),
            storage_pool: self.storage_pool.clone(),
            storage_descriptors: self.storage_descriptors.clone(),
            indexes: self.indexes.clone(),
            odo: self.odo.clone(),
            files: self.files.clone_for_checkpoint_restore(),
            file_status: self.file_status.clone(),
            registry: self.registry.clone(),
            activation_stack: self.activation_stack.clone(),
            last_abend_frame: self.last_abend_frame.clone(),
            alter_table: self.alter_table.clone(),
            rerun_checkpoints: self.rerun_checkpoints.clone(),
            file_error_declaratives: self.file_error_declaratives.clone(),
            active_file_error_declaratives: self.active_file_error_declaratives.clone(),
            debugging_declaratives: self.debugging_declaratives.clone(),
            active_debugging_declaratives: self.active_debugging_declaratives.clone(),
            trace_enabled: self.trace_enabled,
            checkpoint_in_progress: self.checkpoint_in_progress,
            sort_states: self.sort_states.clone(),
            display: self.display.clone(),
        }
    }

    fn restore_checkpoint_snapshot_in_place(&mut self, bytes: &[u8]) -> Result<(), VmError> {
        let text = std::str::from_utf8(bytes)
            .map_err(|error| checkpoint_error(format!("checkpoint is not UTF-8: {error}")))?;
        let mut lines = text.lines();
        if lines.next() != Some(CHECKPOINT_MAGIC) {
            return Err(checkpoint_error("invalid checkpoint magic"));
        }
        while let Some(line) = lines.next() {
            if line == "END" {
                return Ok(());
            }
            let parts = line.split_whitespace().collect::<Vec<_>>();
            match parts.as_slice() {
                ["DIALECT", name] => {
                    self.set_dialect(parse_dialect_profile_token(name)?);
                }
                ["STORAGE", count] => {
                    let count = parse_checkpoint_count(count)?;
                    for _ in 0..count {
                        let parts = next_checkpoint_parts(&mut lines)?;
                        let ["S", program, item, occurrence, bytes] = parts.as_slice() else {
                            return Err(checkpoint_error("invalid storage checkpoint row"));
                        };
                        let key = StorageKey::new(
                            unhex_string(program)?,
                            unhex_string(item)?,
                            parse_storage_key_occurrence(occurrence)?,
                        );
                        self.storage_pool
                            .define_or_write_cell(key, hex_decode(bytes)?)?;
                    }
                }
                ["INDEX", count] => {
                    self.indexes.clear();
                    let count = parse_checkpoint_count(count)?;
                    for _ in 0..count {
                        let parts = next_checkpoint_parts(&mut lines)?;
                        let ["I", name, table, min, max, occurrence] = parts.as_slice() else {
                            return Err(checkpoint_error("invalid index checkpoint row"));
                        };
                        let name = unhex_string(name)?;
                        self.indexes.insert(
                            name.clone(),
                            VmIndexState {
                                name,
                                table: unhex_string(table)?,
                                min: parse_checkpoint_count(min)?,
                                max: parse_checkpoint_count(max)?,
                                occurrence: parse_option_usize_token(occurrence)?,
                            },
                        );
                    }
                }
                ["ODO", count] => {
                    self.odo.clear();
                    let count = parse_checkpoint_count(count)?;
                    for _ in 0..count {
                        let parts = next_checkpoint_parts(&mut lines)?;
                        let ["O", name, program, table, depending_on, active, min, max] =
                            parts.as_slice()
                        else {
                            return Err(checkpoint_error("invalid ODO checkpoint row"));
                        };
                        let program = if *program == "-" {
                            None
                        } else {
                            Some(unhex_string(program)?)
                        };
                        let table = unhex_string(table)?;
                        let active = parse_checkpoint_count(active)?;
                        if let Some(program_name) = &program {
                            if self
                                .storage_pool
                                .odo_descriptor(program_name, &table)
                                .is_ok()
                            {
                                self.storage_pool
                                    .resize_odo_table(program_name, &table, active)?;
                            }
                        }
                        self.odo.insert(
                            unhex_string(name)?,
                            VmOdoState {
                                program,
                                table,
                                depending_on: unhex_string(depending_on)?,
                                active,
                                min: parse_checkpoint_count(min)?,
                                max: parse_checkpoint_count(max)?,
                            },
                        );
                    }
                }
                ["ALTER", count] => {
                    self.alter_table.clear();
                    let count = parse_checkpoint_count(count)?;
                    for _ in 0..count {
                        let parts = next_checkpoint_parts(&mut lines)?;
                        let ["A", slot, target] = parts.as_slice() else {
                            return Err(checkpoint_error("invalid ALTER checkpoint row"));
                        };
                        self.alter_table
                            .insert(unhex_string(slot)?, unhex_string(target)?);
                    }
                }
                ["SORT", count] => {
                    self.sort_states.clear();
                    let count = parse_checkpoint_count(count)?;
                    for _ in 0..count {
                        let parts = next_checkpoint_parts(&mut lines)?;
                        if parts.len() != 12 && parts.len() != 15 {
                            return Err(checkpoint_error("invalid SORT checkpoint row"));
                        }
                        if parts.first().copied() != Some("SORTSTATE") {
                            return Err(checkpoint_error("invalid SORT checkpoint row"));
                        }
                        let file = &parts[1];
                        let phase = &parts[2];
                        let record_target = &parts[3];
                        let record_len = &parts[4];
                        let cursor = &parts[5];
                        let key_offset = &parts[6];
                        let key_len = &parts[7];
                        let key_direction = &parts[8];
                        let key_category = &parts[9];
                        let key_usage = &parts[10];
                        let (key_digits, key_scale, key_signed, released_count) =
                            if parts.len() == 15 {
                                (parts[11], parts[12], parts[13], parts[14])
                            } else {
                                ("0", "0", "0", parts[11])
                            };
                        let released_count = parse_checkpoint_count(released_count)?;
                        let mut released_records = Vec::with_capacity(released_count);
                        for _ in 0..released_count {
                            let parts = next_checkpoint_parts(&mut lines)?;
                            let ["SR", bytes] = parts.as_slice() else {
                                return Err(checkpoint_error(
                                    "invalid SORT released record checkpoint row",
                                ));
                            };
                            released_records.push(hex_decode(bytes)?);
                        }
                        let parts = next_checkpoint_parts(&mut lines)?;
                        let ["SS", sorted_count] = parts.as_slice() else {
                            return Err(checkpoint_error("invalid SORT sorted count row"));
                        };
                        let sorted_count = parse_checkpoint_count(sorted_count)?;
                        let mut sorted_records = Vec::with_capacity(sorted_count);
                        for _ in 0..sorted_count {
                            let parts = next_checkpoint_parts(&mut lines)?;
                            let ["SR", bytes] = parts.as_slice() else {
                                return Err(checkpoint_error(
                                    "invalid SORT sorted record checkpoint row",
                                ));
                            };
                            sorted_records.push(hex_decode(bytes)?);
                        }
                        let key = if *key_offset == "-" {
                            None
                        } else {
                            Some(VmSortKeyDescriptor {
                                offset: parse_checkpoint_count(key_offset)?,
                                byte_len: parse_checkpoint_count(key_len)?,
                                direction: parse_sort_direction_token(key_direction)?,
                                encoding: parse_sort_key_encoding_checkpoint(
                                    key_category,
                                    key_usage,
                                    key_digits,
                                    key_scale,
                                    key_signed,
                                )?,
                            })
                        };
                        self.sort_states.push(VmSortState {
                            file: unhex_string(file)?,
                            phase: parse_sort_phase_token(phase)?,
                            record: VmAccessPath {
                                target: unhex_string(record_target)?,
                                condition_name: None,
                                subscripts: Vec::new(),
                                reference_modifier: None,
                                result_len: None,
                            },
                            record_len: parse_checkpoint_count(record_len)?,
                            released_records,
                            sorted_records,
                            cursor: parse_checkpoint_count(cursor)?,
                            key,
                        });
                    }
                }
                ["RERUN", count] => {
                    self.rerun_checkpoints.clear();
                    let count = parse_checkpoint_count(count)?;
                    for _ in 0..count {
                        let parts = next_checkpoint_parts(&mut lines)?;
                        let ["R", checkpoint_file, watched_file, every_records, record_count] =
                            parts.as_slice()
                        else {
                            return Err(checkpoint_error("invalid RERUN checkpoint row"));
                        };
                        let watched_file = unhex_string(watched_file)?;
                        self.rerun_checkpoints.push(VmRerunCheckpoint {
                            checkpoint_file: unhex_string(checkpoint_file)?,
                            watched_key: normalize_vm_key(&watched_file),
                            watched_file,
                            every_records: parse_checkpoint_count(every_records)?,
                            record_count: parse_checkpoint_count(record_count)?,
                        });
                    }
                }
                ["FILE", count] => {
                    let count = parse_checkpoint_count(count)?;
                    for _ in 0..count {
                        let parts = next_checkpoint_parts(&mut lines)?;
                        let ["F", name, kind, path, cursor, open_mode, status, last_record_index, last_record_len, current_line, linage, record_count, tail @ ..] =
                            parts.as_slice()
                        else {
                            return Err(checkpoint_error("invalid file checkpoint row"));
                        };
                        let (fixed_record_len, fingerprint, platform_disposition) = match tail {
                            [] => ("-", "-", None),
                            [fingerprint] => ("-", *fingerprint, None),
                            [fixed_record_len, fingerprint] => {
                                (*fixed_record_len, *fingerprint, None)
                            }
                            [fixed_record_len, fingerprint, platform_disposition] => {
                                (*fixed_record_len, *fingerprint, Some(*platform_disposition))
                            }
                            _ => return Err(checkpoint_error("invalid file checkpoint tail")),
                        };
                        let name = unhex_string(name)?;
                        let snapshot_path = if *path == "-" {
                            None
                        } else {
                            Some(PathBuf::from(unhex_string(path)?))
                        };
                        let record_count = parse_checkpoint_count(record_count)?;
                        let mut records = Vec::with_capacity(record_count);
                        for _ in 0..record_count {
                            let parts = next_checkpoint_parts(&mut lines)?;
                            let ["FR", bytes] = parts.as_slice() else {
                                return Err(checkpoint_error("invalid file record checkpoint row"));
                            };
                            records.push(hex_decode(bytes)?);
                        }
                        if let Some(file) = self.files.files.get_mut(&name) {
                            file.cursor = parse_checkpoint_count(cursor)?;
                            file.open_mode = parse_open_mode_token(open_mode)?;
                            file.last_status = if *status == "-" {
                                None
                            } else {
                                Some(unhex_string(status)?)
                            };
                            file.last_record_index = parse_option_usize_token(last_record_index)?;
                            file.last_record_len = parse_option_usize_token(last_record_len)?;
                            file.fixed_record_len = parse_option_usize_token(fixed_record_len)?;
                            if let Some(platform_disposition) = platform_disposition {
                                file.platform_disposition =
                                    parse_platform_disposition_token(platform_disposition)?;
                            }
                            file.current_line = parse_checkpoint_count(current_line)?;
                            file.linage = parse_option_usize_token(linage)?;
                            match (&mut file.backing, *kind) {
                                (VmFileBacking::Memory { records: target }, "MEM") => {
                                    *target = records;
                                }
                                (
                                    VmFileBacking::Tape {
                                        path: target_path,
                                        records: target,
                                        ..
                                    },
                                    "TAPE",
                                ) => {
                                    if let Some(snapshot_path) = snapshot_path.clone() {
                                        *target_path = snapshot_path;
                                    }
                                    *target = records;
                                }
                                (
                                    VmFileBacking::OsSequential {
                                        path: target_path,
                                        handle,
                                    },
                                    "OS",
                                ) => {
                                    let snapshot_path = snapshot_path.clone().ok_or_else(|| {
                                        checkpoint_error(format!(
                                            "OS file {name} checkpoint row is missing path"
                                        ))
                                    })?;
                                    validate_os_file_fingerprint(
                                        &name,
                                        &snapshot_path,
                                        fingerprint,
                                    )?;
                                    *target_path = snapshot_path;
                                    *handle = None;
                                }
                                (_, other) => {
                                    return Err(checkpoint_error(format!(
                                        "file {name} backing mismatch for snapshot kind {other}"
                                    )));
                                }
                            }
                        } else {
                            let backing = match *kind {
                                "MEM" => VmFileBacking::Memory { records },
                                "TAPE" => VmFileBacking::Tape {
                                    path: snapshot_path.unwrap_or_default(),
                                    records,
                                },
                                "OS" => {
                                    let path = snapshot_path.ok_or_else(|| {
                                        checkpoint_error(format!(
                                            "OS file {name} checkpoint row is missing path"
                                        ))
                                    })?;
                                    validate_os_file_fingerprint(&name, &path, fingerprint)?;
                                    VmFileBacking::OsSequential { path, handle: None }
                                }
                                other => {
                                    return Err(checkpoint_error(format!(
                                        "unknown file checkpoint kind {other}"
                                    )));
                                }
                            };
                            self.files.files.insert(
                                name.clone(),
                                VmFile {
                                    name: name.clone(),
                                    organization: VmFileOrganization::Sequential,
                                    backing,
                                    cursor: parse_checkpoint_count(cursor)?,
                                    open_mode: parse_open_mode_token(open_mode)?,
                                    last_status: if *status == "-" {
                                        None
                                    } else {
                                        Some(unhex_string(status)?)
                                    },
                                    last_record_index: parse_option_usize_token(last_record_index)?,
                                    last_record_len: parse_option_usize_token(last_record_len)?,
                                    fixed_record_len: parse_option_usize_token(fixed_record_len)?,
                                    platform_disposition: match platform_disposition {
                                        Some(token) => parse_platform_disposition_token(token)?,
                                        None => None,
                                    },
                                    linage: parse_option_usize_token(linage)?,
                                    current_line: parse_checkpoint_count(current_line)?,
                                },
                            );
                            self.files.aliases.insert(normalize_vm_key(&name), name);
                        }
                    }
                }
                ["RUNTIME", trace_enabled, display_count, active_file_count, active_debug_count] => {
                    self.trace_enabled = *trace_enabled == "1";
                    self.display.clear();
                    self.active_file_error_declaratives.clear();
                    self.active_debugging_declaratives.clear();
                    for _ in 0..parse_checkpoint_count(display_count)? {
                        let parts = next_checkpoint_parts(&mut lines)?;
                        let ["D", line] = parts.as_slice() else {
                            return Err(checkpoint_error("invalid display checkpoint row"));
                        };
                        self.display.push(unhex_string(line)?);
                    }
                    for _ in 0..parse_checkpoint_count(active_file_count)? {
                        let parts = next_checkpoint_parts(&mut lines)?;
                        let ["AFE", key] = parts.as_slice() else {
                            return Err(checkpoint_error(
                                "invalid active file declarative checkpoint row",
                            ));
                        };
                        self.active_file_error_declaratives
                            .insert(unhex_string(key)?);
                    }
                    for _ in 0..parse_checkpoint_count(active_debug_count)? {
                        let parts = next_checkpoint_parts(&mut lines)?;
                        let ["ADBG", key] = parts.as_slice() else {
                            return Err(checkpoint_error(
                                "invalid active debugging declarative checkpoint row",
                            ));
                        };
                        self.active_debugging_declaratives
                            .insert(unhex_string(key)?);
                    }
                }
                _ => {
                    return Err(checkpoint_error(format!(
                        "invalid checkpoint section {line}"
                    )))
                }
            }
        }
        Err(checkpoint_error("checkpoint ended without END marker"))
    }

    fn reopen_restored_os_files(&mut self) -> Result<(), VmError> {
        for (name, file) in &mut self.files.files {
            let VmFileBacking::OsSequential { path, handle } = &mut file.backing else {
                continue;
            };
            *handle = None;
            let Some(mode) = file.open_mode else {
                continue;
            };
            let mut opened = match mode {
                VmOpenMode::Input => OpenOptions::new().read(true).open(&*path),
                VmOpenMode::Io => OpenOptions::new().read(true).write(true).open(&*path),
                VmOpenMode::Output => OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&*path),
                VmOpenMode::Extend => OpenOptions::new().append(true).create(true).open(&*path),
            }
            .map_err(|error| VmError::FileRuntime {
                name: name.clone(),
                message: format!(
                    "failed to reopen OS sequential file after checkpoint restore: {error}"
                ),
            })?;
            match mode {
                VmOpenMode::Input | VmOpenMode::Io => {
                    let offset = restored_os_file_offset(
                        name,
                        &*path,
                        file.cursor,
                        file.fixed_record_len,
                        file.last_record_len,
                    )?;
                    opened
                        .seek(SeekFrom::Start(offset))
                        .map_err(|error| VmError::FileRuntime {
                            name: name.clone(),
                            message: format!(
                                "failed to seek OS sequential file after checkpoint restore: {error}"
                            ),
                        })?;
                }
                VmOpenMode::Output | VmOpenMode::Extend => {
                    opened
                        .seek(SeekFrom::End(0))
                        .map_err(|error| VmError::FileRuntime {
                            name: name.clone(),
                            message: format!(
                                "failed to seek OS sequential file after checkpoint restore: {error}"
                            ),
                        })?;
                }
            }
            *handle = Some(opened);
        }
        Ok(())
    }

    pub fn restore_last_rerun_checkpoint(
        &mut self,
        checkpoint_file: &str,
    ) -> Result<bool, VmError> {
        let records = self.files.checkpoint_records(checkpoint_file)?;
        if let Some(record) = records
            .iter()
            .rev()
            .find(|record| record.starts_with(CHECKPOINT_MAGIC.as_bytes()))
        {
            self.restore_checkpoint_snapshot(record)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn bind_odo_storage_table(
        &mut self,
        target: impl Into<String>,
        program: impl Into<String>,
        table: impl Into<String>,
    ) {
        self.bind_occurs_storage_cell(target, program, table);
    }

    pub fn define_storage_cell(&mut self, key: StorageKey, bytes: Vec<u8>) -> Result<(), VmError> {
        self.storage_pool.define_cell(key, bytes)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn define_odo_storage_table(
        &mut self,
        program: impl Into<String>,
        table: impl Into<String>,
        depending_on: StorageKey,
        element_len: usize,
        min: usize,
        max: usize,
        active: usize,
    ) -> Result<(), VmError> {
        let program = program.into();
        let table = table.into();
        self.storage_pool.define_odo_table(
            program.clone(),
            table.clone(),
            depending_on,
            element_len,
            min,
            max,
            active,
        )?;
        self.bind_occurs_storage_cell(table.clone(), program, table);
        Ok(())
    }

    pub fn define_index(
        &mut self,
        name: impl Into<String>,
        table: impl Into<String>,
        min: usize,
        max: usize,
    ) {
        let name = name.into();
        self.indexes.insert(
            name.clone(),
            VmIndexState {
                name,
                table: table.into(),
                occurrence: None,
                min,
                max,
            },
        );
    }

    pub fn set_index(&mut self, name: &str, occurrence: usize) -> Result<(), VmError> {
        let name = self.resolve_index_key(name);
        let index = self
            .indexes
            .get_mut(&name)
            .ok_or_else(|| VmError::UnsupportedIndex { name: name.clone() })?;
        if occurrence < index.min || occurrence > index.max {
            return Err(VmError::InvalidSubscript {
                target: index.table.clone(),
                value: occurrence as i128,
                min: index.min,
                max: index.max,
            });
        }
        index.occurrence = Some(occurrence);
        Ok(())
    }

    pub fn adjust_index(&mut self, name: &str, delta: i128) -> Result<(), VmError> {
        let current = self.index_occurrence(name)? as i128;
        let next = current.saturating_add(delta);
        if next <= 0 {
            return Err(VmError::InvalidSubscript {
                target: name.to_string(),
                value: next,
                min: 1,
                max: usize::MAX,
            });
        }
        self.set_index(name, next as usize)
    }

    pub fn index_occurrence(&self, name: &str) -> Result<usize, VmError> {
        let name = self.resolve_index_key(name);
        self.indexes
            .get(&name)
            .and_then(|index| index.occurrence)
            .ok_or(VmError::UnsupportedIndex { name })
    }

    pub fn define_odo(
        &mut self,
        table: impl Into<String>,
        depending_on: impl Into<String>,
        min: usize,
        max: usize,
        active: usize,
    ) -> Result<(), VmError> {
        let table = table.into();
        if active < min || active > max {
            return Err(VmError::OdoRuntime {
                table,
                message: format!("active count {active} is outside {min}..={max}"),
            });
        }
        self.odo.insert(
            table.clone(),
            VmOdoState {
                program: None,
                table,
                depending_on: depending_on.into(),
                active,
                min,
                max,
            },
        );
        Ok(())
    }

    pub fn define_odo_for_program(
        &mut self,
        program: impl Into<String>,
        table: impl Into<String>,
        depending_on: impl Into<String>,
        min: usize,
        max: usize,
        active: usize,
    ) -> Result<(), VmError> {
        let program = program.into();
        let table = table.into();
        if active < min || active > max {
            return Err(VmError::OdoRuntime {
                table,
                message: format!("active count {active} is outside {min}..={max}"),
            });
        }
        self.odo.insert(
            scoped_runtime_name(&program, &table),
            VmOdoState {
                program: Some(program),
                table,
                depending_on: depending_on.into(),
                active,
                min,
                max,
            },
        );
        Ok(())
    }

    pub fn set_odo_active(&mut self, table: &str, active: usize) -> Result<(), VmError> {
        let table_key = self.resolve_odo_key(table);
        self.set_odo_active_by_key(&table_key, active)
    }

    fn set_odo_active_by_key(&mut self, table_key: &str, active: usize) -> Result<(), VmError> {
        let odo = self
            .odo
            .get_mut(table_key)
            .ok_or_else(|| VmError::OdoRuntime {
                table: table_key.to_string(),
                message: "ODO descriptor is not defined".to_string(),
            })?;
        if active < odo.min || active > odo.max {
            return Err(VmError::OdoRuntime {
                table: odo.table.clone(),
                message: format!("active count {active} is outside {}..={}", odo.min, odo.max),
            });
        }
        odo.active = active;
        let scoped_pool_table = odo
            .program
            .as_ref()
            .map(|program| (program.clone(), odo.table.clone()));
        let table = odo.table.clone();
        if let Some((program, table)) = scoped_pool_table {
            self.storage_pool
                .resize_odo_table(&program, &table, active)?;
            return Ok(());
        }
        if self
            .storage_pool
            .resize_odo_tables_for_name(&table, active)?
        {
            return Ok(());
        }
        let pool_tables = self
            .storage_descriptors
            .values()
            .filter_map(|descriptor| match descriptor {
                VmBinding::OccursCell {
                    program,
                    item: pool_item,
                } if pool_item.eq_ignore_ascii_case(&table) => {
                    Some((program.clone(), pool_item.clone()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        for (program, pool_table) in pool_tables {
            if self
                .storage_pool
                .odo_descriptor(&program, &pool_table)
                .is_ok()
            {
                self.storage_pool
                    .resize_odo_table(&program, &pool_table, active)?;
            }
        }
        Ok(())
    }

    pub fn eval_condition(&self, condition: &VmCondition) -> Result<bool, VmError> {
        let condition = self.materialize_condition(condition)?;
        self.eval_condition_runtime(&condition)
    }

    pub fn eval_expr(&self, expr: &VmExpr) -> Result<VmEvaluatedValue, VmError> {
        let expr = self.materialize_expr(expr)?;
        self.eval_expr_runtime(&expr)
    }

    pub fn eval_evaluate(&self, evaluate: &VmEvaluate) -> Result<Option<usize>, VmError> {
        let evaluate = self.materialize_evaluate(evaluate)?;
        self.eval_evaluate_runtime(&evaluate)
    }

    pub fn set_condition_name_at(&mut self, name: &str) -> Result<(), VmError> {
        let condition = self.program.condition(name)?.clone();
        let first = condition
            .values
            .first()
            .ok_or_else(|| VmError::UnsupportedOperand {
                message: format!("condition name {name} has no values"),
            })?;
        let value = match first {
            VmConditionValue::Single(value) => value.clone(),
            VmConditionValue::Range { start, .. } => start.clone(),
        };
        let field = self.field_for_target(&condition.parent)?.clone();
        if let Some(children) = self
            .program
            .condition_declared_view(&condition)
            .map(|view| view.children.clone())
        {
            if !children.is_empty() {
                match field.category {
                    VmCategory::Alphanumeric | VmCategory::Alphabetic | VmCategory::Group => {
                        return self.write_declared_view_bytes_runtime(
                            &children,
                            &[],
                            value.as_bytes(),
                        );
                    }
                    _ => {}
                }
            }
        }
        let target = VmAccessPath {
            target: condition.parent,
            condition_name: None,
            subscripts: Vec::new(),
            reference_modifier: None,
            result_len: None,
        };
        let value = if is_numeric(field.category) {
            VmEvaluatedValue {
                value: VmValue::Decimal(parse_decimal(&value)?),
                category: field.category,
                byte_len: field.byte_len,
            }
        } else {
            VmEvaluatedValue {
                value: VmValue::Text(value),
                category: field.category,
                byte_len: field.byte_len,
            }
        };
        self.write_value_to_access_path(&target, &value)
    }

    fn eval_condition_runtime(&self, condition: &VmCondition) -> Result<bool, VmError> {
        match condition {
            VmCondition::Relation { left, op, right } => {
                let left = self.eval_expr(left)?;
                let right = self.eval_expr(right)?;
                let ordering = self.program.compare_values(&left, &right)?;
                Ok(match op {
                    VmRelOp::Equal => ordering == Ordering::Equal,
                    VmRelOp::NotEqual => ordering != Ordering::Equal,
                    VmRelOp::Greater => ordering == Ordering::Greater,
                    VmRelOp::GreaterOrEqual => {
                        matches!(ordering, Ordering::Greater | Ordering::Equal)
                    }
                    VmRelOp::Less => ordering == Ordering::Less,
                    VmRelOp::LessOrEqual => matches!(ordering, Ordering::Less | Ordering::Equal),
                })
            }
            VmCondition::ClassTest {
                operand,
                class,
                negated,
            } => {
                let result = self.eval_class_test_runtime(operand, *class)?;
                Ok(if *negated { !result } else { result })
            }
            VmCondition::SignTest {
                operand,
                sign,
                negated,
            } => {
                let value = to_decimal(&self.eval_expr(operand)?)?;
                let zero = Decimal::ZERO;
                let result = match sign {
                    VmSignTest::Positive => value > zero,
                    VmSignTest::Negative => value < zero,
                    VmSignTest::Zero => value == zero,
                };
                Ok(if *negated { !result } else { result })
            }
            VmCondition::ConditionName { reference } => self.eval_condition_name_runtime(reference),
            VmCondition::Not(inner) => Ok(!self.eval_condition_runtime(inner)?),
            VmCondition::And(left, right) => {
                if !self.eval_condition_runtime(left)? {
                    return Ok(false);
                }
                self.eval_condition_runtime(right)
            }
            VmCondition::Or(left, right) => {
                if self.eval_condition_runtime(left)? {
                    return Ok(true);
                }
                self.eval_condition_runtime(right)
            }
        }
    }

    fn eval_expr_runtime(&self, expr: &VmExpr) -> Result<VmEvaluatedValue, VmError> {
        match expr {
            VmExpr::Access(path) => {
                if self.storage_descriptor_for(&path.target).is_some() {
                    self.eval_storage_pool_access_path(path)
                } else if !self.program.condition_candidates(&path.target).is_empty() {
                    Ok(VmEvaluatedValue {
                        value: VmValue::Bool(self.eval_condition_name_runtime(&path.target)?),
                        category: VmCategory::Unsupported,
                        byte_len: 1,
                    })
                } else {
                    Err(VmError::StoragePool {
                        key: path.target.clone(),
                        message: "access path has no StoragePool descriptor".to_string(),
                    })
                }
            }
            VmExpr::Function { function, args } => self.eval_runtime_function(*function, args),
            VmExpr::Condition(condition) => Ok(VmEvaluatedValue {
                value: VmValue::Bool(self.eval_condition_runtime(condition)?),
                category: VmCategory::Unsupported,
                byte_len: 1,
            }),
            VmExpr::Add(left, right) => {
                let left = to_decimal(&self.eval_expr(left)?)?;
                let right = to_decimal(&self.eval_expr(right)?)?;
                Ok(VmEvaluatedValue {
                    value: VmValue::Decimal(left + right),
                    category: VmCategory::NumericDisplay,
                    byte_len: 0,
                })
            }
            VmExpr::Subtract(left, right) => {
                let left = to_decimal(&self.eval_expr(left)?)?;
                let right = to_decimal(&self.eval_expr(right)?)?;
                Ok(VmEvaluatedValue {
                    value: VmValue::Decimal(left - right),
                    category: VmCategory::NumericDisplay,
                    byte_len: 0,
                })
            }
            VmExpr::Multiply(left, right) => {
                let left = to_decimal(&self.eval_expr(left)?)?;
                let right = to_decimal(&self.eval_expr(right)?)?;
                Ok(VmEvaluatedValue {
                    value: VmValue::Decimal(left * right),
                    category: VmCategory::NumericDisplay,
                    byte_len: 0,
                })
            }
            VmExpr::Divide(left, right) => {
                let left = to_decimal(&self.eval_expr(left)?)?;
                let right = to_decimal(&self.eval_expr(right)?)?;
                if right.is_zero() {
                    return Err(VmError::InvalidDecimal {
                        value: "division by zero".to_string(),
                    });
                }
                Ok(VmEvaluatedValue {
                    value: VmValue::Decimal(left / right),
                    category: VmCategory::NumericDisplay,
                    byte_len: 0,
                })
            }
            VmExpr::Identifier(name) => self.eval_identifier_runtime(name),
            other => self.program.eval_expr(&[], other),
        }
    }

    fn eval_evaluate_runtime(&self, evaluate: &VmEvaluate) -> Result<Option<usize>, VmError> {
        let subjects = evaluate
            .subjects
            .iter()
            .map(|subject| self.eval_expr(subject))
            .collect::<Result<Vec<_>, _>>()?;
        for (idx, branch) in evaluate.branches.iter().enumerate() {
            if branch.patterns.len() != subjects.len() {
                return Err(VmError::UnsupportedOperand {
                    message: format!(
                        "EVALUATE branch has {} patterns for {} subjects",
                        branch.patterns.len(),
                        subjects.len()
                    ),
                });
            }
            let mut matched = true;
            for (subject, pattern) in subjects.iter().zip(&branch.patterns) {
                if !self.match_evaluate_pattern_runtime(subject, pattern)? {
                    matched = false;
                    break;
                }
            }
            if matched {
                return Ok(Some(idx));
            }
        }
        Ok(None)
    }

    fn match_evaluate_pattern_runtime(
        &self,
        subject: &VmEvaluatedValue,
        pattern: &VmEvaluatePattern,
    ) -> Result<bool, VmError> {
        match pattern {
            VmEvaluatePattern::Any => Ok(true),
            VmEvaluatePattern::Operand(operand) => {
                let value = self.eval_expr(operand)?;
                self.program.values_equal(subject, &value)
            }
            VmEvaluatePattern::Range { start, end } => {
                let start = self.eval_expr(start)?;
                let end = self.eval_expr(end)?;
                self.program.value_in_range(subject, &start, &end)
            }
            VmEvaluatePattern::Condition(condition) => {
                let result = self.eval_condition_runtime(condition)?;
                if matches!(subject.value, VmValue::Bool(_)) {
                    self.program.values_equal(
                        subject,
                        &VmEvaluatedValue {
                            value: VmValue::Bool(result),
                            category: VmCategory::Unsupported,
                            byte_len: 1,
                        },
                    )
                } else {
                    Ok(result)
                }
            }
        }
    }

    fn eval_runtime_function(
        &self,
        function: VmFunction,
        args: &[VmExpr],
    ) -> Result<VmEvaluatedValue, VmError> {
        match function {
            VmFunction::Length => {
                let arg = args.first().ok_or_else(|| VmError::UnsupportedOperand {
                    message: "FUNCTION LENGTH requires one argument".to_string(),
                })?;
                let value = self.eval_expr(arg)?;
                Ok(VmEvaluatedValue {
                    value: VmValue::Integer(value.byte_len as i64),
                    category: VmCategory::NumericDisplay,
                    byte_len: 0,
                })
            }
            VmFunction::Ord => {
                let arg = args.first().ok_or_else(|| VmError::UnsupportedOperand {
                    message: "FUNCTION ORD requires one argument".to_string(),
                })?;
                let value = self.eval_expr(arg)?;
                let text = value_text(&value).ok_or_else(|| VmError::UnsupportedOperand {
                    message: "FUNCTION ORD argument is not text".to_string(),
                })?;
                let code = text.bytes().next().unwrap_or(0) as i64;
                Ok(VmEvaluatedValue {
                    value: VmValue::Integer(code),
                    category: VmCategory::NumericDisplay,
                    byte_len: 0,
                })
            }
            VmFunction::Numval => {
                let arg = args.first().ok_or_else(|| VmError::UnsupportedOperand {
                    message: "FUNCTION NUMVAL requires one argument".to_string(),
                })?;
                let value = self.eval_expr(arg)?;
                let text = match value.value {
                    VmValue::Text(text) => text,
                    VmValue::Decimal(value) => value.to_string(),
                    other => {
                        return Err(VmError::UnsupportedOperand {
                            message: format!("FUNCTION NUMVAL cannot parse {other:?}"),
                        })
                    }
                };
                Ok(VmEvaluatedValue {
                    value: VmValue::Decimal(parse_decimal(text.trim())?),
                    category: VmCategory::NumericDisplay,
                    byte_len: 0,
                })
            }
            VmFunction::UserDefined => Err(VmError::UnsupportedFunction {
                name: "user-defined".to_string(),
            }),
        }
    }

    fn eval_identifier_runtime(&self, name: &str) -> Result<VmEvaluatedValue, VmError> {
        if !self.program.condition_candidates(name).is_empty() {
            return Ok(VmEvaluatedValue {
                value: VmValue::Bool(self.eval_condition_name_runtime(name)?),
                category: VmCategory::Unsupported,
                byte_len: 1,
            });
        }
        self.eval_expr_runtime(&VmExpr::Access(VmAccessPath {
            target: name.to_string(),
            condition_name: None,
            subscripts: Vec::new(),
            reference_modifier: None,
            result_len: None,
        }))
    }

    fn eval_condition_name_runtime(&self, name: &str) -> Result<bool, VmError> {
        let condition = self.program.condition(name)?;
        let parent = self.eval_condition_parent_runtime(condition, &[])?;
        self.condition_matches_value(condition, &parent)
    }

    fn eval_condition_parent_runtime(
        &self,
        condition: &VmConditionName,
        subscripts: &[VmSubscript],
    ) -> Result<VmEvaluatedValue, VmError> {
        let Some(view) = self.program.condition_declared_view(condition) else {
            return self.eval_identifier_runtime(&condition.parent);
        };
        if view.children.is_empty() {
            return self.eval_identifier_runtime(&condition.parent);
        }
        let bytes = self.read_declared_view_bytes_runtime(&view.children, subscripts)?;
        let field = self.field_for_target(&view.parent)?.clone();
        let mut decode_field = field;
        decode_field.offset = 0;
        decode_field.byte_len = bytes.len();
        self.program
            .decode_field_value(&decode_field, decode_field.category, &bytes)
    }

    fn condition_matches_value(
        &self,
        condition: &VmConditionName,
        parent: &VmEvaluatedValue,
    ) -> Result<bool, VmError> {
        for expected in &condition.values {
            match expected {
                VmConditionValue::Single(value) => {
                    let expected = self.program.literal_for_category(
                        value,
                        parent.category,
                        parent.byte_len,
                    )?;
                    if self.program.values_equal(parent, &expected)? {
                        return Ok(true);
                    }
                }
                VmConditionValue::Range { start, end } => {
                    let start = self.program.literal_for_category(
                        start,
                        parent.category,
                        parent.byte_len,
                    )?;
                    let end =
                        self.program
                            .literal_for_category(end, parent.category, parent.byte_len)?;
                    if self.program.value_in_range(parent, &start, &end)? {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }

    fn eval_class_test_runtime(
        &self,
        operand: &VmOperand,
        class: VmClassTest,
    ) -> Result<bool, VmError> {
        let bytes = self.read_operand_bytes_runtime(operand).ok();
        let value = self.eval_expr(operand)?;
        Ok(match class {
            VmClassTest::Numeric => {
                if let Some(bytes) = bytes {
                    match value.category {
                        VmCategory::NumericDisplay => {
                            let field_name = operand_target_name(operand).unwrap_or_default();
                            let field = self.field_for_target(&field_name)?;
                            self.program.display_bytes_numeric(field, &bytes)
                        }
                        VmCategory::PackedDecimal => {
                            let field_name = operand_target_name(operand).unwrap_or_default();
                            let field = self.field_for_target(&field_name)?;
                            self.program.packed_bytes_numeric(field, &bytes)
                        }
                        VmCategory::Binary | VmCategory::NativeBinary => true,
                        _ => false,
                    }
                } else {
                    to_decimal(&value).is_ok()
                }
            }
            VmClassTest::Alphabetic
            | VmClassTest::AlphabeticUpper
            | VmClassTest::AlphabeticLower => value_text(&value)
                .map(|text| alphabetic_text(&text, class))
                .unwrap_or(false),
        })
    }

    fn read_operand_bytes_runtime(&self, operand: &VmOperand) -> Result<Vec<u8>, VmError> {
        match operand {
            VmExpr::Access(path) => self.read_bytes_from_access_path(path),
            VmExpr::Identifier(name) => self.read_bytes_from_access_path(&VmAccessPath {
                target: name.clone(),
                condition_name: None,
                subscripts: Vec::new(),
                reference_modifier: None,
                result_len: None,
            }),
            _ => Err(VmError::UnsupportedOperand {
                message: "operand does not have storage bytes".to_string(),
            }),
        }
    }

    pub fn move_value_to_access_path(
        &mut self,
        source: &VmExpr,
        target: &VmAccessPath,
    ) -> Result<(), VmError> {
        if let VmExpr::Access(source_path) = source {
            let source_path = match self.materialize_expr(&VmExpr::Access(source_path.clone()))? {
                VmExpr::Access(path) => path,
                _ => source_path.clone(),
            };
            let target_path = match self.materialize_expr(&VmExpr::Access(target.clone()))? {
                VmExpr::Access(path) => path,
                _ => target.clone(),
            };
            if self.access_path_is_group(&source_path) || self.access_path_is_group(&target_path) {
                let bytes = self.read_bytes_from_storage_pool_access_path(&source_path)?;
                return self.write_bytes_to_storage_pool_access_path(&target_path, &bytes);
            }
        }
        if let VmExpr::Figurative(figurative) = source {
            let target_path = match self.materialize_expr(&VmExpr::Access(target.clone()))? {
                VmExpr::Access(path) => path,
                _ => target.clone(),
            };
            let field = self.field_for_target(&target_path.target)?;
            if matches!(
                field.category,
                VmCategory::Group
                    | VmCategory::Alphanumeric
                    | VmCategory::Alphabetic
                    | VmCategory::NumericEdited
            ) {
                let len = self.read_bytes_from_access_path(&target_path)?.len();
                let bytes = vec![figurative_display_byte(*figurative); len];
                return self.write_bytes_to_access_path(&target_path, &bytes);
            }
        }
        let source = self.eval_expr(source)?;
        self.write_value_to_access_path(target, &source)
    }

    fn numeric_update_access_path<F>(
        &mut self,
        source: &VmExpr,
        target: &VmAccessPath,
        update: F,
    ) -> Result<(), VmError>
    where
        F: FnOnce(Decimal, Decimal) -> Decimal,
    {
        let current = to_decimal(&self.eval_expr(&VmExpr::Access(target.clone()))?)?;
        let source = to_decimal(&self.eval_expr(source)?)?;
        let next = update(current, source);
        self.write_value_to_access_path(
            target,
            &VmEvaluatedValue {
                value: VmValue::Decimal(next),
                category: VmCategory::NumericDisplay,
                byte_len: 0,
            },
        )
    }

    fn compute_op(
        &mut self,
        target: &VmAccessPath,
        expr: &VmExpr,
        rounded: bool,
    ) -> Result<bool, VmError> {
        let Some(value) = self.eval_decimal_expr_checked(expr)? else {
            return Ok(true);
        };
        let value = if rounded {
            self.round_decimal_for_access_path(target, value)?
        } else {
            value
        };
        if !self.decimal_fits_access_path(target, value)? {
            return Ok(true);
        }
        let field = self.field_for_target(&target.target)?.clone();
        self.write_value_to_access_path(
            target,
            &VmEvaluatedValue {
                value: VmValue::Decimal(value),
                category: field.category,
                byte_len: field.byte_len,
            },
        )?;
        Ok(false)
    }

    fn round_decimal_for_access_path(
        &self,
        target: &VmAccessPath,
        value: Decimal,
    ) -> Result<Decimal, VmError> {
        let materialized = self.materialize_expr(&VmExpr::Access(target.clone()))?;
        let VmExpr::Access(path) = materialized else {
            return Err(VmError::UnsupportedOperand {
                message: "COMPUTE target did not materialize to an access path".to_string(),
            });
        };
        let field = self.field_for_target(&path.target)?;
        if !matches!(
            field.category,
            VmCategory::NumericDisplay | VmCategory::PackedDecimal
        ) {
            return Ok(value);
        }
        let Some(picture) = &field.picture else {
            return Ok(value);
        };
        Ok(value.round_dp_with_strategy(picture.scale, RoundingStrategy::MidpointAwayFromZero))
    }

    fn eval_decimal_expr_checked(&self, expr: &VmExpr) -> Result<Option<Decimal>, VmError> {
        let expr = self.materialize_expr(expr)?;
        self.eval_decimal_expr_checked_runtime(&expr)
    }

    fn eval_decimal_expr_checked_runtime(&self, expr: &VmExpr) -> Result<Option<Decimal>, VmError> {
        match expr {
            VmExpr::Add(left, right) => {
                let Some(left) = self.eval_decimal_expr_checked_runtime(left)? else {
                    return Ok(None);
                };
                let Some(right) = self.eval_decimal_expr_checked_runtime(right)? else {
                    return Ok(None);
                };
                Ok(left.checked_add(right))
            }
            VmExpr::Subtract(left, right) => {
                let Some(left) = self.eval_decimal_expr_checked_runtime(left)? else {
                    return Ok(None);
                };
                let Some(right) = self.eval_decimal_expr_checked_runtime(right)? else {
                    return Ok(None);
                };
                Ok(left.checked_sub(right))
            }
            VmExpr::Multiply(left, right) => {
                let Some(left) = self.eval_decimal_expr_checked_runtime(left)? else {
                    return Ok(None);
                };
                let Some(right) = self.eval_decimal_expr_checked_runtime(right)? else {
                    return Ok(None);
                };
                Ok(left.checked_mul(right))
            }
            VmExpr::Divide(left, right) => {
                let Some(left) = self.eval_decimal_expr_checked_runtime(left)? else {
                    return Ok(None);
                };
                let Some(right) = self.eval_decimal_expr_checked_runtime(right)? else {
                    return Ok(None);
                };
                if right.is_zero() {
                    return Ok(None);
                }
                Ok(left.checked_div(right))
            }
            other => Ok(Some(to_decimal(&self.eval_expr_runtime(other)?)?)),
        }
    }

    fn decimal_fits_access_path(
        &self,
        target: &VmAccessPath,
        value: Decimal,
    ) -> Result<bool, VmError> {
        let materialized = self.materialize_expr(&VmExpr::Access(target.clone()))?;
        let VmExpr::Access(path) = materialized else {
            return Err(VmError::UnsupportedOperand {
                message: "COMPUTE target did not materialize to an access path".to_string(),
            });
        };
        let field = self.field_for_target(&path.target)?;
        match field.category {
            VmCategory::NumericDisplay => Ok(field
                .picture
                .as_ref()
                .map(|picture| decimal_fits_picture(value, picture))
                .unwrap_or_else(|| decimal_fits_display_width(value, field.byte_len))),
            VmCategory::PackedDecimal => Ok(self.program.encode_packed(field, value).is_ok()),
            VmCategory::Binary | VmCategory::NativeBinary => {
                Ok(self.program.encode_binary(field, value).is_ok())
            }
            VmCategory::Float => Ok(decimal_to_f64(value)
                .and_then(|value| self.program.encode_float(field, value).map(|_| ()))
                .is_ok()),
            _ => Ok(true),
        }
    }

    fn write_value_to_access_path(
        &mut self,
        target: &VmAccessPath,
        value: &VmEvaluatedValue,
    ) -> Result<(), VmError> {
        if target.condition_name.is_some()
            || !self.program.condition_candidates(&target.target).is_empty()
        {
            return Err(VmError::UnsupportedOperand {
                message: format!(
                    "MOVE target {} is a condition-name predicate",
                    target.target
                ),
            });
        }
        let materialized = self.materialize_expr(&VmExpr::Access(target.clone()))?;
        let VmExpr::Access(path) = materialized else {
            return Err(VmError::UnsupportedOperand {
                message: "MOVE target did not materialize to an access path".to_string(),
            });
        };
        self.write_value_to_storage_pool_access_path(&path, value)
    }

    pub fn read_bytes_from_access_path(&self, source: &VmAccessPath) -> Result<Vec<u8>, VmError> {
        let materialized = self.materialize_expr(&VmExpr::Access(source.clone()))?;
        let VmExpr::Access(path) = materialized else {
            return Err(VmError::UnsupportedOperand {
                message: "source did not materialize to an access path".to_string(),
            });
        };
        self.read_bytes_from_storage_pool_access_path(&path)
    }

    pub fn write_bytes_to_access_path(
        &mut self,
        target: &VmAccessPath,
        bytes: &[u8],
    ) -> Result<(), VmError> {
        let materialized = self.materialize_expr(&VmExpr::Access(target.clone()))?;
        let VmExpr::Access(path) = materialized else {
            return Err(VmError::UnsupportedOperand {
                message: "target did not materialize to an access path".to_string(),
            });
        };
        self.write_bytes_to_storage_pool_access_path(&path, bytes)
    }

    fn set_file_status(&mut self, file: &str, status: &str) -> Result<(), VmError> {
        let Some(target) = self.file_status.get(&normalize_vm_key(file)).cloned() else {
            return Ok(());
        };
        self.write_bytes_to_access_path(&target, status.as_bytes())
    }

    fn note_rerun_successful_read(&mut self, file: &str) -> Result<(), VmError> {
        if self.checkpoint_in_progress {
            return Ok(());
        }
        let key = normalize_vm_key(file);
        let mut checkpoint_files = Vec::new();
        for checkpoint in &mut self.rerun_checkpoints {
            if checkpoint.watched_key != key {
                continue;
            }
            checkpoint.record_count = checkpoint.record_count.saturating_add(1);
            if checkpoint.record_count % checkpoint.every_records == 0 {
                checkpoint_files.push(checkpoint.checkpoint_file.clone());
            }
        }
        if checkpoint_files.is_empty() {
            return Ok(());
        }
        self.checkpoint_in_progress = true;
        let record = self.checkpoint_snapshot_bytes();
        for checkpoint_file in checkpoint_files {
            let result =
                self.files
                    .write_with_advancing(&checkpoint_file, &record, VmWriteAdvancing::None);
            if let Err(error) = result {
                self.checkpoint_in_progress = false;
                return Err(error);
            }
        }
        self.checkpoint_in_progress = false;
        Ok(())
    }

    fn set_program_status(&mut self, status: &str) -> Result<(), VmError> {
        self.write_bytes_to_access_path(
            &VmAccessPath {
                target: PROGRAM_STATUS_REGISTER.to_string(),
                condition_name: None,
                subscripts: Vec::new(),
                reference_modifier: None,
                result_len: Some(2),
            },
            status.as_bytes(),
        )
    }

    fn set_file_status_if_bound(&mut self, file: &str, status: &str) -> Result<bool, VmError> {
        if !self.file_status.contains_key(&normalize_vm_key(file)) {
            return Ok(false);
        }
        self.set_file_status(file, status)?;
        Ok(true)
    }

    fn handle_file_error(
        &mut self,
        procedure: &VmProcedure,
        file: &str,
        error: VmError,
        statement_branch_will_handle: bool,
    ) -> Result<VmProcedureSignal, VmError> {
        let status = match &error {
            VmError::FileRuntime { .. } => self.files.last_status(file).map(str::to_string),
            _ => None,
        };
        let mut handled = false;
        if let Some(status) = status {
            handled = self.set_file_status_if_bound(file, &status)?;
        }

        let key = normalize_vm_key(file);
        if let Some(ops) = self.file_error_declaratives.get(&key).cloned() {
            if !self.active_file_error_declaratives.contains(&key) {
                handled = true;
                self.active_file_error_declaratives.insert(key.clone());
                let signal = self.execute_ops(procedure, &ops);
                self.active_file_error_declaratives.remove(&key);
                let signal = signal?;
                if signal != VmProcedureSignal::Continue {
                    return Ok(signal);
                }
            }
        }

        if handled || statement_branch_will_handle {
            Ok(VmProcedureSignal::Continue)
        } else {
            Err(error)
        }
    }

    fn handle_debugging_declarative(
        &mut self,
        procedure: &VmProcedure,
        block: &str,
    ) -> Result<VmProcedureSignal, VmError> {
        let key = normalize_vm_key(block);
        let Some(ops) = self.debugging_declaratives.get(&key).cloned() else {
            return Ok(VmProcedureSignal::Continue);
        };
        if self.active_debugging_declaratives.contains(&key) {
            return Ok(VmProcedureSignal::Continue);
        }

        self.set_debug_special_registers(block, "ENTER")?;
        self.active_debugging_declaratives.insert(key.clone());
        let signal = self.execute_ops(procedure, &ops);
        self.active_debugging_declaratives.remove(&key);
        signal
    }

    fn handle_procedure_entry(
        &mut self,
        procedure: &VmProcedure,
        block: &str,
    ) -> Result<VmProcedureSignal, VmError> {
        if self.trace_enabled {
            self.display.push(format!("TRACE {block}"));
        }
        self.handle_debugging_declarative(procedure, block)
    }

    fn set_debug_special_registers(&mut self, block: &str, contents: &str) -> Result<(), VmError> {
        self.write_special_register(DEBUG_ITEM_REGISTER, block.as_bytes())?;
        self.write_special_register(DEBUG_CONTENTS_REGISTER, contents.as_bytes())
    }

    fn write_special_register(&mut self, name: &str, source: &[u8]) -> Result<(), VmError> {
        let key = StorageKey::special(name);
        let len = self.storage_pool.bytes(&key)?.len();
        let mut bytes = vec![b' '; len];
        for (idx, byte) in source.iter().take(len).enumerate() {
            bytes[idx] = *byte;
        }
        self.storage_pool.write_cell(&key, &bytes)
    }

    fn storage_descriptor_for(&self, target: &str) -> Option<&VmBinding> {
        self.activation_stack
            .iter()
            .rev()
            .find_map(|frame| binding_map_descriptor_for(&frame.local_bindings, target))
            .or_else(|| {
                self.current_program()
                    .and_then(|program| self.scoped_storage_descriptor_for(program, target))
            })
            .or_else(|| binding_map_descriptor_for(&self.storage_descriptors, target))
    }

    fn current_program(&self) -> Option<&str> {
        self.activation_stack
            .last()
            .map(|frame| frame.program.as_str())
    }

    fn scoped_storage_descriptor_for(&self, program: &str, target: &str) -> Option<&VmBinding> {
        let scoped = scoped_runtime_name(program, target);
        binding_map_descriptor_for(&self.storage_descriptors, &scoped)
    }

    fn resolve_index_key(&self, name: &str) -> String {
        self.current_program()
            .map(|program| scoped_runtime_name(program, name))
            .and_then(|scoped| map_key_case_insensitive(&self.indexes, &scoped))
            .or_else(|| map_key_case_insensitive(&self.indexes, name))
            .unwrap_or_else(|| name.to_string())
    }

    fn resolve_odo_key(&self, table: &str) -> String {
        self.current_program()
            .map(|program| scoped_runtime_name(program, table))
            .and_then(|scoped| map_key_case_insensitive(&self.odo, &scoped))
            .or_else(|| map_key_case_insensitive(&self.odo, table))
            .unwrap_or_else(|| table.to_string())
    }

    fn field_for_target(&self, target: &str) -> Result<&VmField, VmError> {
        if let Some(program) = self.current_program() {
            let scoped = scoped_runtime_name(program, target);
            if let Ok(field) = self.program.field(&scoped) {
                return Ok(field);
            }
        }
        self.program.field(target)
    }

    fn access_path_is_group(&self, path: &VmAccessPath) -> bool {
        matches!(
            self.storage_descriptor_for(&path.target),
            Some(VmBinding::Group { .. })
        ) || self
            .field_for_target(&path.target)
            .map(|field| field.category == VmCategory::Group)
            .unwrap_or(false)
    }

    fn eval_storage_pool_access_path(
        &self,
        path: &VmAccessPath,
    ) -> Result<VmEvaluatedValue, VmError> {
        if let Some(condition_name) = &path.condition_name {
            let condition = self.program.condition(condition_name)?;
            let parent = if self
                .program
                .condition_declared_view(condition)
                .map(|view| !view.children.is_empty())
                .unwrap_or(false)
            {
                self.eval_condition_parent_runtime(condition, &path.subscripts)?
            } else {
                let field = self.field_for_target(&path.target)?.clone();
                let slice = self.read_bytes_from_storage_pool_access_path(path)?;
                let mut decode_field = field;
                decode_field.offset = 0;
                decode_field.byte_len = slice.len();
                self.program
                    .decode_field_value(&decode_field, decode_field.category, &slice)?
            };
            return Ok(VmEvaluatedValue {
                value: VmValue::Bool(self.condition_matches_value(condition, &parent)?),
                category: VmCategory::Unsupported,
                byte_len: 1,
            });
        }
        let field = self.field_for_target(&path.target)?.clone();
        let slice = self.read_bytes_from_storage_pool_access_path(path)?;
        let mut decode_field = field;
        decode_field.offset = 0;
        decode_field.byte_len = slice.len();
        let category = if path.reference_modifier.is_some() {
            VmCategory::Alphanumeric
        } else {
            decode_field.category
        };
        self.program
            .decode_field_value(&decode_field, category, &slice)
    }

    fn read_bytes_from_storage_pool_access_path(
        &self,
        path: &VmAccessPath,
    ) -> Result<Vec<u8>, VmError> {
        let mut base = path.clone();
        base.reference_modifier = None;
        let mut bytes = self.read_storage_descriptor_bytes(&base)?;
        if let Some(reference_modifier) = &path.reference_modifier {
            bytes = self.reference_modified_bytes(&path.target, &bytes, reference_modifier)?;
        }
        Ok(bytes)
    }

    fn read_declared_view_bytes_runtime(
        &self,
        children: &[String],
        subscripts: &[VmSubscript],
    ) -> Result<Vec<u8>, VmError> {
        let mut bytes = Vec::new();
        for child in children {
            let child_path = VmAccessPath {
                target: child.clone(),
                condition_name: None,
                subscripts: subscripts.to_vec(),
                reference_modifier: None,
                result_len: None,
            };
            bytes.extend(self.read_storage_descriptor_bytes(&child_path)?);
        }
        Ok(bytes)
    }

    fn write_declared_view_bytes_runtime(
        &mut self,
        children: &[String],
        subscripts: &[VmSubscript],
        source: &[u8],
    ) -> Result<(), VmError> {
        let mut cursor = 0usize;
        for child in children {
            let child_path = VmAccessPath {
                target: child.clone(),
                condition_name: None,
                subscripts: subscripts.to_vec(),
                reference_modifier: None,
                result_len: None,
            };
            let child_len = self.read_storage_descriptor_bytes(&child_path)?.len();
            let end = cursor.saturating_add(child_len).min(source.len());
            let chunk = if cursor < source.len() {
                &source[cursor..end]
            } else {
                &[]
            };
            self.write_storage_descriptor_bytes(&child_path, chunk)?;
            cursor = cursor.saturating_add(child_len);
        }
        Ok(())
    }

    fn write_value_to_storage_pool_access_path(
        &mut self,
        path: &VmAccessPath,
        value: &VmEvaluatedValue,
    ) -> Result<(), VmError> {
        let field = self.field_for_target(&path.target)?.clone();
        match field.category {
            VmCategory::Group
            | VmCategory::Alphanumeric
            | VmCategory::Alphabetic
            | VmCategory::NumericEdited => {
                let text = display_value(value);
                self.write_bytes_to_storage_pool_access_path(path, text.as_bytes())
            }
            VmCategory::NumericDisplay => {
                let len = self.read_bytes_from_storage_pool_access_path(path)?.len();
                let text = render_numeric_display_with_picture(
                    &display_value(value),
                    len,
                    field.picture.as_ref(),
                )?;
                self.write_bytes_to_storage_pool_access_path(path, text.as_bytes())
            }
            VmCategory::PackedDecimal => {
                let decimal = to_decimal(value)?;
                let packed = self.program.encode_packed(&field, decimal)?;
                self.write_bytes_to_storage_pool_access_path(path, &packed)
            }
            VmCategory::Binary | VmCategory::NativeBinary => {
                let decimal = to_decimal(value)?;
                let encoded = self.program.encode_binary(&field, decimal)?;
                self.write_bytes_to_storage_pool_access_path(path, &encoded)
            }
            VmCategory::Float => {
                let float = to_f64(value)?;
                let encoded = self.program.encode_float(&field, float)?;
                self.write_bytes_to_storage_pool_access_path(path, &encoded)
            }
            _ => Err(VmError::UnsupportedOperand {
                message: format!(
                    "writing category {:?} through StoragePool access path {} is not enabled",
                    field.category, field.name
                ),
            }),
        }
    }

    fn write_bytes_to_storage_pool_access_path(
        &mut self,
        path: &VmAccessPath,
        source: &[u8],
    ) -> Result<(), VmError> {
        if path.reference_modifier.is_some() {
            let mut base = path.clone();
            let reference_modifier = base.reference_modifier.take().ok_or_else(|| {
                VmError::InvalidReferenceModification {
                    target: path.target.clone(),
                    message: "missing reference modifier".to_string(),
                }
            })?;
            let mut bytes = self.read_storage_descriptor_bytes(&base)?;
            let (offset, len) =
                self.reference_modifier_range(&path.target, bytes.len(), &reference_modifier)?;
            let bytes_len = bytes.len();
            let dst = bytes
                .get_mut(offset..offset.saturating_add(len))
                .ok_or_else(|| VmError::FieldOutOfBounds {
                    name: path.target.clone(),
                    offset,
                    end: offset.saturating_add(len),
                    len: bytes_len,
                })?;
            dst.fill(b' ');
            for (idx, byte) in source.iter().take(dst.len()).enumerate() {
                dst[idx] = *byte;
            }
            return self.write_storage_descriptor_bytes(&base, &bytes);
        }
        let mut bytes = self.read_storage_descriptor_bytes(path)?;
        bytes.fill(b' ');
        for (idx, byte) in source.iter().take(bytes.len()).enumerate() {
            bytes[idx] = *byte;
        }
        self.write_storage_descriptor_bytes(path, &bytes)
    }

    fn read_storage_descriptor_bytes(&self, path: &VmAccessPath) -> Result<Vec<u8>, VmError> {
        let descriptor =
            self.storage_descriptor_for(&path.target)
                .ok_or_else(|| VmError::StoragePool {
                    key: path.target.clone(),
                    message: "access path has no StoragePool descriptor".to_string(),
                })?;
        match descriptor {
            VmBinding::Cell { key } => {
                if self.is_inactive_odo_occurrence_key(key) {
                    return Ok(Vec::new());
                }
                self.storage_pool.bytes(key).map(Vec::from)
            }
            VmBinding::Slice { key, offset, len } => {
                let bytes = self.storage_pool.bytes(key)?;
                let end = offset.saturating_add(*len);
                bytes
                    .get(*offset..end)
                    .map(Vec::from)
                    .ok_or_else(|| VmError::FieldOutOfBounds {
                        name: path.target.clone(),
                        offset: *offset,
                        end,
                        len: bytes.len(),
                    })
            }
            VmBinding::OccursCell { program, item } => {
                let occurrence = self.storage_pool_outer_occurrence(path)?;
                let key = StorageKey::occurrence(program, item, vec![occurrence]);
                let bytes = self.storage_pool.bytes(&key)?;
                let (offset, len) = self.storage_pool_nested_occurs_range(path, bytes.len())?;
                self.storage_pool
                    .bytes(&key)?
                    .get(offset..offset.saturating_add(len))
                    .map(Vec::from)
                    .ok_or_else(|| VmError::FieldOutOfBounds {
                        name: path.target.clone(),
                        offset,
                        end: offset.saturating_add(len),
                        len: bytes.len(),
                    })
            }
            VmBinding::Group { children } => {
                let mut bytes = Vec::new();
                for child in children {
                    let child_path = VmAccessPath {
                        target: child.clone(),
                        condition_name: None,
                        subscripts: path.subscripts.clone(),
                        reference_modifier: None,
                        result_len: None,
                    };
                    bytes.extend(self.read_storage_descriptor_bytes(&child_path)?);
                }
                Ok(bytes)
            }
        }
    }

    fn write_storage_descriptor_bytes(
        &mut self,
        path: &VmAccessPath,
        source: &[u8],
    ) -> Result<(), VmError> {
        let descriptor = self
            .storage_descriptor_for(&path.target)
            .cloned()
            .ok_or_else(|| VmError::StoragePool {
                key: path.target.clone(),
                message: "access path has no StoragePool descriptor".to_string(),
            })?;
        match descriptor {
            VmBinding::Cell { key } => {
                if self.is_inactive_odo_occurrence_key(&key) {
                    return Ok(());
                }
                let mut bytes = self.storage_pool.bytes(&key)?.to_vec();
                bytes.fill(b' ');
                for (idx, byte) in source.iter().take(bytes.len()).enumerate() {
                    bytes[idx] = *byte;
                }
                self.storage_pool.write_cell(&key, &bytes)?;
                self.sync_odo_after_field_write(&path.target)
            }
            VmBinding::Slice { key, offset, len } => {
                let mut bytes = self.storage_pool.bytes(&key)?.to_vec();
                let end = offset.saturating_add(len);
                let cell_len = bytes.len();
                let dst = bytes
                    .get_mut(offset..end)
                    .ok_or_else(|| VmError::FieldOutOfBounds {
                        name: path.target.clone(),
                        offset,
                        end,
                        len: cell_len,
                    })?;
                dst.fill(b' ');
                for (idx, byte) in source.iter().take(dst.len()).enumerate() {
                    dst[idx] = *byte;
                }
                self.storage_pool.write_cell(&key, &bytes)
            }
            VmBinding::OccursCell { program, item } => {
                let occurrence = self.storage_pool_outer_occurrence(path)?;
                let key = StorageKey::occurrence(&program, &item, vec![occurrence]);
                let mut bytes = self.storage_pool.bytes(&key)?.to_vec();
                let (offset, len) = self.storage_pool_nested_occurs_range(path, bytes.len())?;
                let cell_len = bytes.len();
                let dst = bytes
                    .get_mut(offset..offset.saturating_add(len))
                    .ok_or_else(|| VmError::FieldOutOfBounds {
                        name: path.target.clone(),
                        offset,
                        end: offset.saturating_add(len),
                        len: cell_len,
                    })?;
                dst.fill(b' ');
                for (idx, byte) in source.iter().take(dst.len()).enumerate() {
                    dst[idx] = *byte;
                }
                self.storage_pool.write_cell(&key, &bytes)
            }
            VmBinding::Group { children } => {
                let mut cursor = 0usize;
                for child in children {
                    let child_path = VmAccessPath {
                        target: child,
                        condition_name: None,
                        subscripts: path.subscripts.clone(),
                        reference_modifier: None,
                        result_len: None,
                    };
                    let child_len = self.read_storage_descriptor_bytes(&child_path)?.len();
                    let end = cursor.saturating_add(child_len).min(source.len());
                    let chunk = if cursor < source.len() {
                        &source[cursor..end]
                    } else {
                        &[]
                    };
                    self.write_storage_descriptor_bytes(&child_path, chunk)?;
                    cursor = cursor.saturating_add(child_len);
                }
                Ok(())
            }
        }
    }

    fn is_inactive_odo_occurrence_key(&self, key: &StorageKey) -> bool {
        let [occurrence] = key.occurrence.as_slice() else {
            return false;
        };
        self.storage_pool
            .active_count_for_occurs_item(&key.program, &key.item)
            .is_some_and(|active| *occurrence > active)
    }

    fn storage_pool_outer_occurrence(&self, path: &VmAccessPath) -> Result<usize, VmError> {
        let Some(subscript) = path.subscripts.first() else {
            return Err(VmError::UnsupportedOperand {
                message: format!(
                    "StoragePool OCCURS access for {} requires a subscript",
                    path.target
                ),
            });
        };
        let value = self.storage_pool_subscript_value(path, 0, subscript)?;
        Ok(value)
    }

    fn storage_pool_subscript_value(
        &self,
        path: &VmAccessPath,
        index: usize,
        subscript: &VmSubscript,
    ) -> Result<usize, VmError> {
        let value = decimal_to_i128(to_decimal(&self.eval_expr(&subscript.expr)?)?)?;
        let descriptor_active = match (index, self.storage_descriptor_for(&path.target)) {
            (0, Some(VmBinding::OccursCell { program, item })) => self
                .storage_pool
                .active_count_for_occurs_item(program, item),
            _ => None,
        };
        let mut active_max = descriptor_active.unwrap_or(subscript.max);
        if descriptor_active.is_none() {
            if let Some(depending_on) = &subscript.depending_on {
                let active = decimal_to_i128(to_decimal(&self.eval_expr(&VmExpr::Access(
                    VmAccessPath {
                        target: depending_on.clone(),
                        condition_name: None,
                        subscripts: Vec::new(),
                        reference_modifier: None,
                        result_len: None,
                    },
                ))?)?)?;
                active_max = active.max(0) as usize;
            }
        }
        if value < subscript.min as i128 || value > active_max as i128 {
            return Err(VmError::InvalidSubscript {
                target: path.target.clone(),
                value,
                min: subscript.min,
                max: active_max,
            });
        }
        Ok(value as usize)
    }

    fn storage_pool_nested_occurs_range(
        &self,
        path: &VmAccessPath,
        bytes_len: usize,
    ) -> Result<(usize, usize), VmError> {
        let mut offset = 0usize;
        let mut len = path.result_len.unwrap_or(bytes_len);
        for (idx, subscript) in path.subscripts.iter().enumerate().skip(1) {
            let value = self.storage_pool_subscript_value(path, idx, subscript)?;
            offset = offset.saturating_add((value - 1).saturating_mul(subscript.stride));
            len = path.result_len.unwrap_or(subscript.stride);
        }
        if path.subscripts.len() <= 1 {
            len = path.result_len.unwrap_or(bytes_len);
        }
        Ok((offset, len))
    }

    fn reference_modified_bytes(
        &self,
        target: &str,
        bytes: &[u8],
        reference_modifier: &VmReferenceModifier,
    ) -> Result<Vec<u8>, VmError> {
        let (offset, len) =
            self.reference_modifier_range(target, bytes.len(), reference_modifier)?;
        bytes
            .get(offset..offset.saturating_add(len))
            .map(Vec::from)
            .ok_or_else(|| VmError::FieldOutOfBounds {
                name: target.to_string(),
                offset,
                end: offset.saturating_add(len),
                len: bytes.len(),
            })
    }

    fn reference_modifier_range(
        &self,
        target: &str,
        len: usize,
        reference_modifier: &VmReferenceModifier,
    ) -> Result<(usize, usize), VmError> {
        let start = decimal_to_i128(to_decimal(&self.eval_expr(&reference_modifier.start)?)?)?;
        if start <= 0 {
            return Err(VmError::InvalidReferenceModification {
                target: target.to_string(),
                message: format!("start {start} is outside COBOL's 1-based range"),
            });
        }
        let zero_based = (start as usize).saturating_sub(1);
        if zero_based > len {
            return Err(VmError::InvalidReferenceModification {
                target: target.to_string(),
                message: format!("start {start} exceeds length {len}"),
            });
        }
        let requested = if let Some(length) = &reference_modifier.length {
            decimal_to_i128(to_decimal(&self.eval_expr(length)?)?)? as usize
        } else {
            len.saturating_sub(zero_based)
        };
        if zero_based.saturating_add(requested) > len {
            return Err(VmError::InvalidReferenceModification {
                target: target.to_string(),
                message: format!(
                    "slice {}:{} exceeds available length {}",
                    start, requested, len
                ),
            });
        }
        Ok((zero_based, requested))
    }

    fn sync_odo_after_field_write(&mut self, field_name: &str) -> Result<(), VmError> {
        let current_program = self.current_program().map(str::to_string);
        let updates = self
            .odo
            .iter()
            .filter(|(_, odo)| {
                let same_field =
                    normalize_vm_key(&odo.depending_on) == normalize_vm_key(field_name);
                match (&current_program, &odo.program) {
                    (Some(current), Some(program)) => {
                        current.eq_ignore_ascii_case(program) && same_field
                    }
                    (_, None) => same_field,
                    _ => false,
                }
            })
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for table_key in updates {
            let active = {
                let odo = self
                    .odo
                    .get(&table_key)
                    .ok_or_else(|| VmError::OdoRuntime {
                        table: table_key.clone(),
                        message: "ODO descriptor is not defined".to_string(),
                    })?;
                let value = self.eval_expr(&VmExpr::Identifier(odo.depending_on.clone()))?;
                decimal_to_i128(to_decimal(&value)?)?
            };
            if active < 0 {
                return Err(VmError::OdoRuntime {
                    table: table_key,
                    message: format!("active count {active} is negative"),
                });
            }
            self.set_odo_active_by_key(&table_key, active as usize)?;
        }
        Ok(())
    }

    pub fn execute_procedure(&mut self, procedure: &VmProcedure) -> Result<(), VmError> {
        self.execute_procedure_with_bindings(
            procedure,
            normalize_vm_key(&procedure.entry),
            BTreeMap::new(),
        )
    }

    pub fn execute_procedure_as(
        &mut self,
        procedure: &VmProcedure,
        program: impl Into<String>,
    ) -> Result<(), VmError> {
        self.execute_procedure_with_bindings(procedure, program.into(), BTreeMap::new())
    }

    fn set_alter_target(&mut self, paragraph: &str, target: &str) {
        self.alter_table
            .insert(normalize_vm_key(paragraph), target.to_string());
    }

    fn altered_target(&self, paragraph: &str) -> Option<String> {
        self.alter_table.get(&normalize_vm_key(paragraph)).cloned()
    }

    fn resolve_go_to_target(&self, target: &str) -> String {
        self.altered_target(target)
            .unwrap_or_else(|| target.to_string())
    }

    fn execute_procedure_with_bindings(
        &mut self,
        procedure: &VmProcedure,
        program: String,
        local_bindings: BTreeMap<String, VmBinding>,
    ) -> Result<(), VmError> {
        if self.activation_stack.is_empty() {
            self.last_abend_frame = None;
        }
        self.activation_stack.push(VmFrame {
            program,
            current: procedure.entry.clone(),
            return_to: None,
            source_span: None,
            local_bindings,
        });
        let result = self.execute_procedure_body(procedure);
        let frame = self.activation_stack.pop();
        if result.is_err() && self.last_abend_frame.is_none() {
            self.last_abend_frame = frame;
        }
        result
    }

    fn execute_procedure_body(&mut self, procedure: &VmProcedure) -> Result<(), VmError> {
        let mut current = Some(procedure.entry.clone());
        let mut steps = 0usize;
        while let Some(block_name) = current {
            steps += 1;
            if steps > 100_000 {
                return Err(VmError::ProcedureRuntime {
                    block: block_name,
                    message: "procedure step limit exceeded".to_string(),
                });
            }
            let block = procedure
                .blocks
                .iter()
                .find(|block| block.name == block_name)
                .ok_or_else(|| VmError::ProcedureRuntime {
                    block: block_name.clone(),
                    message: "block is not defined".to_string(),
                })?;
            if let Some(frame) = self.activation_stack.last_mut() {
                frame.current = block_name.clone();
            }
            match self.handle_procedure_entry(procedure, &block_name)? {
                VmProcedureSignal::Continue => {}
                VmProcedureSignal::GoTo(target) => {
                    current = Some(target);
                    continue;
                }
                VmProcedureSignal::StopRun => {
                    current = None;
                    continue;
                }
            }
            match self.execute_ops(procedure, &block.ops)? {
                VmProcedureSignal::Continue => {
                    current = self.next_block(procedure, block)?;
                }
                VmProcedureSignal::GoTo(target) => {
                    current = Some(target);
                }
                VmProcedureSignal::StopRun => {
                    current = None;
                }
            }
        }
        Ok(())
    }

    pub fn search_linear(&mut self, search: &VmSearch) -> Result<Option<usize>, VmError> {
        for occurrence in search.min..=search.max {
            self.set_index(&search.index_name, occurrence)?;
            if self.eval_condition(&search.condition)? {
                return Ok(Some(occurrence));
            }
        }
        Ok(None)
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_search_serial(
        &mut self,
        procedure: &VmProcedure,
        table: &str,
        index_name: &str,
        min: usize,
        max: usize,
        whens: &[VmSearchWhen],
        at_end_ops: &[VmProcedureOp],
    ) -> Result<VmProcedureSignal, VmError> {
        let start = self.index_occurrence(index_name)?;
        if start < min {
            return Err(VmError::InvalidSubscript {
                target: table.to_string(),
                value: start as i128,
                min,
                max,
            });
        }
        let max = self.effective_search_max(table, max);
        if start > max {
            return self.execute_ops(procedure, at_end_ops);
        }
        for occurrence in start..=max {
            self.set_index(index_name, occurrence)?;
            for when in whens {
                if self.eval_condition(&when.condition)? {
                    return self.execute_ops(procedure, &when.ops);
                }
            }
        }
        self.execute_ops(procedure, at_end_ops)
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_search_all(
        &mut self,
        procedure: &VmProcedure,
        table: &str,
        index_name: &str,
        min: usize,
        max: usize,
        direction: VmSearchDirection,
        key: &VmExpr,
        target: &VmExpr,
        found_ops: &[VmProcedureOp],
        at_end_ops: &[VmProcedureOp],
    ) -> Result<VmProcedureSignal, VmError> {
        let max = self.effective_search_max(table, max);
        if max < min {
            return self.execute_ops(procedure, at_end_ops);
        }

        let mut low = min;
        let mut high = max;
        while low <= high {
            let mid = low + (high - low) / 2;
            self.set_index(index_name, mid)?;
            let key_value = self.eval_expr(key)?;
            let target_value = self.eval_expr(target)?;
            let ordering = self.program.compare_values(&key_value, &target_value)?;
            if ordering == Ordering::Equal {
                return self.execute_ops(procedure, found_ops);
            }

            let search_right = match direction {
                VmSearchDirection::Ascending => ordering == Ordering::Less,
                VmSearchDirection::Descending => ordering == Ordering::Greater,
            };
            if search_right {
                low = mid.saturating_add(1);
            } else if mid == 0 {
                break;
            } else {
                high = mid - 1;
            }
        }

        self.execute_ops(procedure, at_end_ops)
    }

    fn effective_search_max(&self, table: &str, static_max: usize) -> usize {
        let table_key = self.resolve_odo_key(table);
        self.odo
            .get(&table_key)
            .map(|odo| odo.active.min(static_max))
            .unwrap_or(static_max)
    }

    fn execute_ops(
        &mut self,
        procedure: &VmProcedure,
        ops: &[VmProcedureOp],
    ) -> Result<VmProcedureSignal, VmError> {
        for op in ops {
            match op {
                VmProcedureOp::SetSourceSpan(span) => {
                    if let Some(frame) = self.activation_stack.last_mut() {
                        frame.source_span = Some(span.clone());
                    }
                }
                VmProcedureOp::Display(values) => {
                    let mut line = String::new();
                    for expr in values {
                        line.push_str(&self.display_expr_text(expr)?);
                    }
                    self.display.push(line);
                }
                VmProcedureOp::Move { source, target } => {
                    self.move_value_to_access_path(source, target)?;
                }
                VmProcedureOp::Add { source, target } => {
                    self.numeric_update_access_path(source, target, |left, right| left + right)?;
                }
                VmProcedureOp::Subtract { source, target } => {
                    self.numeric_update_access_path(source, target, |left, right| left - right)?;
                }
                VmProcedureOp::Multiply { source, target } => {
                    self.numeric_update_access_path(source, target, |left, right| left * right)?;
                }
                VmProcedureOp::Divide { source, target } => {
                    let divisor = to_decimal(&self.eval_expr(source)?)?;
                    if divisor.is_zero() {
                        return Err(VmError::InvalidDecimal {
                            value: "division by zero".to_string(),
                        });
                    }
                    self.numeric_update_access_path(
                        &VmExpr::Number(divisor.to_string()),
                        target,
                        |left, right| left / right,
                    )?;
                }
                VmProcedureOp::Compute {
                    target,
                    expr,
                    rounded,
                    on_size_error_ops,
                    not_on_size_error_ops,
                } => {
                    let size_error = self.compute_op(target, expr, *rounded)?;
                    let branch = if size_error {
                        on_size_error_ops
                    } else {
                        not_on_size_error_ops
                    };
                    let signal = self.execute_ops(procedure, branch)?;
                    if signal != VmProcedureSignal::Continue {
                        return Ok(signal);
                    }
                }
                VmProcedureOp::If {
                    condition,
                    then_ops,
                    else_ops,
                } => {
                    let branch = if self.eval_condition(condition)? {
                        then_ops
                    } else {
                        else_ops
                    };
                    let signal = self.execute_ops(procedure, branch)?;
                    if signal != VmProcedureSignal::Continue {
                        return Ok(signal);
                    }
                }
                VmProcedureOp::Evaluate { evaluate, branches } => {
                    if let Some(idx) = self.eval_evaluate(evaluate)? {
                        if let Some(ops) = branches.get(idx) {
                            let signal = self.execute_ops(procedure, ops)?;
                            if signal != VmProcedureSignal::Continue {
                                return Ok(signal);
                            }
                        }
                    }
                }
                VmProcedureOp::SetConditionName { name } => {
                    self.set_condition_name_at(name)?;
                }
                VmProcedureOp::Perform {
                    target,
                    through,
                    times,
                } => {
                    let count = if let Some(times) = times {
                        decimal_to_i128(to_decimal(&self.eval_expr(times)?)?)?.max(0) as usize
                    } else {
                        1
                    };
                    for _ in 0..count {
                        let signal =
                            self.execute_perform_range(procedure, target, through.as_deref())?;
                        if signal != VmProcedureSignal::Continue {
                            return Ok(signal);
                        }
                    }
                }
                VmProcedureOp::DynamicPerform { target } => {
                    let target = value_text(&self.eval_expr(target)?)
                        .map(|value| normalize_vm_key(value.trim()))
                        .ok_or_else(|| VmError::ProcedureRuntime {
                            block: self
                                .activation_stack
                                .last()
                                .map(|frame| frame.current.clone())
                                .unwrap_or_default(),
                            message: "dynamic PERFORM target did not evaluate to text".to_string(),
                        })?;
                    let signal = self.execute_perform_range(procedure, &target, None)?;
                    if signal != VmProcedureSignal::Continue {
                        return Ok(signal);
                    }
                }
                VmProcedureOp::PerformLoop {
                    target,
                    through,
                    varying,
                    until,
                } => {
                    let signal = self.execute_perform_loop(
                        procedure,
                        target,
                        through.as_deref(),
                        varying.as_ref(),
                        until.as_ref(),
                    )?;
                    if signal != VmProcedureSignal::Continue {
                        return Ok(signal);
                    }
                }
                VmProcedureOp::GoTo { target } => {
                    return Ok(VmProcedureSignal::GoTo(self.resolve_go_to_target(target)));
                }
                VmProcedureOp::ComputedGoTo {
                    targets,
                    depending_on,
                } => {
                    let value = decimal_to_i128(to_decimal(&self.eval_expr(depending_on)?)?)?;
                    if value >= 1 {
                        if let Some(target) = targets.get((value as usize).saturating_sub(1)) {
                            return Ok(VmProcedureSignal::GoTo(self.resolve_go_to_target(target)));
                        }
                    }
                }
                VmProcedureOp::Alter { paragraph, target } => {
                    self.set_alter_target(paragraph, target);
                }
                VmProcedureOp::Call { target, using } => {
                    let target_name = self.resolve_call_target(target)?;
                    let Some(registered) = self.registry.registered(&target_name).cloned() else {
                        if matches!(target, VmCallTarget::Dynamic(_)) {
                            self.set_program_status("01")?;
                            continue;
                        }
                        return Err(VmError::NestedProgramRuntime {
                            message: format!("CALL target {target_name} is not registered"),
                        });
                    };
                    self.reset_initial_program_instance(&registered)?;
                    let local_bindings = match self.bind_call_using_arguments(
                        &target_name,
                        &registered.linkage,
                        using,
                    ) {
                        Ok(bindings) => bindings,
                        Err(error) if matches!(target, VmCallTarget::Dynamic(_)) => {
                            self.set_program_status("02")?;
                            let _ = error;
                            continue;
                        }
                        Err(error) => return Err(error),
                    };
                    if matches!(target, VmCallTarget::Dynamic(_)) {
                        self.set_program_status("00")?;
                    }
                    self.execute_procedure_with_bindings(
                        &registered.procedure,
                        target_name.clone(),
                        local_bindings,
                    )?;
                }
                VmProcedureOp::StopRun => {
                    return Ok(VmProcedureSignal::StopRun);
                }
                VmProcedureOp::OpenFile { name, mode } => match self.files.open(name, *mode) {
                    Ok(()) => self.set_file_status(name, "00")?,
                    Err(error) => {
                        let signal = self.handle_file_error(procedure, name, error, false)?;
                        if signal != VmProcedureSignal::Continue {
                            return Ok(signal);
                        }
                    }
                },
                VmProcedureOp::ReadFile {
                    name,
                    target,
                    at_end_ops,
                    not_at_end_ops,
                    on_exception_ops,
                } => {
                    let record_len = self.read_bytes_from_access_path(target)?.len();
                    match self.files.read(name, record_len) {
                        Ok(Some(record)) => {
                            self.write_bytes_to_access_path(target, &record)?;
                            self.set_file_status(name, "00")?;
                            self.note_rerun_successful_read(name)?;
                            let signal = self.execute_ops(procedure, not_at_end_ops)?;
                            if signal != VmProcedureSignal::Continue {
                                return Ok(signal);
                            }
                        }
                        Ok(None) => {
                            self.set_file_status(name, "10")?;
                            let signal = self.execute_ops(procedure, at_end_ops)?;
                            if signal != VmProcedureSignal::Continue {
                                return Ok(signal);
                            }
                        }
                        Err(error) => {
                            let signal = self.handle_file_error(
                                procedure,
                                name,
                                error,
                                !on_exception_ops.is_empty(),
                            )?;
                            if signal != VmProcedureSignal::Continue {
                                return Ok(signal);
                            }
                            let signal = self.execute_ops(procedure, on_exception_ops)?;
                            if signal != VmProcedureSignal::Continue {
                                return Ok(signal);
                            }
                        }
                    }
                }
                VmProcedureOp::WriteFile {
                    name,
                    source,
                    advancing,
                } => {
                    let record = self.read_bytes_from_access_path(source)?;
                    match self.files.write_with_advancing(name, &record, *advancing) {
                        Ok(()) => self.set_file_status(name, "00")?,
                        Err(error) => {
                            let signal = self.handle_file_error(procedure, name, error, false)?;
                            if signal != VmProcedureSignal::Continue {
                                return Ok(signal);
                            }
                        }
                    }
                }
                VmProcedureOp::RewriteFile {
                    name,
                    source,
                    invalid_key_ops,
                    not_invalid_key_ops,
                } => {
                    let record = self.read_bytes_from_access_path(source)?;
                    match self.files.rewrite(name, &record) {
                        Ok(()) => {
                            self.set_file_status(name, "00")?;
                            let signal = self.execute_ops(procedure, not_invalid_key_ops)?;
                            if signal != VmProcedureSignal::Continue {
                                return Ok(signal);
                            }
                        }
                        Err(error) => {
                            let signal = self.handle_file_error(
                                procedure,
                                name,
                                error,
                                !invalid_key_ops.is_empty(),
                            )?;
                            if signal != VmProcedureSignal::Continue {
                                return Ok(signal);
                            }
                            let signal = self.execute_ops(procedure, invalid_key_ops)?;
                            if signal != VmProcedureSignal::Continue {
                                return Ok(signal);
                            }
                        }
                    }
                }
                VmProcedureOp::DeleteFile {
                    name,
                    invalid_key_ops,
                    not_invalid_key_ops,
                } => match self.files.delete(name) {
                    Ok(()) => {
                        self.set_file_status(name, "00")?;
                        let signal = self.execute_ops(procedure, not_invalid_key_ops)?;
                        if signal != VmProcedureSignal::Continue {
                            return Ok(signal);
                        }
                    }
                    Err(error) => {
                        let signal = self.handle_file_error(
                            procedure,
                            name,
                            error,
                            !invalid_key_ops.is_empty(),
                        )?;
                        if signal != VmProcedureSignal::Continue {
                            return Ok(signal);
                        }
                        let signal = self.execute_ops(procedure, invalid_key_ops)?;
                        if signal != VmProcedureSignal::Continue {
                            return Ok(signal);
                        }
                    }
                },
                VmProcedureOp::CloseFile { name } => match self.files.close(name) {
                    Ok(()) => self.set_file_status(name, "00")?,
                    Err(error) => {
                        let signal = self.handle_file_error(procedure, name, error, false)?;
                        if signal != VmProcedureSignal::Continue {
                            return Ok(signal);
                        }
                    }
                },
                VmProcedureOp::SortProcedure {
                    file,
                    record,
                    key,
                    input,
                    output,
                } => {
                    let signal = self.execute_sort_procedure(
                        procedure,
                        file,
                        record,
                        key.clone(),
                        input.as_ref(),
                        output,
                    )?;
                    if signal != VmProcedureSignal::Continue {
                        return Ok(signal);
                    }
                }
                VmProcedureOp::ReleaseSortRecord { record, source } => {
                    self.release_sort_record(record, source.as_ref())?;
                }
                VmProcedureOp::ReturnSortRecord {
                    file,
                    record,
                    target,
                    at_end_ops,
                    not_at_end_ops,
                } => {
                    let signal = self.return_sort_record(
                        procedure,
                        file,
                        record,
                        target,
                        at_end_ops,
                        not_at_end_ops,
                    )?;
                    if signal != VmProcedureSignal::Continue {
                        return Ok(signal);
                    }
                }
                VmProcedureOp::InspectLike {
                    subject,
                    tally,
                    replacing,
                    converting,
                } => {
                    self.inspect_like(
                        subject,
                        tally.as_ref(),
                        replacing.as_ref(),
                        converting.as_ref(),
                    )?;
                }
                VmProcedureOp::StringOp {
                    pieces,
                    target,
                    pointer,
                    on_overflow_ops,
                    not_on_overflow_ops,
                } => {
                    let overflow = self.string_op(pieces, target, pointer.as_ref())?;
                    let branch = if overflow {
                        on_overflow_ops
                    } else {
                        not_on_overflow_ops
                    };
                    let signal = self.execute_ops(procedure, branch)?;
                    if signal != VmProcedureSignal::Continue {
                        return Ok(signal);
                    }
                }
                VmProcedureOp::UnstringOp {
                    source,
                    delimiter,
                    targets,
                    pointer,
                    tallying,
                    on_overflow_ops,
                    not_on_overflow_ops,
                } => {
                    let overflow = self.unstring_op(
                        source,
                        delimiter,
                        targets,
                        pointer.as_ref(),
                        tallying.as_ref(),
                    )?;
                    let branch = if overflow {
                        on_overflow_ops
                    } else {
                        not_on_overflow_ops
                    };
                    let signal = self.execute_ops(procedure, branch)?;
                    if signal != VmProcedureSignal::Continue {
                        return Ok(signal);
                    }
                }
                VmProcedureOp::SetIndex { name, operation } => match operation {
                    VmSetIndexOperation::To(expr) => {
                        let occurrence = decimal_to_i128(to_decimal(&self.eval_expr(expr)?)?)?;
                        self.set_index(name, occurrence as usize)?;
                    }
                    VmSetIndexOperation::UpBy(expr) => {
                        let delta = decimal_to_i128(to_decimal(&self.eval_expr(expr)?)?)?;
                        self.adjust_index(name, delta)?;
                    }
                    VmSetIndexOperation::DownBy(expr) => {
                        let delta = decimal_to_i128(to_decimal(&self.eval_expr(expr)?)?)?;
                        self.adjust_index(name, -delta)?;
                    }
                },
                VmProcedureOp::SearchSerial {
                    table,
                    index_name,
                    min,
                    max,
                    whens,
                    at_end_ops,
                } => {
                    let signal = self.execute_search_serial(
                        procedure, table, index_name, *min, *max, whens, at_end_ops,
                    )?;
                    if signal != VmProcedureSignal::Continue {
                        return Ok(signal);
                    }
                }
                VmProcedureOp::SearchAll {
                    table,
                    index_name,
                    min,
                    max,
                    direction,
                    key,
                    target,
                    found_ops,
                    at_end_ops,
                } => {
                    let signal = self.execute_search_all(
                        procedure, table, index_name, *min, *max, *direction, key, target,
                        found_ops, at_end_ops,
                    )?;
                    if signal != VmProcedureSignal::Continue {
                        return Ok(signal);
                    }
                }
                VmProcedureOp::SetOdo { table, active } => {
                    let active = decimal_to_i128(to_decimal(&self.eval_expr(active)?)?)?;
                    if active < 0 {
                        return Err(VmError::OdoRuntime {
                            table: table.clone(),
                            message: format!("active count {active} is negative"),
                        });
                    }
                    self.set_odo_active(table, active as usize)?;
                }
                VmProcedureOp::TraceOn => {
                    self.trace_enabled = true;
                }
                VmProcedureOp::TraceOff => {
                    self.trace_enabled = false;
                }
                VmProcedureOp::UnsupportedTrap { message } => {
                    return Err(VmError::ProcedureRuntime {
                        block: procedure.entry.clone(),
                        message: message.clone(),
                    });
                }
                VmProcedureOp::Noop => {}
            }
        }
        Ok(VmProcedureSignal::Continue)
    }

    fn display_expr_text(&self, expr: &VmExpr) -> Result<String, VmError> {
        match expr {
            VmExpr::Access(path) => self.display_access_path_text(path),
            VmExpr::Identifier(name) if self.storage_descriptor_for(name).is_some() => self
                .display_access_path_text(&VmAccessPath {
                    target: name.clone(),
                    condition_name: None,
                    subscripts: Vec::new(),
                    reference_modifier: None,
                    result_len: None,
                }),
            _ => {
                let value = self.eval_expr(expr)?;
                Ok(display_value(&value))
            }
        }
    }

    fn display_access_path_text(&self, path: &VmAccessPath) -> Result<String, VmError> {
        let materialized = self.materialize_expr(&VmExpr::Access(path.clone()))?;
        let VmExpr::Access(materialized_path) = materialized else {
            let value = self.eval_expr(&VmExpr::Access(path.clone()))?;
            return Ok(display_value(&value));
        };
        let field = self.field_for_target(&materialized_path.target)?;
        if matches!(field.category, VmCategory::NumericDisplay) {
            let bytes = self.read_bytes_from_storage_pool_access_path(&materialized_path)?;
            return self.display_numeric_display_text(field, &bytes);
        }
        let value = self.eval_expr(&VmExpr::Access(materialized_path))?;
        Ok(display_value(&value))
    }

    fn display_numeric_display_text(
        &self,
        field: &VmField,
        bytes: &[u8],
    ) -> Result<String, VmError> {
        let Some(picture) = field.picture.as_ref() else {
            return Ok(String::from_utf8_lossy(bytes).to_string());
        };
        if !picture.signed {
            return Ok(String::from_utf8_lossy(bytes).to_string());
        }
        let value = self.program.decode_display_decimal(field, bytes)?;
        let mut text = value.normalize().to_string();
        if !text.starts_with('-') {
            text.insert(0, '+');
        }
        render_numeric_display_with_picture(&text, picture.digits.saturating_add(1), Some(picture))
    }

    fn execute_sort_procedure(
        &mut self,
        procedure: &VmProcedure,
        file: &str,
        record: &VmAccessPath,
        key: Option<VmSortKeyDescriptor>,
        input: Option<&VmProcedureRange>,
        output: &VmProcedureRange,
    ) -> Result<VmProcedureSignal, VmError> {
        let record_len = self.read_bytes_from_access_path(record)?.len();
        let sort_depth = self.sort_states.len();
        self.sort_states.push(VmSortState {
            file: normalize_vm_key(file),
            phase: VmSortPhase::Input,
            record: record.clone(),
            record_len,
            released_records: Vec::new(),
            sorted_records: Vec::new(),
            cursor: 0,
            key,
        });

        let result = self.execute_active_sort_procedure(procedure, input, output);
        self.sort_states.truncate(sort_depth);
        result
    }

    fn execute_active_sort_procedure(
        &mut self,
        procedure: &VmProcedure,
        input: Option<&VmProcedureRange>,
        output: &VmProcedureRange,
    ) -> Result<VmProcedureSignal, VmError> {
        if let Some(input) = input {
            let signal =
                self.execute_perform_range(procedure, &input.target, input.through.as_deref())?;
            if signal != VmProcedureSignal::Continue {
                return Ok(signal);
            }
        }

        if let Some(state) = self.sort_states.last_mut() {
            state.phase = VmSortPhase::Sorting;
            state.sorted_records = state.released_records.clone();
            if let Some(key) = state.key.clone() {
                sort_records_by_key(&mut state.sorted_records, &key)?;
            }
            state.cursor = 0;
            state.phase = VmSortPhase::Output;
        }

        let signal =
            self.execute_perform_range(procedure, &output.target, output.through.as_deref())?;
        if signal == VmProcedureSignal::Continue {
            if let Some(state) = self.sort_states.last_mut() {
                state.phase = VmSortPhase::Done;
            }
        }
        Ok(signal)
    }

    fn release_sort_record(
        &mut self,
        record: &VmAccessPath,
        source: Option<&VmAccessPath>,
    ) -> Result<(), VmError> {
        let Some(state) = self.sort_states.last() else {
            return Err(VmError::ProcedureRuntime {
                block: String::new(),
                message: "RELEASE executed without an active SORT".to_string(),
            });
        };
        let phase = state.phase;
        let state_record = state.record.clone();
        let record_len = state.record_len;
        if phase != VmSortPhase::Input {
            return Err(VmError::ProcedureRuntime {
                block: String::new(),
                message: format!("RELEASE executed during SORT {} phase", phase.label()),
            });
        }
        if !access_paths_match(&state_record, record) {
            return Err(VmError::ProcedureRuntime {
                block: String::new(),
                message: format!(
                    "RELEASE record {} does not match active SORT record {}",
                    record.target, state_record.target
                ),
            });
        }
        if let Some(source) = source {
            self.move_value_to_access_path(&VmExpr::Access(source.clone()), record)?;
        }
        let mut bytes = self.read_bytes_from_access_path(record)?;
        normalize_record_bytes(&mut bytes, record_len);
        if let Some(state) = self.sort_states.last_mut() {
            state.released_records.push(bytes);
        }
        Ok(())
    }

    fn return_sort_record(
        &mut self,
        procedure: &VmProcedure,
        file: &str,
        sd_record: &VmAccessPath,
        target: &Option<VmAccessPath>,
        at_end_ops: &[VmProcedureOp],
        not_at_end_ops: &[VmProcedureOp],
    ) -> Result<VmProcedureSignal, VmError> {
        let expected_file = normalize_vm_key(file);
        let Some(state) = self.sort_states.last() else {
            return Err(VmError::ProcedureRuntime {
                block: String::new(),
                message: "RETURN executed without an active SORT".to_string(),
            });
        };
        let phase = state.phase;
        if phase != VmSortPhase::Output {
            return Err(VmError::ProcedureRuntime {
                block: String::new(),
                message: format!("RETURN executed during SORT {} phase", phase.label()),
            });
        }
        if state.file != expected_file {
            return Err(VmError::ProcedureRuntime {
                block: String::new(),
                message: format!(
                    "RETURN for sort file {file} does not match active SORT file {}",
                    state.file
                ),
            });
        }
        let Some(state) = self.sort_states.last_mut() else {
            unreachable!("active sort state checked above");
        };
        let next_record = if state.cursor < state.sorted_records.len() {
            let record = state.sorted_records[state.cursor].clone();
            state.cursor += 1;
            Some(record)
        } else {
            None
        };
        if let Some(record) = next_record {
            self.write_bytes_to_access_path(sd_record, &record)?;
            if let Some(target) = target {
                self.move_value_to_access_path(&VmExpr::Access(sd_record.clone()), target)?;
            }
            self.execute_ops(procedure, not_at_end_ops)
        } else {
            self.execute_ops(procedure, at_end_ops)
        }
    }

    fn inspect_like(
        &mut self,
        subject: &VmAccessPath,
        tally: Option<&VmInspectTally>,
        replacing: Option<&VmInspectReplacing>,
        converting: Option<&VmInspectConverting>,
    ) -> Result<(), VmError> {
        let original = self.read_bytes_from_access_path(subject)?;
        let mut next = original.clone();
        if let Some(tally) = tally {
            let count = count_non_overlapping_bytes(&original, tally.pattern.as_bytes())?;
            self.numeric_update_access_path(
                &VmExpr::Number(count.to_string()),
                &tally.target,
                |current, delta| current + delta,
            )?;
        }
        if let Some(replacing) = replacing {
            next = replace_all_bytes(
                &next,
                replacing.pattern.as_bytes(),
                replacing.replacement.as_bytes(),
            )?;
        }
        if let Some(converting) = converting {
            next = convert_bytes(&next, converting.from.as_bytes(), converting.to.as_bytes())?;
        }
        if replacing.is_some() || converting.is_some() {
            self.write_bytes_to_access_path(subject, &next)?;
        }
        Ok(())
    }

    fn string_op(
        &mut self,
        pieces: &[VmStringPiece],
        target: &VmAccessPath,
        pointer: Option<&VmAccessPath>,
    ) -> Result<bool, VmError> {
        let mut out = Vec::new();
        for piece in pieces {
            let bytes = self.expr_bytes_for_string(&piece.source)?;
            match &piece.delimiter {
                VmStringDelimiter::Size => out.extend_from_slice(&bytes),
                VmStringDelimiter::Literal { value, .. } => {
                    let delimiter = value.as_bytes();
                    if delimiter.is_empty() {
                        return Err(VmError::ProcedureRuntime {
                            block: String::new(),
                            message: "STRING delimiter must not be empty".to_string(),
                        });
                    }
                    let end = find_bytes(&bytes, delimiter).unwrap_or(bytes.len());
                    out.extend_from_slice(&bytes[..end]);
                }
            }
        }
        if let Some(pointer) = pointer {
            let mut target_bytes = self.read_bytes_from_access_path(target)?;
            let pointer_value = self.read_numeric_usize(pointer)?.max(1);
            let start = pointer_value.saturating_sub(1);
            let writable = target_bytes.len().saturating_sub(start);
            let copied = writable.min(out.len());
            if copied > 0 {
                target_bytes[start..start + copied].copy_from_slice(&out[..copied]);
            }
            self.write_bytes_to_access_path(target, &target_bytes)?;
            self.write_numeric_usize(pointer, pointer_value.saturating_add(copied))?;
            Ok(copied < out.len())
        } else {
            let target_len = self.read_bytes_from_access_path(target)?.len();
            self.write_bytes_to_access_path(target, &out)?;
            Ok(out.len() > target_len)
        }
    }

    fn unstring_op(
        &mut self,
        source: &VmExpr,
        delimiter: &VmStringDelimiter,
        targets: &[VmUnstringTarget],
        pointer: Option<&VmAccessPath>,
        tallying: Option<&VmAccessPath>,
    ) -> Result<bool, VmError> {
        let source = self.expr_bytes_for_string(source)?;
        let (delimiter, all) = match delimiter {
            VmStringDelimiter::Size => {
                return Err(VmError::ProcedureRuntime {
                    block: String::new(),
                    message: "UNSTRING DELIMITED BY SIZE is not supported".to_string(),
                })
            }
            VmStringDelimiter::Literal { value, all } => (value.as_bytes(), *all),
        };
        if delimiter.is_empty() {
            return Err(VmError::ProcedureRuntime {
                block: String::new(),
                message: "UNSTRING delimiter must not be empty".to_string(),
            });
        }
        let start = pointer
            .map(|pointer| self.read_numeric_usize(pointer))
            .transpose()?
            .unwrap_or(1)
            .max(1)
            .saturating_sub(1);
        let parts = split_bytes_from(&source, delimiter, all, start);
        let assigned = targets.len().min(parts.len());
        for (idx, target) in targets.iter().enumerate() {
            let part = parts.get(idx).map(Vec::as_slice).unwrap_or(&[]);
            self.write_bytes_to_access_path(&target.target, part)?;
            if let Some(count) = &target.count {
                self.write_numeric_usize(count, part.len())?;
            }
        }
        if let Some(pointer) = pointer {
            let next_cursor = unstring_next_cursor(&source, delimiter, all, start, assigned);
            self.write_numeric_usize(pointer, next_cursor.saturating_add(1))?;
        }
        if let Some(tallying) = tallying {
            self.write_numeric_usize(tallying, assigned)?;
        }
        Ok(parts.len() > targets.len())
    }

    fn expr_bytes_for_string(&self, expr: &VmExpr) -> Result<Vec<u8>, VmError> {
        match expr {
            VmExpr::Access(path) => self.read_bytes_from_access_path(path),
            VmExpr::Identifier(name) => self.read_bytes_from_access_path(&VmAccessPath {
                target: name.clone(),
                condition_name: None,
                subscripts: Vec::new(),
                reference_modifier: None,
                result_len: None,
            }),
            _ => Ok(display_value(&self.eval_expr(expr)?).into_bytes()),
        }
    }

    fn read_numeric_usize(&self, path: &VmAccessPath) -> Result<usize, VmError> {
        let value = decimal_to_i128(to_decimal(&self.eval_expr(&VmExpr::Access(path.clone()))?)?)?;
        Ok(value.max(0) as usize)
    }

    fn write_numeric_usize(&mut self, path: &VmAccessPath, value: usize) -> Result<(), VmError> {
        self.move_value_to_access_path(&VmExpr::Number(value.to_string()), path)
    }

    fn next_block(
        &mut self,
        procedure: &VmProcedure,
        block: &VmBasicBlock,
    ) -> Result<Option<String>, VmError> {
        match &block.transfer {
            VmControlTransfer::FallThrough(target) => Ok(target.clone()),
            VmControlTransfer::GoTo(target) => Ok(Some(self.resolve_go_to_target(target))),
            VmControlTransfer::AlteredGoTo { slot } => Ok(self
                .altered_target(slot)
                .or_else(|| self.fallthrough_after(procedure, &block.name))),
            VmControlTransfer::StopRun => Ok(None),
            VmControlTransfer::Perform {
                target,
                through,
                times,
            } => {
                let count = if let Some(times) = times {
                    decimal_to_i128(to_decimal(&self.eval_expr(times)?)?)?.max(0) as usize
                } else {
                    1
                };
                for _ in 0..count {
                    match self.execute_perform_range(procedure, target, through.as_deref())? {
                        VmProcedureSignal::Continue => {}
                        VmProcedureSignal::GoTo(target) => return Ok(Some(target)),
                        VmProcedureSignal::StopRun => return Ok(None),
                    }
                }
                Ok(self.fallthrough_after(procedure, &block.name))
            }
        }
    }

    fn execute_perform_range(
        &mut self,
        procedure: &VmProcedure,
        target: &str,
        through: Option<&str>,
    ) -> Result<VmProcedureSignal, VmError> {
        let through = through.unwrap_or(target);
        let mut perform_stack = vec![VmPerformFrame {
            target: target.to_string(),
            through: through.to_string(),
            return_to: None,
        }];
        let mut current = Some(target.to_string());
        let mut steps = 0usize;
        'perform_loop: while let Some(block_name) = current.clone() {
            steps += 1;
            if steps > 100_000 {
                return Err(VmError::ProcedureRuntime {
                    block: block_name,
                    message: "PERFORM step limit exceeded".to_string(),
                });
            }
            let block = procedure
                .blocks
                .iter()
                .find(|block| block.name == block_name)
                .ok_or_else(|| VmError::ProcedureRuntime {
                    block: block_name.clone(),
                    message: "PERFORM target block is not defined".to_string(),
                })?;
            if let Some(frame) = self.activation_stack.last_mut() {
                frame.current = block_name.clone();
            }
            match self.handle_procedure_entry(procedure, &block_name)? {
                VmProcedureSignal::Continue => {}
                VmProcedureSignal::GoTo(target) => {
                    self.unwind_perform_stack_for_goto(procedure, &mut perform_stack, &target)?;
                    if perform_stack.is_empty() {
                        return Ok(VmProcedureSignal::GoTo(target));
                    }
                    current = Some(target);
                    continue;
                }
                VmProcedureSignal::StopRun => return Ok(VmProcedureSignal::StopRun),
            }
            match self.execute_ops(procedure, &block.ops)? {
                VmProcedureSignal::Continue => {}
                VmProcedureSignal::GoTo(target) => {
                    self.unwind_perform_stack_for_goto(procedure, &mut perform_stack, &target)?;
                    if perform_stack.is_empty() {
                        return Ok(VmProcedureSignal::GoTo(target));
                    }
                    current = Some(target);
                    continue;
                }
                VmProcedureSignal::StopRun => return Ok(VmProcedureSignal::StopRun),
            }
            match &block.transfer {
                VmControlTransfer::GoTo(target) => {
                    let target = self.resolve_go_to_target(target);
                    self.unwind_perform_stack_for_goto(procedure, &mut perform_stack, &target)?;
                    if perform_stack.is_empty() {
                        return Ok(VmProcedureSignal::GoTo(target));
                    }
                    current = Some(target);
                    continue;
                }
                VmControlTransfer::AlteredGoTo { slot } => {
                    if let Some(target) = self.altered_target(slot) {
                        self.unwind_perform_stack_for_goto(procedure, &mut perform_stack, &target)?;
                        if perform_stack.is_empty() {
                            return Ok(VmProcedureSignal::GoTo(target));
                        }
                        current = Some(target);
                        continue;
                    }
                }
                VmControlTransfer::StopRun => return Ok(VmProcedureSignal::StopRun),
                VmControlTransfer::FallThrough(_) | VmControlTransfer::Perform { .. } => {}
            }
            if perform_stack
                .last()
                .map(|frame| frame.through.eq_ignore_ascii_case(&block.name))
                .unwrap_or(false)
            {
                perform_stack.pop();
                if perform_stack.is_empty() {
                    break;
                }
                current = perform_stack
                    .last()
                    .and_then(|frame| frame.return_to.clone());
                continue;
            }
            current = match &block.transfer {
                VmControlTransfer::FallThrough(target) => target.clone(),
                VmControlTransfer::GoTo(target) => {
                    let target = self.resolve_go_to_target(target);
                    self.unwind_perform_stack_for_goto(procedure, &mut perform_stack, &target)?;
                    if perform_stack.is_empty() {
                        return Ok(VmProcedureSignal::GoTo(target));
                    }
                    Some(target)
                }
                VmControlTransfer::AlteredGoTo { slot } => {
                    if let Some(target) = self.altered_target(slot) {
                        self.unwind_perform_stack_for_goto(procedure, &mut perform_stack, &target)?;
                        if perform_stack.is_empty() {
                            return Ok(VmProcedureSignal::GoTo(target));
                        }
                        Some(target)
                    } else {
                        self.fallthrough_after(procedure, &block.name)
                    }
                }
                VmControlTransfer::StopRun => return Ok(VmProcedureSignal::StopRun),
                VmControlTransfer::Perform {
                    target,
                    through,
                    times,
                } => {
                    let count = if let Some(times) = times {
                        decimal_to_i128(to_decimal(&self.eval_expr(times)?)?)?.max(0) as usize
                    } else {
                        1
                    };
                    for _ in 0..count {
                        let signal =
                            self.execute_perform_range(procedure, target, through.as_deref())?;
                        if signal != VmProcedureSignal::Continue {
                            match signal {
                                VmProcedureSignal::GoTo(target) => {
                                    self.unwind_perform_stack_for_goto(
                                        procedure,
                                        &mut perform_stack,
                                        &target,
                                    )?;
                                    if perform_stack.is_empty() {
                                        return Ok(VmProcedureSignal::GoTo(target));
                                    }
                                    current = Some(target);
                                    continue 'perform_loop;
                                }
                                VmProcedureSignal::StopRun => {
                                    return Ok(VmProcedureSignal::StopRun);
                                }
                                VmProcedureSignal::Continue => {}
                            }
                        }
                    }
                    self.fallthrough_after(procedure, &block.name)
                }
            };
        }
        Ok(VmProcedureSignal::Continue)
    }

    fn execute_perform_loop(
        &mut self,
        procedure: &VmProcedure,
        target: &str,
        through: Option<&str>,
        varying: Option<&VmPerformVarying>,
        until: Option<&VmCondition>,
    ) -> Result<VmProcedureSignal, VmError> {
        if let Some(varying) = varying {
            self.initialize_varying_target(varying)?;
        }

        let mut iterations = 0usize;
        loop {
            iterations += 1;
            if iterations > 100_000 {
                return Err(VmError::ProcedureRuntime {
                    block: target.to_string(),
                    message: "PERFORM loop iteration limit exceeded".to_string(),
                });
            }

            if until
                .map(|condition| self.eval_condition(condition))
                .transpose()?
                .unwrap_or(false)
            {
                return Ok(VmProcedureSignal::Continue);
            }

            let signal = self.execute_perform_range(procedure, target, through)?;
            if signal != VmProcedureSignal::Continue {
                return Ok(signal);
            }

            if let Some(varying) = varying {
                self.increment_varying_target(varying)?;
            } else if until.is_none() {
                return Ok(VmProcedureSignal::Continue);
            }
        }
    }

    fn resolve_call_target(&self, target: &VmCallTarget) -> Result<String, VmError> {
        match target {
            VmCallTarget::Literal(name) => Ok(name.trim().to_string()),
            VmCallTarget::Dynamic(expr) => {
                let value = self.eval_expr(expr)?;
                let name = display_value(&value).trim().to_string();
                if name.is_empty() {
                    return Err(VmError::NestedProgramRuntime {
                        message: "dynamic CALL target evaluated to an empty program name"
                            .to_string(),
                    });
                }
                if name.contains(['/', '\\']) {
                    return Err(VmError::NestedProgramRuntime {
                        message: "dynamic CALL target must be a program name, not a path"
                            .to_string(),
                    });
                }
                Ok(name)
            }
        }
    }

    fn reset_initial_program_instance(
        &mut self,
        registered: &VmRegisteredProgram,
    ) -> Result<(), VmError> {
        if !registered.is_initial {
            return Ok(());
        }
        for file in &registered.initial_files {
            self.files.reset_lifecycle_file(file)?;
        }
        for (key, bytes) in &registered.initial_cells {
            self.storage_pool
                .define_or_write_cell(key.clone(), bytes.clone())?;
        }
        for odo in &registered.initial_odo {
            self.storage_pool
                .resize_odo_table(&odo.program, &odo.table, odo.active)?;
        }
        Ok(())
    }

    fn bind_call_using_arguments(
        &self,
        target_name: &str,
        linkage: &[VmLinkageParam],
        using: &[VmAccessPath],
    ) -> Result<BTreeMap<String, VmBinding>, VmError> {
        if linkage.len() != using.len() {
            return Err(VmError::NestedProgramRuntime {
                message: format!(
                    "CALL target {target_name} expects {} USING arguments but got {}",
                    linkage.len(),
                    using.len()
                ),
            });
        }

        let mut bindings = BTreeMap::new();
        for (callee, caller_path) in linkage.iter().zip(using) {
            let binding = self.binding_for_call_argument(caller_path)?;
            bind_call_aliases(
                &mut bindings,
                std::iter::once(callee.name.as_str()),
                binding.clone(),
            );
            if !callee.children.is_empty() {
                let VmBinding::Group { children } = binding else {
                    return Err(VmError::NestedProgramRuntime {
                        message: format!(
                            "CALL target {target_name} expects group argument {} but caller passed scalar storage",
                            callee.name
                        ),
                    });
                };
                if callee.children.len() != children.len() {
                    return Err(VmError::NestedProgramRuntime {
                        message: format!(
                            "CALL target {target_name} group argument {} expects {} child cells but got {}",
                            callee.name,
                            callee.children.len(),
                            children.len()
                        ),
                    });
                }
                for (formal_child, caller_child) in callee.children.iter().zip(children) {
                    let child_binding = self
                        .storage_descriptor_for(&caller_child)
                        .cloned()
                        .ok_or_else(|| VmError::StoragePool {
                            key: caller_child.clone(),
                            message: "CALL USING group child has no StoragePool descriptor"
                                .to_string(),
                        })?;
                    bind_call_aliases(
                        &mut bindings,
                        formal_child.aliases.iter().map(String::as_str),
                        child_binding,
                    );
                }
            }
        }
        Ok(bindings)
    }

    fn binding_for_call_argument(&self, argument: &VmAccessPath) -> Result<VmBinding, VmError> {
        if argument.reference_modifier.is_some() {
            return Err(VmError::NestedProgramRuntime {
                message: format!(
                    "CALL USING reference-modified argument {} is not executable yet",
                    argument.target
                ),
            });
        }
        let materialized = self.materialize_expr(&VmExpr::Access(argument.clone()))?;
        let VmExpr::Access(path) = materialized else {
            return Err(VmError::NestedProgramRuntime {
                message: "CALL USING argument did not materialize to a storage access path"
                    .to_string(),
            });
        };
        let descriptor = self
            .storage_descriptor_for(&path.target)
            .cloned()
            .ok_or_else(|| VmError::StoragePool {
                key: path.target.clone(),
                message: "CALL USING argument has no StoragePool descriptor".to_string(),
            })?;
        match descriptor {
            VmBinding::Cell { key } => Ok(VmBinding::Cell { key }),
            VmBinding::Slice { key, offset, len } => Ok(VmBinding::Slice { key, offset, len }),
            VmBinding::OccursCell { program, item } => {
                let occurrence = self.storage_pool_outer_occurrence(&path)?;
                Ok(VmBinding::Cell {
                    key: StorageKey::occurrence(program, item, vec![occurrence]),
                })
            }
            VmBinding::Group { children } => Ok(VmBinding::Group { children }),
        }
    }

    fn initialize_varying_target(&mut self, varying: &VmPerformVarying) -> Result<(), VmError> {
        match &varying.target {
            VmVaryingTarget::Access(target) => {
                let value = self.eval_expr(&varying.from)?;
                self.write_value_to_access_path(target, &value)
            }
            VmVaryingTarget::Index(name) => {
                let occurrence = decimal_to_i128(to_decimal(&self.eval_expr(&varying.from)?)?)?;
                if occurrence < 1 {
                    return Err(VmError::InvalidSubscript {
                        target: name.clone(),
                        value: occurrence,
                        min: 1,
                        max: usize::MAX,
                    });
                }
                self.set_index(name, occurrence as usize)
            }
        }
    }

    fn increment_varying_target(&mut self, varying: &VmPerformVarying) -> Result<(), VmError> {
        match &varying.target {
            VmVaryingTarget::Access(target) => {
                self.numeric_update_access_path(&varying.by, target, |left, right| left + right)
            }
            VmVaryingTarget::Index(name) => {
                let delta = decimal_to_i128(to_decimal(&self.eval_expr(&varying.by)?)?)?;
                self.adjust_index(name, delta)
            }
        }
    }

    fn unwind_perform_stack_for_goto(
        &self,
        procedure: &VmProcedure,
        stack: &mut Vec<VmPerformFrame>,
        target: &str,
    ) -> Result<(), VmError> {
        while let Some(frame) = stack.last() {
            if self.block_in_perform_scope(procedure, target, frame)? {
                break;
            }
            stack.pop();
        }
        Ok(())
    }

    fn block_in_perform_scope(
        &self,
        procedure: &VmProcedure,
        block: &str,
        frame: &VmPerformFrame,
    ) -> Result<bool, VmError> {
        let Some(block_idx) = self.block_index(procedure, block) else {
            return Err(VmError::ProcedureRuntime {
                block: block.to_string(),
                message: "GO TO target block is not defined".to_string(),
            });
        };
        let Some(start_idx) = self.block_index(procedure, &frame.target) else {
            return Err(VmError::ProcedureRuntime {
                block: frame.target.clone(),
                message: "PERFORM start block is not defined".to_string(),
            });
        };
        let Some(end_idx) = self.block_index(procedure, &frame.through) else {
            return Err(VmError::ProcedureRuntime {
                block: frame.through.clone(),
                message: "PERFORM end block is not defined".to_string(),
            });
        };
        let min = start_idx.min(end_idx);
        let max = start_idx.max(end_idx);
        Ok((min..=max).contains(&block_idx))
    }

    fn block_index(&self, procedure: &VmProcedure, block: &str) -> Option<usize> {
        procedure
            .blocks
            .iter()
            .position(|candidate| candidate.name.eq_ignore_ascii_case(block))
    }

    fn fallthrough_after(&self, procedure: &VmProcedure, block_name: &str) -> Option<String> {
        let idx = self.block_index(procedure, block_name)?;
        procedure
            .blocks
            .get(idx + 1)
            .map(|block| block.name.clone())
    }

    fn materialize_condition(&self, condition: &VmCondition) -> Result<VmCondition, VmError> {
        match condition {
            VmCondition::Relation { left, op, right } => Ok(VmCondition::Relation {
                left: self.materialize_expr(left)?,
                op: *op,
                right: self.materialize_expr(right)?,
            }),
            VmCondition::ClassTest {
                operand,
                class,
                negated,
            } => Ok(VmCondition::ClassTest {
                operand: self.materialize_expr(operand)?,
                class: *class,
                negated: *negated,
            }),
            VmCondition::SignTest {
                operand,
                sign,
                negated,
            } => Ok(VmCondition::SignTest {
                operand: self.materialize_expr(operand)?,
                sign: *sign,
                negated: *negated,
            }),
            VmCondition::ConditionName { reference } => Ok(VmCondition::ConditionName {
                reference: reference.clone(),
            }),
            VmCondition::Not(inner) => Ok(VmCondition::Not(Box::new(
                self.materialize_condition(inner)?,
            ))),
            VmCondition::And(left, right) => Ok(VmCondition::And(
                Box::new(self.materialize_condition(left)?),
                Box::new(self.materialize_condition(right)?),
            )),
            VmCondition::Or(left, right) => Ok(VmCondition::Or(
                Box::new(self.materialize_condition(left)?),
                Box::new(self.materialize_condition(right)?),
            )),
        }
    }

    fn materialize_evaluate(&self, evaluate: &VmEvaluate) -> Result<VmEvaluate, VmError> {
        Ok(VmEvaluate {
            subjects: evaluate
                .subjects
                .iter()
                .map(|subject| self.materialize_expr(subject))
                .collect::<Result<Vec<_>, _>>()?,
            branches: evaluate
                .branches
                .iter()
                .map(|branch| {
                    Ok(VmBranch {
                        patterns: branch
                            .patterns
                            .iter()
                            .map(|pattern| self.materialize_evaluate_pattern(pattern))
                            .collect::<Result<Vec<_>, _>>()?,
                    })
                })
                .collect::<Result<Vec<_>, VmError>>()?,
        })
    }

    fn materialize_evaluate_pattern(
        &self,
        pattern: &VmEvaluatePattern,
    ) -> Result<VmEvaluatePattern, VmError> {
        match pattern {
            VmEvaluatePattern::Any => Ok(VmEvaluatePattern::Any),
            VmEvaluatePattern::Operand(operand) => {
                Ok(VmEvaluatePattern::Operand(self.materialize_expr(operand)?))
            }
            VmEvaluatePattern::Range { start, end } => Ok(VmEvaluatePattern::Range {
                start: self.materialize_expr(start)?,
                end: self.materialize_expr(end)?,
            }),
            VmEvaluatePattern::Condition(condition) => Ok(VmEvaluatePattern::Condition(
                self.materialize_condition(condition)?,
            )),
        }
    }

    fn materialize_expr(&self, expr: &VmExpr) -> Result<VmExpr, VmError> {
        match expr {
            VmExpr::Access(path) => {
                let mut path = path.clone();
                for subscript in &mut path.subscripts {
                    if let Some(index_name) = subscript.index_name.take() {
                        let occurrence = self.index_occurrence(&index_name)?;
                        *subscript.expr = VmExpr::Number(occurrence.to_string());
                    } else {
                        let materialized = self.materialize_expr(&subscript.expr)?;
                        *subscript.expr = materialized;
                    }
                }
                Ok(VmExpr::Access(path))
            }
            VmExpr::Function { function, args } => Ok(VmExpr::Function {
                function: *function,
                args: args
                    .iter()
                    .map(|arg| self.materialize_expr(arg))
                    .collect::<Result<Vec<_>, _>>()?,
            }),
            VmExpr::Condition(condition) => Ok(VmExpr::Condition(Box::new(
                self.materialize_condition(condition)?,
            ))),
            VmExpr::Add(left, right) => Ok(VmExpr::Add(
                Box::new(self.materialize_expr(left)?),
                Box::new(self.materialize_expr(right)?),
            )),
            VmExpr::Subtract(left, right) => Ok(VmExpr::Subtract(
                Box::new(self.materialize_expr(left)?),
                Box::new(self.materialize_expr(right)?),
            )),
            VmExpr::Multiply(left, right) => Ok(VmExpr::Multiply(
                Box::new(self.materialize_expr(left)?),
                Box::new(self.materialize_expr(right)?),
            )),
            VmExpr::Divide(left, right) => Ok(VmExpr::Divide(
                Box::new(self.materialize_expr(left)?),
                Box::new(self.materialize_expr(right)?),
            )),
            VmExpr::Index(name) => Ok(VmExpr::Number(self.index_occurrence(name)?.to_string())),
            other => Ok(other.clone()),
        }
    }
}

impl VmProgram {
    pub fn new(
        dialect: DialectProfile,
        fields: Vec<VmField>,
        conditions: Vec<VmConditionName>,
    ) -> Self {
        Self::with_declared_views(dialect, fields, conditions, Vec::new())
    }

    pub fn with_declared_views(
        dialect: DialectProfile,
        mut fields: Vec<VmField>,
        conditions: Vec<VmConditionName>,
        declared_views: Vec<VmDeclaredView>,
    ) -> Self {
        if !fields
            .iter()
            .any(|field| normalize_vm_key(&field.name) == PROGRAM_STATUS_REGISTER)
        {
            fields.push(VmField {
                name: PROGRAM_STATUS_REGISTER.to_string(),
                offset: 0,
                byte_len: 2,
                category: VmCategory::Alphanumeric,
                usage: VmUsage::Display,
                picture: None,
            });
        }
        if !fields
            .iter()
            .any(|field| normalize_vm_key(&field.name) == TALLY_REGISTER)
        {
            fields.push(VmField {
                name: TALLY_REGISTER.to_string(),
                offset: 0,
                byte_len: 9,
                category: VmCategory::NumericDisplay,
                usage: VmUsage::Display,
                picture: Some(VmPicture {
                    signed: false,
                    digits: 9,
                    scale: 0,
                    char_len: 9,
                }),
            });
        }
        if !fields
            .iter()
            .any(|field| normalize_vm_key(&field.name) == DEBUG_ITEM_REGISTER)
        {
            fields.push(VmField {
                name: DEBUG_ITEM_REGISTER.to_string(),
                offset: 0,
                byte_len: 64,
                category: VmCategory::Alphanumeric,
                usage: VmUsage::Display,
                picture: None,
            });
        }
        if !fields
            .iter()
            .any(|field| normalize_vm_key(&field.name) == DEBUG_CONTENTS_REGISTER)
        {
            fields.push(VmField {
                name: DEBUG_CONTENTS_REGISTER.to_string(),
                offset: 0,
                byte_len: 16,
                category: VmCategory::Alphanumeric,
                usage: VmUsage::Display,
                picture: None,
            });
        }
        let condition_views = declared_views
            .into_iter()
            .map(|view| (normalize_vm_key(&view.condition), view))
            .collect();
        Self {
            dialect,
            fields,
            conditions,
            condition_views,
        }
    }

    pub fn eval_condition(&self, bytes: &[u8], condition: &VmCondition) -> Result<bool, VmError> {
        match condition {
            VmCondition::Relation { left, op, right } => {
                self.eval_relation(bytes, left, *op, right)
            }
            VmCondition::ClassTest {
                operand,
                class,
                negated,
            } => {
                let result = self.eval_class_test(bytes, operand, *class)?;
                Ok(if *negated { !result } else { result })
            }
            VmCondition::SignTest {
                operand,
                sign,
                negated,
            } => {
                let result = self.eval_sign_test(bytes, operand, *sign)?;
                Ok(if *negated { !result } else { result })
            }
            VmCondition::ConditionName { reference } => self.eval_condition_name(bytes, reference),
            VmCondition::Not(inner) => Ok(!self.eval_condition(bytes, inner)?),
            VmCondition::And(left, right) => {
                if !self.eval_condition(bytes, left)? {
                    return Ok(false);
                }
                self.eval_condition(bytes, right)
            }
            VmCondition::Or(left, right) => {
                if self.eval_condition(bytes, left)? {
                    return Ok(true);
                }
                self.eval_condition(bytes, right)
            }
        }
    }

    pub fn eval_operand(
        &self,
        bytes: &[u8],
        operand: &VmOperand,
    ) -> Result<VmEvaluatedValue, VmError> {
        self.eval_expr(bytes, operand)
    }

    pub fn eval_expr(&self, bytes: &[u8], operand: &VmExpr) -> Result<VmEvaluatedValue, VmError> {
        match operand {
            VmExpr::Access(path) => self.eval_access_path(bytes, path),
            VmOperand::Identifier(name) => self.eval_identifier(bytes, name),
            VmOperand::Index(name) => Err(VmError::UnsupportedIndex { name: name.clone() }),
            VmOperand::Literal(value) => Ok(VmEvaluatedValue {
                value: VmValue::Text(value.clone()),
                category: VmCategory::Alphanumeric,
                byte_len: value.len(),
            }),
            VmOperand::Number(value) => Ok(VmEvaluatedValue {
                value: VmValue::Decimal(parse_decimal(value)?),
                category: VmCategory::NumericDisplay,
                byte_len: value.len(),
            }),
            VmOperand::Figurative(figurative) => Ok(VmEvaluatedValue {
                value: match figurative {
                    VmFigurative::Zero => VmValue::Decimal(Decimal::ZERO),
                    VmFigurative::Space => VmValue::Text(" ".to_string()),
                    VmFigurative::HighValue => VmValue::Text('\u{00FF}'.to_string()),
                    VmFigurative::LowValue => VmValue::Text("\0".to_string()),
                    VmFigurative::Quote => VmValue::Text("\"".to_string()),
                },
                category: match figurative {
                    VmFigurative::Zero => VmCategory::NumericDisplay,
                    _ => VmCategory::Alphanumeric,
                },
                byte_len: 1,
            }),
            VmOperand::AllLiteral(value) => Ok(VmEvaluatedValue {
                value: VmValue::Text(value.clone()),
                category: VmCategory::Alphanumeric,
                byte_len: value.len(),
            }),
            VmExpr::Function { function, args } => self.eval_function(bytes, *function, args),
            VmExpr::Condition(condition) => Ok(VmEvaluatedValue {
                value: VmValue::Bool(self.eval_condition(bytes, condition)?),
                category: VmCategory::Unsupported,
                byte_len: 1,
            }),
            VmExpr::Add(left, right) => {
                let left = to_decimal(&self.eval_expr(bytes, left)?)?;
                let right = to_decimal(&self.eval_expr(bytes, right)?)?;
                Ok(VmEvaluatedValue {
                    value: VmValue::Decimal(left + right),
                    category: VmCategory::NumericDisplay,
                    byte_len: 0,
                })
            }
            VmExpr::Subtract(left, right) => {
                let left = to_decimal(&self.eval_expr(bytes, left)?)?;
                let right = to_decimal(&self.eval_expr(bytes, right)?)?;
                Ok(VmEvaluatedValue {
                    value: VmValue::Decimal(left - right),
                    category: VmCategory::NumericDisplay,
                    byte_len: 0,
                })
            }
            VmExpr::Multiply(left, right) => {
                let left = to_decimal(&self.eval_expr(bytes, left)?)?;
                let right = to_decimal(&self.eval_expr(bytes, right)?)?;
                Ok(VmEvaluatedValue {
                    value: VmValue::Decimal(left * right),
                    category: VmCategory::NumericDisplay,
                    byte_len: 0,
                })
            }
            VmExpr::Divide(left, right) => {
                let left = to_decimal(&self.eval_expr(bytes, left)?)?;
                let right = to_decimal(&self.eval_expr(bytes, right)?)?;
                if right.is_zero() {
                    return Err(VmError::InvalidDecimal {
                        value: "division by zero".to_string(),
                    });
                }
                Ok(VmEvaluatedValue {
                    value: VmValue::Decimal(left / right),
                    category: VmCategory::NumericDisplay,
                    byte_len: 0,
                })
            }
            VmOperand::Bool(value) => Ok(VmEvaluatedValue {
                value: VmValue::Bool(*value),
                category: VmCategory::Unsupported,
                byte_len: 1,
            }),
        }
    }

    pub fn eval_access_path(
        &self,
        bytes: &[u8],
        path: &VmAccessPath,
    ) -> Result<VmEvaluatedValue, VmError> {
        if let Some(condition_name) = &path.condition_name {
            if path.reference_modifier.is_some() {
                return Err(VmError::InvalidReferenceModification {
                    target: condition_name.clone(),
                    message: "condition-name predicates cannot be reference-modified".to_string(),
                });
            }
            return Ok(VmEvaluatedValue {
                value: VmValue::Bool(self.eval_condition_name_at_access_path(
                    bytes,
                    path,
                    condition_name,
                )?),
                category: VmCategory::Unsupported,
                byte_len: 1,
            });
        }
        if !self.condition_candidates(&path.target).is_empty() {
            if path.reference_modifier.is_some() {
                return Err(VmError::InvalidReferenceModification {
                    target: path.target.clone(),
                    message: "condition-name predicates cannot be reference-modified".to_string(),
                });
            }
            return Ok(VmEvaluatedValue {
                value: VmValue::Bool(self.eval_condition_name(bytes, &path.target)?),
                category: VmCategory::Unsupported,
                byte_len: 1,
            });
        }

        let field = self.field(&path.target)?;
        let (offset, len) = self.access_range(bytes, field, path)?;
        let slice = bytes
            .get(offset..offset.saturating_add(len))
            .ok_or_else(|| VmError::FieldOutOfBounds {
                name: field.name.clone(),
                offset,
                end: offset.saturating_add(len),
                len: bytes.len(),
            })?;
        let category = if path.reference_modifier.is_some() {
            VmCategory::Alphanumeric
        } else {
            field.category
        };
        self.decode_field_value(field, category, slice)
    }

    pub fn eval_evaluate(
        &self,
        bytes: &[u8],
        evaluate: &VmEvaluate,
    ) -> Result<Option<usize>, VmError> {
        let subjects = evaluate
            .subjects
            .iter()
            .map(|subject| self.eval_expr(bytes, subject))
            .collect::<Result<Vec<_>, _>>()?;
        for (idx, branch) in evaluate.branches.iter().enumerate() {
            if branch.patterns.len() != subjects.len() {
                return Err(VmError::UnsupportedOperand {
                    message: format!(
                        "EVALUATE branch has {} patterns for {} subjects",
                        branch.patterns.len(),
                        subjects.len()
                    ),
                });
            }
            let mut matched = true;
            for (subject, pattern) in subjects.iter().zip(&branch.patterns) {
                if !self.match_evaluate_pattern(bytes, subject, pattern)? {
                    matched = false;
                    break;
                }
            }
            if matched {
                return Ok(Some(idx));
            }
        }
        Ok(None)
    }

    pub fn match_evaluate_pattern(
        &self,
        bytes: &[u8],
        subject: &VmEvaluatedValue,
        pattern: &VmEvaluatePattern,
    ) -> Result<bool, VmError> {
        match pattern {
            VmEvaluatePattern::Any => Ok(true),
            VmEvaluatePattern::Operand(operand) => {
                let value = self.eval_operand(bytes, operand)?;
                self.values_equal(subject, &value)
            }
            VmEvaluatePattern::Range { start, end } => {
                let start = self.eval_operand(bytes, start)?;
                let end = self.eval_operand(bytes, end)?;
                self.value_in_range(subject, &start, &end)
            }
            VmEvaluatePattern::Condition(condition) => {
                let result = self.eval_condition(bytes, condition)?;
                if matches!(subject.value, VmValue::Bool(_)) {
                    self.values_equal(
                        subject,
                        &VmEvaluatedValue {
                            value: VmValue::Bool(result),
                            category: VmCategory::Unsupported,
                            byte_len: 1,
                        },
                    )
                } else {
                    Ok(result)
                }
            }
        }
    }

    pub fn set_condition_name(&self, bytes: &mut [u8], name: &str) -> Result<(), VmError> {
        let condition = self.condition(name)?;
        let first = condition
            .values
            .first()
            .ok_or_else(|| VmError::UnsupportedOperand {
                message: format!("condition name {name} has no values"),
            })?;
        let value = match first {
            VmConditionValue::Single(value) => value.clone(),
            VmConditionValue::Range { start, .. } => start.clone(),
        };
        let field = self.field(&condition.parent)?;
        if let Some(view) = self.condition_declared_view(condition) {
            if !view.children.is_empty() {
                return self.write_declared_view_bytes(bytes, &view.children, value.as_bytes());
            }
        }
        let field_bytes = self.field_bytes_mut(bytes, field)?;
        match field.category {
            VmCategory::Alphanumeric | VmCategory::Alphabetic | VmCategory::Group => {
                field_bytes.fill(b' ');
                for (idx, byte) in value.as_bytes().iter().take(field.byte_len).enumerate() {
                    field_bytes[idx] = *byte;
                }
                Ok(())
            }
            VmCategory::NumericDisplay => {
                let rendered = render_numeric_display_with_picture(
                    &value,
                    field.byte_len,
                    field.picture.as_ref(),
                )?;
                field_bytes.copy_from_slice(rendered.as_bytes());
                Ok(())
            }
            _ => Err(VmError::UnsupportedOperand {
                message: format!(
                    "SET condition-name for {:?} parent {} is not enabled",
                    field.category, field.name
                ),
            }),
        }
    }

    fn write_declared_view_bytes(
        &self,
        record: &mut [u8],
        children: &[String],
        source: &[u8],
    ) -> Result<(), VmError> {
        let mut cursor = 0usize;
        for child in children {
            let field = self.field(child)?;
            let field_len = field.byte_len;
            let end = cursor.saturating_add(field_len).min(source.len());
            let chunk = if cursor < source.len() {
                &source[cursor..end]
            } else {
                &[]
            };
            let field_bytes = self.field_bytes_mut(record, field)?;
            field_bytes.fill(b' ');
            for (idx, byte) in chunk.iter().enumerate() {
                field_bytes[idx] = *byte;
            }
            cursor = cursor.saturating_add(field_len);
        }
        Ok(())
    }

    fn eval_relation(
        &self,
        bytes: &[u8],
        left: &VmOperand,
        op: VmRelOp,
        right: &VmOperand,
    ) -> Result<bool, VmError> {
        let left_value = self.eval_operand(bytes, left)?;
        let right_value = self.eval_operand(bytes, right)?;
        let ordering = self.compare_values(&left_value, &right_value)?;
        Ok(match op {
            VmRelOp::Equal => ordering == Ordering::Equal,
            VmRelOp::NotEqual => ordering != Ordering::Equal,
            VmRelOp::Greater => ordering == Ordering::Greater,
            VmRelOp::GreaterOrEqual => matches!(ordering, Ordering::Greater | Ordering::Equal),
            VmRelOp::Less => ordering == Ordering::Less,
            VmRelOp::LessOrEqual => matches!(ordering, Ordering::Less | Ordering::Equal),
        })
    }

    fn eval_identifier(&self, bytes: &[u8], name: &str) -> Result<VmEvaluatedValue, VmError> {
        if !self.condition_candidates(name).is_empty() {
            return Ok(VmEvaluatedValue {
                value: VmValue::Bool(self.eval_condition_name(bytes, name)?),
                category: VmCategory::Unsupported,
                byte_len: 1,
            });
        }
        let field = self.field(name)?;
        let field_bytes = self.field_bytes(bytes, field)?;
        self.decode_field_value(field, field.category, field_bytes)
    }

    fn decode_field_value(
        &self,
        field: &VmField,
        category: VmCategory,
        field_bytes: &[u8],
    ) -> Result<VmEvaluatedValue, VmError> {
        let value = match category {
            VmCategory::Group
            | VmCategory::Alphanumeric
            | VmCategory::Alphabetic
            | VmCategory::NumericEdited => {
                VmValue::Text(String::from_utf8_lossy(field_bytes).trim_end().to_string())
            }
            VmCategory::National => {
                if field_bytes.len() % 2 != 0 {
                    return Err(VmError::UnsupportedOperand {
                        message: format!(
                            "national field {} has odd byte length {}",
                            field.name,
                            field_bytes.len()
                        ),
                    });
                }
                let mut units = Vec::with_capacity(field_bytes.len() / 2);
                for chunk in field_bytes.chunks_exact(2) {
                    units.push(u16::from_be_bytes([chunk[0], chunk[1]]));
                }
                let text = String::from_utf16(&units).map_err(|err| VmError::Codec {
                    name: field.name.clone(),
                    message: err.to_string(),
                })?;
                VmValue::NationalText(text.trim_end().to_string())
            }
            VmCategory::Dbcs => VmValue::DbcsText(field_bytes.to_vec()),
            VmCategory::NumericDisplay => {
                VmValue::Decimal(self.decode_display_decimal(field, field_bytes)?)
            }
            VmCategory::PackedDecimal => VmValue::Decimal(self.decode_packed(field, field_bytes)?),
            VmCategory::Binary | VmCategory::NativeBinary => {
                let signed = field
                    .picture
                    .as_ref()
                    .map(|picture| picture.signed)
                    .unwrap_or(false);
                match decode_binary_integer(field_bytes, signed, Endian::Big).map_err(|err| {
                    VmError::Codec {
                        name: field.name.clone(),
                        message: err.to_string(),
                    }
                })? {
                    DecodedValue::Integer(value) => VmValue::Integer(value),
                    DecodedValue::UnsignedInteger(value) => VmValue::UnsignedInteger(value),
                    other => VmValue::Text(format!("{other:?}")),
                }
            }
            VmCategory::Float => VmValue::Float(self.decode_float(field, field_bytes)?),
            VmCategory::Unsupported => VmValue::Bytes(field_bytes.to_vec()),
        };
        Ok(VmEvaluatedValue {
            value,
            category,
            byte_len: field_bytes.len(),
        })
    }

    fn access_range(
        &self,
        bytes: &[u8],
        field: &VmField,
        path: &VmAccessPath,
    ) -> Result<(usize, usize), VmError> {
        let mut offset = field.offset;
        let mut len = path.result_len.unwrap_or(field.byte_len);
        for subscript in &path.subscripts {
            let value = self.subscript_value(bytes, subscript)?;
            if value < subscript.min as i128 || value > subscript.max as i128 {
                return Err(VmError::InvalidSubscript {
                    target: path.target.clone(),
                    value,
                    min: subscript.min,
                    max: subscript.max,
                });
            }
            if let Some(depending_on) = &subscript.depending_on {
                let active = self.eval_identifier(bytes, depending_on)?;
                let active = decimal_to_i128(to_decimal(&active)?)?;
                if value > active {
                    return Err(VmError::InvalidSubscript {
                        target: path.target.clone(),
                        value,
                        min: subscript.min,
                        max: active.max(0) as usize,
                    });
                }
            }
            offset = offset.saturating_add((value as usize - 1).saturating_mul(subscript.stride));
            len = path.result_len.unwrap_or(subscript.stride);
        }
        if let Some(reference_modifier) = &path.reference_modifier {
            if !matches!(
                field.category,
                VmCategory::Group
                    | VmCategory::Alphanumeric
                    | VmCategory::Alphabetic
                    | VmCategory::NumericEdited
            ) {
                return Err(VmError::InvalidReferenceModification {
                    target: path.target.clone(),
                    message: format!("category {:?} is not reference-modifiable", field.category),
                });
            }
            let start = decimal_to_i128(to_decimal(
                &self.eval_expr(bytes, &reference_modifier.start)?,
            )?)?;
            if start <= 0 {
                return Err(VmError::InvalidReferenceModification {
                    target: path.target.clone(),
                    message: format!("start {start} is outside COBOL's 1-based range"),
                });
            }
            let zero_based = (start as usize).saturating_sub(1);
            if zero_based > len {
                return Err(VmError::InvalidReferenceModification {
                    target: path.target.clone(),
                    message: format!("start {start} exceeds length {len}"),
                });
            }
            let requested = if let Some(length) = &reference_modifier.length {
                decimal_to_i128(to_decimal(&self.eval_expr(bytes, length)?)?)? as usize
            } else {
                len.saturating_sub(zero_based)
            };
            if zero_based.saturating_add(requested) > len {
                return Err(VmError::InvalidReferenceModification {
                    target: path.target.clone(),
                    message: format!(
                        "slice {}:{} exceeds available length {}",
                        start, requested, len
                    ),
                });
            }
            offset = offset.saturating_add(zero_based);
            len = requested;
        }
        Ok((offset, len))
    }

    fn subscript_value(&self, bytes: &[u8], subscript: &VmSubscript) -> Result<i128, VmError> {
        if let Some(index_name) = &subscript.index_name {
            return Err(VmError::UnsupportedIndex {
                name: index_name.clone(),
            });
        }
        decimal_to_i128(to_decimal(&self.eval_expr(bytes, &subscript.expr)?)?)
    }

    fn eval_function(
        &self,
        bytes: &[u8],
        function: VmFunction,
        args: &[VmExpr],
    ) -> Result<VmEvaluatedValue, VmError> {
        match function {
            VmFunction::Length => {
                let arg = args.first().ok_or_else(|| VmError::UnsupportedOperand {
                    message: "FUNCTION LENGTH requires one argument".to_string(),
                })?;
                let value = self.eval_expr(bytes, arg)?;
                Ok(VmEvaluatedValue {
                    value: VmValue::Integer(value.byte_len as i64),
                    category: VmCategory::NumericDisplay,
                    byte_len: 0,
                })
            }
            VmFunction::Ord => {
                let arg = args.first().ok_or_else(|| VmError::UnsupportedOperand {
                    message: "FUNCTION ORD requires one argument".to_string(),
                })?;
                let value = self.eval_expr(bytes, arg)?;
                let text = value_text(&value).ok_or_else(|| VmError::UnsupportedOperand {
                    message: "FUNCTION ORD argument is not text".to_string(),
                })?;
                let code = text.bytes().next().unwrap_or(0) as i64;
                Ok(VmEvaluatedValue {
                    value: VmValue::Integer(code),
                    category: VmCategory::NumericDisplay,
                    byte_len: 0,
                })
            }
            VmFunction::Numval => {
                let arg = args.first().ok_or_else(|| VmError::UnsupportedOperand {
                    message: "FUNCTION NUMVAL requires one argument".to_string(),
                })?;
                let value = self.eval_expr(bytes, arg)?;
                let text = match value.value {
                    VmValue::Text(text) => text,
                    VmValue::Decimal(value) => value.to_string(),
                    other => {
                        return Err(VmError::UnsupportedOperand {
                            message: format!("FUNCTION NUMVAL cannot parse {other:?}"),
                        })
                    }
                };
                Ok(VmEvaluatedValue {
                    value: VmValue::Decimal(parse_decimal(&text)?),
                    category: VmCategory::NumericDisplay,
                    byte_len: 0,
                })
            }
            VmFunction::UserDefined => Err(VmError::UnsupportedFunction {
                name: "USER-DEFINED".to_string(),
            }),
        }
    }

    fn eval_condition_name(&self, bytes: &[u8], name: &str) -> Result<bool, VmError> {
        let condition = self.condition(name)?;
        let parent = self.eval_condition_parent(bytes, condition)?;
        self.condition_name_matches_value(condition, &parent)
    }

    fn eval_condition_name_at_access_path(
        &self,
        bytes: &[u8],
        path: &VmAccessPath,
        name: &str,
    ) -> Result<bool, VmError> {
        let condition = self.condition(name)?;
        let field = self.field(&path.target)?;
        let (offset, len) = self.access_range(bytes, field, path)?;
        let slice = bytes
            .get(offset..offset.saturating_add(len))
            .ok_or_else(|| VmError::FieldOutOfBounds {
                name: field.name.clone(),
                offset,
                end: offset.saturating_add(len),
                len: bytes.len(),
            })?;
        let mut decode_field = self
            .field(&condition.parent)
            .cloned()
            .unwrap_or_else(|_| field.clone());
        decode_field.offset = 0;
        decode_field.byte_len = slice.len();
        let parent = self.decode_field_value(&decode_field, decode_field.category, slice)?;
        self.condition_name_matches_value(condition, &parent)
    }

    fn condition_name_matches_value(
        &self,
        condition: &VmConditionName,
        parent: &VmEvaluatedValue,
    ) -> Result<bool, VmError> {
        for expected in &condition.values {
            match expected {
                VmConditionValue::Single(value) => {
                    let expected =
                        self.literal_for_category(value, parent.category, parent.byte_len)?;
                    if self.values_equal(&parent, &expected)? {
                        return Ok(true);
                    }
                }
                VmConditionValue::Range { start, end } => {
                    let start =
                        self.literal_for_category(start, parent.category, parent.byte_len)?;
                    let end = self.literal_for_category(end, parent.category, parent.byte_len)?;
                    if self.value_in_range(&parent, &start, &end)? {
                        return Ok(true);
                    }
                }
            }
        }
        Ok(false)
    }

    fn eval_condition_parent(
        &self,
        bytes: &[u8],
        condition: &VmConditionName,
    ) -> Result<VmEvaluatedValue, VmError> {
        let Some(view) = self.condition_declared_view(condition) else {
            return self.eval_identifier(bytes, &condition.parent);
        };
        if view.children.is_empty() {
            return self.eval_identifier(bytes, &condition.parent);
        }
        let view_bytes = self.read_declared_view_bytes(bytes, &view.children)?;
        let field = self.field(&view.parent)?.clone();
        let mut decode_field = field;
        decode_field.offset = 0;
        decode_field.byte_len = view_bytes.len();
        self.decode_field_value(&decode_field, decode_field.category, &view_bytes)
    }

    fn read_declared_view_bytes(
        &self,
        record: &[u8],
        children: &[String],
    ) -> Result<Vec<u8>, VmError> {
        let mut bytes = Vec::new();
        for child in children {
            let field = self.field(child)?;
            bytes.extend_from_slice(self.field_bytes(record, field)?);
        }
        Ok(bytes)
    }

    fn eval_class_test(
        &self,
        bytes: &[u8],
        operand: &VmOperand,
        class: VmClassTest,
    ) -> Result<bool, VmError> {
        if let VmExpr::Access(path) = operand {
            let field = self.field(&path.target)?;
            let (offset, len) = self.access_range(bytes, field, path)?;
            let field_bytes = bytes
                .get(offset..offset.saturating_add(len))
                .ok_or_else(|| VmError::FieldOutOfBounds {
                    name: field.name.clone(),
                    offset,
                    end: offset.saturating_add(len),
                    len: bytes.len(),
                })?;
            return Ok(match class {
                VmClassTest::Numeric => match field.category {
                    VmCategory::NumericDisplay => self.display_bytes_numeric(field, field_bytes),
                    VmCategory::PackedDecimal => self.packed_bytes_numeric(field, field_bytes),
                    VmCategory::Binary | VmCategory::NativeBinary => true,
                    _ => false,
                },
                VmClassTest::Alphabetic
                | VmClassTest::AlphabeticUpper
                | VmClassTest::AlphabeticLower => {
                    alphabetic_text(&String::from_utf8_lossy(field_bytes), class)
                }
            });
        }
        let VmExpr::Identifier(name) = operand else {
            let value = self.eval_operand(bytes, operand)?;
            return Ok(match class {
                VmClassTest::Numeric => to_decimal(&value).is_ok(),
                VmClassTest::Alphabetic
                | VmClassTest::AlphabeticUpper
                | VmClassTest::AlphabeticLower => value_text(&value)
                    .map(|text| alphabetic_text(&text, class))
                    .unwrap_or(false),
            });
        };
        let field = self.field(name)?;
        let field_bytes = self.field_bytes(bytes, field)?;
        Ok(match class {
            VmClassTest::Numeric => match field.category {
                VmCategory::NumericDisplay => self.display_bytes_numeric(field, field_bytes),
                VmCategory::PackedDecimal => self.packed_bytes_numeric(field, field_bytes),
                VmCategory::Binary | VmCategory::NativeBinary => true,
                _ => false,
            },
            VmClassTest::Alphabetic
            | VmClassTest::AlphabeticUpper
            | VmClassTest::AlphabeticLower => {
                alphabetic_text(&String::from_utf8_lossy(field_bytes), class)
            }
        })
    }

    fn eval_sign_test(
        &self,
        bytes: &[u8],
        operand: &VmOperand,
        sign: VmSignTest,
    ) -> Result<bool, VmError> {
        let value = self.eval_operand(bytes, operand)?;
        let decimal = to_decimal(&value)?;
        Ok(match sign {
            VmSignTest::Positive => decimal > Decimal::ZERO,
            VmSignTest::Negative => decimal < Decimal::ZERO,
            VmSignTest::Zero => decimal == Decimal::ZERO,
        })
    }

    fn compare_values(
        &self,
        left: &VmEvaluatedValue,
        right: &VmEvaluatedValue,
    ) -> Result<Ordering, VmError> {
        if is_numeric(left.category) && is_numeric(right.category) {
            return Ok(to_decimal(left)?.cmp(&to_decimal(right)?));
        }
        if is_nonnumeric(left.category) && is_nonnumeric(right.category) {
            return Ok(self.compare_text(
                &text_for_compare(left, right.byte_len)?,
                &text_for_compare(right, left.byte_len)?,
            ));
        }
        if matches!(left.value, VmValue::Bool(_)) || matches!(right.value, VmValue::Bool(_)) {
            return Ok(bool_value(left)?.cmp(&bool_value(right)?));
        }
        Err(VmError::UnsupportedComparison {
            left: left.category,
            right: right.category,
        })
    }

    fn values_equal(
        &self,
        left: &VmEvaluatedValue,
        right: &VmEvaluatedValue,
    ) -> Result<bool, VmError> {
        Ok(self.compare_values(left, right)? == Ordering::Equal)
    }

    fn value_in_range(
        &self,
        value: &VmEvaluatedValue,
        start: &VmEvaluatedValue,
        end: &VmEvaluatedValue,
    ) -> Result<bool, VmError> {
        Ok(self.compare_values(value, start)? != Ordering::Less
            && self.compare_values(value, end)? != Ordering::Greater)
    }

    fn literal_for_category(
        &self,
        value: &str,
        category: VmCategory,
        byte_len: usize,
    ) -> Result<VmEvaluatedValue, VmError> {
        if is_numeric(category) {
            Ok(VmEvaluatedValue {
                value: VmValue::Decimal(parse_decimal(value)?),
                category,
                byte_len,
            })
        } else {
            Ok(VmEvaluatedValue {
                value: VmValue::Text(value.to_string()),
                category,
                byte_len: value.len(),
            })
        }
    }

    fn decode_display_decimal(&self, field: &VmField, bytes: &[u8]) -> Result<Decimal, VmError> {
        let text = String::from_utf8_lossy(bytes).trim().to_string();
        if !display_bytes_numeric(bytes) {
            return match self.dialect.invalid_numeric_policy {
                InvalidNumericPolicy::TreatAsZero => Ok(Decimal::ZERO),
                InvalidNumericPolicy::Error => Err(VmError::InvalidDecimal { value: text }),
            };
        }
        let scale = field
            .picture
            .as_ref()
            .map(|picture| picture.scale)
            .unwrap_or(0);
        let signed = text.starts_with('-') || text.ends_with('-');
        let digits = text.trim_matches(['+', '-']).to_string();
        let mantissa = i128::from_str(&digits).map_err(|_| VmError::InvalidDecimal {
            value: text.clone(),
        })?;
        let mantissa = if signed { -mantissa } else { mantissa };
        let mut value = Decimal::from_i128_with_scale(mantissa, scale);
        if signed && mantissa == 0 {
            value.set_sign_negative(true);
        }
        Ok(value)
    }

    fn decode_packed(&self, field: &VmField, bytes: &[u8]) -> Result<Decimal, VmError> {
        let picture = field
            .picture
            .as_ref()
            .ok_or_else(|| VmError::UnsupportedOperand {
                message: format!("packed field {} has no picture", field.name),
            })?;
        decode_packed_decimal(bytes, picture.digits, picture.scale, picture.signed).map_err(|err| {
            VmError::Codec {
                name: field.name.clone(),
                message: err.to_string(),
            }
        })
    }

    fn display_bytes_numeric(&self, field: &VmField, bytes: &[u8]) -> bool {
        let Some(parts) = display_numeric_parts(bytes) else {
            return false;
        };
        let Some(picture) = &field.picture else {
            return true;
        };
        if !picture.signed && parts.sign.is_some() {
            return false;
        }
        if self.dialect.numproc == Numproc::Pfd && picture.signed && parts.is_zero {
            return !matches!(parts.sign, Some('-'));
        }
        true
    }

    fn packed_bytes_numeric(&self, field: &VmField, bytes: &[u8]) -> bool {
        let Ok(value) = self.decode_packed(field, bytes) else {
            return false;
        };
        if self.dialect.numproc != Numproc::Pfd {
            return true;
        }
        let Some(picture) = &field.picture else {
            return true;
        };
        if !picture.signed {
            return true;
        }
        let Some(sign) = bytes.last().map(|byte| byte & 0x0F) else {
            return false;
        };
        match sign {
            0x0C => !value.is_sign_negative(),
            0x0D => value < Decimal::ZERO,
            0x0F => value == Decimal::ZERO,
            _ => false,
        }
    }

    fn encode_packed(&self, field: &VmField, value: Decimal) -> Result<Vec<u8>, VmError> {
        let picture = field
            .picture
            .as_ref()
            .ok_or_else(|| VmError::UnsupportedOperand {
                message: format!("packed field {} has no picture", field.name),
            })?;
        encode_packed_decimal(value, picture.digits, picture.scale, picture.signed).map_err(|err| {
            VmError::Codec {
                name: field.name.clone(),
                message: err.to_string(),
            }
        })
    }

    fn encode_binary(&self, field: &VmField, value: Decimal) -> Result<Vec<u8>, VmError> {
        let signed = field
            .picture
            .as_ref()
            .map(|picture| picture.signed)
            .unwrap_or(false);
        let value = decimal_to_i128(value)?;
        encode_binary_integer(value, signed, field.byte_len, Endian::Big).map_err(|err| {
            VmError::Codec {
                name: field.name.clone(),
                message: err.to_string(),
            }
        })
    }

    fn decode_float(&self, field: &VmField, bytes: &[u8]) -> Result<f64, VmError> {
        let decoded =
            match (self.dialect.float_format, field.usage, bytes.len()) {
                (FloatFormat::IbmHex, VmUsage::Float32, _) | (FloatFormat::IbmHex, _, 4) => {
                    decode_ibm_float32(bytes, Endian::Big)
                }
                (FloatFormat::IbmHex, VmUsage::Float64, _) | (FloatFormat::IbmHex, _, 8) => {
                    decode_ibm_float64(bytes, Endian::Big)
                }
                (FloatFormat::IeeeBinary, VmUsage::Float32, _)
                | (FloatFormat::IeeeBinary, _, 4) => decode_ieee_float32(bytes, Endian::Big),
                (FloatFormat::IeeeBinary, VmUsage::Float64, _)
                | (FloatFormat::IeeeBinary, _, 8) => decode_ieee_float64(bytes, Endian::Big),
                _ => {
                    return Err(VmError::Codec {
                        name: field.name.clone(),
                        message: format!("invalid float width {}", bytes.len()),
                    });
                }
            };
        decoded.map_err(|err| VmError::Codec {
            name: field.name.clone(),
            message: err.to_string(),
        })
    }

    fn encode_float(&self, field: &VmField, value: f64) -> Result<Vec<u8>, VmError> {
        let encoded =
            match (self.dialect.float_format, field.usage, field.byte_len) {
                (FloatFormat::IbmHex, VmUsage::Float32, _) | (FloatFormat::IbmHex, _, 4) => {
                    encode_ibm_float32(value, Endian::Big)
                }
                (FloatFormat::IbmHex, VmUsage::Float64, _) | (FloatFormat::IbmHex, _, 8) => {
                    encode_ibm_float64(value, Endian::Big)
                }
                (FloatFormat::IeeeBinary, VmUsage::Float32, _)
                | (FloatFormat::IeeeBinary, _, 4) => encode_ieee_float32(value, Endian::Big),
                (FloatFormat::IeeeBinary, VmUsage::Float64, _)
                | (FloatFormat::IeeeBinary, _, 8) => encode_ieee_float64(value, Endian::Big),
                _ => {
                    return Err(VmError::Codec {
                        name: field.name.clone(),
                        message: format!("invalid float width {}", field.byte_len),
                    });
                }
            };
        encoded.map_err(|err| VmError::Codec {
            name: field.name.clone(),
            message: err.to_string(),
        })
    }

    fn compare_text(&self, left: &str, right: &str) -> Ordering {
        match self.dialect.collating_sequence {
            CollatingSequence::Ascii => left.as_bytes().cmp(right.as_bytes()),
            CollatingSequence::Ebcdic => left
                .bytes()
                .map(ascii_to_ebcdic_order)
                .cmp(right.bytes().map(ascii_to_ebcdic_order)),
        }
    }

    fn field(&self, name: &str) -> Result<&VmField, VmError> {
        self.fields
            .iter()
            .find(|field| {
                field.name.eq_ignore_ascii_case(name)
                    || field.name.replace('.', "_").eq_ignore_ascii_case(name)
            })
            .ok_or_else(|| VmError::UnknownReference {
                name: name.to_string(),
            })
    }

    fn condition(&self, name: &str) -> Result<&VmConditionName, VmError> {
        let matches = self.condition_candidates(name);
        match matches.as_slice() {
            [condition] => Ok(*condition),
            [] => Err(VmError::UnknownConditionName {
                name: name.to_string(),
            }),
            many => Err(VmError::AmbiguousConditionName {
                name: name.to_string(),
                candidates: many
                    .iter()
                    .map(|condition| format!("{}.{}", condition.parent, condition.name))
                    .collect::<Vec<_>>()
                    .join(", "),
            }),
        }
    }

    fn condition_declared_view(&self, condition: &VmConditionName) -> Option<&VmDeclaredView> {
        self.condition_views
            .get(&normalize_vm_key(&condition.name))
            .or_else(|| {
                self.condition_views.get(&normalize_vm_key(&format!(
                    "{}.{}",
                    condition.parent, condition.name
                )))
            })
    }

    fn condition_candidates(&self, name: &str) -> Vec<&VmConditionName> {
        let normalized = normalize_vm_key(name);
        self.conditions
            .iter()
            .filter(|condition| {
                condition.name.eq_ignore_ascii_case(name)
                    || format!("{}.{}", condition.parent, condition.name).eq_ignore_ascii_case(name)
                    || format!(
                        "{}.{}",
                        normalize_vm_key(&condition.parent),
                        normalize_vm_key(&condition.name)
                    )
                    .eq_ignore_ascii_case(&normalized)
            })
            .collect()
    }

    fn field_bytes<'a>(&self, bytes: &'a [u8], field: &VmField) -> Result<&'a [u8], VmError> {
        let end = field.offset.saturating_add(field.byte_len);
        bytes
            .get(field.offset..end)
            .ok_or_else(|| VmError::FieldOutOfBounds {
                name: field.name.clone(),
                offset: field.offset,
                end,
                len: bytes.len(),
            })
    }

    fn field_bytes_mut<'a>(
        &self,
        bytes: &'a mut [u8],
        field: &VmField,
    ) -> Result<&'a mut [u8], VmError> {
        let end = field.offset.saturating_add(field.byte_len);
        let len = bytes.len();
        bytes
            .get_mut(field.offset..end)
            .ok_or_else(|| VmError::FieldOutOfBounds {
                name: field.name.clone(),
                offset: field.offset,
                end,
                len,
            })
    }
}

fn parse_decimal(value: &str) -> Result<Decimal, VmError> {
    Decimal::from_str(value.trim()).map_err(|_| VmError::InvalidDecimal {
        value: value.to_string(),
    })
}

fn normalize_vm_key(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .replace('-', "_")
        .to_ascii_uppercase()
}

fn scoped_runtime_name(program: &str, name: &str) -> String {
    format!("{}.{}", normalize_vm_key(program), normalize_vm_key(name))
}

fn map_key_case_insensitive<T>(map: &BTreeMap<String, T>, key: &str) -> Option<String> {
    map.get_key_value(key)
        .map(|(key, _)| key.clone())
        .or_else(|| {
            map.keys()
                .find(|candidate| candidate.eq_ignore_ascii_case(key))
                .cloned()
        })
}

fn binding_map_descriptor_for<'a>(
    bindings: &'a BTreeMap<String, VmBinding>,
    target: &str,
) -> Option<&'a VmBinding> {
    bindings.get(target).or_else(|| {
        bindings
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(target))
            .map(|(_, descriptor)| descriptor)
    })
}

fn bind_call_aliases<'a>(
    bindings: &mut BTreeMap<String, VmBinding>,
    aliases: impl IntoIterator<Item = &'a str>,
    binding: VmBinding,
) {
    for alias in aliases {
        bindings.insert(alias.to_string(), binding.clone());
        bindings.insert(normalize_vm_key(alias), binding.clone());
    }
}

fn operand_target_name(operand: &VmOperand) -> Option<String> {
    match operand {
        VmExpr::Access(path) => Some(path.target.clone()),
        VmExpr::Identifier(name) => Some(name.clone()),
        _ => None,
    }
}

fn to_decimal(value: &VmEvaluatedValue) -> Result<Decimal, VmError> {
    match &value.value {
        VmValue::Decimal(value) => Ok(*value),
        VmValue::Integer(value) => Ok(Decimal::from(*value)),
        VmValue::UnsignedInteger(value) => {
            Decimal::from_str(&value.to_string()).map_err(|_| VmError::InvalidDecimal {
                value: value.to_string(),
            })
        }
        VmValue::Float(value) => {
            if !value.is_finite() {
                return Err(VmError::InvalidDecimal {
                    value: value.to_string(),
                });
            }
            Decimal::from_str(&value.to_string()).map_err(|_| VmError::InvalidDecimal {
                value: value.to_string(),
            })
        }
        VmValue::Text(value) => parse_decimal(value),
        VmValue::NationalText(value) => parse_decimal(value),
        other => Err(VmError::UnsupportedOperand {
            message: format!("cannot treat {other:?} as decimal"),
        }),
    }
}

fn to_f64(value: &VmEvaluatedValue) -> Result<f64, VmError> {
    match &value.value {
        VmValue::Float(value) => {
            if value.is_finite() {
                Ok(*value)
            } else {
                Err(VmError::InvalidDecimal {
                    value: value.to_string(),
                })
            }
        }
        VmValue::Decimal(value) => decimal_to_f64(*value),
        VmValue::Integer(value) => Ok(*value as f64),
        VmValue::UnsignedInteger(value) => Ok(*value as f64),
        VmValue::Text(value) | VmValue::NationalText(value) => value
            .trim()
            .parse::<f64>()
            .map_err(|_| VmError::InvalidDecimal {
                value: value.clone(),
            })
            .and_then(|value| {
                if value.is_finite() {
                    Ok(value)
                } else {
                    Err(VmError::InvalidDecimal {
                        value: value.to_string(),
                    })
                }
            }),
        other => Err(VmError::UnsupportedOperand {
            message: format!("cannot treat {other:?} as float"),
        }),
    }
}

fn decimal_to_f64(value: Decimal) -> Result<f64, VmError> {
    let text = value.to_string();
    let parsed = text.parse::<f64>().map_err(|_| VmError::InvalidDecimal {
        value: text.clone(),
    })?;
    if parsed.is_finite() {
        Ok(parsed)
    } else {
        Err(VmError::InvalidDecimal { value: text })
    }
}

fn decimal_to_i128(value: Decimal) -> Result<i128, VmError> {
    let truncated = value.trunc();
    if truncated != value {
        return Err(VmError::InvalidDecimal {
            value: value.to_string(),
        });
    }
    i128::from_str(&truncated.to_string()).map_err(|_| VmError::InvalidDecimal {
        value: truncated.to_string(),
    })
}

fn bool_value(value: &VmEvaluatedValue) -> Result<bool, VmError> {
    match value.value {
        VmValue::Bool(value) => Ok(value),
        _ => Err(VmError::UnsupportedOperand {
            message: "value is not boolean".to_string(),
        }),
    }
}

fn display_value(value: &VmEvaluatedValue) -> String {
    value_text(value).unwrap_or_else(|| match &value.value {
        VmValue::Decimal(value) => display_decimal_value(*value),
        VmValue::Integer(value) => value.to_string(),
        VmValue::UnsignedInteger(value) => value.to_string(),
        VmValue::Float(value) => value.to_string(),
        VmValue::Bool(value) => value.to_string(),
        VmValue::Null => String::new(),
        VmValue::Text(_) | VmValue::NationalText(_) | VmValue::DbcsText(_) | VmValue::Bytes(_) => {
            String::new()
        }
    })
}

fn display_decimal_value(value: Decimal) -> String {
    let text = value.to_string();
    if value.is_zero() && value.is_sign_negative() && !text.starts_with('-') {
        format!("-{text}")
    } else {
        text
    }
}

fn figurative_display_byte(value: VmFigurative) -> u8 {
    match value {
        VmFigurative::Zero => b'0',
        VmFigurative::Space => b' ',
        VmFigurative::HighValue => 0xFF,
        VmFigurative::LowValue => 0x00,
        VmFigurative::Quote => b'"',
    }
}

fn access_paths_match(left: &VmAccessPath, right: &VmAccessPath) -> bool {
    normalize_vm_key(&left.target) == normalize_vm_key(&right.target)
        && left.subscripts == right.subscripts
        && left.reference_modifier == right.reference_modifier
}

fn normalize_record_bytes(bytes: &mut Vec<u8>, len: usize) {
    if bytes.len() > len {
        bytes.truncate(len);
    } else if bytes.len() < len {
        bytes.resize(len, b' ');
    }
}

fn count_non_overlapping_bytes(bytes: &[u8], pattern: &[u8]) -> Result<usize, VmError> {
    if pattern.is_empty() {
        return Err(VmError::ProcedureRuntime {
            block: String::new(),
            message: "INSPECT/EXAMINE pattern must not be empty".to_string(),
        });
    }
    let mut count = 0usize;
    let mut cursor = 0usize;
    while cursor.saturating_add(pattern.len()) <= bytes.len() {
        if &bytes[cursor..cursor + pattern.len()] == pattern {
            count += 1;
            cursor += pattern.len();
        } else {
            cursor += 1;
        }
    }
    Ok(count)
}

fn replace_all_bytes(bytes: &[u8], pattern: &[u8], replacement: &[u8]) -> Result<Vec<u8>, VmError> {
    if pattern.is_empty() {
        return Err(VmError::ProcedureRuntime {
            block: String::new(),
            message: "INSPECT/EXAMINE replacement pattern must not be empty".to_string(),
        });
    }
    let mut out = Vec::with_capacity(bytes.len());
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if cursor.saturating_add(pattern.len()) <= bytes.len()
            && &bytes[cursor..cursor + pattern.len()] == pattern
        {
            out.extend_from_slice(replacement);
            cursor += pattern.len();
        } else {
            out.push(bytes[cursor]);
            cursor += 1;
        }
    }
    Ok(out)
}

fn convert_bytes(bytes: &[u8], from: &[u8], to: &[u8]) -> Result<Vec<u8>, VmError> {
    if from.is_empty() || from.len() != to.len() {
        return Err(VmError::ProcedureRuntime {
            block: String::new(),
            message: "INSPECT CONVERTING requires non-empty FROM and TO literals with equal length"
                .to_string(),
        });
    }
    let mut out = bytes.to_vec();
    for byte in &mut out {
        if let Some(idx) = from.iter().position(|candidate| candidate == byte) {
            *byte = to[idx];
        }
    }
    Ok(out)
}

fn find_bytes(bytes: &[u8], pattern: &[u8]) -> Option<usize> {
    if pattern.is_empty() || pattern.len() > bytes.len() {
        return None;
    }
    bytes
        .windows(pattern.len())
        .position(|window| window == pattern)
}

fn split_bytes_from(bytes: &[u8], delimiter: &[u8], all: bool, start: usize) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut cursor = start.min(bytes.len());
    while cursor <= bytes.len() {
        if let Some(relative) = find_bytes(&bytes[cursor..], delimiter) {
            let end = cursor + relative;
            out.push(bytes[cursor..end].to_vec());
            cursor = end + delimiter.len();
            if all {
                while cursor < bytes.len()
                    && cursor.saturating_add(delimiter.len()) <= bytes.len()
                    && &bytes[cursor..cursor + delimiter.len()] == delimiter
                {
                    cursor += delimiter.len();
                }
            }
        } else {
            out.push(bytes[cursor..].to_vec());
            break;
        }
    }
    out
}

fn unstring_next_cursor(
    bytes: &[u8],
    delimiter: &[u8],
    all: bool,
    start: usize,
    assigned: usize,
) -> usize {
    let mut cursor = start.min(bytes.len());
    for _ in 0..assigned {
        if let Some(relative) = find_bytes(&bytes[cursor..], delimiter) {
            cursor += relative + delimiter.len();
            if all {
                while cursor < bytes.len()
                    && cursor.saturating_add(delimiter.len()) <= bytes.len()
                    && &bytes[cursor..cursor + delimiter.len()] == delimiter
                {
                    cursor += delimiter.len();
                }
            }
        } else {
            return bytes.len();
        }
    }
    cursor.min(bytes.len())
}

fn sort_records_by_key(
    records: &mut Vec<Vec<u8>>,
    key: &VmSortKeyDescriptor,
) -> Result<(), VmError> {
    let mut keyed = Vec::with_capacity(records.len());
    for record in records.iter() {
        let sort_key = sort_key_value(record, key)?;
        keyed.push((sort_key, record.clone()));
    }
    keyed.sort_by(|left, right| compare_sort_key_value(&left.0, &right.0, key.direction));
    *records = keyed.into_iter().map(|(_, record)| record).collect();
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum VmSortKeyValue {
    Bytes(Vec<u8>),
    Decimal(Decimal),
}

fn sort_key_value(record: &[u8], key: &VmSortKeyDescriptor) -> Result<VmSortKeyValue, VmError> {
    let key_bytes = sort_key_slice(record, key)?;
    match &key.encoding {
        VmSortKeyEncoding::NumericDisplay { .. } => Ok(VmSortKeyValue::Decimal(
            decimal_from_display_bytes(key_bytes)?,
        )),
        VmSortKeyEncoding::PackedDecimal {
            digits,
            scale,
            signed,
        } => {
            let value =
                decode_packed_decimal(key_bytes, *digits, *scale, *signed).map_err(|err| {
                    VmError::Codec {
                        name: "SORT-KEY".to_string(),
                        message: err.to_string(),
                    }
                })?;
            Ok(VmSortKeyValue::Decimal(value))
        }
        VmSortKeyEncoding::Bytes => Ok(VmSortKeyValue::Bytes(key_bytes.to_vec())),
    }
}

fn compare_sort_key_value(
    left: &VmSortKeyValue,
    right: &VmSortKeyValue,
    direction: VmSortDirection,
) -> Ordering {
    let ordering = match (left, right) {
        (VmSortKeyValue::Decimal(left), VmSortKeyValue::Decimal(right)) => left.cmp(right),
        (VmSortKeyValue::Bytes(left), VmSortKeyValue::Bytes(right)) => left.cmp(right),
        (VmSortKeyValue::Decimal(_), VmSortKeyValue::Bytes(_)) => Ordering::Less,
        (VmSortKeyValue::Bytes(_), VmSortKeyValue::Decimal(_)) => Ordering::Greater,
    };
    match direction {
        VmSortDirection::Ascending => ordering,
        VmSortDirection::Descending => ordering.reverse(),
    }
}

fn sort_key_slice<'a>(record: &'a [u8], key: &VmSortKeyDescriptor) -> Result<&'a [u8], VmError> {
    let end = key
        .offset
        .checked_add(key.byte_len)
        .ok_or_else(|| VmError::ProcedureRuntime {
            block: String::new(),
            message: "SORT key range overflows usize".to_string(),
        })?;
    if end > record.len() {
        return Err(VmError::ProcedureRuntime {
            block: String::new(),
            message: format!(
                "SORT key range {}..{} exceeds record length {}",
                key.offset,
                end,
                record.len()
            ),
        });
    }
    Ok(&record[key.offset..end])
}

fn decimal_from_display_bytes(bytes: &[u8]) -> Result<Decimal, VmError> {
    let text = String::from_utf8_lossy(bytes).trim().to_string();
    if text.is_empty() {
        Ok(Decimal::ZERO)
    } else {
        Decimal::from_str(&text).map_err(|err| VmError::Codec {
            name: "SORT-KEY".to_string(),
            message: format!("invalid numeric display sort key {text:?}: {err}"),
        })
    }
}

fn value_text(value: &VmEvaluatedValue) -> Option<String> {
    match &value.value {
        VmValue::Text(value) => Some(value.clone()),
        VmValue::NationalText(value) => Some(value.clone()),
        VmValue::DbcsText(bytes) => Some(String::from_utf8_lossy(bytes).to_string()),
        VmValue::Bytes(bytes) => Some(String::from_utf8_lossy(bytes).to_string()),
        _ => None,
    }
}

fn text_for_compare(value: &VmEvaluatedValue, other_len: usize) -> Result<String, VmError> {
    let mut text = match &value.value {
        VmValue::Text(value) => value.clone(),
        VmValue::NationalText(value) => value.clone(),
        VmValue::DbcsText(bytes) => String::from_utf8_lossy(bytes).to_string(),
        VmValue::Bytes(bytes) => String::from_utf8_lossy(bytes).to_string(),
        other => {
            return Err(VmError::UnsupportedOperand {
                message: format!("cannot compare {other:?} as text"),
            })
        }
    };
    let target_len = value.byte_len.max(other_len);
    while text.len() < target_len {
        text.push(' ');
    }
    Ok(text)
}

fn is_numeric(category: VmCategory) -> bool {
    matches!(
        category,
        VmCategory::NumericDisplay
            | VmCategory::PackedDecimal
            | VmCategory::Binary
            | VmCategory::NativeBinary
            | VmCategory::Float
    )
}

fn is_nonnumeric(category: VmCategory) -> bool {
    matches!(
        category,
        VmCategory::Group
            | VmCategory::Alphanumeric
            | VmCategory::Alphabetic
            | VmCategory::National
            | VmCategory::Dbcs
            | VmCategory::NumericEdited
    )
}

struct DisplayNumericParts {
    sign: Option<char>,
    is_zero: bool,
}

fn display_numeric_parts(bytes: &[u8]) -> Option<DisplayNumericParts> {
    let text = String::from_utf8_lossy(bytes);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let sign_count = trimmed.chars().filter(|ch| matches!(ch, '+' | '-')).count();
    if sign_count > 1 {
        return None;
    }
    let last_idx = trimmed.chars().count().saturating_sub(1);
    let mut sign = None;
    let mut saw_digit = false;
    let mut is_zero = true;
    for (idx, ch) in trimmed.chars().enumerate() {
        if ch.is_ascii_digit() {
            saw_digit = true;
            if ch != '0' {
                is_zero = false;
            }
        } else if matches!(ch, '+' | '-') && (idx == 0 || idx == last_idx) {
            sign = Some(ch);
        } else {
            return None;
        }
    }
    if !saw_digit {
        return None;
    }
    Some(DisplayNumericParts { sign, is_zero })
}

fn display_bytes_numeric(bytes: &[u8]) -> bool {
    display_numeric_parts(bytes).is_some()
}

fn alphabetic_text(text: &str, class: VmClassTest) -> bool {
    text.chars().all(|ch| match class {
        VmClassTest::Alphabetic => ch == ' ' || ch.is_ascii_alphabetic(),
        VmClassTest::AlphabeticUpper => ch == ' ' || ch.is_ascii_uppercase(),
        VmClassTest::AlphabeticLower => ch == ' ' || ch.is_ascii_lowercase(),
        VmClassTest::Numeric => false,
    })
}

fn render_numeric_display(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let trimmed = value.trim();
    let (sign, digits) = match trimmed.as_bytes().first().copied() {
        Some(b'+' | b'-') => (&trimmed[..1], &trimmed[1..]),
        _ => ("", trimmed),
    };
    let digit_width = width.saturating_sub(sign.len());
    let mut out = if digits.len() > digit_width {
        digits[digits.len() - digit_width..].to_string()
    } else {
        digits.to_string()
    };
    while out.len() < digit_width {
        out.insert(0, '0');
    }
    format!("{sign}{out}")
}

fn render_numeric_display_with_picture(
    value: &str,
    width: usize,
    picture: Option<&VmPicture>,
) -> Result<String, VmError> {
    let Some(picture) = picture else {
        return Ok(render_numeric_display(value, width));
    };
    if picture.scale == 0 {
        return Ok(render_numeric_display(value, width));
    }

    let trimmed = value.trim();
    let decimal = parse_decimal(trimmed)?;
    let factor = decimal_scale_factor(picture.scale)?;
    let scaled = decimal
        .checked_mul(factor)
        .ok_or_else(|| VmError::InvalidDecimal {
            value: value.to_string(),
        })?;
    let mantissa = decimal_to_i128(scaled)?;
    let sign = if mantissa < 0 {
        "-"
    } else if trimmed.starts_with('+') {
        "+"
    } else {
        ""
    };
    let unsigned = if mantissa < 0 {
        mantissa
            .checked_neg()
            .ok_or_else(|| VmError::InvalidDecimal {
                value: value.to_string(),
            })?
    } else {
        mantissa
    };
    let digit_width = width.saturating_sub(sign.len());
    let digits = unsigned.to_string();
    let mut out = if digits.len() > digit_width {
        digits[digits.len() - digit_width..].to_string()
    } else {
        digits
    };
    while out.len() < digit_width {
        out.insert(0, '0');
    }
    Ok(format!("{sign}{out}"))
}

fn decimal_scale_factor(scale: u32) -> Result<Decimal, VmError> {
    let mut factor = Decimal::from(1);
    for _ in 0..scale {
        factor = factor
            .checked_mul(Decimal::from(10))
            .ok_or_else(|| VmError::InvalidDecimal {
                value: format!("scale factor 10^{scale}"),
            })?;
    }
    Ok(factor)
}

fn decimal_fits_picture(value: Decimal, picture: &VmPicture) -> bool {
    let normalized = value.normalize().to_string();
    if normalized.starts_with('-') && !picture.signed {
        return false;
    }
    let unsigned = normalized.trim_start_matches(['+', '-']);
    let (integer, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let integer_digits = integer.trim_start_matches('0').len();
    let fraction_digits = fraction.trim_end_matches('0').len();
    let integer_capacity = picture.digits.saturating_sub(picture.scale as usize);
    integer_digits <= integer_capacity && fraction_digits <= picture.scale as usize
}

fn decimal_fits_display_width(value: Decimal, width: usize) -> bool {
    let normalized = value.normalize().to_string();
    let unsigned = normalized.trim_start_matches(['+', '-']);
    let (integer, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    fraction.trim_end_matches('0').is_empty() && integer.trim_start_matches('0').len() <= width
}

fn ascii_to_ebcdic_order(byte: u8) -> u16 {
    match byte {
        b' ' => 0x40,
        b'0'..=b'9' => 0xF0 + u16::from(byte - b'0'),
        b'A'..=b'I' => 0xC1 + u16::from(byte - b'A'),
        b'J'..=b'R' => 0xD1 + u16::from(byte - b'J'),
        b'S'..=b'Z' => 0xE2 + u16::from(byte - b'S'),
        b'a'..=b'i' => 0x81 + u16::from(byte - b'a'),
        b'j'..=b'r' => 0x91 + u16::from(byte - b'j'),
        b's'..=b'z' => 0xA2 + u16::from(byte - b's'),
        _ => u16::from(byte),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_file_path(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "cobol_vm_{label}_{}_{}.dat",
            std::process::id(),
            nanos
        ))
    }

    fn sample_program() -> VmProgram {
        VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![
                VmField {
                    name: "A".to_string(),
                    offset: 0,
                    byte_len: 3,
                    category: VmCategory::NumericDisplay,
                    usage: VmUsage::Display,
                    picture: Some(VmPicture {
                        signed: false,
                        digits: 3,
                        scale: 0,
                        char_len: 3,
                    }),
                },
                VmField {
                    name: "B".to_string(),
                    offset: 3,
                    byte_len: 3,
                    category: VmCategory::Alphanumeric,
                    usage: VmUsage::Display,
                    picture: None,
                },
            ],
            vec![VmConditionName {
                name: "A_HIGH".to_string(),
                parent: "A".to_string(),
                values: vec![VmConditionValue::Range {
                    start: "100".to_string(),
                    end: "999".to_string(),
                }],
            }],
        )
    }

    fn runtime_with_scalar(program: VmProgram, target: &str, bytes: &[u8]) -> VmRuntime {
        let mut pool = StoragePool::default();
        let key = StorageKey::scalar("MAIN", target);
        pool.define_cell(key.clone(), bytes.to_vec()).unwrap();
        let mut runtime = VmRuntime::new(program, pool);
        runtime.bind_storage_cell(target, key);
        runtime
    }

    fn runtime_with_empty_pool(program: VmProgram) -> VmRuntime {
        VmRuntime::new(program, StoragePool::default())
    }

    #[test]
    fn computed_go_to_selected_target_honors_alter_table() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![VmField {
                name: "SEL".to_string(),
                offset: 0,
                byte_len: 1,
                category: VmCategory::NumericDisplay,
                usage: VmUsage::Display,
                picture: Some(VmPicture {
                    signed: false,
                    digits: 1,
                    scale: 0,
                    char_len: 1,
                }),
            }],
            Vec::new(),
        );
        let selector = VmAccessPath {
            target: "SEL".to_string(),
            condition_name: None,
            subscripts: Vec::new(),
            reference_modifier: None,
            result_len: None,
        };
        let mut runtime = runtime_with_scalar(program, "SEL", b"2");
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![
                VmBasicBlock {
                    name: "MAIN".to_string(),
                    ops: vec![
                        VmProcedureOp::Alter {
                            paragraph: "PATH_B".to_string(),
                            target: "PATH_C".to_string(),
                        },
                        VmProcedureOp::ComputedGoTo {
                            targets: vec!["PATH_A".to_string(), "PATH_B".to_string()],
                            depending_on: VmExpr::Access(selector),
                        },
                        VmProcedureOp::Display(vec![VmExpr::Literal("FALLTHROUGH".to_string())]),
                    ],
                    transfer: VmControlTransfer::StopRun,
                },
                VmBasicBlock {
                    name: "PATH_A".to_string(),
                    ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                        "A".to_string(),
                    )])],
                    transfer: VmControlTransfer::StopRun,
                },
                VmBasicBlock {
                    name: "PATH_B".to_string(),
                    ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                        "B".to_string(),
                    )])],
                    transfer: VmControlTransfer::StopRun,
                },
                VmBasicBlock {
                    name: "PATH_C".to_string(),
                    ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                        "C".to_string(),
                    )])],
                    transfer: VmControlTransfer::StopRun,
                },
            ],
        };

        runtime.execute_procedure(&procedure).unwrap();

        assert_eq!(runtime.display, vec!["C"]);
    }

    fn sort_test_access_path(name: &str, len: usize) -> VmAccessPath {
        VmAccessPath {
            target: name.to_string(),
            condition_name: None,
            subscripts: Vec::new(),
            reference_modifier: None,
            result_len: Some(len),
        }
    }

    fn packed_access_path(name: &str) -> VmAccessPath {
        VmAccessPath {
            target: name.to_string(),
            condition_name: None,
            subscripts: Vec::new(),
            reference_modifier: None,
            result_len: Some(2),
        }
    }

    fn binary_access_path(name: &str) -> VmAccessPath {
        VmAccessPath {
            target: name.to_string(),
            condition_name: None,
            subscripts: Vec::new(),
            reference_modifier: None,
            result_len: Some(2),
        }
    }

    fn float_access_path(name: &str, len: usize) -> VmAccessPath {
        VmAccessPath {
            target: name.to_string(),
            condition_name: None,
            subscripts: Vec::new(),
            reference_modifier: None,
            result_len: Some(len),
        }
    }

    fn packed_field_program() -> VmProgram {
        VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![VmField {
                name: "PACKED".to_string(),
                offset: 0,
                byte_len: 2,
                category: VmCategory::PackedDecimal,
                usage: VmUsage::PackedDecimal,
                picture: Some(VmPicture {
                    signed: true,
                    digits: 3,
                    scale: 0,
                    char_len: 3,
                }),
            }],
            Vec::new(),
        )
    }

    fn binary_field_program() -> VmProgram {
        VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![VmField {
                name: "BINARY".to_string(),
                offset: 0,
                byte_len: 2,
                category: VmCategory::Binary,
                usage: VmUsage::Binary,
                picture: Some(VmPicture {
                    signed: true,
                    digits: 4,
                    scale: 0,
                    char_len: 4,
                }),
            }],
            Vec::new(),
        )
    }

    fn float_field_program(dialect: DialectProfile, usage: VmUsage, byte_len: usize) -> VmProgram {
        VmProgram::new(
            dialect,
            vec![VmField {
                name: "FLOAT".to_string(),
                offset: 0,
                byte_len,
                category: VmCategory::Float,
                usage,
                picture: None,
            }],
            Vec::new(),
        )
    }

    fn packed_runtime(initial: &[u8]) -> VmRuntime {
        runtime_with_scalar(packed_field_program(), "PACKED", initial)
    }

    fn binary_runtime(initial: &[u8]) -> VmRuntime {
        runtime_with_scalar(binary_field_program(), "BINARY", initial)
    }

    fn float_runtime(dialect: DialectProfile, usage: VmUsage, initial: &[u8]) -> VmRuntime {
        runtime_with_scalar(
            float_field_program(dialect, usage, initial.len()),
            "FLOAT",
            initial,
        )
    }

    #[test]
    fn release_without_active_sort_returns_procedure_runtime_error() {
        let program = VmProgram::new(DialectProfile::ibm_zos(), Vec::new(), Vec::new());
        let mut runtime = VmRuntime::new(program, StoragePool::default());
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![VmBasicBlock {
                name: "MAIN".to_string(),
                ops: vec![VmProcedureOp::ReleaseSortRecord {
                    record: sort_test_access_path("REC", 2),
                    source: None,
                }],
                transfer: VmControlTransfer::StopRun,
            }],
        };

        let error = runtime
            .execute_procedure(&procedure)
            .expect_err("RELEASE outside SORT must fail");
        assert!(matches!(
            error,
            VmError::ProcedureRuntime { message, .. } if message.contains("RELEASE")
        ));
    }

    #[test]
    fn return_without_active_sort_returns_procedure_runtime_error() {
        let program = VmProgram::new(DialectProfile::ibm_zos(), Vec::new(), Vec::new());
        let mut runtime = VmRuntime::new(program, StoragePool::default());
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![VmBasicBlock {
                name: "MAIN".to_string(),
                ops: vec![VmProcedureOp::ReturnSortRecord {
                    file: "SORT_FILE".to_string(),
                    record: sort_test_access_path("REC", 2),
                    target: None,
                    at_end_ops: Vec::new(),
                    not_at_end_ops: Vec::new(),
                }],
                transfer: VmControlTransfer::StopRun,
            }],
        };

        let error = runtime
            .execute_procedure(&procedure)
            .expect_err("RETURN outside SORT must fail");
        assert!(matches!(
            error,
            VmError::ProcedureRuntime { message, .. } if message.contains("RETURN")
        ));
    }

    #[test]
    fn numeric_display_sort_keys_compare_numerically() {
        let key = VmSortKeyDescriptor {
            offset: 0,
            byte_len: 5,
            direction: VmSortDirection::Ascending,
            encoding: VmSortKeyEncoding::NumericDisplay {
                digits: 5,
                scale: 0,
                signed: false,
            },
        };
        let mut records = vec![b"10000".to_vec(), b" 2000".to_vec()];

        sort_records_by_key(&mut records, &key).unwrap();

        assert_eq!(records, vec![b" 2000".to_vec(), b"10000".to_vec()]);
    }

    #[test]
    fn packed_decimal_sort_keys_compare_numerically_not_bytewise() {
        let key = VmSortKeyDescriptor {
            offset: 0,
            byte_len: 2,
            direction: VmSortDirection::Ascending,
            encoding: VmSortKeyEncoding::PackedDecimal {
                digits: 3,
                scale: 0,
                signed: true,
            },
        };
        let mut records = vec![
            vec![0x00, 0x1c, b'P'],
            vec![0x00, 0x1d, b'N'],
            vec![0x00, 0x2c, b'T'],
        ];

        sort_records_by_key(&mut records, &key).unwrap();

        assert_eq!(
            records,
            vec![
                vec![0x00, 0x1d, b'N'],
                vec![0x00, 0x1c, b'P'],
                vec![0x00, 0x2c, b'T'],
            ]
        );
    }

    #[test]
    fn sort_key_decode_error_does_not_mutate_records() {
        let key = VmSortKeyDescriptor {
            offset: 0,
            byte_len: 2,
            direction: VmSortDirection::Ascending,
            encoding: VmSortKeyEncoding::PackedDecimal {
                digits: 3,
                scale: 0,
                signed: true,
            },
        };
        let mut records = vec![
            vec![0x00, 0x1c, b'P'],
            vec![0x00, 0x1a, b'X'],
            vec![0x00, 0x2c, b'T'],
        ];
        let original = records.clone();

        let error =
            sort_records_by_key(&mut records, &key).expect_err("invalid packed sort key must fail");

        assert!(matches!(error, VmError::Codec { .. }), "{error:?}");
        assert_eq!(records, original);
    }

    #[test]
    fn sort_key_range_past_record_end_is_runtime_error() {
        let key = VmSortKeyDescriptor {
            offset: 2,
            byte_len: 2,
            direction: VmSortDirection::Ascending,
            encoding: VmSortKeyEncoding::Bytes,
        };
        let mut records = vec![b"ABC".to_vec()];

        let error =
            sort_records_by_key(&mut records, &key).expect_err("out-of-range sort key must fail");

        assert!(
            matches!(error, VmError::ProcedureRuntime { ref message, .. } if message.contains("SORT key range")),
            "{error:?}"
        );
        assert_eq!(records, vec![b"ABC".to_vec()]);
    }

    fn bind_occurs_runtime(program: VmProgram, target: &str, occurrences: &[&[u8]]) -> VmRuntime {
        let mut pool = StoragePool::default();
        for (idx, bytes) in occurrences.iter().enumerate() {
            pool.define_cell(
                StorageKey::occurrence("MAIN", target, vec![idx + 1]),
                (*bytes).to_vec(),
            )
            .unwrap();
        }
        let mut runtime = VmRuntime::new(program, pool);
        runtime.bind_occurs_storage_cell(target, "MAIN", target);
        runtime
    }

    #[test]
    fn evaluates_numeric_relation_and_condition_name_range() {
        let program = sample_program();
        let bytes = b"123ABC";
        assert!(program
            .eval_condition(
                bytes,
                &VmCondition::Relation {
                    left: VmOperand::Identifier("A".to_string()),
                    op: VmRelOp::Greater,
                    right: VmOperand::Number("100".to_string()),
                },
            )
            .unwrap());
        assert!(program
            .eval_condition(
                bytes,
                &VmCondition::ConditionName {
                    reference: "A_HIGH".to_string(),
                },
            )
            .unwrap());
    }

    #[test]
    fn evaluates_class_sign_and_evaluate_pattern() {
        let program = sample_program();
        let bytes = b"000ABC";
        assert!(program
            .eval_condition(
                bytes,
                &VmCondition::SignTest {
                    operand: VmOperand::Identifier("A".to_string()),
                    sign: VmSignTest::Zero,
                    negated: false,
                },
            )
            .unwrap());
        assert!(program
            .eval_condition(
                bytes,
                &VmCondition::ClassTest {
                    operand: VmOperand::Identifier("B".to_string()),
                    class: VmClassTest::AlphabeticUpper,
                    negated: false,
                },
            )
            .unwrap());
        let subject = program
            .eval_operand(bytes, &VmOperand::Identifier("B".to_string()))
            .unwrap();
        assert!(program
            .match_evaluate_pattern(
                bytes,
                &subject,
                &VmEvaluatePattern::Operand(VmOperand::Literal("ABC".to_string())),
            )
            .unwrap());
    }

    #[test]
    fn display_numeric_class_honors_numproc_pfd_preferred_zero_sign() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![VmField {
                name: "N".to_string(),
                offset: 0,
                byte_len: 2,
                category: VmCategory::NumericDisplay,
                usage: VmUsage::Display,
                picture: Some(VmPicture {
                    signed: true,
                    digits: 1,
                    scale: 0,
                    char_len: 1,
                }),
            }],
            Vec::new(),
        );
        let condition = VmCondition::ClassTest {
            operand: VmOperand::Identifier("N".to_string()),
            class: VmClassTest::Numeric,
            negated: false,
        };

        assert!(!program.eval_condition(b"0-", &condition).unwrap());
        assert!(program.eval_condition(b"0+", &condition).unwrap());
        assert!(program.eval_condition(b"00", &condition).unwrap());
    }

    #[test]
    fn display_numeric_class_nopfd_accepts_negative_zero() {
        let mut dialect = DialectProfile::ibm_zos();
        dialect.numproc = Numproc::Nopfd;
        let program = VmProgram::new(
            dialect,
            vec![VmField {
                name: "N".to_string(),
                offset: 0,
                byte_len: 2,
                category: VmCategory::NumericDisplay,
                usage: VmUsage::Display,
                picture: Some(VmPicture {
                    signed: true,
                    digits: 1,
                    scale: 0,
                    char_len: 1,
                }),
            }],
            Vec::new(),
        );

        assert!(program
            .eval_condition(
                b"0-",
                &VmCondition::ClassTest {
                    operand: VmOperand::Identifier("N".to_string()),
                    class: VmClassTest::Numeric,
                    negated: false,
                }
            )
            .unwrap());
    }

    #[test]
    fn display_numeric_class_rejects_sign_for_unsigned_field() {
        let program = sample_program();
        let condition = VmCondition::ClassTest {
            operand: VmOperand::Identifier("A".to_string()),
            class: VmClassTest::Numeric,
            negated: false,
        };

        assert!(!program.eval_condition(b"01+ABC", &condition).unwrap());
        assert!(program.eval_condition(b"001ABC", &condition).unwrap());
    }

    #[test]
    fn packed_numeric_class_honors_numproc_pfd_preferred_signs() {
        let program = packed_field_program();
        let condition = VmCondition::ClassTest {
            operand: VmOperand::Identifier("PACKED".to_string()),
            class: VmClassTest::Numeric,
            negated: false,
        };

        assert!(!program.eval_condition(&[0x00, 0x0d], &condition).unwrap());
        assert!(!program.eval_condition(&[0x12, 0x3f], &condition).unwrap());
        assert!(program.eval_condition(&[0x00, 0x0f], &condition).unwrap());
        assert!(program.eval_condition(&[0x12, 0x3c], &condition).unwrap());
        assert!(program.eval_condition(&[0x12, 0x3d], &condition).unwrap());
    }

    #[test]
    fn packed_numeric_class_nopfd_accepts_decodable_signs() {
        let mut dialect = DialectProfile::ibm_zos();
        dialect.numproc = Numproc::Nopfd;
        let mut program = packed_field_program();
        program.dialect = dialect;
        let condition = VmCondition::ClassTest {
            operand: VmOperand::Identifier("PACKED".to_string()),
            class: VmClassTest::Numeric,
            negated: false,
        };

        assert!(program.eval_condition(&[0x00, 0x0d], &condition).unwrap());
        assert!(program.eval_condition(&[0x12, 0x3f], &condition).unwrap());
    }

    #[test]
    fn set_condition_name_writes_first_truth_value() {
        let program = sample_program();
        let mut bytes = *b"000ABC";
        program.set_condition_name(&mut bytes, "A_HIGH").unwrap();
        assert_eq!(&bytes[..3], b"100");
    }

    #[test]
    fn access_path_supports_reference_modification_and_functions() {
        let program = sample_program();
        let bytes = b"123ABC";
        let path = VmAccessPath {
            target: "B".to_string(),
            condition_name: None,
            subscripts: Vec::new(),
            reference_modifier: Some(VmReferenceModifier {
                start: Box::new(VmExpr::Number("2".to_string())),
                length: Some(Box::new(VmExpr::Number("2".to_string()))),
            }),
            result_len: None,
        };
        let value = program.eval_expr(bytes, &VmExpr::Access(path)).unwrap();
        assert_eq!(value.value, VmValue::Text("BC".to_string()));
        let length = program
            .eval_expr(
                bytes,
                &VmExpr::Function {
                    function: VmFunction::Length,
                    args: vec![VmExpr::Identifier("B".to_string())],
                },
            )
            .unwrap();
        assert_eq!(length.value, VmValue::Integer(3));
        let ord = program
            .eval_expr(
                bytes,
                &VmExpr::Function {
                    function: VmFunction::Ord,
                    args: vec![VmExpr::Literal("A".to_string())],
                },
            )
            .unwrap();
        assert_eq!(ord.value, VmValue::Integer(65));
    }

    #[test]
    fn access_path_supports_fixed_occurs_and_odo_bounds() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![
                VmField {
                    name: "N".to_string(),
                    offset: 0,
                    byte_len: 1,
                    category: VmCategory::NumericDisplay,
                    usage: VmUsage::Display,
                    picture: Some(VmPicture {
                        signed: false,
                        digits: 1,
                        scale: 0,
                        char_len: 1,
                    }),
                },
                VmField {
                    name: "T".to_string(),
                    offset: 1,
                    byte_len: 3,
                    category: VmCategory::Alphanumeric,
                    usage: VmUsage::Display,
                    picture: None,
                },
            ],
            Vec::new(),
        );
        let bytes = b"2ABC";
        let path = VmAccessPath {
            target: "T".to_string(),
            condition_name: None,
            subscripts: vec![VmSubscript {
                expr: Box::new(VmExpr::Number("2".to_string())),
                stride: 1,
                min: 1,
                max: 3,
                depending_on: Some("N".to_string()),
                index_name: None,
            }],
            reference_modifier: None,
            result_len: Some(1),
        };
        let value = program.eval_expr(bytes, &VmExpr::Access(path)).unwrap();
        assert_eq!(value.value, VmValue::Text("B".to_string()));
        let bad = VmAccessPath {
            target: "T".to_string(),
            condition_name: None,
            subscripts: vec![VmSubscript {
                expr: Box::new(VmExpr::Number("3".to_string())),
                stride: 1,
                min: 1,
                max: 3,
                depending_on: Some("N".to_string()),
                index_name: None,
            }],
            reference_modifier: None,
            result_len: Some(1),
        };
        assert!(matches!(
            program.eval_expr(bytes, &VmExpr::Access(bad)),
            Err(VmError::InvalidSubscript { .. })
        ));
    }

    #[test]
    fn access_path_condition_name_respects_subscripted_parent_slice() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![VmField {
                name: "T.CELL".to_string(),
                offset: 0,
                byte_len: 3,
                category: VmCategory::Alphanumeric,
                usage: VmUsage::Display,
                picture: None,
            }],
            vec![VmConditionName {
                name: "IS_A".to_string(),
                parent: "T.CELL".to_string(),
                values: vec![VmConditionValue::Single("A".to_string())],
            }],
        );
        let bytes = b"A   B ";
        let path = |row: &str, cell: &str| VmAccessPath {
            target: "T.CELL".to_string(),
            condition_name: Some("T.CELL.IS_A".to_string()),
            subscripts: vec![
                VmSubscript {
                    expr: Box::new(VmExpr::Number(row.to_string())),
                    stride: 3,
                    min: 1,
                    max: 2,
                    depending_on: None,
                    index_name: None,
                },
                VmSubscript {
                    expr: Box::new(VmExpr::Number(cell.to_string())),
                    stride: 1,
                    min: 1,
                    max: 3,
                    depending_on: None,
                    index_name: None,
                },
            ],
            reference_modifier: None,
            result_len: Some(1),
        };

        let first = program
            .eval_expr(bytes, &VmExpr::Access(path("1", "1")))
            .unwrap();
        let second = program
            .eval_expr(bytes, &VmExpr::Access(path("2", "2")))
            .unwrap();

        assert_eq!(first.value, VmValue::Bool(true));
        assert_eq!(second.value, VmValue::Bool(false));
    }

    #[test]
    fn evaluate_snapshots_subjects_and_matches_bool_condition_patterns() {
        let program = sample_program();
        let bytes = b"123ABC";
        let evaluate = VmEvaluate {
            subjects: vec![VmExpr::Bool(true), VmExpr::Identifier("B".to_string())],
            branches: vec![VmBranch {
                patterns: vec![
                    VmEvaluatePattern::Condition(VmCondition::ConditionName {
                        reference: "A_HIGH".to_string(),
                    }),
                    VmEvaluatePattern::Operand(VmExpr::Literal("ABC".to_string())),
                ],
            }],
        };
        assert_eq!(program.eval_evaluate(bytes, &evaluate).unwrap(), Some(0));
    }

    #[test]
    fn ambiguous_unqualified_condition_names_fail_closed() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![
                VmField {
                    name: "F1".to_string(),
                    offset: 0,
                    byte_len: 1,
                    category: VmCategory::Alphanumeric,
                    usage: VmUsage::Display,
                    picture: None,
                },
                VmField {
                    name: "F2".to_string(),
                    offset: 1,
                    byte_len: 1,
                    category: VmCategory::Alphanumeric,
                    usage: VmUsage::Display,
                    picture: None,
                },
            ],
            vec![
                VmConditionName {
                    name: "OK".to_string(),
                    parent: "F1".to_string(),
                    values: vec![VmConditionValue::Single("Y".to_string())],
                },
                VmConditionName {
                    name: "OK".to_string(),
                    parent: "F2".to_string(),
                    values: vec![VmConditionValue::Single("Y".to_string())],
                },
            ],
        );
        assert!(matches!(
            program.eval_condition(
                b"YY",
                &VmCondition::ConditionName {
                    reference: "OK".to_string()
                }
            ),
            Err(VmError::AmbiguousConditionName { .. })
        ));
        assert!(program
            .eval_condition(
                b"YY",
                &VmCondition::ConditionName {
                    reference: "F2.OK".to_string()
                }
            )
            .unwrap());
    }

    #[test]
    fn runtime_materializes_index_subscripts_before_eval() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![VmField {
                name: "T".to_string(),
                offset: 0,
                byte_len: 3,
                category: VmCategory::Alphanumeric,
                usage: VmUsage::Display,
                picture: None,
            }],
            Vec::new(),
        );
        let mut runtime = bind_occurs_runtime(program, "T", &[b"A", b"B", b"C"]);
        runtime.define_index("IDX", "T", 1, 3);
        runtime.set_index("IDX", 2).unwrap();
        let value = runtime
            .eval_expr(&VmExpr::Access(VmAccessPath {
                target: "T".to_string(),
                condition_name: None,
                subscripts: vec![VmSubscript {
                    expr: Box::new(VmExpr::Number("0".to_string())),
                    stride: 1,
                    min: 1,
                    max: 3,
                    depending_on: None,
                    index_name: Some("IDX".to_string()),
                }],
                reference_modifier: None,
                result_len: Some(1),
            }))
            .unwrap();
        assert_eq!(value.value, VmValue::Text("B".to_string()));
    }

    #[test]
    fn runtime_search_linear_updates_index_until_match() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![VmField {
                name: "T".to_string(),
                offset: 0,
                byte_len: 3,
                category: VmCategory::Alphanumeric,
                usage: VmUsage::Display,
                picture: None,
            }],
            Vec::new(),
        );
        let mut runtime = bind_occurs_runtime(program, "T", &[b"A", b"B", b"C"]);
        runtime.define_index("IDX", "T", 1, 3);
        let condition = VmCondition::Relation {
            left: VmExpr::Access(VmAccessPath {
                target: "T".to_string(),
                condition_name: None,
                subscripts: vec![VmSubscript {
                    expr: Box::new(VmExpr::Number("0".to_string())),
                    stride: 1,
                    min: 1,
                    max: 3,
                    depending_on: None,
                    index_name: Some("IDX".to_string()),
                }],
                reference_modifier: None,
                result_len: Some(1),
            }),
            op: VmRelOp::Equal,
            right: VmExpr::Literal("B".to_string()),
        };
        let found = runtime
            .search_linear(&VmSearch {
                table: "T".to_string(),
                index_name: "IDX".to_string(),
                min: 1,
                max: 3,
                condition,
            })
            .unwrap();
        assert_eq!(found, Some(2));
        assert_eq!(runtime.index_occurrence("IDX").unwrap(), 2);
    }

    #[test]
    fn runtime_search_all_binary_search_sets_index_and_runs_found_ops() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![VmField {
                name: "T".to_string(),
                offset: 0,
                byte_len: 3,
                category: VmCategory::Alphanumeric,
                usage: VmUsage::Display,
                picture: None,
            }],
            Vec::new(),
        );
        let mut runtime = bind_occurs_runtime(program, "T", &[b"A", b"B", b"C"]);
        runtime.define_index("IDX", "T", 1, 3);
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![VmBasicBlock {
                name: "MAIN".to_string(),
                ops: vec![VmProcedureOp::SearchAll {
                    table: "T".to_string(),
                    index_name: "IDX".to_string(),
                    min: 1,
                    max: 3,
                    direction: VmSearchDirection::Ascending,
                    key: VmExpr::Access(VmAccessPath {
                        target: "T".to_string(),
                        condition_name: None,
                        subscripts: vec![VmSubscript {
                            expr: Box::new(VmExpr::Number("0".to_string())),
                            stride: 1,
                            min: 1,
                            max: 3,
                            depending_on: None,
                            index_name: Some("IDX".to_string()),
                        }],
                        reference_modifier: None,
                        result_len: Some(1),
                    }),
                    target: VmExpr::Literal("B".to_string()),
                    found_ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                        "FOUND".to_string(),
                    )])],
                    at_end_ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                        "NONE".to_string(),
                    )])],
                }],
                transfer: VmControlTransfer::StopRun,
            }],
        };

        runtime.execute_procedure(&procedure).unwrap();
        assert_eq!(runtime.index_occurrence("IDX").unwrap(), 2);
        assert_eq!(runtime.display, vec!["FOUND".to_string()]);
    }

    #[test]
    fn runtime_odo_active_count_is_checked() {
        let program = sample_program();
        let mut runtime = runtime_with_empty_pool(program);
        runtime.define_odo("TAB", "N", 0, 3, 2).unwrap();
        runtime.set_odo_active("TAB", 3).unwrap();
        assert!(matches!(
            runtime.set_odo_active("TAB", 4),
            Err(VmError::OdoRuntime { .. })
        ));
    }

    #[test]
    fn storage_pool_allocates_and_resizes_odo_occurrence_cells() {
        let mut pool = StoragePool::default();
        let depending_on = StorageKey::scalar("MAIN", "ODO-COUNT");
        pool.define_cell(depending_on.clone(), b"02".to_vec())
            .unwrap();
        pool.define_odo_table("MAIN", "TAB", depending_on, 3, 0, 4, 2)
            .unwrap();

        pool.write_occurrence("MAIN", "TAB", 2, b"ABC").unwrap();
        pool.resize_odo_table("MAIN", "TAB", 4).unwrap();

        assert_eq!(pool.occurrence_bytes("MAIN", "TAB", 2).unwrap(), b"ABC");
        assert_eq!(pool.occurrence_bytes("MAIN", "TAB", 4).unwrap(), b"   ");

        pool.resize_odo_table("MAIN", "TAB", 1).unwrap();
        assert!(matches!(
            pool.occurrence_bytes("MAIN", "TAB", 2),
            Err(VmError::InvalidSubscript { .. })
        ));
        assert!(matches!(
            pool.bytes(&StorageKey::occurrence("MAIN", "TAB", vec![2])),
            Err(VmError::StoragePool { .. })
        ));
    }

    #[test]
    fn storage_pool_recreates_fresh_odo_cells_after_reexpand() {
        let mut pool = StoragePool::default();
        let depending_on = StorageKey::scalar("MAIN", "ODO-COUNT");
        pool.define_cell(depending_on.clone(), b"02".to_vec())
            .unwrap();
        pool.define_odo_table("MAIN", "TAB", depending_on, 3, 0, 4, 2)
            .unwrap();
        pool.write_occurrence("MAIN", "TAB", 2, b"ZZZ").unwrap();

        pool.resize_odo_table("MAIN", "TAB", 1).unwrap();
        pool.resize_odo_table("MAIN", "TAB", 2).unwrap();

        assert_eq!(pool.occurrence_bytes("MAIN", "TAB", 2).unwrap(), b"   ");
    }

    #[test]
    fn storage_pool_rejects_invalid_odo_ranges_and_resize_counts() {
        let mut pool = StoragePool::default();
        let depending_on = StorageKey::scalar("MAIN", "ODO-COUNT");
        pool.define_cell(depending_on.clone(), b"00".to_vec())
            .unwrap();

        assert!(matches!(
            pool.define_odo_table("MAIN", "BAD", depending_on.clone(), 3, 5, 4, 0),
            Err(VmError::OdoRuntime { .. })
        ));
        pool.define_odo_table("MAIN", "TAB", depending_on, 3, 0, 4, 0)
            .unwrap();
        assert!(matches!(
            pool.resize_odo_table("MAIN", "TAB", 5),
            Err(VmError::OdoRuntime { .. })
        ));
    }

    fn pool_occurs_program(byte_len: usize) -> VmProgram {
        VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![VmField {
                name: "T".to_string(),
                offset: 0,
                byte_len,
                category: VmCategory::Alphanumeric,
                usage: VmUsage::Display,
                picture: None,
            }],
            Vec::new(),
        )
    }

    fn pool_occurs_path(occurrence: &str, result_len: usize) -> VmAccessPath {
        VmAccessPath {
            target: "T".to_string(),
            condition_name: None,
            subscripts: vec![VmSubscript {
                expr: Box::new(VmExpr::Number(occurrence.to_string())),
                stride: result_len,
                min: 1,
                max: 3,
                depending_on: None,
                index_name: None,
            }],
            reference_modifier: None,
            result_len: Some(result_len),
        }
    }

    fn pool_nested_occurs_path(row: &str, cell: &str) -> VmAccessPath {
        VmAccessPath {
            target: "T".to_string(),
            condition_name: None,
            subscripts: vec![
                VmSubscript {
                    expr: Box::new(VmExpr::Number(row.to_string())),
                    stride: 3,
                    min: 1,
                    max: 2,
                    depending_on: None,
                    index_name: None,
                },
                VmSubscript {
                    expr: Box::new(VmExpr::Number(cell.to_string())),
                    stride: 1,
                    min: 1,
                    max: 3,
                    depending_on: None,
                    index_name: None,
                },
            ],
            reference_modifier: None,
            result_len: Some(1),
        }
    }

    #[test]
    fn runtime_reads_bound_odo_occurrence_from_storage_pool() {
        let program = pool_occurs_program(1);
        let mut runtime = runtime_with_empty_pool(program);
        let depending_on = StorageKey::scalar("MAIN", "ODO-COUNT");
        runtime
            .define_storage_cell(depending_on.clone(), b"03".to_vec())
            .unwrap();
        runtime
            .define_odo_storage_table("MAIN", "T", depending_on, 1, 0, 3, 3)
            .unwrap();
        runtime
            .storage_pool
            .write_occurrence("MAIN", "T", 2, b"B")
            .unwrap();

        let value = runtime
            .eval_expr(&VmExpr::Access(pool_occurs_path("2", 1)))
            .unwrap();

        assert_eq!(value.value, VmValue::Text("B".to_string()));
    }

    #[test]
    fn runtime_storage_pool_occurs_access_slices_nested_subscripts() {
        let program = pool_occurs_program(3);
        let mut runtime = runtime_with_empty_pool(program);
        runtime
            .define_storage_cell(
                StorageKey::occurrence("MAIN", "T", vec![1]),
                b"ABC".to_vec(),
            )
            .unwrap();
        runtime
            .define_storage_cell(
                StorageKey::occurrence("MAIN", "T", vec![2]),
                b"DEF".to_vec(),
            )
            .unwrap();
        runtime.bind_occurs_storage_cell("T", "MAIN", "T");

        let value = runtime
            .eval_expr(&VmExpr::Access(pool_nested_occurs_path("2", "3")))
            .unwrap();
        assert_eq!(value.value, VmValue::Text("F".to_string()));

        runtime
            .move_value_to_access_path(
                &VmExpr::Literal("Z".to_string()),
                &pool_nested_occurs_path("2", "3"),
            )
            .unwrap();
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::occurrence("MAIN", "T", vec![1]))
                .unwrap(),
            b"ABC"
        );
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::occurrence("MAIN", "T", vec![2]))
                .unwrap(),
            b"DEZ"
        );
    }

    #[test]
    fn runtime_conditions_read_bound_odo_occurrences_from_storage_pool() {
        let program = pool_occurs_program(1);
        let mut runtime = runtime_with_empty_pool(program);
        let depending_on = StorageKey::scalar("MAIN", "ODO-COUNT");
        runtime
            .define_storage_cell(depending_on.clone(), b"03".to_vec())
            .unwrap();
        runtime
            .define_odo_storage_table("MAIN", "T", depending_on, 1, 0, 3, 3)
            .unwrap();
        runtime
            .storage_pool
            .write_occurrence("MAIN", "T", 2, b"B")
            .unwrap();

        assert!(runtime
            .eval_condition(&VmCondition::Relation {
                left: VmExpr::Access(pool_occurs_path("2", 1)),
                op: VmRelOp::Equal,
                right: VmExpr::Literal("B".to_string()),
            })
            .unwrap());
    }

    #[test]
    fn runtime_reference_modifies_bound_odo_occurrence_from_storage_pool() {
        let program = pool_occurs_program(4);
        let mut runtime = runtime_with_empty_pool(program);
        let depending_on = StorageKey::scalar("MAIN", "ODO-COUNT");
        runtime
            .define_storage_cell(depending_on.clone(), b"01".to_vec())
            .unwrap();
        runtime
            .define_odo_storage_table("MAIN", "T", depending_on, 4, 0, 3, 1)
            .unwrap();
        runtime
            .storage_pool
            .write_occurrence("MAIN", "T", 1, b"ABCD")
            .unwrap();
        let mut path = pool_occurs_path("1", 4);
        path.reference_modifier = Some(VmReferenceModifier {
            start: Box::new(VmExpr::Number("2".to_string())),
            length: Some(Box::new(VmExpr::Number("2".to_string()))),
        });

        let value = runtime.eval_expr(&VmExpr::Access(path)).unwrap();

        assert_eq!(value.value, VmValue::Text("BC".to_string()));
    }

    #[test]
    fn runtime_move_writes_bound_odo_occurrence_through_storage_pool() {
        let program = pool_occurs_program(1);
        let mut runtime = runtime_with_empty_pool(program);
        let depending_on = StorageKey::scalar("MAIN", "ODO-COUNT");
        runtime
            .define_storage_cell(depending_on.clone(), b"03".to_vec())
            .unwrap();
        runtime
            .define_odo_storage_table("MAIN", "T", depending_on, 1, 0, 3, 3)
            .unwrap();

        runtime
            .move_value_to_access_path(&VmExpr::Literal("Z".to_string()), &pool_occurs_path("3", 1))
            .unwrap();

        assert_eq!(
            runtime
                .storage_pool
                .occurrence_bytes("MAIN", "T", 3)
                .unwrap(),
            b"Z"
        );
    }

    #[test]
    fn runtime_storage_pool_odo_reads_respect_active_bounds() {
        let program = pool_occurs_program(1);
        let mut runtime = runtime_with_empty_pool(program);
        let depending_on = StorageKey::scalar("MAIN", "ODO-COUNT");
        runtime
            .define_storage_cell(depending_on.clone(), b"01".to_vec())
            .unwrap();
        runtime
            .define_odo_storage_table("MAIN", "T", depending_on, 1, 0, 3, 1)
            .unwrap();

        assert!(matches!(
            runtime.eval_expr(&VmExpr::Access(pool_occurs_path("2", 1))),
            Err(VmError::InvalidSubscript { .. })
        ));
    }

    #[test]
    fn runtime_group_byte_walk_skips_inactive_odo_occurrence_aliases() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![VmField {
                name: "N".to_string(),
                offset: 0,
                byte_len: 1,
                category: VmCategory::NumericDisplay,
                usage: VmUsage::Display,
                picture: Some(VmPicture {
                    signed: false,
                    digits: 1,
                    scale: 0,
                    char_len: 1,
                }),
            }],
            Vec::new(),
        );
        let mut runtime = runtime_with_empty_pool(program);
        let depending_on = StorageKey::scalar("MAIN", "N");
        runtime
            .define_storage_cell(depending_on.clone(), b"0".to_vec())
            .unwrap();
        runtime.bind_storage_cell("N", depending_on.clone());
        runtime
            .define_odo_storage_table("MAIN", "T", depending_on, 1, 0, 3, 0)
            .unwrap();
        runtime.define_odo("T", "N", 0, 3, 0).unwrap();
        for occurrence in 1..=3 {
            runtime.bind_storage_cell(
                format!("__T_OCC_{occurrence}"),
                StorageKey::occurrence("MAIN", "T", vec![occurrence]),
            );
        }
        runtime.bind_group_storage(
            "R",
            vec![
                "N".to_string(),
                "__T_OCC_1".to_string(),
                "__T_OCC_2".to_string(),
                "__T_OCC_3".to_string(),
            ],
        );

        runtime
            .move_value_to_access_path(
                &VmExpr::Number("2".to_string()),
                &VmAccessPath {
                    target: "N".to_string(),
                    condition_name: None,
                    subscripts: Vec::new(),
                    reference_modifier: None,
                    result_len: None,
                },
            )
            .unwrap();
        runtime
            .write_bytes_to_access_path(
                &VmAccessPath {
                    target: "R".to_string(),
                    condition_name: None,
                    subscripts: Vec::new(),
                    reference_modifier: None,
                    result_len: None,
                },
                b"2AB",
            )
            .unwrap();

        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "N"))
                .unwrap(),
            b"2"
        );
        assert_eq!(
            runtime
                .storage_pool
                .occurrence_bytes("MAIN", "T", 1)
                .unwrap(),
            b"A"
        );
        assert_eq!(
            runtime
                .storage_pool
                .occurrence_bytes("MAIN", "T", 2)
                .unwrap(),
            b"B"
        );
        assert!(matches!(
            runtime.storage_pool.occurrence_bytes("MAIN", "T", 3),
            Err(VmError::InvalidSubscript { .. })
        ));
    }

    #[test]
    fn in_memory_file_runtime_enforces_open_modes() {
        let mut files = VmFileRuntime::default();
        files.define_file("F", vec![b"ONE".to_vec()]);
        assert!(matches!(
            files.write("F", b"TWO"),
            Err(VmError::FileRuntime { .. })
        ));
        files.open("F", VmOpenMode::Input).unwrap();
        assert_eq!(files.read("F", 3).unwrap(), Some(b"ONE".to_vec()));
        assert_eq!(files.read("F", 3).unwrap(), None);
        files.close("F").unwrap();
        files.open("F", VmOpenMode::Output).unwrap();
        files.write("F", b"TWO").unwrap();
        assert_eq!(files.records("F").unwrap(), &[b"TWO".to_vec()]);
    }

    #[test]
    fn file_runtime_resolves_normalized_cobol_name_aliases() {
        let mut files = VmFileRuntime::default();
        files.define_file("TRANS-IN", vec![b"ONE".to_vec()]);

        files.open("TRANS_IN", VmOpenMode::Input).unwrap();
        assert_eq!(files.read("TRANS_IN", 3).unwrap(), Some(b"ONE".to_vec()));
        assert_eq!(files.last_status("TRANS_IN"), Some("00"));
        files.close("TRANS_IN").unwrap();

        files.open("TRANS_IN", VmOpenMode::Output).unwrap();
        files.write("TRANS_IN", b"TWO").unwrap();
        assert_eq!(files.records("TRANS_IN").unwrap(), &[b"TWO".to_vec()]);
    }

    #[test]
    fn platform_config_defines_fixed_os_sequential_file() {
        let path = temp_file_path("platform_read");
        fs::write(&path, b"AAABBB").expect("input bytes");
        let config = cobol_platform::PlatformConfig {
            files: vec![cobol_platform::FileBinding {
                name: "INFILE".to_string(),
                path: path.clone(),
                organization: cobol_platform::DatasetOrganization::Sequential,
                record_format: cobol_platform::RecordFormat::Fixed { record_len: 3 },
                disposition: cobol_platform::FileDisposition::Old,
                encoding: cobol_platform::DataEncoding::Ascii,
            }],
        };
        let mut files = VmFileRuntime::default();

        files
            .apply_platform_config(&config)
            .expect("platform config");

        files.open("INFILE", VmOpenMode::Input).unwrap();
        assert_eq!(files.read("INFILE", 3).unwrap(), Some(b"AAA".to_vec()));
        assert_eq!(files.read("INFILE", 3).unwrap(), Some(b"BBB".to_vec()));
        files.close("INFILE").unwrap();
        let _ = fs::remove_file(path);
    }

    #[test]
    fn platform_config_mod_disposition_can_extend_os_file() {
        let path = temp_file_path("platform_mod");
        fs::write(&path, b"OLD").expect("existing bytes");
        let config = cobol_platform::PlatformConfig {
            files: vec![cobol_platform::FileBinding {
                name: "OUTFILE".to_string(),
                path: path.clone(),
                organization: cobol_platform::DatasetOrganization::Sequential,
                record_format: cobol_platform::RecordFormat::Fixed { record_len: 3 },
                disposition: cobol_platform::FileDisposition::Mod,
                encoding: cobol_platform::DataEncoding::Ascii,
            }],
        };
        let mut files = VmFileRuntime::default();

        files
            .apply_platform_config(&config)
            .expect("platform config");

        files.open("OUTFILE", VmOpenMode::Extend).unwrap();
        files.write("OUTFILE", b"NEW").unwrap();
        files.close("OUTFILE").unwrap();
        assert_eq!(fs::read(&path).expect("output bytes"), b"OLDNEW");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn platform_config_new_disposition_truncates_only_on_open_output() {
        let path = temp_file_path("platform_new");
        fs::write(&path, b"OLD").expect("existing bytes");
        let config = cobol_platform::PlatformConfig {
            files: vec![cobol_platform::FileBinding {
                name: "OUTFILE".to_string(),
                path: path.clone(),
                organization: cobol_platform::DatasetOrganization::Sequential,
                record_format: cobol_platform::RecordFormat::Fixed { record_len: 3 },
                disposition: cobol_platform::FileDisposition::New,
                encoding: cobol_platform::DataEncoding::Ascii,
            }],
        };
        let mut files = VmFileRuntime::default();

        files
            .apply_platform_config(&config)
            .expect("platform config");
        assert_eq!(
            fs::read(&path).expect("bytes before open"),
            b"OLD",
            "applying config must not truncate before OPEN"
        );

        files.open("OUTFILE", VmOpenMode::Output).unwrap();
        files.write("OUTFILE", b"NEW").unwrap();
        files.close("OUTFILE").unwrap();
        assert_eq!(fs::read(&path).expect("output bytes"), b"NEW");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn platform_config_mod_disposition_rejects_output_truncate() {
        let path = temp_file_path("platform_mod_output");
        fs::write(&path, b"OLD").expect("existing bytes");
        let config = cobol_platform::PlatformConfig {
            files: vec![cobol_platform::FileBinding {
                name: "OUTFILE".to_string(),
                path: path.clone(),
                organization: cobol_platform::DatasetOrganization::Sequential,
                record_format: cobol_platform::RecordFormat::Fixed { record_len: 3 },
                disposition: cobol_platform::FileDisposition::Mod,
                encoding: cobol_platform::DataEncoding::Ascii,
            }],
        };
        let mut files = VmFileRuntime::default();

        files
            .apply_platform_config(&config)
            .expect("platform config");
        let err = files
            .open("OUTFILE", VmOpenMode::Output)
            .expect_err("MOD disposition must not allow OUTPUT truncation");

        assert![
            matches!(err, VmError::FileRuntime { ref name, ref message }
                if name == "OUTFILE" && message.contains("does not allow OPEN OUTPUT")),
            "{err:?}"
        ];
        assert_eq!(fs::read(&path).expect("output bytes"), b"OLD");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn platform_config_rejects_unsupported_organization() {
        let path = temp_file_path("platform_indexed");
        let config = cobol_platform::PlatformConfig {
            files: vec![cobol_platform::FileBinding {
                name: "VSAMFILE".to_string(),
                path,
                organization: cobol_platform::DatasetOrganization::Indexed,
                record_format: cobol_platform::RecordFormat::Fixed { record_len: 80 },
                disposition: cobol_platform::FileDisposition::Old,
                encoding: cobol_platform::DataEncoding::Ascii,
            }],
        };
        let mut files = VmFileRuntime::default();

        let err = files
            .apply_platform_config(&config)
            .expect_err("indexed config must fail closed");

        assert!(
            matches!(err, VmError::FileRuntime { ref name, ref message }
                if name == "VSAMFILE" && message.contains("unsupported organization")),
            "{err:?}"
        );
    }

    #[test]
    fn eof_read_clears_last_record_for_rewrite() {
        let mut files = VmFileRuntime::default();
        files.define_file("F", vec![b"ONE".to_vec()]);
        files.open("F", VmOpenMode::Io).unwrap();

        assert_eq!(files.read("F", 3).unwrap(), Some(b"ONE".to_vec()));
        assert_eq!(files.read("F", 3).unwrap(), None);

        assert!(matches!(
            files.rewrite("F", b"BAD"),
            Err(VmError::FileRuntime { .. })
        ));
        assert_eq!(files.records("F").unwrap(), &[b"ONE".to_vec()]);
    }

    #[test]
    fn delete_consumes_last_read_record_state() {
        let mut files = VmFileRuntime::default();
        files.define_file("F", vec![b"ONE".to_vec(), b"TWO".to_vec()]);
        files.open("F", VmOpenMode::Io).unwrap();

        assert_eq!(files.read("F", 3).unwrap(), Some(b"ONE".to_vec()));
        files.delete("F").unwrap();

        assert!(matches!(
            files.delete("F"),
            Err(VmError::FileRuntime { .. })
        ));
        assert_eq!(files.records("F").unwrap(), &[b"TWO".to_vec()]);
    }

    #[test]
    fn rewrite_consumes_last_read_record_state() {
        let mut files = VmFileRuntime::default();
        files.define_file("F", vec![b"ONE".to_vec()]);
        files.open("F", VmOpenMode::Io).unwrap();

        assert_eq!(files.read("F", 3).unwrap(), Some(b"ONE".to_vec()));
        files.rewrite("F", b"TWO").unwrap();

        assert!(matches!(
            files.rewrite("F", b"BAD"),
            Err(VmError::FileRuntime { .. })
        ));
        assert_eq!(files.records("F").unwrap(), &[b"TWO".to_vec()]);
    }

    #[test]
    fn render_numeric_display_keeps_sign_before_zero_padding() {
        assert_eq!(render_numeric_display("-5", 3), "-05");
        assert_eq!(render_numeric_display("+5", 3), "+05");
    }

    #[test]
    fn numeric_display_decode_accepts_leading_and_trailing_signs() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![VmField {
                name: "N".to_string(),
                offset: 0,
                byte_len: 4,
                category: VmCategory::NumericDisplay,
                usage: VmUsage::Display,
                picture: Some(VmPicture {
                    signed: true,
                    digits: 3,
                    scale: 0,
                    char_len: 4,
                }),
            }],
            Vec::new(),
        );
        let mut runtime = runtime_with_scalar(program, "N", b"123-");

        assert_eq!(
            runtime
                .eval_expr(&VmExpr::Identifier("N".to_string()))
                .unwrap()
                .value,
            VmValue::Decimal(Decimal::new(-123, 0))
        );

        runtime
            .storage_pool
            .write_cell(&StorageKey::scalar("MAIN", "N"), b"123+")
            .unwrap();
        assert_eq!(
            runtime
                .eval_expr(&VmExpr::Identifier("N".to_string()))
                .unwrap()
                .value,
            VmValue::Decimal(Decimal::new(123, 0))
        );

        runtime
            .storage_pool
            .write_cell(&StorageKey::scalar("MAIN", "N"), b"-12+")
            .unwrap();
        assert!(matches!(
            runtime.eval_expr(&VmExpr::Identifier("N".to_string())),
            Err(VmError::InvalidDecimal { .. })
        ));
    }

    #[test]
    fn numeric_display_negative_zero_preserves_sign_when_moved_to_packed() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![
                VmField {
                    name: "DISPLAY_ZERO".to_string(),
                    offset: 0,
                    byte_len: 2,
                    category: VmCategory::NumericDisplay,
                    usage: VmUsage::Display,
                    picture: Some(VmPicture {
                        signed: true,
                        digits: 1,
                        scale: 0,
                        char_len: 1,
                    }),
                },
                VmField {
                    name: "PACKED_ZERO".to_string(),
                    offset: 2,
                    byte_len: 1,
                    category: VmCategory::PackedDecimal,
                    usage: VmUsage::PackedDecimal,
                    picture: Some(VmPicture {
                        signed: true,
                        digits: 1,
                        scale: 0,
                        char_len: 1,
                    }),
                },
            ],
            Vec::new(),
        );
        let mut pool = StoragePool::default();
        pool.define_cell(StorageKey::scalar("MAIN", "DISPLAY_ZERO"), b"0-".to_vec())
            .unwrap();
        pool.define_cell(StorageKey::scalar("MAIN", "PACKED_ZERO"), vec![0x0c])
            .unwrap();
        let mut runtime = VmRuntime::new(program, pool);
        runtime.bind_storage_cell("DISPLAY_ZERO", StorageKey::scalar("MAIN", "DISPLAY_ZERO"));
        runtime.bind_storage_cell("PACKED_ZERO", StorageKey::scalar("MAIN", "PACKED_ZERO"));

        let decoded = runtime
            .eval_expr(&VmExpr::Identifier("DISPLAY_ZERO".to_string()))
            .unwrap();
        assert_eq!(decoded.value, VmValue::Decimal(Decimal::ZERO));
        assert!(to_decimal(&decoded).unwrap().is_sign_negative());

        runtime
            .move_value_to_access_path(
                &VmExpr::Identifier("DISPLAY_ZERO".to_string()),
                &sort_test_access_path("PACKED_ZERO", 1),
            )
            .unwrap();

        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "PACKED_ZERO"))
                .unwrap(),
            &[0x0d]
        );
    }

    #[test]
    fn packed_negative_zero_preserves_sign_when_moved_to_display_and_packed() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![
                VmField {
                    name: "PACKED_ZERO".to_string(),
                    offset: 0,
                    byte_len: 1,
                    category: VmCategory::PackedDecimal,
                    usage: VmUsage::PackedDecimal,
                    picture: Some(VmPicture {
                        signed: true,
                        digits: 1,
                        scale: 0,
                        char_len: 1,
                    }),
                },
                VmField {
                    name: "DISPLAY_ZERO".to_string(),
                    offset: 1,
                    byte_len: 2,
                    category: VmCategory::NumericDisplay,
                    usage: VmUsage::Display,
                    picture: Some(VmPicture {
                        signed: true,
                        digits: 1,
                        scale: 0,
                        char_len: 1,
                    }),
                },
                VmField {
                    name: "PACKED_COPY".to_string(),
                    offset: 3,
                    byte_len: 1,
                    category: VmCategory::PackedDecimal,
                    usage: VmUsage::PackedDecimal,
                    picture: Some(VmPicture {
                        signed: true,
                        digits: 1,
                        scale: 0,
                        char_len: 1,
                    }),
                },
            ],
            Vec::new(),
        );
        let mut pool = StoragePool::default();
        pool.define_cell(StorageKey::scalar("MAIN", "PACKED_ZERO"), vec![0x0d])
            .unwrap();
        pool.define_cell(StorageKey::scalar("MAIN", "DISPLAY_ZERO"), b"00".to_vec())
            .unwrap();
        pool.define_cell(StorageKey::scalar("MAIN", "PACKED_COPY"), vec![0x0c])
            .unwrap();
        let mut runtime = VmRuntime::new(program, pool);
        runtime.bind_storage_cell("PACKED_ZERO", StorageKey::scalar("MAIN", "PACKED_ZERO"));
        runtime.bind_storage_cell("DISPLAY_ZERO", StorageKey::scalar("MAIN", "DISPLAY_ZERO"));
        runtime.bind_storage_cell("PACKED_COPY", StorageKey::scalar("MAIN", "PACKED_COPY"));

        let decoded = runtime
            .eval_expr(&VmExpr::Identifier("PACKED_ZERO".to_string()))
            .unwrap();
        assert_eq!(decoded.value, VmValue::Decimal(Decimal::ZERO));
        assert!(to_decimal(&decoded).unwrap().is_sign_negative());

        runtime
            .move_value_to_access_path(
                &VmExpr::Identifier("PACKED_ZERO".to_string()),
                &sort_test_access_path("DISPLAY_ZERO", 2),
            )
            .unwrap();
        runtime
            .move_value_to_access_path(
                &VmExpr::Identifier("PACKED_ZERO".to_string()),
                &sort_test_access_path("PACKED_COPY", 1),
            )
            .unwrap();

        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "DISPLAY_ZERO"))
                .unwrap(),
            b"-0"
        );
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "PACKED_COPY"))
                .unwrap(),
            &[0x0d]
        );
    }

    #[test]
    fn numeric_display_write_respects_implied_decimal_scale() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![VmField {
                name: "AMT".to_string(),
                offset: 0,
                byte_len: 3,
                category: VmCategory::NumericDisplay,
                usage: VmUsage::Display,
                picture: Some(VmPicture {
                    signed: false,
                    digits: 3,
                    scale: 2,
                    char_len: 3,
                }),
            }],
            Vec::new(),
        );
        let mut runtime = runtime_with_scalar(program, "AMT", b"000");
        let path = sort_test_access_path("AMT", 3);

        runtime
            .write_value_to_access_path(
                &path,
                &VmEvaluatedValue {
                    value: VmValue::Decimal(Decimal::from_str("1.23").unwrap()),
                    category: VmCategory::NumericDisplay,
                    byte_len: 3,
                },
            )
            .unwrap();

        assert_eq!(runtime.read_bytes_from_access_path(&path).unwrap(), b"123");
        let decoded = runtime.eval_expr(&VmExpr::Access(path)).unwrap();
        assert_eq!(
            to_decimal(&decoded).unwrap(),
            Decimal::from_str("1.23").unwrap()
        );
    }

    #[test]
    fn figurative_constants_fill_alphanumeric_target_bytes() {
        let mut runtime = runtime_with_scalar(sample_program(), "B", b"ABC");
        let target = VmAccessPath {
            target: "B".to_string(),
            condition_name: None,
            subscripts: Vec::new(),
            reference_modifier: None,
            result_len: None,
        };

        runtime
            .move_value_to_access_path(&VmExpr::Figurative(VmFigurative::Zero), &target)
            .unwrap();
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "B"))
                .unwrap(),
            b"000"
        );

        runtime
            .move_value_to_access_path(&VmExpr::Figurative(VmFigurative::HighValue), &target)
            .unwrap();
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "B"))
                .unwrap(),
            &[0xFF, 0xFF, 0xFF]
        );
    }

    #[test]
    fn os_sequential_file_runtime_reads_fixed_records() {
        let path = temp_file_path("read");
        fs::write(&path, b"AAABBB").unwrap();
        let mut files = VmFileRuntime::default();
        files.define_os_sequential_file("F", path.clone());

        files.open("F", VmOpenMode::Input).unwrap();
        assert_eq!(files.read("F", 3).unwrap(), Some(b"AAA".to_vec()));
        assert_eq!(files.read("F", 3).unwrap(), Some(b"BBB".to_vec()));
        assert_eq!(files.read("F", 3).unwrap(), None);
        assert_eq!(files.read("F", 3).unwrap(), None);
        files.close("F").unwrap();

        let _ = fs::remove_file(path);
    }

    #[test]
    fn os_sequential_file_runtime_writes_output_records() {
        let path = temp_file_path("write");
        fs::write(&path, b"OLD").unwrap();
        let mut files = VmFileRuntime::default();
        files.define_os_sequential_file("F", path.clone());

        files.open("F", VmOpenMode::Output).unwrap();
        files.write("F", b"ABC").unwrap();
        files.write("F", b"DEF").unwrap();
        files.close("F").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"ABCDEF");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn os_sequential_file_runtime_extend_appends_records() {
        let path = temp_file_path("extend");
        fs::write(&path, b"ABC").unwrap();
        let mut files = VmFileRuntime::default();
        files.define_os_sequential_file("F", path.clone());

        files.open("F", VmOpenMode::Extend).unwrap();
        files.write("F", b"DEF").unwrap();
        files.close("F").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"ABCDEF");
        let _ = fs::remove_file(path);
    }

    #[test]
    fn tape_file_runtime_writes_length_prefixed_records() {
        let path = temp_file_path("tape_write");
        let mut files = VmFileRuntime::default();
        files.define_tape_file("TAPE", path.clone());

        files.open("TAPE", VmOpenMode::Extend).unwrap();
        files.write("TAPE", b"ABC").unwrap();
        files.write("TAPE", b"DE").unwrap();
        files.close("TAPE").unwrap();

        let image = fs::read(&path).unwrap();
        assert!(image
            .windows(b"COBOLVM-TAPE-1".len())
            .any(|window| window == b"COBOLVM-TAPE-1"));

        let mut reopened = VmFileRuntime::default();
        reopened.define_tape_file("TAPE", path.clone());
        reopened.open("TAPE", VmOpenMode::Input).unwrap();
        assert_eq!(reopened.read("TAPE", 3).unwrap(), Some(b"ABC".to_vec()));
        assert_eq!(reopened.read("TAPE", 3).unwrap(), Some(b"DE ".to_vec()));
        assert_eq!(reopened.read("TAPE", 3).unwrap(), None);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn rerun_checkpoint_snapshot_restores_storage_and_file_cursor_from_tape() {
        let path = temp_file_path("rerun_restore");
        let mut pool = StoragePool::default();
        let rec_key = StorageKey::scalar("MAIN", "REC");
        pool.define_cell(rec_key.clone(), b"  ".to_vec()).unwrap();
        let mut runtime = VmRuntime::new(
            VmProgram::new(
                DialectProfile::ibm_zos(),
                vec![VmField {
                    name: "REC".to_string(),
                    offset: 0,
                    byte_len: 2,
                    category: VmCategory::Alphanumeric,
                    usage: VmUsage::Display,
                    picture: None,
                }],
                Vec::new(),
            ),
            pool,
        );
        runtime.bind_storage_cell("REC", rec_key.clone());
        runtime.files.define_file(
            "MASTER",
            vec![b"A1".to_vec(), b"B2".to_vec(), b"C3".to_vec()],
        );
        runtime.files.define_tape_file("JUNK", path.clone());
        runtime.register_rerun_checkpoint("JUNK", "MASTER", 2);
        runtime.files.open("MASTER", VmOpenMode::Input).unwrap();
        runtime.files.open("JUNK", VmOpenMode::Extend).unwrap();

        let target = VmAccessPath {
            target: "REC".to_string(),
            condition_name: None,
            subscripts: Vec::new(),
            reference_modifier: None,
            result_len: None,
        };
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![VmBasicBlock {
                name: "MAIN".to_string(),
                ops: vec![
                    VmProcedureOp::ReadFile {
                        name: "MASTER".to_string(),
                        target: target.clone(),
                        at_end_ops: Vec::new(),
                        not_at_end_ops: Vec::new(),
                        on_exception_ops: Vec::new(),
                    },
                    VmProcedureOp::ReadFile {
                        name: "MASTER".to_string(),
                        target: target.clone(),
                        at_end_ops: Vec::new(),
                        not_at_end_ops: Vec::new(),
                        on_exception_ops: Vec::new(),
                    },
                    VmProcedureOp::Move {
                        source: VmExpr::Literal("ZZ".to_string()),
                        target: target.clone(),
                    },
                ],
                transfer: VmControlTransfer::StopRun,
            }],
        };

        runtime.execute_procedure(&procedure).unwrap();
        assert_eq!(runtime.storage_pool.bytes(&rec_key).unwrap(), b"ZZ");

        assert!(runtime.restore_last_rerun_checkpoint("JUNK").unwrap());
        assert_eq!(runtime.storage_pool.bytes(&rec_key).unwrap(), b"B2");
        assert_eq!(
            runtime.files.read("MASTER", 2).unwrap(),
            Some(b"C3".to_vec())
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn checkpoint_snapshot_restores_sort_stack_for_return_record() {
        let mut pool = StoragePool::default();
        let rec_key = StorageKey::scalar("MAIN", "SORT_REC");
        pool.define_cell(rec_key.clone(), b"  ".to_vec()).unwrap();
        let mut runtime = VmRuntime::new(
            VmProgram::new(
                DialectProfile::ibm_zos(),
                vec![VmField {
                    name: "SORT_REC".to_string(),
                    offset: 0,
                    byte_len: 2,
                    category: VmCategory::Alphanumeric,
                    usage: VmUsage::Display,
                    picture: None,
                }],
                Vec::new(),
            ),
            pool,
        );
        runtime.bind_storage_cell("SORT_REC", rec_key.clone());
        let record = VmAccessPath {
            target: "SORT_REC".to_string(),
            condition_name: None,
            subscripts: Vec::new(),
            reference_modifier: None,
            result_len: None,
        };
        runtime.sort_states.push(VmSortState {
            file: "SORT_FILE".to_string(),
            phase: VmSortPhase::Output,
            record: record.clone(),
            record_len: 2,
            released_records: vec![b"A1".to_vec(), b"B2".to_vec()],
            sorted_records: vec![b"A1".to_vec(), b"B2".to_vec()],
            cursor: 1,
            key: Some(VmSortKeyDescriptor {
                offset: 0,
                byte_len: 2,
                direction: VmSortDirection::Ascending,
                encoding: VmSortKeyEncoding::Bytes,
            }),
        });

        let snapshot = runtime.checkpoint_snapshot_bytes();
        runtime.sort_states.clear();
        runtime.storage_pool.write_cell(&rec_key, b"ZZ").unwrap();
        runtime.restore_checkpoint_snapshot(&snapshot).unwrap();

        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![VmBasicBlock {
                name: "MAIN".to_string(),
                ops: vec![VmProcedureOp::ReturnSortRecord {
                    file: "SORT_FILE".to_string(),
                    record: record.clone(),
                    target: None,
                    at_end_ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                        "END".to_string(),
                    )])],
                    not_at_end_ops: Vec::new(),
                }],
                transfer: VmControlTransfer::StopRun,
            }],
        };
        runtime.execute_procedure(&procedure).unwrap();

        assert_eq!(runtime.storage_pool.bytes(&rec_key).unwrap(), b"B2");
        assert!(runtime.display.is_empty());
    }

    #[test]
    fn checkpoint_snapshot_restores_trace_display_and_declarative_guards() {
        let mut runtime = VmRuntime::new(
            VmProgram::new(DialectProfile::ibm_zos(), Vec::new(), Vec::new()),
            StoragePool::default(),
        );
        runtime.trace_enabled = true;
        runtime.display.push("BEFORE".to_string());
        runtime
            .active_file_error_declaratives
            .insert(normalize_vm_key("ERR-FILE"));
        runtime
            .active_debugging_declaratives
            .insert(normalize_vm_key("TRACE-PARA"));

        let snapshot = runtime.checkpoint_snapshot_bytes();
        runtime.trace_enabled = false;
        runtime.display.clear();
        runtime.active_file_error_declaratives.clear();
        runtime.active_debugging_declaratives.clear();
        runtime.restore_checkpoint_snapshot(&snapshot).unwrap();

        assert!(runtime.trace_enabled);
        assert_eq!(runtime.display, vec!["BEFORE".to_string()]);
        assert!(runtime
            .active_file_error_declaratives
            .contains(&normalize_vm_key("ERR-FILE")));
        assert!(runtime
            .active_debugging_declaratives
            .contains(&normalize_vm_key("TRACE-PARA")));
    }

    #[test]
    fn unsupported_trap_returns_procedure_runtime_error() {
        let mut runtime = VmRuntime::new(
            VmProgram::new(DialectProfile::ibm_zos(), Vec::new(), Vec::new()),
            StoragePool::default(),
        );
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![VmBasicBlock {
                name: "MAIN".to_string(),
                ops: vec![VmProcedureOp::UnsupportedTrap {
                    message: "unsupported fallback".to_string(),
                }],
                transfer: VmControlTransfer::StopRun,
            }],
        };

        let error = runtime.execute_procedure(&procedure).unwrap_err();

        assert!(matches!(
            error,
            VmError::ProcedureRuntime { block, message }
                if block == "MAIN" && message == "unsupported fallback"
        ));
    }

    #[test]
    fn vm_error_exposes_stable_runtime_code() {
        let error = VmError::FileRuntime {
            name: "MASTER".to_string(),
            message: "file is not open for input".to_string(),
        };

        assert_eq!(error.code(), "CBL-RT-FILE");
    }

    #[test]
    fn runtime_abend_report_json_includes_error_code_and_state_dump() {
        let program = VmProgram::new(DialectProfile::ibm_zos(), Vec::new(), Vec::new());
        let mut runtime = VmRuntime::new(program, StoragePool::default());
        runtime.files.define_file("MASTER", vec![b"A1".to_vec()]);
        runtime.activation_stack.push(VmFrame {
            program: "ABENDPGM".to_string(),
            current: "READ-MASTER".to_string(),
            return_to: Some("AFTER-READ".to_string()),
            source_span: Some(VmSourceSpan {
                file: "master.cbl".to_string(),
                line: 42,
                column: 7,
            }),
            local_bindings: BTreeMap::new(),
        });

        let error = VmError::FileRuntime {
            name: "MASTER".to_string(),
            message: "file is not open for input".to_string(),
        };
        let report = runtime.abend_report_json(&error);
        let parsed: serde_json::Value = serde_json::from_str(&report).expect("ABEND report JSON");

        assert_eq!(parsed["type"], "ABEND");
        assert_eq!(parsed["error"]["code"], "CBL-RT-FILE");
        assert_eq!(parsed["error"]["message"], error.to_string());
        assert_eq!(parsed["state"]["current_frame"]["program"], "ABENDPGM");
        assert_eq!(parsed["state"]["current_frame"]["paragraph"], "READ-MASTER");
        assert_eq!(parsed["state"]["current_frame"]["return_to"], "AFTER-READ");
        assert_eq!(
            parsed["state"]["current_frame"]["source_span"]["file"],
            "master.cbl"
        );
        assert_eq!(parsed["state"]["current_frame"]["source_span"]["line"], 42);
        assert_eq!(parsed["state"]["current_frame"]["source_span"]["column"], 7);
        assert_eq!(parsed["state"]["files"][0]["name"], "MASTER");
        assert_eq!(parsed["state"]["files"][0]["cursor"], 0);
    }

    #[test]
    fn checkpoint_snapshot_restores_runtime_dialect() {
        let mut runtime = VmRuntime::new(
            VmProgram::new(DialectProfile::gnucobol(), Vec::new(), Vec::new()),
            StoragePool::default(),
        );

        let snapshot = runtime.checkpoint_snapshot_bytes();
        runtime.program.dialect = DialectProfile::ibm_zos();
        runtime.restore_checkpoint_snapshot(&snapshot).unwrap();

        assert_eq!(runtime.program.dialect, DialectProfile::gnucobol());
    }

    #[test]
    fn checkpoint_restore_rejects_corrupt_snapshot_without_mutating_runtime() {
        let mut pool = StoragePool::default();
        let rec_key = StorageKey::scalar("MAIN", "REC");
        pool.define_cell(rec_key.clone(), b"AA".to_vec()).unwrap();
        let mut runtime = VmRuntime::new(
            VmProgram::new(DialectProfile::ibm_zos(), Vec::new(), Vec::new()),
            pool,
        );

        let mut corrupt = String::from_utf8(runtime.checkpoint_snapshot_bytes()).unwrap();
        corrupt = corrupt.replace("S 4D41494E 524543 - 4141", "S 4D41494E 524543 - 4242");
        corrupt = corrupt.replace("\nEND\n", "\nBROKEN\n");

        runtime.storage_pool.write_cell(&rec_key, b"ZZ").unwrap();
        assert!(runtime
            .restore_checkpoint_snapshot(corrupt.as_bytes())
            .is_err());
        assert_eq!(runtime.storage_pool.bytes(&rec_key).unwrap(), b"ZZ");
    }

    #[test]
    fn checkpoint_snapshot_restores_os_sequential_file_cursor() {
        let path = temp_file_path("rerun_os_cursor");
        fs::write(&path, b"A1B2C3").unwrap();
        let mut runtime = VmRuntime::new(
            VmProgram::new(DialectProfile::ibm_zos(), Vec::new(), Vec::new()),
            StoragePool::default(),
        );
        runtime
            .files
            .define_os_sequential_file("MASTER", path.clone());
        runtime.files.open("MASTER", VmOpenMode::Input).unwrap();
        assert_eq!(
            runtime.files.read("MASTER", 2).unwrap(),
            Some(b"A1".to_vec())
        );

        let snapshot = runtime.checkpoint_snapshot_bytes();
        assert_eq!(
            runtime.files.read("MASTER", 2).unwrap(),
            Some(b"B2".to_vec())
        );
        runtime.restore_checkpoint_snapshot(&snapshot).unwrap();

        assert_eq!(
            runtime.files.read("MASTER", 2).unwrap(),
            Some(b"B2".to_vec())
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn checkpoint_restore_rejects_os_file_change_after_first_fingerprint_chunk() {
        let path = temp_file_path("rerun_os_full_fingerprint");
        let original = vec![b'A'; 2048];
        fs::write(&path, &original).unwrap();
        let mut runtime = VmRuntime::new(
            VmProgram::new(DialectProfile::ibm_zos(), Vec::new(), Vec::new()),
            StoragePool::default(),
        );
        runtime
            .files
            .define_os_sequential_file("MASTER", path.clone());

        let snapshot = runtime.checkpoint_snapshot_bytes();
        let mut changed = original;
        changed[1500] = b'Z';
        fs::write(&path, &changed).unwrap();

        let error = runtime
            .restore_checkpoint_snapshot(&snapshot)
            .expect_err("restore must reject OS sequential content drift");
        assert!(matches!(
            error,
            VmError::ProcedureRuntime { ref message, .. }
                if message.contains("RERUN-MISMATCH")
        ));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn checkpoint_restore_uses_snapshot_os_file_path_after_remap() {
        let original_path = temp_file_path("rerun_os_original_path");
        let remapped_path = temp_file_path("rerun_os_remapped_path");
        fs::write(&original_path, b"A1B2").unwrap();
        fs::write(&remapped_path, b"XXYY").unwrap();
        let mut runtime = VmRuntime::new(
            VmProgram::new(DialectProfile::ibm_zos(), Vec::new(), Vec::new()),
            StoragePool::default(),
        );
        runtime
            .files
            .define_os_sequential_file("MASTER", original_path.clone());
        runtime.files.open("MASTER", VmOpenMode::Input).unwrap();
        assert_eq!(
            runtime.files.read("MASTER", 2).unwrap(),
            Some(b"A1".to_vec())
        );
        let snapshot = runtime.checkpoint_snapshot_bytes();

        runtime
            .files
            .map_external_name("MASTER", remapped_path.clone());
        runtime.files.open("MASTER", VmOpenMode::Input).unwrap();
        assert_eq!(
            runtime.files.read("MASTER", 2).unwrap(),
            Some(b"XX".to_vec())
        );

        runtime.restore_checkpoint_snapshot(&snapshot).unwrap();
        assert_eq!(
            runtime.files.read("MASTER", 2).unwrap(),
            Some(b"B2".to_vec())
        );
        let restored = runtime.files.files.get("MASTER").unwrap();
        match &restored.backing {
            VmFileBacking::OsSequential { path, .. } => assert_eq!(path, &original_path),
            other => panic!("expected OS sequential backing, got {other:?}"),
        }

        let _ = fs::remove_file(original_path);
        let _ = fs::remove_file(remapped_path);
    }

    #[test]
    fn checkpoint_snapshot_restores_os_sequential_file_after_eof() {
        let path = temp_file_path("rerun_os_eof");
        fs::write(&path, b"A1B2").unwrap();
        let mut runtime = VmRuntime::new(
            VmProgram::new(DialectProfile::ibm_zos(), Vec::new(), Vec::new()),
            StoragePool::default(),
        );
        runtime
            .files
            .define_os_sequential_file("MASTER", path.clone());
        runtime.files.open("MASTER", VmOpenMode::Input).unwrap();
        assert_eq!(
            runtime.files.read("MASTER", 2).unwrap(),
            Some(b"A1".to_vec())
        );
        assert_eq!(
            runtime.files.read("MASTER", 2).unwrap(),
            Some(b"B2".to_vec())
        );
        assert_eq!(runtime.files.read("MASTER", 2).unwrap(), None);

        let snapshot = runtime.checkpoint_snapshot_bytes();
        runtime.restore_checkpoint_snapshot(&snapshot).unwrap();

        assert_eq!(runtime.files.read("MASTER", 2).unwrap(), None);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn checkpoint_snapshot_persists_os_fixed_record_len_after_eof() {
        let path = temp_file_path("rerun_os_fixed_len");
        fs::write(&path, b"A1B2").unwrap();
        let mut runtime = VmRuntime::new(
            VmProgram::new(DialectProfile::ibm_zos(), Vec::new(), Vec::new()),
            StoragePool::default(),
        );
        runtime
            .files
            .define_os_sequential_file_with_record_len("MASTER", path.clone(), 2);
        runtime.files.open("MASTER", VmOpenMode::Input).unwrap();
        assert_eq!(
            runtime.files.read("MASTER", 2).unwrap(),
            Some(b"A1".to_vec())
        );
        assert_eq!(
            runtime.files.read("MASTER", 2).unwrap(),
            Some(b"B2".to_vec())
        );
        assert_eq!(runtime.files.read("MASTER", 2).unwrap(), None);

        let snapshot = runtime.checkpoint_snapshot_bytes();
        runtime.restore_checkpoint_snapshot(&snapshot).unwrap();

        let restored = runtime.files.files.get("MASTER").unwrap();
        assert_eq!(restored.fixed_record_len, Some(2));
        assert_eq!(runtime.files.read("MASTER", 2).unwrap(), None);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn rerun_checkpoint_skips_when_checkpoint_already_in_progress() {
        let path = temp_file_path("rerun_reentrant");
        let mut runtime = VmRuntime::new(
            VmProgram::new(DialectProfile::ibm_zos(), Vec::new(), Vec::new()),
            StoragePool::default(),
        );
        runtime.files.define_tape_file("JUNK", path.clone());
        runtime.register_rerun_checkpoint("JUNK", "MASTER", 1);
        runtime.files.open("JUNK", VmOpenMode::Extend).unwrap();
        runtime.checkpoint_in_progress = true;

        runtime.note_rerun_successful_read("MASTER").unwrap();

        assert!(runtime.files.records("JUNK").unwrap().is_empty());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn os_sequential_file_runtime_opens_io_for_rewrite_delete_support() {
        let path = temp_file_path("open_io");
        fs::write(&path, b"ABC").unwrap();
        let mut files = VmFileRuntime::default();
        files.define_os_sequential_file("F", path.clone());

        files.open("F", VmOpenMode::Io).unwrap();
        assert_eq!(files.last_status("F"), Some("00"));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn os_sequential_file_runtime_open_input_missing_file_errors() {
        let path = temp_file_path("missing");
        let mut files = VmFileRuntime::default();
        files.define_os_sequential_file("F", path);

        assert!(matches!(
            files.open("F", VmOpenMode::Input),
            Err(VmError::FileRuntime { .. })
        ));
    }

    #[test]
    fn os_sequential_file_runtime_partial_fixed_record_errors() {
        let path = temp_file_path("partial");
        fs::write(&path, b"AB").unwrap();
        let mut files = VmFileRuntime::default();
        files.define_os_sequential_file("F", path.clone());

        files.open("F", VmOpenMode::Input).unwrap();
        assert!(matches!(
            files.read("F", 3),
            Err(VmError::FileRuntime { .. })
        ));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn os_sequential_partial_read_clears_last_record_for_rewrite() {
        let path = temp_file_path("partial_rewrite");
        fs::write(&path, b"AAB").unwrap();
        let mut files = VmFileRuntime::default();
        files.define_os_sequential_file("F", path.clone());

        files.open("F", VmOpenMode::Io).unwrap();
        assert_eq!(files.read("F", 2).unwrap(), Some(b"AA".to_vec()));
        assert!(matches!(
            files.read("F", 2),
            Err(VmError::FileRuntime { .. })
        ));
        assert!(matches!(
            files.rewrite("F", b"ZZ"),
            Err(VmError::FileRuntime { .. })
        ));
        files.close("F").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"AAB");

        let _ = fs::remove_file(path);
    }

    #[test]
    fn initial_program_reset_closes_lifecycle_files_without_clearing_records() {
        let program = VmProgram::new(DialectProfile::ibm_zos(), Vec::new(), Vec::new());
        let mut runtime = runtime_with_empty_pool(program);
        runtime
            .files
            .define_file("F", vec![b"ONE".to_vec(), b"TWO".to_vec()]);
        runtime.files.open("F", VmOpenMode::Input).unwrap();
        assert_eq!(runtime.files.read("F", 3).unwrap(), Some(b"ONE".to_vec()));

        let registered = VmRegisteredProgram {
            procedure: VmProcedure {
                entry: "MAIN".to_string(),
                blocks: Vec::new(),
            },
            linkage: Vec::new(),
            is_initial: true,
            initial_cells: Vec::new(),
            initial_odo: Vec::new(),
            initial_files: vec!["F".to_string()],
        };
        runtime.reset_initial_program_instance(&registered).unwrap();

        let file = runtime.files.files.get("F").expect("file lifecycle entry");
        assert_eq!(file.open_mode, None);
        assert_eq!(file.cursor, 0);
        assert_eq!(
            runtime.files.records("F").unwrap(),
            &[b"ONE".to_vec(), b"TWO".to_vec()]
        );
        assert!(matches!(
            runtime.files.read("F", 3),
            Err(VmError::FileRuntime { .. })
        ));
    }

    #[test]
    fn initial_program_reset_closes_lifecycle_os_file_without_clearing_contents() {
        let path = temp_file_path("reset");
        fs::write(&path, b"ONE").unwrap();
        let program = VmProgram::new(DialectProfile::ibm_zos(), Vec::new(), Vec::new());
        let mut runtime = runtime_with_empty_pool(program);
        runtime.files.define_os_sequential_file("F", path.clone());
        runtime.files.open("F", VmOpenMode::Input).unwrap();
        assert_eq!(runtime.files.read("F", 3).unwrap(), Some(b"ONE".to_vec()));

        let registered = VmRegisteredProgram {
            procedure: VmProcedure {
                entry: "MAIN".to_string(),
                blocks: Vec::new(),
            },
            linkage: Vec::new(),
            is_initial: true,
            initial_cells: Vec::new(),
            initial_odo: Vec::new(),
            initial_files: vec!["F".to_string()],
        };
        runtime.reset_initial_program_instance(&registered).unwrap();

        let file = runtime.files.files.get("F").expect("file lifecycle entry");
        assert_eq!(file.open_mode, None);
        assert_eq!(file.cursor, 0);
        assert_eq!(fs::read(&path).unwrap(), b"ONE");
        assert!(matches!(
            runtime.files.read("F", 3),
            Err(VmError::FileRuntime { .. })
        ));
        runtime.files.open("F", VmOpenMode::Input).unwrap();
        assert_eq!(runtime.files.read("F", 3).unwrap(), Some(b"ONE".to_vec()));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn runtime_procedure_executes_basic_file_read_and_write_ops() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![VmField {
                name: "REC".to_string(),
                offset: 0,
                byte_len: 3,
                category: VmCategory::Alphanumeric,
                usage: VmUsage::Display,
                picture: None,
            }],
            Vec::new(),
        );
        let mut runtime = runtime_with_scalar(program, "REC", b"   ");
        runtime.files.define_file("IN", vec![b"ONE".to_vec()]);
        runtime.files.define_file("OUT", Vec::new());
        let rec = VmAccessPath {
            target: "REC".to_string(),
            condition_name: None,
            subscripts: Vec::new(),
            reference_modifier: None,
            result_len: None,
        };
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![VmBasicBlock {
                name: "MAIN".to_string(),
                ops: vec![
                    VmProcedureOp::OpenFile {
                        name: "IN".to_string(),
                        mode: VmOpenMode::Input,
                    },
                    VmProcedureOp::ReadFile {
                        name: "IN".to_string(),
                        target: rec.clone(),
                        at_end_ops: Vec::new(),
                        not_at_end_ops: Vec::new(),
                        on_exception_ops: Vec::new(),
                    },
                    VmProcedureOp::OpenFile {
                        name: "OUT".to_string(),
                        mode: VmOpenMode::Output,
                    },
                    VmProcedureOp::WriteFile {
                        name: "OUT".to_string(),
                        source: rec,
                        advancing: VmWriteAdvancing::None,
                    },
                    VmProcedureOp::CloseFile {
                        name: "OUT".to_string(),
                    },
                ],
                transfer: VmControlTransfer::StopRun,
            }],
        };
        runtime.execute_procedure(&procedure).unwrap();
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "REC"))
                .unwrap(),
            b"ONE"
        );
        assert_eq!(runtime.files.records("OUT").unwrap(), &[b"ONE".to_vec()]);
    }

    #[test]
    fn runtime_file_read_executes_not_at_end_ops_after_filling_record() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![
                VmField {
                    name: "REC".to_string(),
                    offset: 0,
                    byte_len: 3,
                    category: VmCategory::Alphanumeric,
                    usage: VmUsage::Display,
                    picture: None,
                },
                VmField {
                    name: "WS".to_string(),
                    offset: 3,
                    byte_len: 3,
                    category: VmCategory::Alphanumeric,
                    usage: VmUsage::Display,
                    picture: None,
                },
            ],
            Vec::new(),
        );
        let mut pool = StoragePool::default();
        pool.define_cell(StorageKey::scalar("MAIN", "REC"), b"   ".to_vec())
            .unwrap();
        pool.define_cell(StorageKey::scalar("MAIN", "WS"), b"   ".to_vec())
            .unwrap();
        let mut runtime = VmRuntime::new(program, pool);
        runtime.bind_storage_cell("REC", StorageKey::scalar("MAIN", "REC"));
        runtime.bind_storage_cell("WS", StorageKey::scalar("MAIN", "WS"));
        runtime.files.define_file("IN", vec![b"ONE".to_vec()]);
        let rec = VmAccessPath {
            target: "REC".to_string(),
            condition_name: None,
            subscripts: Vec::new(),
            reference_modifier: None,
            result_len: None,
        };
        let ws = VmAccessPath {
            target: "WS".to_string(),
            condition_name: None,
            subscripts: Vec::new(),
            reference_modifier: None,
            result_len: None,
        };
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![VmBasicBlock {
                name: "MAIN".to_string(),
                ops: vec![
                    VmProcedureOp::OpenFile {
                        name: "IN".to_string(),
                        mode: VmOpenMode::Input,
                    },
                    VmProcedureOp::ReadFile {
                        name: "IN".to_string(),
                        target: rec.clone(),
                        at_end_ops: Vec::new(),
                        not_at_end_ops: vec![VmProcedureOp::Move {
                            source: VmExpr::Access(rec),
                            target: ws,
                        }],
                        on_exception_ops: Vec::new(),
                    },
                ],
                transfer: VmControlTransfer::StopRun,
            }],
        };
        runtime.execute_procedure(&procedure).unwrap();
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "WS"))
                .unwrap(),
            b"ONE"
        );
    }

    #[test]
    fn runtime_open_missing_os_file_sets_file_status_when_bound() {
        let path = temp_file_path("missing_status");
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![VmField {
                name: "FS".to_string(),
                offset: 0,
                byte_len: 2,
                category: VmCategory::Alphanumeric,
                usage: VmUsage::Display,
                picture: None,
            }],
            Vec::new(),
        );
        let mut pool = StoragePool::default();
        pool.define_cell(StorageKey::scalar("MAIN", "FS"), b"  ".to_vec())
            .unwrap();
        let mut runtime = VmRuntime::new(program, pool);
        runtime.bind_storage_cell("FS", StorageKey::scalar("MAIN", "FS"));
        runtime.bind_file_status(
            "IN",
            VmAccessPath {
                target: "FS".to_string(),
                condition_name: None,
                subscripts: Vec::new(),
                reference_modifier: None,
                result_len: None,
            },
        );
        runtime.files.define_os_sequential_file("IN", path);
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![VmBasicBlock {
                name: "MAIN".to_string(),
                ops: vec![VmProcedureOp::OpenFile {
                    name: "IN".to_string(),
                    mode: VmOpenMode::Input,
                }],
                transfer: VmControlTransfer::StopRun,
            }],
        };

        runtime.execute_procedure(&procedure).unwrap();
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "FS"))
                .unwrap(),
            b"35"
        );
    }

    #[test]
    fn runtime_open_missing_os_file_errors_without_file_status() {
        let path = temp_file_path("missing_no_status");
        let program = VmProgram::new(DialectProfile::ibm_zos(), Vec::new(), Vec::new());
        let mut runtime = runtime_with_empty_pool(program);
        runtime.files.define_os_sequential_file("IN", path);
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![VmBasicBlock {
                name: "MAIN".to_string(),
                ops: vec![VmProcedureOp::OpenFile {
                    name: "IN".to_string(),
                    mode: VmOpenMode::Input,
                }],
                transfer: VmControlTransfer::StopRun,
            }],
        };

        assert!(matches!(
            runtime.execute_procedure(&procedure),
            Err(VmError::FileRuntime { .. })
        ));
    }

    #[test]
    fn runtime_procedure_perform_times_executes_target_before_fallthrough() {
        let program = VmProgram::new(DialectProfile::ibm_zos(), Vec::new(), Vec::new());
        let mut runtime = runtime_with_empty_pool(program);
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![
                VmBasicBlock {
                    name: "MAIN".to_string(),
                    ops: Vec::new(),
                    transfer: VmControlTransfer::Perform {
                        target: "SUB".to_string(),
                        through: None,
                        times: Some(VmExpr::Number("3".to_string())),
                    },
                },
                VmBasicBlock {
                    name: "DONE".to_string(),
                    ops: Vec::new(),
                    transfer: VmControlTransfer::StopRun,
                },
                VmBasicBlock {
                    name: "SUB".to_string(),
                    ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                        "X".to_string(),
                    )])],
                    transfer: VmControlTransfer::FallThrough(None),
                },
            ],
        };
        runtime.execute_procedure(&procedure).unwrap();
        assert_eq!(runtime.display, vec!["X", "X", "X"]);
    }

    #[test]
    fn runtime_procedure_perform_until_executes_until_condition_changes() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![VmField {
                name: "FLAG".to_string(),
                offset: 0,
                byte_len: 1,
                category: VmCategory::Alphanumeric,
                usage: VmUsage::Display,
                picture: None,
            }],
            Vec::new(),
        );
        let flag = VmAccessPath {
            target: "FLAG".to_string(),
            condition_name: None,
            subscripts: Vec::new(),
            reference_modifier: None,
            result_len: None,
        };
        let mut runtime = runtime_with_scalar(program, "FLAG", b"N");
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![
                VmBasicBlock {
                    name: "MAIN".to_string(),
                    ops: vec![
                        VmProcedureOp::PerformLoop {
                            target: "SUB".to_string(),
                            through: None,
                            varying: None,
                            until: Some(VmCondition::Relation {
                                left: VmExpr::Access(flag.clone()),
                                op: VmRelOp::Equal,
                                right: VmExpr::Literal("Y".to_string()),
                            }),
                        },
                        VmProcedureOp::Display(vec![VmExpr::Literal("AFTER".to_string())]),
                    ],
                    transfer: VmControlTransfer::StopRun,
                },
                VmBasicBlock {
                    name: "SUB".to_string(),
                    ops: vec![
                        VmProcedureOp::Display(vec![VmExpr::Literal("LOOP".to_string())]),
                        VmProcedureOp::Move {
                            source: VmExpr::Literal("Y".to_string()),
                            target: flag,
                        },
                    ],
                    transfer: VmControlTransfer::FallThrough(None),
                },
            ],
        };
        runtime.execute_procedure(&procedure).unwrap();
        assert_eq!(runtime.display, vec!["LOOP", "AFTER"]);
    }

    #[test]
    fn runtime_procedure_perform_varying_updates_data_item_until_condition() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![VmField {
                name: "I".to_string(),
                offset: 0,
                byte_len: 1,
                category: VmCategory::NumericDisplay,
                usage: VmUsage::Display,
                picture: Some(VmPicture {
                    signed: false,
                    digits: 1,
                    scale: 0,
                    char_len: 1,
                }),
            }],
            Vec::new(),
        );
        let counter = VmAccessPath {
            target: "I".to_string(),
            condition_name: None,
            subscripts: Vec::new(),
            reference_modifier: None,
            result_len: None,
        };
        let mut runtime = runtime_with_scalar(program, "I", b"0");
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![
                VmBasicBlock {
                    name: "MAIN".to_string(),
                    ops: vec![
                        VmProcedureOp::PerformLoop {
                            target: "SUB".to_string(),
                            through: None,
                            varying: Some(VmPerformVarying {
                                target: VmVaryingTarget::Access(counter.clone()),
                                from: VmExpr::Number("1".to_string()),
                                by: VmExpr::Number("1".to_string()),
                            }),
                            until: Some(VmCondition::Relation {
                                left: VmExpr::Access(counter.clone()),
                                op: VmRelOp::Greater,
                                right: VmExpr::Number("3".to_string()),
                            }),
                        },
                        VmProcedureOp::Display(vec![VmExpr::Literal("DONE".to_string())]),
                    ],
                    transfer: VmControlTransfer::StopRun,
                },
                VmBasicBlock {
                    name: "SUB".to_string(),
                    ops: vec![VmProcedureOp::Display(vec![VmExpr::Access(counter)])],
                    transfer: VmControlTransfer::FallThrough(None),
                },
            ],
        };
        runtime.execute_procedure(&procedure).unwrap();
        assert_eq!(runtime.display, vec!["1", "2", "3", "DONE"]);
    }

    #[test]
    fn runtime_call_dispatches_registered_literal_program_with_shared_storage() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![VmField {
                name: "FLAG".to_string(),
                offset: 0,
                byte_len: 1,
                category: VmCategory::Alphanumeric,
                usage: VmUsage::Display,
                picture: None,
            }],
            Vec::new(),
        );
        let flag = VmAccessPath {
            target: "FLAG".to_string(),
            condition_name: None,
            subscripts: Vec::new(),
            reference_modifier: None,
            result_len: None,
        };
        let mut runtime = runtime_with_scalar(program, "FLAG", b"N");
        runtime.registry.insert(
            "SUBPROG",
            VmProcedure {
                entry: "SUB".to_string(),
                blocks: vec![VmBasicBlock {
                    name: "SUB".to_string(),
                    ops: vec![
                        VmProcedureOp::Move {
                            source: VmExpr::Literal("Y".to_string()),
                            target: flag.clone(),
                        },
                        VmProcedureOp::Display(vec![VmExpr::Literal("SUB".to_string())]),
                    ],
                    transfer: VmControlTransfer::StopRun,
                }],
            },
        );
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![VmBasicBlock {
                name: "MAIN".to_string(),
                ops: vec![
                    VmProcedureOp::Call {
                        target: VmCallTarget::Literal("SUBPROG".to_string()),
                        using: Vec::new(),
                    },
                    VmProcedureOp::If {
                        condition: VmCondition::Relation {
                            left: VmExpr::Access(flag),
                            op: VmRelOp::Equal,
                            right: VmExpr::Literal("Y".to_string()),
                        },
                        then_ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                            "AFTER".to_string(),
                        )])],
                        else_ops: Vec::new(),
                    },
                ],
                transfer: VmControlTransfer::StopRun,
            }],
        };

        runtime.execute_procedure(&procedure).unwrap();
        assert_eq!(runtime.display, vec!["SUB", "AFTER"]);
        assert!(runtime.activation_stack.is_empty());
    }

    #[test]
    fn runtime_call_using_binds_linkage_name_to_caller_cell_by_reference() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![
                VmField {
                    name: "FLAG".to_string(),
                    offset: 0,
                    byte_len: 1,
                    category: VmCategory::Alphanumeric,
                    usage: VmUsage::Display,
                    picture: None,
                },
                VmField {
                    name: "LK-FLAG".to_string(),
                    offset: 0,
                    byte_len: 1,
                    category: VmCategory::Alphanumeric,
                    usage: VmUsage::Display,
                    picture: None,
                },
            ],
            Vec::new(),
        );
        let flag = VmAccessPath {
            target: "FLAG".to_string(),
            condition_name: None,
            subscripts: Vec::new(),
            reference_modifier: None,
            result_len: None,
        };
        let linkage_flag = VmAccessPath {
            target: "LK-FLAG".to_string(),
            condition_name: None,
            subscripts: Vec::new(),
            reference_modifier: None,
            result_len: None,
        };
        let mut runtime = runtime_with_scalar(program, "FLAG", b"N");
        runtime.registry.insert_with_linkage(
            "SUBPROG",
            VmProcedure {
                entry: "SUB".to_string(),
                blocks: vec![VmBasicBlock {
                    name: "SUB".to_string(),
                    ops: vec![
                        VmProcedureOp::Move {
                            source: VmExpr::Literal("Y".to_string()),
                            target: linkage_flag.clone(),
                        },
                        VmProcedureOp::Display(vec![VmExpr::Access(linkage_flag)]),
                    ],
                    transfer: VmControlTransfer::StopRun,
                }],
            },
            vec!["LK-FLAG".to_string()],
        );
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![VmBasicBlock {
                name: "MAIN".to_string(),
                ops: vec![
                    VmProcedureOp::Call {
                        target: VmCallTarget::Literal("SUBPROG".to_string()),
                        using: vec![flag.clone()],
                    },
                    VmProcedureOp::Display(vec![VmExpr::Access(flag)]),
                ],
                transfer: VmControlTransfer::StopRun,
            }],
        };

        runtime.execute_procedure(&procedure).unwrap();
        assert_eq!(runtime.display, vec!["Y", "Y"]);
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "FLAG"))
                .unwrap(),
            b"Y"
        );
    }

    #[test]
    fn runtime_call_dispatches_registered_dynamic_program_name() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![VmField {
                name: "PGM".to_string(),
                offset: 0,
                byte_len: 7,
                category: VmCategory::Alphanumeric,
                usage: VmUsage::Display,
                picture: None,
            }],
            Vec::new(),
        );
        let pgm = VmAccessPath {
            target: "PGM".to_string(),
            condition_name: None,
            subscripts: Vec::new(),
            reference_modifier: None,
            result_len: None,
        };
        let mut runtime = runtime_with_scalar(program, "PGM", b"SUBPROG");
        runtime.registry.insert(
            "SUBPROG",
            VmProcedure {
                entry: "SUB".to_string(),
                blocks: vec![VmBasicBlock {
                    name: "SUB".to_string(),
                    ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                        "DYNAMIC".to_string(),
                    )])],
                    transfer: VmControlTransfer::StopRun,
                }],
            },
        );
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![VmBasicBlock {
                name: "MAIN".to_string(),
                ops: vec![VmProcedureOp::Call {
                    target: VmCallTarget::Dynamic(VmExpr::Access(pgm)),
                    using: Vec::new(),
                }],
                transfer: VmControlTransfer::StopRun,
            }],
        };

        runtime.execute_procedure(&procedure).unwrap();
        assert_eq!(runtime.display, vec!["DYNAMIC"]);
    }

    #[test]
    fn runtime_dynamic_call_rejects_path_like_program_name() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![VmField {
                name: "PGM".to_string(),
                offset: 0,
                byte_len: 6,
                category: VmCategory::Alphanumeric,
                usage: VmUsage::Display,
                picture: None,
            }],
            Vec::new(),
        );
        let pgm = VmAccessPath {
            target: "PGM".to_string(),
            condition_name: None,
            subscripts: Vec::new(),
            reference_modifier: None,
            result_len: None,
        };
        let mut runtime = runtime_with_scalar(program, "PGM", b"../SUB");
        runtime.registry.insert(
            "../SUB",
            VmProcedure {
                entry: "SUB".to_string(),
                blocks: vec![VmBasicBlock {
                    name: "SUB".to_string(),
                    ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                        "SHOULD-NOT-RUN".to_string(),
                    )])],
                    transfer: VmControlTransfer::StopRun,
                }],
            },
        );
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![VmBasicBlock {
                name: "MAIN".to_string(),
                ops: vec![VmProcedureOp::Call {
                    target: VmCallTarget::Dynamic(VmExpr::Access(pgm)),
                    using: Vec::new(),
                }],
                transfer: VmControlTransfer::StopRun,
            }],
        };

        let error = runtime.execute_procedure(&procedure).unwrap_err();
        assert!(matches!(error, VmError::NestedProgramRuntime { .. }));
        assert!(error
            .to_string()
            .contains("dynamic CALL target must be a program name"));
        assert!(runtime.display.is_empty());
    }

    #[test]
    fn runtime_missing_dynamic_call_sets_program_status_and_continues() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![VmField {
                name: "PGM".to_string(),
                offset: 0,
                byte_len: 7,
                category: VmCategory::Alphanumeric,
                usage: VmUsage::Display,
                picture: None,
            }],
            Vec::new(),
        );
        let pgm = VmAccessPath {
            target: "PGM".to_string(),
            condition_name: None,
            subscripts: Vec::new(),
            reference_modifier: None,
            result_len: None,
        };
        let program_status = VmAccessPath {
            target: PROGRAM_STATUS_REGISTER.to_string(),
            condition_name: None,
            subscripts: Vec::new(),
            reference_modifier: None,
            result_len: Some(2),
        };
        let mut runtime = runtime_with_scalar(program, "PGM", b"MISSING");
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![VmBasicBlock {
                name: "MAIN".to_string(),
                ops: vec![
                    VmProcedureOp::Call {
                        target: VmCallTarget::Dynamic(VmExpr::Access(pgm)),
                        using: Vec::new(),
                    },
                    VmProcedureOp::Display(vec![VmExpr::Access(program_status)]),
                    VmProcedureOp::Display(vec![VmExpr::Literal("AFTER".to_string())]),
                ],
                transfer: VmControlTransfer::StopRun,
            }],
        };

        runtime.execute_procedure(&procedure).unwrap();
        assert_eq!(runtime.display, vec!["01", "AFTER"]);
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::special(PROGRAM_STATUS_REGISTER))
                .unwrap(),
            b"01"
        );
    }

    #[test]
    fn runtime_perform_thru_keeps_goto_inside_active_scope() {
        let program = VmProgram::new(DialectProfile::ibm_zos(), Vec::new(), Vec::new());
        let mut runtime = runtime_with_empty_pool(program);
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![
                VmBasicBlock {
                    name: "MAIN".to_string(),
                    ops: vec![VmProcedureOp::Perform {
                        target: "SUB".to_string(),
                        through: Some("ENDSUB".to_string()),
                        times: None,
                    }],
                    transfer: VmControlTransfer::FallThrough(Some("DONE".to_string())),
                },
                VmBasicBlock {
                    name: "DONE".to_string(),
                    ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                        "DONE".to_string(),
                    )])],
                    transfer: VmControlTransfer::StopRun,
                },
                VmBasicBlock {
                    name: "SUB".to_string(),
                    ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                        "SUB".to_string(),
                    )])],
                    transfer: VmControlTransfer::GoTo("ENDSUB".to_string()),
                },
                VmBasicBlock {
                    name: "MID".to_string(),
                    ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                        "MID".to_string(),
                    )])],
                    transfer: VmControlTransfer::FallThrough(Some("ENDSUB".to_string())),
                },
                VmBasicBlock {
                    name: "ENDSUB".to_string(),
                    ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                        "END".to_string(),
                    )])],
                    transfer: VmControlTransfer::FallThrough(None),
                },
            ],
        };

        runtime.execute_procedure(&procedure).unwrap();

        assert_eq!(runtime.display, vec!["SUB", "END", "DONE"]);
    }

    #[test]
    fn runtime_goto_exits_active_perform_scope() {
        let program = VmProgram::new(DialectProfile::ibm_zos(), Vec::new(), Vec::new());
        let mut runtime = runtime_with_empty_pool(program);
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![
                VmBasicBlock {
                    name: "MAIN".to_string(),
                    ops: vec![VmProcedureOp::Perform {
                        target: "SUB".to_string(),
                        through: Some("ENDSUB".to_string()),
                        times: None,
                    }],
                    transfer: VmControlTransfer::FallThrough(Some("AFTER".to_string())),
                },
                VmBasicBlock {
                    name: "AFTER".to_string(),
                    ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                        "AFTER".to_string(),
                    )])],
                    transfer: VmControlTransfer::StopRun,
                },
                VmBasicBlock {
                    name: "SUB".to_string(),
                    ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                        "SUB".to_string(),
                    )])],
                    transfer: VmControlTransfer::GoTo("OUT".to_string()),
                },
                VmBasicBlock {
                    name: "ENDSUB".to_string(),
                    ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                        "END".to_string(),
                    )])],
                    transfer: VmControlTransfer::FallThrough(Some("OUT".to_string())),
                },
                VmBasicBlock {
                    name: "OUT".to_string(),
                    ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                        "OUT".to_string(),
                    )])],
                    transfer: VmControlTransfer::StopRun,
                },
            ],
        };

        runtime.execute_procedure(&procedure).unwrap();

        assert_eq!(runtime.display, vec!["SUB", "OUT"]);
    }

    #[test]
    fn runtime_nested_goto_op_unwinds_perform_scope() {
        let program = VmProgram::new(DialectProfile::ibm_zos(), Vec::new(), Vec::new());
        let mut runtime = runtime_with_empty_pool(program);
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![
                VmBasicBlock {
                    name: "MAIN".to_string(),
                    ops: vec![VmProcedureOp::Perform {
                        target: "SUB".to_string(),
                        through: Some("ENDSUB".to_string()),
                        times: None,
                    }],
                    transfer: VmControlTransfer::FallThrough(Some("AFTER".to_string())),
                },
                VmBasicBlock {
                    name: "AFTER".to_string(),
                    ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                        "AFTER".to_string(),
                    )])],
                    transfer: VmControlTransfer::StopRun,
                },
                VmBasicBlock {
                    name: "SUB".to_string(),
                    ops: vec![VmProcedureOp::If {
                        condition: VmCondition::Relation {
                            left: VmExpr::Bool(true),
                            op: VmRelOp::Equal,
                            right: VmExpr::Bool(true),
                        },
                        then_ops: vec![VmProcedureOp::GoTo {
                            target: "OUT".to_string(),
                        }],
                        else_ops: Vec::new(),
                    }],
                    transfer: VmControlTransfer::FallThrough(Some("ENDSUB".to_string())),
                },
                VmBasicBlock {
                    name: "ENDSUB".to_string(),
                    ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                        "END".to_string(),
                    )])],
                    transfer: VmControlTransfer::FallThrough(Some("OUT".to_string())),
                },
                VmBasicBlock {
                    name: "OUT".to_string(),
                    ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                        "OUT".to_string(),
                    )])],
                    transfer: VmControlTransfer::StopRun,
                },
            ],
        };

        runtime.execute_procedure(&procedure).unwrap();

        assert_eq!(runtime.display, vec!["OUT"]);
    }

    #[test]
    fn runtime_procedure_executes_move_if_and_set_condition_name() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![VmField {
                name: "FLAG".to_string(),
                offset: 0,
                byte_len: 1,
                category: VmCategory::Alphanumeric,
                usage: VmUsage::Display,
                picture: None,
            }],
            vec![VmConditionName {
                name: "OK".to_string(),
                parent: "FLAG".to_string(),
                values: vec![VmConditionValue::Single("Y".to_string())],
            }],
        );
        let mut runtime = runtime_with_scalar(program, "FLAG", b" ");
        let flag = VmAccessPath {
            target: "FLAG".to_string(),
            condition_name: None,
            subscripts: Vec::new(),
            reference_modifier: None,
            result_len: None,
        };
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![VmBasicBlock {
                name: "MAIN".to_string(),
                ops: vec![
                    VmProcedureOp::Move {
                        source: VmExpr::Literal("N".to_string()),
                        target: flag.clone(),
                    },
                    VmProcedureOp::SetConditionName {
                        name: "OK".to_string(),
                    },
                    VmProcedureOp::If {
                        condition: VmCondition::ConditionName {
                            reference: "OK".to_string(),
                        },
                        then_ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                            "OK".to_string(),
                        )])],
                        else_ops: Vec::new(),
                    },
                ],
                transfer: VmControlTransfer::StopRun,
            }],
        };
        runtime.execute_procedure(&procedure).unwrap();
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "FLAG"))
                .unwrap(),
            b"Y"
        );
        assert_eq!(runtime.display, vec!["OK"]);
    }

    #[test]
    fn runtime_group_move_copies_raw_child_cells_without_decode() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![
                VmField {
                    name: "SRC".to_string(),
                    offset: 0,
                    byte_len: 5,
                    category: VmCategory::Group,
                    usage: VmUsage::Group,
                    picture: None,
                },
                VmField {
                    name: "DST".to_string(),
                    offset: 0,
                    byte_len: 5,
                    category: VmCategory::Group,
                    usage: VmUsage::Group,
                    picture: None,
                },
            ],
            Vec::new(),
        );
        let mut pool = StoragePool::default();
        for (name, bytes) in [
            ("SRC_A", vec![b'A']),
            ("SRC_PACKED", vec![0x12, 0x3c]),
            ("SRC_BIN", vec![0x00, 0x05]),
            ("DST_A", vec![b' ']),
            ("DST_PACKED", vec![0x00, 0x00]),
            ("DST_BIN", vec![0x00, 0x00]),
        ] {
            pool.define_cell(StorageKey::scalar("MAIN", name), bytes)
                .unwrap();
        }
        let mut runtime = VmRuntime::new(program, pool);
        for name in [
            "SRC_A",
            "SRC_PACKED",
            "SRC_BIN",
            "DST_A",
            "DST_PACKED",
            "DST_BIN",
        ] {
            runtime.bind_storage_cell(name, StorageKey::scalar("MAIN", name));
        }
        runtime.bind_group_storage(
            "SRC",
            vec![
                "SRC_A".to_string(),
                "SRC_PACKED".to_string(),
                "SRC_BIN".to_string(),
            ],
        );
        runtime.bind_group_storage(
            "DST",
            vec![
                "DST_A".to_string(),
                "DST_PACKED".to_string(),
                "DST_BIN".to_string(),
            ],
        );
        runtime
            .move_value_to_access_path(
                &VmExpr::Access(VmAccessPath {
                    target: "SRC".to_string(),
                    condition_name: None,
                    subscripts: Vec::new(),
                    reference_modifier: None,
                    result_len: None,
                }),
                &VmAccessPath {
                    target: "DST".to_string(),
                    condition_name: None,
                    subscripts: Vec::new(),
                    reference_modifier: None,
                    result_len: None,
                },
            )
            .unwrap();

        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "DST_PACKED"))
                .unwrap(),
            &[0x12, 0x3c]
        );
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "DST_BIN"))
                .unwrap(),
            &[0x00, 0x05]
        );
    }

    #[test]
    fn compute_writes_decimal_result_to_packed_target() {
        let mut runtime = packed_runtime(&[0x00, 0x0c]);
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![VmBasicBlock {
                name: "MAIN".to_string(),
                ops: vec![
                    VmProcedureOp::Compute {
                        target: packed_access_path("PACKED"),
                        expr: VmExpr::Add(
                            Box::new(VmExpr::Number("120".to_string())),
                            Box::new(VmExpr::Number("3".to_string())),
                        ),
                        rounded: false,
                        on_size_error_ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                            "SIZE".to_string(),
                        )])],
                        not_on_size_error_ops: vec![VmProcedureOp::Display(vec![VmExpr::Access(
                            packed_access_path("PACKED"),
                        )])],
                    },
                    VmProcedureOp::StopRun,
                ],
                transfer: VmControlTransfer::StopRun,
            }],
        };

        runtime.execute_procedure(&procedure).unwrap();

        assert_eq!(runtime.display, vec!["123"]);
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "PACKED"))
                .unwrap(),
            &[0x12, 0x3c]
        );
    }

    #[test]
    fn compute_packed_overflow_takes_size_error_without_mutating_target() {
        let mut runtime = packed_runtime(&[0x12, 0x3c]);
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![VmBasicBlock {
                name: "MAIN".to_string(),
                ops: vec![
                    VmProcedureOp::Compute {
                        target: packed_access_path("PACKED"),
                        expr: VmExpr::Number("1234".to_string()),
                        rounded: false,
                        on_size_error_ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                            "SIZE".to_string(),
                        )])],
                        not_on_size_error_ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                            "BAD".to_string(),
                        )])],
                    },
                    VmProcedureOp::Display(vec![VmExpr::Access(packed_access_path("PACKED"))]),
                    VmProcedureOp::StopRun,
                ],
                transfer: VmControlTransfer::StopRun,
            }],
        };

        runtime.execute_procedure(&procedure).unwrap();

        assert_eq!(runtime.display, vec!["SIZE", "123"]);
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "PACKED"))
                .unwrap(),
            &[0x12, 0x3c]
        );
    }

    #[test]
    fn compute_divide_by_zero_takes_size_error_without_mutating_target() {
        let mut runtime = packed_runtime(&[0x12, 0x3c]);
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![VmBasicBlock {
                name: "MAIN".to_string(),
                ops: vec![
                    VmProcedureOp::Compute {
                        target: packed_access_path("PACKED"),
                        expr: VmExpr::Divide(
                            Box::new(VmExpr::Number("9".to_string())),
                            Box::new(VmExpr::Number("0".to_string())),
                        ),
                        rounded: false,
                        on_size_error_ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                            "SIZE".to_string(),
                        )])],
                        not_on_size_error_ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                            "BAD".to_string(),
                        )])],
                    },
                    VmProcedureOp::Display(vec![VmExpr::Access(packed_access_path("PACKED"))]),
                    VmProcedureOp::StopRun,
                ],
                transfer: VmControlTransfer::StopRun,
            }],
        };

        runtime.execute_procedure(&procedure).unwrap();

        assert_eq!(runtime.display, vec!["SIZE", "123"]);
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "PACKED"))
                .unwrap(),
            &[0x12, 0x3c]
        );
    }

    #[test]
    fn compute_rounded_writes_numeric_display_to_target_scale() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![VmField {
                name: "AMT".to_string(),
                offset: 0,
                byte_len: 3,
                category: VmCategory::NumericDisplay,
                usage: VmUsage::Display,
                picture: Some(VmPicture {
                    signed: false,
                    digits: 3,
                    scale: 1,
                    char_len: 3,
                }),
            }],
            Vec::new(),
        );
        let mut runtime = runtime_with_scalar(program, "AMT", b"000");
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![VmBasicBlock {
                name: "MAIN".to_string(),
                ops: vec![
                    VmProcedureOp::Compute {
                        target: sort_test_access_path("AMT", 3),
                        expr: VmExpr::Number("1.26".to_string()),
                        rounded: true,
                        on_size_error_ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                            "SIZE".to_string(),
                        )])],
                        not_on_size_error_ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                            "OK".to_string(),
                        )])],
                    },
                    VmProcedureOp::StopRun,
                ],
                transfer: VmControlTransfer::StopRun,
            }],
        };

        runtime.execute_procedure(&procedure).unwrap();

        assert_eq!(runtime.display, vec!["OK"]);
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "AMT"))
                .unwrap(),
            b"013"
        );
    }

    #[test]
    fn compute_rounded_overflow_takes_size_error_without_mutating_target() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![VmField {
                name: "N".to_string(),
                offset: 0,
                byte_len: 2,
                category: VmCategory::NumericDisplay,
                usage: VmUsage::Display,
                picture: Some(VmPicture {
                    signed: false,
                    digits: 2,
                    scale: 0,
                    char_len: 2,
                }),
            }],
            Vec::new(),
        );
        let mut runtime = runtime_with_scalar(program, "N", b"42");
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![VmBasicBlock {
                name: "MAIN".to_string(),
                ops: vec![
                    VmProcedureOp::Compute {
                        target: sort_test_access_path("N", 2),
                        expr: VmExpr::Number("99.6".to_string()),
                        rounded: true,
                        on_size_error_ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                            "SIZE".to_string(),
                        )])],
                        not_on_size_error_ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                            "BAD".to_string(),
                        )])],
                    },
                    VmProcedureOp::StopRun,
                ],
                transfer: VmControlTransfer::StopRun,
            }],
        };

        runtime.execute_procedure(&procedure).unwrap();

        assert_eq!(runtime.display, vec!["SIZE"]);
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "N"))
                .unwrap(),
            b"42"
        );
    }

    #[test]
    fn move_to_packed_target_encodes_or_rejects_without_truncating() {
        let mut runtime = packed_runtime(&[0x00, 0x0c]);
        let target = packed_access_path("PACKED");

        runtime
            .move_value_to_access_path(&VmExpr::Number("-45".to_string()), &target)
            .unwrap();
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "PACKED"))
                .unwrap(),
            &[0x04, 0x5d]
        );

        let err = runtime
            .move_value_to_access_path(&VmExpr::Number("1234".to_string()), &target)
            .expect_err("oversized packed MOVE must reject");
        assert!(
            matches!(err, VmError::Codec { ref name, .. } if name == "PACKED"),
            "{err:?}"
        );
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "PACKED"))
                .unwrap(),
            &[0x04, 0x5d]
        );
    }

    #[test]
    fn move_to_binary_target_encodes_or_rejects_without_truncating() {
        let mut runtime = binary_runtime(&[0x00, 0x00]);
        let target = binary_access_path("BINARY");

        runtime
            .move_value_to_access_path(&VmExpr::Number("-45".to_string()), &target)
            .unwrap();
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "BINARY"))
                .unwrap(),
            &[0xff, 0xd3]
        );

        let err = runtime
            .move_value_to_access_path(&VmExpr::Number("32768".to_string()), &target)
            .expect_err("oversized signed binary MOVE must reject");
        assert!(
            matches!(err, VmError::Codec { ref name, .. } if name == "BINARY"),
            "{err:?}"
        );
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "BINARY"))
                .unwrap(),
            &[0xff, 0xd3]
        );

        let err = runtime
            .move_value_to_access_path(&VmExpr::Number("1.5".to_string()), &target)
            .expect_err("fractional binary MOVE must reject");
        assert!(matches!(err, VmError::InvalidDecimal { .. }), "{err:?}");
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "BINARY"))
                .unwrap(),
            &[0xff, 0xd3]
        );
    }

    #[test]
    fn float_fields_decode_using_runtime_dialect_format() {
        let ibm = float_runtime(
            DialectProfile::ibm_zos(),
            VmUsage::Float32,
            &[0x42, 0x64, 0x00, 0x00],
        );
        let decoded = ibm
            .eval_expr(&VmExpr::Identifier("FLOAT".to_string()))
            .unwrap();
        assert_eq!(decoded.value, VmValue::Float(100.0));

        let gnu = float_runtime(
            DialectProfile::gnucobol(),
            VmUsage::Float32,
            &[0x42, 0xc8, 0x00, 0x00],
        );
        let decoded = gnu
            .eval_expr(&VmExpr::Identifier("FLOAT".to_string()))
            .unwrap();
        assert_eq!(decoded.value, VmValue::Float(100.0));
    }

    #[test]
    fn move_to_float_target_encodes_using_runtime_dialect_format() {
        let mut runtime = float_runtime(
            DialectProfile::ibm_zos(),
            VmUsage::Float32,
            &[0x00, 0x00, 0x00, 0x00],
        );
        let target = float_access_path("FLOAT", 4);

        runtime
            .move_value_to_access_path(&VmExpr::Number("-1".to_string()), &target)
            .unwrap();

        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "FLOAT"))
                .unwrap(),
            encode_ibm_float32(-1.0, Endian::Big).unwrap().as_slice()
        );
        assert_eq!(
            runtime
                .eval_expr(&VmExpr::Identifier("FLOAT".to_string()))
                .unwrap()
                .value,
            VmValue::Float(-1.0)
        );
    }

    #[test]
    fn compute_writes_decimal_result_to_float_target() {
        let mut runtime = float_runtime(
            DialectProfile::gnucobol(),
            VmUsage::Float32,
            &[0x00, 0x00, 0x00, 0x00],
        );
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![VmBasicBlock {
                name: "MAIN".to_string(),
                ops: vec![
                    VmProcedureOp::Compute {
                        target: float_access_path("FLOAT", 4),
                        expr: VmExpr::Add(
                            Box::new(VmExpr::Number("1.5".to_string())),
                            Box::new(VmExpr::Number("2.25".to_string())),
                        ),
                        rounded: false,
                        on_size_error_ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                            "SIZE".to_string(),
                        )])],
                        not_on_size_error_ops: vec![VmProcedureOp::Display(vec![VmExpr::Access(
                            float_access_path("FLOAT", 4),
                        )])],
                    },
                    VmProcedureOp::StopRun,
                ],
                transfer: VmControlTransfer::StopRun,
            }],
        };

        runtime.execute_procedure(&procedure).unwrap();

        assert_eq!(runtime.display, vec!["3.75"]);
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "FLOAT"))
                .unwrap(),
            encode_ieee_float32(3.75, Endian::Big).unwrap().as_slice()
        );
    }

    #[test]
    fn compute_binary_overflow_takes_size_error_without_mutating_target() {
        let mut runtime = binary_runtime(&[0x00, 0x7b]);
        let procedure = VmProcedure {
            entry: "MAIN".to_string(),
            blocks: vec![VmBasicBlock {
                name: "MAIN".to_string(),
                ops: vec![
                    VmProcedureOp::Compute {
                        target: binary_access_path("BINARY"),
                        expr: VmExpr::Number("32768".to_string()),
                        rounded: false,
                        on_size_error_ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                            "SIZE".to_string(),
                        )])],
                        not_on_size_error_ops: vec![VmProcedureOp::Display(vec![VmExpr::Literal(
                            "BAD".to_string(),
                        )])],
                    },
                    VmProcedureOp::Display(vec![VmExpr::Access(binary_access_path("BINARY"))]),
                    VmProcedureOp::StopRun,
                ],
                transfer: VmControlTransfer::StopRun,
            }],
        };

        runtime.execute_procedure(&procedure).unwrap();

        assert_eq!(runtime.display, vec!["SIZE", "123"]);
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "BINARY"))
                .unwrap(),
            &[0x00, 0x7b]
        );
    }

    #[test]
    fn runtime_group_condition_name_reads_and_sets_child_cells() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![VmField {
                name: "PAIR".to_string(),
                offset: 0,
                byte_len: 2,
                category: VmCategory::Group,
                usage: VmUsage::Group,
                picture: None,
            }],
            vec![VmConditionName {
                name: "PAIR_OK".to_string(),
                parent: "PAIR".to_string(),
                values: vec![VmConditionValue::Single("YZ".to_string())],
            }],
        );
        let mut pool = StoragePool::default();
        pool.define_cell(StorageKey::scalar("MAIN", "P1"), b" ".to_vec())
            .unwrap();
        pool.define_cell(StorageKey::scalar("MAIN", "P2"), b" ".to_vec())
            .unwrap();
        let mut runtime = VmRuntime::new(program, pool);
        runtime.bind_storage_cell("P1", StorageKey::scalar("MAIN", "P1"));
        runtime.bind_storage_cell("P2", StorageKey::scalar("MAIN", "P2"));
        runtime.bind_group_storage("PAIR", vec!["P1".to_string(), "P2".to_string()]);

        runtime.set_condition_name_at("PAIR_OK").unwrap();
        assert!(runtime.eval_condition_name_runtime("PAIR_OK").unwrap());
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "P1"))
                .unwrap(),
            b"Y"
        );
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "P2"))
                .unwrap(),
            b"Z"
        );
    }

    #[test]
    fn runtime_group_condition_name_uses_declared_view_not_current_group_binding() {
        let program = VmProgram::with_declared_views(
            DialectProfile::ibm_zos(),
            vec![VmField {
                name: "PAIR".to_string(),
                offset: 0,
                byte_len: 2,
                category: VmCategory::Group,
                usage: VmUsage::Group,
                picture: None,
            }],
            vec![VmConditionName {
                name: "PAIR_OK".to_string(),
                parent: "PAIR".to_string(),
                values: vec![VmConditionValue::Single("YZ".to_string())],
            }],
            vec![VmDeclaredView {
                condition: "PAIR_OK".to_string(),
                parent: "PAIR".to_string(),
                children: vec!["P1".to_string(), "P2".to_string()],
            }],
        );
        let mut pool = StoragePool::default();
        for (name, bytes) in [
            ("P1", b" ".to_vec()),
            ("P2", b" ".to_vec()),
            ("Q1", b"Q".to_vec()),
            ("Q2", b"Q".to_vec()),
        ] {
            pool.define_cell(StorageKey::scalar("MAIN", name), bytes)
                .unwrap();
        }
        let mut runtime = VmRuntime::new(program, pool);
        for name in ["P1", "P2", "Q1", "Q2"] {
            runtime.bind_storage_cell(name, StorageKey::scalar("MAIN", name));
        }
        runtime.bind_group_storage("PAIR", vec!["Q1".to_string(), "Q2".to_string()]);

        runtime.set_condition_name_at("PAIR_OK").unwrap();

        assert!(runtime.eval_condition_name_runtime("PAIR_OK").unwrap());
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "P1"))
                .unwrap(),
            b"Y"
        );
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "P2"))
                .unwrap(),
            b"Z"
        );
        assert_eq!(
            runtime
                .storage_pool
                .bytes(&StorageKey::scalar("MAIN", "Q1"))
                .unwrap(),
            b"Q"
        );
    }

    #[test]
    fn runtime_odo_resize_uses_static_child_value_templates() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![VmField {
                name: "ITEM".to_string(),
                offset: 0,
                byte_len: 1,
                category: VmCategory::Alphanumeric,
                usage: VmUsage::Display,
                picture: None,
            }],
            vec![VmConditionName {
                name: "IS_A".to_string(),
                parent: "ITEM".to_string(),
                values: vec![VmConditionValue::Single("A".to_string())],
            }],
        );
        let mut pool = StoragePool::default();
        let depending_on = StorageKey::scalar("MAIN", "ODO_COUNT");
        pool.define_cell(depending_on.clone(), b"0".to_vec())
            .unwrap();
        let mut templates = BTreeMap::new();
        templates.insert("ITEM".to_string(), b"A".to_vec());
        pool.define_odo_table_with_templates("MAIN", "TAB", depending_on, 1, 0, 3, 0, templates)
            .unwrap();
        let mut runtime = VmRuntime::new(program, pool);
        runtime.define_odo("TAB", "ODO_COUNT", 0, 3, 0).unwrap();
        runtime.bind_occurs_storage_cell("ITEM", "MAIN", "ITEM");

        runtime.set_odo_active("TAB", 3).unwrap();
        let value = runtime
            .eval_expr(&VmExpr::Access(VmAccessPath {
                target: "ITEM".to_string(),
                condition_name: Some("IS_A".to_string()),
                subscripts: vec![VmSubscript {
                    expr: Box::new(VmExpr::Number("2".to_string())),
                    stride: 1,
                    min: 1,
                    max: 3,
                    depending_on: Some("ODO_COUNT".to_string()),
                    index_name: None,
                }],
                reference_modifier: None,
                result_len: Some(1),
            }))
            .unwrap();

        assert_eq!(value.value, VmValue::Bool(true));
    }

    #[test]
    fn national_fields_decode_as_utf16_text() {
        let program = VmProgram::new(
            DialectProfile::ibm_zos(),
            vec![VmField {
                name: "N".to_string(),
                offset: 0,
                byte_len: 4,
                category: VmCategory::National,
                usage: VmUsage::National,
                picture: None,
            }],
            Vec::new(),
        );
        let bytes = [0x00, b'A', 0x00, b'B'];
        let value = program
            .eval_expr(&bytes, &VmExpr::Identifier("N".to_string()))
            .unwrap();
        assert_eq!(value.value, VmValue::NationalText("AB".to_string()));
    }
}
