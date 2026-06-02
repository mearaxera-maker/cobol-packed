pub use cobol_record::RecordPlan;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

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
    pub is_common: bool,
    pub is_initial: bool,
    pub dialect: CobolDialect,
    pub dialect_profile: DialectProfileIr,
    pub data_items: Vec<DataItemIr>,
    pub storage: StoragePlanIr,
    pub paragraphs: Vec<ParagraphIr>,
    pub declaratives: Vec<DeclarativeIr>,
    pub control_flow: ControlFlowIr,
    pub procedure_cfg: ProcedureCfgIr,
    pub files: Vec<FileIr>,
    pub same_record_areas: Vec<SameRecordAreaIr>,
    pub rerun_clauses: Vec<RerunIr>,
    pub indexes: Vec<IndexItemIr>,
    pub odo_descriptors: Vec<OdoDescriptorIr>,
    pub program_units: Vec<ProgramUnitIr>,
    pub linkage_signature: LinkageSignatureIr,
    pub semantic: SemanticModelIr,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SameRecordAreaIr {
    pub files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RerunIr {
    pub checkpoint_file: String,
    pub every_records: usize,
    pub watched_file: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompilationIr {
    pub entry_program: String,
    pub programs: Vec<ProgramIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkageSignatureIr {
    pub program: String,
    pub parameters: Vec<LinkageParamIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkageParamIr {
    pub name: String,
    pub qualified_name: String,
    pub category: ValueCategoryIr,
    pub usage: UsageIr,
}

impl ProgramIr {
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
    }

    pub fn diagnostics_by_severity(&self, severity: Severity) -> Vec<&Diagnostic> {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == severity)
            .collect()
    }

    pub fn shape_diagnostics(&self) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        if self.name.trim().is_empty() {
            diagnostics.push(shape_error("IR001", "program name is empty"));
        }

        push_duplicate_name_diagnostics(
            &mut diagnostics,
            "IR002",
            "duplicate paragraph",
            self.paragraphs
                .iter()
                .map(|paragraph| paragraph.name.as_str()),
        );
        push_duplicate_name_diagnostics(
            &mut diagnostics,
            "IR003",
            "duplicate file",
            self.files.iter().map(|file| file.name.as_str()),
        );
        push_duplicate_name_diagnostics(
            &mut diagnostics,
            "IR004",
            "duplicate storage cell",
            self.storage
                .storage_cells
                .iter()
                .map(|cell| cell.key.as_str()),
        );
        push_duplicate_name_diagnostics(
            &mut diagnostics,
            "IR005",
            "duplicate storage binding",
            self.storage
                .storage_bindings
                .iter()
                .map(|(name, _)| name.as_str()),
        );

        let paragraph_names = self
            .paragraphs
            .iter()
            .map(|paragraph| normalize_ir_name(&paragraph.name))
            .collect::<BTreeSet<_>>();
        let mut block_ids = BTreeSet::new();
        let mut block_labels = BTreeSet::new();

        for block in &self.procedure_cfg.blocks {
            if !block_ids.insert(block.id) {
                diagnostics.push(shape_error(
                    "IR006",
                    format!("duplicate basic block id {}", block.id),
                ));
            }
            let label = normalize_ir_name(&block.label);
            if !block_labels.insert(label) {
                diagnostics.push(shape_error(
                    "IR007",
                    format!("duplicate basic block label {}", block.label),
                ));
            }
        }

        if let Some(entry) = &self.procedure_cfg.entry {
            push_missing_target_diagnostic(
                &mut diagnostics,
                "IR008",
                "procedure CFG entry",
                entry,
                &paragraph_names,
                &block_labels,
            );
        }

        for block in &self.procedure_cfg.blocks {
            push_missing_target_diagnostic(
                &mut diagnostics,
                "IR009",
                "basic block paragraph",
                &block.paragraph,
                &paragraph_names,
                &block_labels,
            );
            match &block.transfer {
                ControlTransferIr::FallThrough(Some(target))
                | ControlTransferIr::NextSentence {
                    target: Some(target),
                } => push_missing_target_diagnostic(
                    &mut diagnostics,
                    "IR010",
                    "control-flow target",
                    target,
                    &paragraph_names,
                    &block_labels,
                ),
                ControlTransferIr::Perform(perform) => {
                    push_missing_target_diagnostic(
                        &mut diagnostics,
                        "IR011",
                        "perform target",
                        &perform.target,
                        &paragraph_names,
                        &block_labels,
                    );
                    if let Some(through) = &perform.through {
                        push_missing_target_diagnostic(
                            &mut diagnostics,
                            "IR012",
                            "perform through target",
                            through,
                            &paragraph_names,
                            &block_labels,
                        );
                    }
                }
                ControlTransferIr::GoTo(go_to) => push_missing_target_diagnostic(
                    &mut diagnostics,
                    "IR013",
                    "GO TO target",
                    &go_to.target,
                    &paragraph_names,
                    &block_labels,
                ),
                ControlTransferIr::FallThrough(None)
                | ControlTransferIr::NextSentence { target: None }
                | ControlTransferIr::StopRun => {}
            }
        }

        for target in &self.procedure_cfg.next_sentence_targets {
            push_missing_target_diagnostic(
                &mut diagnostics,
                "IR014",
                "NEXT SENTENCE source block",
                &target.source_block,
                &paragraph_names,
                &block_labels,
            );
            if let Some(target) = &target.target {
                push_missing_target_diagnostic(
                    &mut diagnostics,
                    "IR015",
                    "NEXT SENTENCE target",
                    target,
                    &paragraph_names,
                    &block_labels,
                );
            }
        }

        diagnostics
    }
}

fn shape_error(code: &'static str, message: impl Into<String>) -> Diagnostic {
    Diagnostic::error(code, message, SourceSpan::generated())
}

fn push_duplicate_name_diagnostics<'a>(
    diagnostics: &mut Vec<Diagnostic>,
    code: &'static str,
    label: &'static str,
    names: impl IntoIterator<Item = &'a str>,
) {
    let mut seen = BTreeSet::new();
    let mut reported = BTreeSet::new();
    for name in names {
        let normalized = normalize_ir_name(name);
        if !seen.insert(normalized.clone()) && reported.insert(normalized) {
            diagnostics.push(shape_error(code, format!("{label} {name}")));
        }
    }
}

fn push_missing_target_diagnostic(
    diagnostics: &mut Vec<Diagnostic>,
    code: &'static str,
    label: &'static str,
    target: &str,
    paragraph_names: &BTreeSet<String>,
    block_labels: &BTreeSet<String>,
) {
    let normalized = normalize_ir_name(target);
    if !paragraph_names.contains(&normalized) && !block_labels.contains(&normalized) {
        diagnostics.push(shape_error(code, format!("{label} {target} is missing")));
    }
}

fn normalize_ir_name(name: &str) -> String {
    name.trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_end_matches('.')
        .replace('-', "_")
        .to_ascii_uppercase()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CobolDialect {
    Ibm,
    GnuCobol,
    MicroFocus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DialectProfileIr {
    pub dialect: CobolDialect,
    pub source_encoding: String,
    pub default_codepage: String,
    pub binary_endian: String,
    pub binary_sizing: String,
    pub numproc: String,
    pub truncation: String,
    pub arithmetic: String,
    pub sync_profile: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SemanticModelIr {
    pub references: Vec<ReferenceResolutionIr>,
    pub resolved_data_refs: Vec<ResolvedDataRefIr>,
    pub conditions: Vec<ConditionAnalysisIr>,
    pub evaluates: Vec<EvaluateAnalysisIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceResolutionIr {
    pub raw: String,
    pub normalized: String,
    pub paragraph: String,
    pub statement_index: usize,
    pub role: ReferenceRoleIr,
    pub status: ReferenceResolutionStatusIr,
    pub target: Option<String>,
    pub candidates: Vec<String>,
    pub category: Option<ValueCategoryIr>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedDataRefIr {
    pub raw: String,
    pub normalized: String,
    pub target: Option<String>,
    pub condition_name_target: Option<String>,
    pub subscripts: Vec<String>,
    pub reference_modifier: Option<ReferenceModifierIr>,
    pub category: Option<ValueCategoryIr>,
    pub byte_range: Option<ByteRangeIr>,
    pub layout_id: Option<String>,
    pub in_redefines: bool,
    pub in_occurs: bool,
    pub in_odo: bool,
    pub status: ReferenceResolutionStatusIr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByteRangeIr {
    pub offset: usize,
    pub length: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReferenceRoleIr {
    Display,
    Source,
    Target,
    ComputeTarget,
    ArithmeticSource,
    ArithmeticTarget,
    ConditionOperand,
    ProcedureTarget,
    ProcedureArgument,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReferenceResolutionStatusIr {
    Resolved,
    Missing,
    Ambiguous,
    InvalidSubscript,
    UnsupportedDynamic,
    UnsupportedRedefines,
    UnsupportedReferenceModification,
    UnsupportedCategory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageAreaIr {
    WorkingStorage,
    LocalStorage,
    Linkage,
    FileSection,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValueCategoryIr {
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
    ConditionName,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConditionAnalysisIr {
    pub raw: String,
    pub paragraph: String,
    pub statement_index: usize,
    pub status: ConditionStatusIr,
    pub tree: Option<ConditionIr>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConditionStatusIr {
    Parsed,
    ParseError,
    SemanticError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluateAnalysisIr {
    pub raw: String,
    pub paragraph: String,
    pub statement_index: usize,
    pub status: ConditionStatusIr,
    pub evaluate: Option<EvaluateIr>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConditionIr {
    Relation {
        left: ConditionOperandIr,
        op: RelOpIr,
        right: ConditionOperandIr,
    },
    ClassTest {
        operand: ConditionOperandIr,
        class: ClassTestIr,
        negated: bool,
    },
    SignTest {
        operand: ConditionOperandIr,
        sign: SignTestIr,
        negated: bool,
    },
    ConditionName {
        reference: DataRefIr,
    },
    Not(Box<ConditionIr>),
    And(Box<ConditionIr>, Box<ConditionIr>),
    Or(Box<ConditionIr>, Box<ConditionIr>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConditionOperandIr {
    Identifier(DataRefIr),
    Literal(String),
    Number(String),
    Figurative(FigurativeConstantIr),
    AllLiteral(String),
    Function(FunctionOperandIr),
    Bool(bool),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExprIr {
    Access(ResolvedAccessPathIr),
    Literal(String),
    Number(String),
    Figurative(FigurativeConstantIr),
    AllLiteral(String),
    Bool(bool),
    Function(FunctionOperandIr),
    Binary {
        left: Box<ExprIr>,
        op: ExprBinaryOpIr,
        right: Box<ExprIr>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExprBinaryOpIr {
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubscriptExprIr {
    Literal(String),
    DataRef(DataRefIr),
    Add(Box<SubscriptExprIr>, Box<SubscriptExprIr>),
    Subtract(Box<SubscriptExprIr>, Box<SubscriptExprIr>),
    Multiply(Box<SubscriptExprIr>, Box<SubscriptExprIr>),
    Divide(Box<SubscriptExprIr>, Box<SubscriptExprIr>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FunctionOperandIr {
    Length(Box<ConditionOperandIr>),
    Ord(Box<ConditionOperandIr>),
    Numval(Box<ConditionOperandIr>),
    UserDefined {
        name: String,
        args: Vec<ConditionOperandIr>,
        raw: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConditionNameTargetIr {
    pub name: String,
    pub parent: String,
    pub qualified_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedAccessPathIr {
    pub raw: String,
    pub target: String,
    pub condition_name_target: Option<ConditionNameTargetIr>,
    pub subscripts: Vec<SubscriptExprIr>,
    pub reference_modifier: Option<ReferenceModifierIr>,
    pub category: ValueCategoryIr,
    pub byte_range: Option<ByteRangeIr>,
    pub layout_id: Option<String>,
    pub in_redefines: bool,
    pub in_occurs: bool,
    pub in_odo: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelOpIr {
    Equal,
    NotEqual,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClassTestIr {
    Numeric,
    Alphabetic,
    AlphabeticUpper,
    AlphabeticLower,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignTestIr {
    Positive,
    Negative,
    Zero,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FigurativeConstantIr {
    Zero,
    Space,
    HighValue,
    LowValue,
    Quote,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluateIr {
    pub raw: String,
    pub subjects: Vec<EvaluateSubjectIr>,
    pub arms: Vec<EvaluateArmIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvaluateSubjectIr {
    Operand(ConditionOperandIr),
    Condition(ConditionIr),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvaluateArmIr {
    pub raw: String,
    pub patterns: Vec<EvaluatePatternIr>,
    pub statements: Vec<StatementIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvaluatePatternIr {
    Any,
    Operand(ConditionOperandIr),
    Range {
        start: ConditionOperandIr,
        end: ConditionOperandIr,
    },
    Condition(ConditionIr),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataItemIr {
    pub level: u8,
    pub name: String,
    pub rust_name: String,
    pub picture: Option<String>,
    pub picture_ast: Option<PicIr>,
    pub usage: UsageIr,
    pub occurs: Option<OccursIr>,
    pub redefines: Option<String>,
    pub parent: Option<String>,
    pub qualified_name: String,
    pub path: Vec<String>,
    pub addressable: bool,
    pub storage_area: StorageAreaIr,
    pub external: bool,
    pub value_category: ValueCategoryIr,
    pub layout_id: Option<String>,
    pub offset: Option<usize>,
    pub byte_len: Option<usize>,
    pub sync: bool,
    pub value: Option<String>,
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
    National,
    Dbcs,
    Alphanumeric,
    Group,
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OccursIr {
    pub min: usize,
    pub max: usize,
    pub depending_on: Option<String>,
    pub indexed_by: Vec<String>,
    pub keys: Vec<OccursKeyIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OccursKeyIr {
    pub direction: OccursKeyDirectionIr,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OccursKeyDirectionIr {
    Ascending,
    Descending,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PicIr {
    pub raw: String,
    pub category: PicCategoryIr,
    pub signed: bool,
    pub digits: usize,
    pub scale: usize,
    pub char_len: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PicCategoryIr {
    Alphanumeric,
    Alphabetic,
    NumericDisplay,
    NumericEdited,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConditionNameIr {
    pub name: String,
    pub rust_name: String,
    pub parent: String,
    pub values: Vec<String>,
    pub value_set: Vec<ConditionValueIr>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConditionValueIr {
    Single(String),
    Range { start: String, end: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageCellIr {
    pub key: String,
    pub item_id: String,
    pub byte_len: usize,
    pub usage: UsageIr,
    pub category: ValueCategoryIr,
    pub picture: Option<PicIr>,
    pub initial_bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageBindingIr {
    Cell {
        key: String,
    },
    OccursCell {
        program: String,
        item: String,
        subscripts: Vec<SubscriptExprIr>,
    },
    Group {
        children: Vec<String>,
    },
    RefMod {
        base: Box<StorageBindingIr>,
        start: Box<ExprIr>,
        length: Option<Box<ExprIr>>,
    },
    ConditionName {
        parent: Box<StorageBindingIr>,
        condition: ConditionNameTargetIr,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OdoTemplateIr {
    pub table: String,
    pub depending_on: String,
    pub min: usize,
    pub max: usize,
    pub fields: Vec<(String, Vec<u8>)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StoragePlanIr {
    pub record_length: usize,
    pub items: Vec<StorageItemIr>,
    pub redefines: Vec<RedefinesIr>,
    pub renames: Vec<RenamesIr>,
    pub condition_names: Vec<ConditionNameIr>,
    pub storage_cells: Vec<StorageCellIr>,
    pub storage_bindings: Vec<(String, StorageBindingIr)>,
    pub odo_templates: Vec<OdoTemplateIr>,
    pub record_plan: RecordPlan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenamesIr {
    pub renaming_item: String,
    pub targets: Vec<String>,
    pub offset: usize,
    pub byte_len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageItemIr {
    pub name: String,
    pub qualified_name: String,
    pub path: Vec<String>,
    pub offset: usize,
    pub byte_len: usize,
    pub usage: UsageIr,
    pub storage_area: StorageAreaIr,
    pub external: bool,
    pub value_category: ValueCategoryIr,
    pub picture: Option<PicIr>,
    pub occurs: Option<OccursIr>,
    pub redefines: Option<String>,
    pub parent: Option<String>,
    pub addressable: bool,
    pub layout_id: String,
    pub sync: bool,
    pub value: Option<String>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedefinesIr {
    pub redefining_item: String,
    pub base_item: String,
    pub offset: usize,
    pub byte_len: usize,
    pub base_byte_len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileIr {
    pub name: String,
    pub kind: FileKindIr,
    pub record_name: Option<String>,
    pub assign: Option<String>,
    pub assign_is_literal: bool,
    pub organization: Option<String>,
    pub access_mode: Option<String>,
    pub file_status: Option<String>,
    pub open_mode: Option<String>,
    pub linage: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileKindIr {
    Fd,
    Sd,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProcedureCfgIr {
    pub entry: Option<String>,
    pub blocks: Vec<BasicBlockIr>,
    #[serde(default)]
    pub next_sentence_targets: Vec<NextSentenceTargetIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BasicBlockIr {
    pub id: usize,
    pub label: String,
    pub paragraph: String,
    pub sentence_index: usize,
    pub statements: Vec<StatementIr>,
    pub transfer: ControlTransferIr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlTransferIr {
    FallThrough(Option<String>),
    NextSentence { target: Option<String> },
    Perform(Box<PerformIr>),
    GoTo(GoToIr),
    StopRun,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NextSentenceTargetIr {
    pub source_block: String,
    pub target: Option<String>,
    pub path: Vec<StatementPathElementIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatementPathElementIr {
    Statement(usize),
    Branch(StatementBranchIr),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatementBranchIr {
    Then,
    Else,
    OnSizeError,
    NotOnSizeError,
    EvaluateArm(usize),
    AtEnd,
    NotAtEnd,
    OnException,
    InvalidKey,
    NotInvalidKey,
    SearchWhen(usize),
    SearchAllBody,
    OnOverflow,
    NotOnOverflow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerformIr {
    pub target: String,
    pub through: Option<String>,
    pub varying: Option<String>,
    pub until: Option<String>,
    pub times: Option<OperandIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerformVaryingIr {
    pub target: DataRefIr,
    pub from: OperandIr,
    pub by: OperandIr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoToIr {
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CallTargetIr {
    Literal(String),
    Identifier(DataRefIr),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallIr {
    pub target: CallTargetIr,
    pub using: Vec<DataRefIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexItemIr {
    pub name: String,
    pub table: String,
    pub occurrence_min: usize,
    pub occurrence_max: usize,
    pub representation: IndexRepresentationIr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexRepresentationIr {
    Occurrence,
    Displacement,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchIr {
    pub table: String,
    pub index: Option<String>,
    pub at_end: Vec<StatementIr>,
    pub whens: Vec<SearchWhenIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchWhenIr {
    pub condition: ConditionIr,
    pub statements: Vec<StatementIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchAllIr {
    pub table: String,
    pub index: Option<String>,
    pub declared_key: Option<SearchAllKeyIr>,
    pub key_condition: ConditionIr,
    pub at_end: Vec<StatementIr>,
    pub statements: Vec<StatementIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchAllKeyIr {
    pub direction: OccursKeyDirectionIr,
    pub name: String,
    pub qualified_name: String,
    pub children: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetIndexIr {
    pub index: String,
    pub operation: SetIndexOperationIr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetIndexOperationIr {
    To(SubscriptExprIr),
    UpBy(SubscriptExprIr),
    DownBy(SubscriptExprIr),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcedureRangeIr {
    pub target: String,
    pub through: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortDirectionIr {
    Ascending,
    Descending,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortKeyIr {
    pub direction: SortDirectionIr,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortProcedureIr {
    pub file: String,
    pub key: Option<SortKeyIr>,
    pub input_range: Option<ProcedureRangeIr>,
    pub output_range: ProcedureRangeIr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseSortRecordIr {
    pub record: DataRefIr,
    pub from: Option<DataRefIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReturnSortRecordIr {
    pub file: String,
    pub into: Option<DataRefIr>,
    pub at_end_ops: Vec<StatementIr>,
    pub not_at_end_ops: Vec<StatementIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadFileIr {
    pub file: String,
    pub into: Option<DataRefIr>,
    pub at_end_ops: Vec<StatementIr>,
    pub not_at_end_ops: Vec<StatementIr>,
    pub on_exception_ops: Vec<StatementIr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileOpenModeIr {
    Input,
    Output,
    Io,
    Extend,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenFileIr {
    pub file: String,
    pub mode: FileOpenModeIr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WriteAdvancingIr {
    None,
    BeforeLines(usize),
    AfterLines(usize),
    BeforePage,
    AfterPage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteFileIr {
    pub record: DataRefIr,
    pub advancing: WriteAdvancingIr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RewriteFileIr {
    pub record: DataRefIr,
    pub invalid_key_ops: Vec<StatementIr>,
    pub not_invalid_key_ops: Vec<StatementIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteFileIr {
    pub file: String,
    pub invalid_key_ops: Vec<StatementIr>,
    pub not_invalid_key_ops: Vec<StatementIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloseFileIr {
    pub file: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectLikeIr {
    pub subject: DataRefIr,
    pub tally: Option<InspectTallyIr>,
    pub replacing: Option<InspectReplacingIr>,
    pub converting: Option<InspectConvertingIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectTallyIr {
    pub target: DataRefIr,
    pub pattern: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectReplacingIr {
    pub pattern: String,
    pub replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectConvertingIr {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StringOpIr {
    pub pieces: Vec<StringPieceIr>,
    pub target: DataRefIr,
    pub pointer: Option<DataRefIr>,
    pub on_overflow_ops: Vec<StatementIr>,
    pub not_on_overflow_ops: Vec<StatementIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StringPieceIr {
    pub source: OperandIr,
    pub delimiter: StringDelimiterIr,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StringDelimiterIr {
    Size,
    Literal { value: String, all: bool },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnstringOpIr {
    pub source: OperandIr,
    pub delimiter: StringDelimiterIr,
    pub targets: Vec<UnstringTargetIr>,
    pub pointer: Option<DataRefIr>,
    pub tallying: Option<DataRefIr>,
    pub on_overflow_ops: Vec<StatementIr>,
    pub not_on_overflow_ops: Vec<StatementIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnstringTargetIr {
    pub target: DataRefIr,
    pub count: Option<DataRefIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OdoDescriptorIr {
    pub table: String,
    pub depending_on: String,
    pub min: usize,
    pub max: usize,
    pub stride: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgramUnitIr {
    pub name: String,
    pub parent: Option<String>,
    pub is_common: bool,
    pub is_initial: bool,
    pub contained_programs: Vec<String>,
    pub global_items: Vec<String>,
    pub external_items: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParagraphIr {
    pub name: String,
    pub rust_name: String,
    pub statements: Vec<StatementIr>,
    pub sentences: Vec<ProcedureSentenceIr>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcedureSentenceIr {
    pub statements: Vec<StatementIr>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclarativeIr {
    pub name: String,
    pub trigger: DeclarativeTriggerIr,
    pub statements: Vec<StatementIr>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeclarativeTriggerIr {
    FileError { file: String },
    Debugging { paragraph: String },
    Unsupported { raw: String },
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatementIr {
    Display(Vec<OperandIr>),
    Move {
        source: OperandIr,
        target: DataRefIr,
    },
    MoveCorresponding {
        source: DataRefIr,
        target: DataRefIr,
    },
    Add {
        source: OperandIr,
        target: DataRefIr,
    },
    Subtract {
        source: OperandIr,
        target: DataRefIr,
    },
    Multiply {
        source: OperandIr,
        target: DataRefIr,
    },
    Divide {
        source: OperandIr,
        target: DataRefIr,
    },
    Compute {
        target: DataRefIr,
        expression: String,
        on_size_error_ops: Vec<StatementIr>,
        not_on_size_error_ops: Vec<StatementIr>,
    },
    NextSentence,
    If {
        condition: String,
        condition_tree: Option<ConditionIr>,
        then_statements: Vec<StatementIr>,
        else_statements: Vec<StatementIr>,
    },
    Evaluate(EvaluateIr),
    Search(SearchIr),
    SearchAll(SearchAllIr),
    SetCondition {
        condition: DataRefIr,
        value: bool,
    },
    SetIndex {
        index: String,
        operation: SetIndexOperationIr,
    },
    Perform {
        target: String,
        through: Option<String>,
        varying: Option<String>,
        varying_ir: Option<Box<PerformVaryingIr>>,
        until: Option<String>,
        until_tree: Option<Box<ConditionIr>>,
        times: Option<OperandIr>,
    },
    GoTo(String),
    ComputedGoTo {
        targets: Vec<String>,
        depending_on: OperandIr,
    },
    Alter {
        paragraph: String,
        target: String,
    },
    Call(Box<CallIr>),
    OpenFile(OpenFileIr),
    ReadFile(ReadFileIr),
    WriteFile(WriteFileIr),
    RewriteFile(RewriteFileIr),
    DeleteFile(DeleteFileIr),
    CloseFile(CloseFileIr),
    SortProcedure(SortProcedureIr),
    ReleaseSortRecord(ReleaseSortRecordIr),
    ReturnSortRecord(ReturnSortRecordIr),
    InspectLike(InspectLikeIr),
    StringOp(StringOpIr),
    UnstringOp(UnstringOpIr),
    ReadyTrace,
    ResetTrace,
    Continue,
    StopRun,
    Unsupported {
        keyword: String,
        raw: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperandIr {
    Identifier(DataRefIr),
    Literal(String),
    Number(String),
    Function(FunctionOperandIr),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataRefIr {
    pub raw: String,
    pub normalized: String,
    pub parts: Vec<String>,
    pub subscripts: Vec<String>,
    pub reference_modifier: Option<ReferenceModifierIr>,
}

impl DataRefIr {
    pub fn simple(name: impl Into<String>) -> Self {
        let raw = name.into();
        Self {
            normalized: raw.clone(),
            parts: vec![raw.clone()],
            raw,
            subscripts: Vec::new(),
            reference_modifier: None,
        }
    }

    pub fn is_subscripted(&self) -> bool {
        !self.subscripts.is_empty()
    }

    pub fn has_reference_modifier(&self) -> bool {
        self.reference_modifier.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceModifierIr {
    pub start: String,
    pub length: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ControlFlowIr {
    pub paragraphs: Vec<ParagraphFlowIr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParagraphFlowIr {
    pub name: String,
    pub index: usize,
    pub edges: Vec<ControlFlowEdgeIr>,
    pub can_fall_through: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlFlowEdgeIr {
    pub kind: ControlFlowEdgeKindIr,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlFlowEdgeKindIr {
    FallThrough,
    Perform,
    GoTo,
    StopRun,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_profile() -> DialectProfileIr {
        DialectProfileIr {
            dialect: CobolDialect::Ibm,
            source_encoding: "ASCII".to_string(),
            default_codepage: "IBM-037".to_string(),
            binary_endian: "big".to_string(),
            binary_sizing: "ibm".to_string(),
            numproc: "nopfd".to_string(),
            truncation: "std".to_string(),
            arithmetic: "native".to_string(),
            sync_profile: "ibm".to_string(),
        }
    }

    fn paragraph(name: &str) -> ParagraphIr {
        ParagraphIr {
            name: name.to_string(),
            rust_name: name.to_ascii_lowercase(),
            statements: Vec::new(),
            sentences: Vec::new(),
            span: SourceSpan::generated(),
        }
    }

    fn block(id: usize, label: &str, transfer: ControlTransferIr) -> BasicBlockIr {
        BasicBlockIr {
            id,
            label: label.to_string(),
            paragraph: label.to_string(),
            sentence_index: 0,
            statements: Vec::new(),
            transfer,
        }
    }

    fn program() -> ProgramIr {
        ProgramIr {
            name: "MAIN".to_string(),
            is_common: false,
            is_initial: false,
            dialect: CobolDialect::Ibm,
            dialect_profile: test_profile(),
            data_items: Vec::new(),
            storage: StoragePlanIr::default(),
            paragraphs: vec![paragraph("MAIN")],
            declaratives: Vec::new(),
            control_flow: ControlFlowIr::default(),
            procedure_cfg: ProcedureCfgIr {
                entry: Some("MAIN".to_string()),
                blocks: vec![block(0, "MAIN", ControlTransferIr::StopRun)],
                next_sentence_targets: Vec::new(),
            },
            files: Vec::new(),
            same_record_areas: Vec::new(),
            rerun_clauses: Vec::new(),
            indexes: Vec::new(),
            odo_descriptors: Vec::new(),
            program_units: Vec::new(),
            linkage_signature: LinkageSignatureIr {
                program: "MAIN".to_string(),
                parameters: Vec::new(),
            },
            semantic: SemanticModelIr::default(),
            diagnostics: Vec::new(),
        }
    }

    fn codes(diagnostics: &[Diagnostic]) -> Vec<&str> {
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect()
    }

    #[test]
    fn has_errors_and_diagnostics_by_severity_report_existing_diagnostics() {
        let mut ir = program();
        ir.diagnostics.push(Diagnostic::warning(
            "W001",
            "heads up",
            SourceSpan::generated(),
        ));
        ir.diagnostics
            .push(Diagnostic::error("E001", "broken", SourceSpan::generated()));

        assert!(ir.has_errors());
        assert_eq!(ir.diagnostics_by_severity(Severity::Warning).len(), 1);
        assert_eq!(ir.diagnostics_by_severity(Severity::Error).len(), 1);
    }

    #[test]
    fn shape_diagnostics_accepts_a_minimal_well_formed_program() {
        assert!(program().shape_diagnostics().is_empty());
    }

    #[test]
    fn shape_diagnostics_reports_duplicates_and_missing_cfg_targets() {
        let mut ir = program();
        ir.paragraphs.push(paragraph("main"));
        ir.files.push(FileIr {
            name: "INPUT-FILE".to_string(),
            kind: FileKindIr::Fd,
            record_name: None,
            assign: None,
            assign_is_literal: false,
            organization: None,
            access_mode: None,
            file_status: None,
            open_mode: None,
            linage: None,
        });
        ir.files.push(FileIr {
            name: "INPUT_FILE".to_string(),
            kind: FileKindIr::Fd,
            record_name: None,
            assign: None,
            assign_is_literal: false,
            organization: None,
            access_mode: None,
            file_status: None,
            open_mode: None,
            linage: None,
        });
        ir.procedure_cfg.blocks.push(BasicBlockIr {
            id: 0,
            label: "DUP".to_string(),
            paragraph: "MISSING-PARA".to_string(),
            sentence_index: 0,
            statements: Vec::new(),
            transfer: ControlTransferIr::GoTo(GoToIr {
                target: "NO-SUCH-PARA".to_string(),
            }),
        });

        let diagnostics = ir.shape_diagnostics();
        let codes = codes(&diagnostics);

        assert!(codes.contains(&"IR002"));
        assert!(codes.contains(&"IR003"));
        assert!(codes.contains(&"IR006"));
        assert!(codes.contains(&"IR009"));
        assert!(codes.contains(&"IR013"));
    }
}
