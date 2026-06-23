use cobol_ir::{
    AcceptIr, BasicBlockIr, ByteRangeIr, CallIr, CallTargetIr, CancelIr, ChainIr, ClassTestIr,
    CloseFileIr, CobolDialect, CommunicationControlIr, CommunicationMessageIr, ConditionAnalysisIr,
    ConditionIr, ConditionNameIr, ConditionOperandIr, ConditionStatusIr, ConditionValueIr,
    ControlFlowEdgeIr, ControlFlowEdgeKindIr, ControlFlowIr, ControlTransferIr, DataItemIr,
    DataRefIr, DeclarativeIr, DeclarativeTriggerIr, DeleteFileIr, Diagnostic, DialectProfileIr,
    EnterLanguageIr, EntryIr, EvaluateAnalysisIr, EvaluateArmIr, EvaluateIr, EvaluatePatternIr,
    EvaluateSubjectIr, FigurativeConstantIr, FileIr, FileKindIr, FileOpenModeIr, FunctionOperandIr,
    GenerateReportIr, GoToIr, IndexItemIr, IndexRepresentationIr, InitializeIr,
    InspectConvertingIr, InspectLikeIr, InspectReplacingIr, InspectTallyIr, LinkageParamIr,
    LinkageSignatureIr, MergeFileIr, NextSentenceTargetIr, OccursIr, OccursKeyDirectionIr,
    OccursKeyIr, OdoDescriptorIr, OdoTemplateIr, OpenFileIr, OperandIr, ParagraphFlowIr,
    ParagraphIr, PerformIr, PerformVaryingIr, PicCategoryIr, PicIr, ProcedureCfgIr,
    ProcedureRangeIr, ProcedureSentenceIr, ProgramIr, ProgramUnitIr, PurgeQueueIr, ReadFileIr,
    RedefinesIr, ReferenceModifierIr, ReferenceResolutionIr, ReferenceResolutionStatusIr,
    ReferenceRoleIr, RelOpIr, ReleaseSortRecordIr, RenamesIr, ReportLifecycleIr, RerunIr,
    ResolvedDataRefIr, ReturnSortRecordIr, RewriteFileIr, SameRecordAreaIr, SearchAllIr,
    SearchAllKeyIr, SearchIr, SearchWhenIr, SemanticModelIr, SetIndexOperationIr, Severity,
    SignTestIr, SortDirectionIr, SortKeyIr, SortProcedureIr, SourceSpan, StartFileIr,
    StartPositionIr, StatementBranchIr, StatementIr, StatementPathElementIr, StorageAreaIr,
    StorageBindingIr, StorageCellIr, StorageItemIr, StoragePlanIr, StringDelimiterIr, StringOpIr,
    StringPieceIr, SubscriptExprIr, SuppressReportIr, UnlockFileIr, UnstringOpIr, UnstringTargetIr,
    UsageIr, ValueCategoryIr, WriteAdvancingIr, WriteFileIr,
};
use cobol_record::{
    align_offset as record_align_offset, coverage_summary, elementary_byte_len as record_byte_len,
    encode_packed_decimal, packed_decimal_len, sync_alignment, CoverageKind, CoverageRange,
    LayoutMode, PicCategory, PlatformProfile, RecordConditionName, RecordConditionValue,
    RecordField, RecordOccurs, RecordPicture, RecordPlan, RecordRedefines, RecordUsage, SourceRef,
};
use cobol_syntax::{
    DataClauseAst, DataDeclAst, DataOccursKeyDirectionAst, DataValueAst, DeclarativeTriggerAst,
    FileAst, FileKindAst, FileOpenModeAst, InspectLikeAst, ProgramAst, RerunClauseAst,
    SameRecordAreaAst, SortDirectionAst, StatementKindAst, StorageAreaAst, StringDelimiterAst,
    WriteAdvancingAst,
};
use rust_decimal::Decimal;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::str::FromStr;

const PROGRAM_STATUS_REGISTER: &str = "PROGRAM_STATUS";
const TALLY_REGISTER: &str = "TALLY";
const DEBUG_ITEM_REGISTER: &str = "DEBUG_ITEM";
const DEBUG_CONTENTS_REGISTER: &str = "DEBUG_CONTENTS";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Ibm,
    GnuCobol,
    MicroFocus,
}

#[derive(Debug, Clone, Default)]
pub struct ProgramCatalog {
    signatures: BTreeMap<String, Vec<ProgramCatalogParam>>,
}

#[derive(Debug, Clone)]
struct ProgramCatalogParam {
    name: String,
    category: ValueCategoryIr,
}

impl ProgramCatalog {
    pub fn from_asts(asts: &[ProgramAst]) -> Self {
        let mut signatures = BTreeMap::new();
        for ast in asts {
            signatures.insert(normalize_name(&ast.name), linkage_params_from_ast(ast));
        }
        Self { signatures }
    }

    fn linkage_params_for(&self, program: &str) -> Option<&[ProgramCatalogParam]> {
        self.signatures
            .get(&normalize_name(program))
            .map(Vec::as_slice)
    }
}

#[derive(Debug, Clone)]
struct PlannedData {
    item: DataItemIr,
    conditions: Vec<ConditionNameIr>,
}

#[derive(Debug, Clone)]
struct PendingRenames {
    name: String,
    first: String,
    last: Option<String>,
    storage_area: StorageAreaIr,
    span: SourceSpan,
}

pub fn analyze(ast: ProgramAst, dialect: Dialect) -> ProgramIr {
    analyze_with_catalog(ast, dialect, &ProgramCatalog::default())
}

pub fn analyze_with_catalog(
    ast: ProgramAst,
    dialect: Dialect,
    catalog: &ProgramCatalog,
) -> ProgramIr {
    let program_name = ast.name.clone();
    let mut diagnostics = ast.diagnostics;
    let dialect = match dialect {
        Dialect::Ibm => CobolDialect::Ibm,
        Dialect::GnuCobol => CobolDialect::GnuCobol,
        Dialect::MicroFocus => CobolDialect::MicroFocus,
    };
    let platform_profile = platform_profile_for_dialect(dialect);
    if ast.is_common && ast.is_initial {
        diagnostics.push(Diagnostic::error(
            "E_COMMON_INITIAL_CONFLICT",
            format!("program {program_name} cannot be both COMMON and INITIAL"),
            cobol_ir::SourceSpan::generated(),
        ));
    }
    let files = lower_files(ast.files);
    let same_record_areas = lower_same_record_areas(ast.same_record_areas);
    let rerun_clauses = lower_rerun_clauses(ast.rerun_clauses);
    let (data_items, mut storage) =
        analyze_data_items(ast.data_items, dialect, platform_profile, &mut diagnostics);
    let indexes = collect_index_items(&data_items);
    let linkage_signature = linkage_signature(&program_name, &data_items);
    let mut paragraphs = lower_paragraphs(ast.paragraphs, &mut diagnostics);
    let declaratives = lower_declaratives(ast.declaratives, &files, &paragraphs, &mut diagnostics);
    diagnose_nested_unsupported_statements(&paragraphs, &declaratives, &mut diagnostics);
    resolve_search_all_declared_keys(
        &mut paragraphs,
        &data_items,
        &storage.condition_names,
        &mut diagnostics,
    );
    let control_flow = build_control_flow(&paragraphs);
    let procedure_cfg = build_procedure_cfg(&paragraphs);
    let dialect_profile = dialect_profile(dialect);
    let semantic = analyze_semantics(
        SemanticInputs {
            program_name: &program_name,
            data_items: &data_items,
            storage: &storage,
            paragraphs: &paragraphs,
            declaratives: &declaratives,
            files: &files,
            indexes: &indexes,
            catalog,
        },
        &mut diagnostics,
    );
    let (valid_odo_tables, odo_descriptors) = {
        let data_index = DataReferenceIndex::new(&data_items, &storage.condition_names);
        (
            collect_valid_odo_tables(&data_items, &data_index),
            collect_odo_descriptors(&data_items, &data_index),
        )
    };
    storage
        .odo_templates
        .retain(|template| valid_odo_tables.contains(&template.table));

    let external_items = data_items
        .iter()
        .filter(|item| item.external && item.addressable)
        .map(|item| item.qualified_name.clone())
        .collect();

    ProgramIr {
        name: program_name.clone(),
        is_common: ast.is_common,
        is_initial: ast.is_initial,
        dialect,
        dialect_profile,
        data_items,
        storage,
        paragraphs,
        declaratives,
        control_flow,
        procedure_cfg,
        files,
        same_record_areas,
        rerun_clauses,
        indexes,
        odo_descriptors,
        program_units: vec![ProgramUnitIr {
            name: program_name,
            parent: None,
            is_common: ast.is_common,
            is_initial: ast.is_initial,
            contained_programs: Vec::new(),
            global_items: Vec::new(),
            external_items,
        }],
        linkage_signature,
        semantic,
        diagnostics: dedupe_diagnostics(diagnostics),
    }
}

fn dialect_profile(dialect: CobolDialect) -> DialectProfileIr {
    match dialect {
        CobolDialect::Ibm => DialectProfileIr {
            dialect,
            source_encoding: "fixed-or-free source after preprocessing".to_string(),
            default_codepage: "EBCDIC CCSID 037 for host display data unless schema says otherwise"
                .to_string(),
            binary_endian: "big".to_string(),
            binary_sizing: "IBM digit-range widths: 1-4=2 bytes, 5-9=4 bytes, 10-18=8 bytes"
                .to_string(),
            numproc: "PFD preferred sign policy unless explicitly relaxed".to_string(),
            truncation: "fail-closed when TRUNC behavior is not modeled".to_string(),
            arithmetic:
                "decimal correctness preferred; binary/float arithmetic codegen blocked until typed"
                    .to_string(),
            sync_profile: "IBM z/OS natural binary alignment".to_string(),
            implicit_subject_scope: "cross-parentheses".to_string(),
            subscript_policy: "strict bounds".to_string(),
            invalid_numeric_policy: "error".to_string(),
            odo_group_length_rule: "maximum".to_string(),
            float_format: "IBM hexadecimal".to_string(),
        },
        CobolDialect::GnuCobol => DialectProfileIr {
            dialect,
            source_encoding: "free/fixed GnuCOBOL source after preprocessing".to_string(),
            default_codepage: "ASCII display data unless schema says otherwise".to_string(),
            binary_endian: "host/native unless explicit schema lowering overrides it".to_string(),
            binary_sizing: "GnuCOBOL-compatible sizing must be explicit for non-IBM layouts"
                .to_string(),
            numproc: "non-IBM sign behavior must be explicit".to_string(),
            truncation: "fail-closed when compiler TRUNC behavior is not modeled".to_string(),
            arithmetic: "GnuCOBOL oracle intended for supported fixtures".to_string(),
            sync_profile: "no synthetic SYNC slack by default in shared record plan".to_string(),
            implicit_subject_scope: "parenthesized group".to_string(),
            subscript_policy: "strict bounds".to_string(),
            invalid_numeric_policy: "error".to_string(),
            odo_group_length_rule: "maximum".to_string(),
            float_format: "IEEE binary".to_string(),
        },
        CobolDialect::MicroFocus => DialectProfileIr {
            dialect,
            source_encoding: "Micro Focus source after preprocessing".to_string(),
            default_codepage: "ASCII display data unless schema says otherwise".to_string(),
            binary_endian: "native or directive-controlled; explicit lowering required".to_string(),
            binary_sizing: "Micro Focus binary variants require explicit semantic support"
                .to_string(),
            numproc: "directive-controlled sign behavior must be explicit".to_string(),
            truncation: "fail-closed when directive behavior is not modeled".to_string(),
            arithmetic: "decimal/runtime semantics required before broad codegen".to_string(),
            sync_profile: "Micro Focus alignment profile placeholder".to_string(),
            implicit_subject_scope: "parenthesized group".to_string(),
            subscript_policy: "strict bounds".to_string(),
            invalid_numeric_policy: "error".to_string(),
            odo_group_length_rule: "maximum".to_string(),
            float_format: "IEEE binary".to_string(),
        },
    }
}

fn platform_profile_for_dialect(dialect: CobolDialect) -> PlatformProfile {
    match dialect {
        CobolDialect::Ibm => PlatformProfile::IbmZOs,
        CobolDialect::GnuCobol => PlatformProfile::GnuCobol,
        CobolDialect::MicroFocus => PlatformProfile::MicroFocus,
    }
}

fn lower_files(files: Vec<FileAst>) -> Vec<FileIr> {
    files
        .into_iter()
        .map(|file| FileIr {
            name: file.name,
            kind: match file.kind {
                FileKindAst::Fd => FileKindIr::Fd,
                FileKindAst::Sd => FileKindIr::Sd,
            },
            record_name: file.record_name,
            assign: file.assign,
            assign_is_literal: file.assign_is_literal,
            organization: file.organization,
            access_mode: file.access_mode,
            file_status: file.file_status,
            open_mode: None,
            linage: file.linage,
        })
        .collect()
}

fn lower_same_record_areas(areas: Vec<SameRecordAreaAst>) -> Vec<SameRecordAreaIr> {
    areas
        .into_iter()
        .map(|area| SameRecordAreaIr { files: area.files })
        .collect()
}

fn lower_rerun_clauses(clauses: Vec<RerunClauseAst>) -> Vec<RerunIr> {
    clauses
        .into_iter()
        .map(|clause| RerunIr {
            checkpoint_file: clause.checkpoint_file,
            every_records: clause.every_records,
            watched_file: clause.watched_file,
        })
        .collect()
}

fn collect_index_items(data_items: &[DataItemIr]) -> Vec<IndexItemIr> {
    let mut indexes = Vec::new();
    for item in data_items {
        let Some(occurs) = &item.occurs else {
            continue;
        };
        for index_name in &occurs.indexed_by {
            indexes.push(IndexItemIr {
                name: index_name.clone(),
                table: item.qualified_name.clone(),
                occurrence_min: 1,
                occurrence_max: occurs.max.max(occurs.min).max(1),
                representation: IndexRepresentationIr::Occurrence,
            });
        }
    }
    indexes
}

fn linkage_params_from_ast(ast: &ProgramAst) -> Vec<ProgramCatalogParam> {
    ast.data_items
        .iter()
        .filter(|item| item.storage_area == StorageAreaAst::Linkage && matches!(item.level, 1 | 77))
        .map(|item| ProgramCatalogParam {
            name: item.name.clone(),
            category: ast_data_category(item),
        })
        .collect()
}

fn ast_data_category(item: &DataDeclAst) -> ValueCategoryIr {
    let usage = item.clause_ast.iter().find_map(|clause| match clause {
        DataClauseAst::Usage(value) => Some(value.as_str()),
        _ => None,
    });
    if let Some(usage) = usage {
        match usage {
            "COMP-3" | "PACKED-DECIMAL" => return ValueCategoryIr::PackedDecimal,
            "COMP-5" => return ValueCategoryIr::NativeBinary,
            "COMP" | "COMP-4" | "BINARY" => return ValueCategoryIr::Binary,
            "COMP-1" => return ValueCategoryIr::Float,
            "COMP-2" => return ValueCategoryIr::Float,
            "NATIONAL" | "DISPLAY-1" => return ValueCategoryIr::National,
            "DBCS" | "KANJI" => return ValueCategoryIr::Dbcs,
            _ => {}
        }
    }
    let Some(picture) = item.clause_ast.iter().find_map(|clause| match clause {
        DataClauseAst::Picture(value) => Some(value.to_ascii_uppercase()),
        _ => None,
    }) else {
        return ValueCategoryIr::Group;
    };
    let picture = picture.trim_start();
    if picture.starts_with('N') {
        return ValueCategoryIr::National;
    }
    if picture.starts_with('G') {
        return ValueCategoryIr::Dbcs;
    }
    match parse_picture(picture).category {
        PicCategoryIr::Alphanumeric => ValueCategoryIr::Alphanumeric,
        PicCategoryIr::Alphabetic => ValueCategoryIr::Alphabetic,
        PicCategoryIr::NumericDisplay => ValueCategoryIr::NumericDisplay,
        PicCategoryIr::NumericEdited => ValueCategoryIr::NumericEdited,
        PicCategoryIr::Unknown => ValueCategoryIr::Unsupported,
    }
}

fn linkage_signature(program: &str, items: &[DataItemIr]) -> LinkageSignatureIr {
    LinkageSignatureIr {
        program: program.to_string(),
        parameters: items
            .iter()
            .filter(|item| {
                item.storage_area == StorageAreaIr::Linkage && matches!(item.level, 1 | 77)
            })
            .map(|item| LinkageParamIr {
                name: item.name.clone(),
                qualified_name: item.qualified_name.clone(),
                category: item.value_category,
                usage: item.usage.clone(),
            })
            .collect(),
    }
}

struct SemanticEnv<'a> {
    program_name: &'a str,
    paragraphs: &'a [ParagraphIr],
    data_items: &'a [DataItemIr],
    files: &'a [FileIr],
    indexes: &'a [IndexItemIr],
    catalog: &'a ProgramCatalog,
    data_index: &'a DataReferenceIndex<'a>,
}

struct SemanticInputs<'a> {
    program_name: &'a str,
    data_items: &'a [DataItemIr],
    storage: &'a StoragePlanIr,
    paragraphs: &'a [ParagraphIr],
    declaratives: &'a [DeclarativeIr],
    files: &'a [FileIr],
    indexes: &'a [IndexItemIr],
    catalog: &'a ProgramCatalog,
}

fn analyze_semantics(
    inputs: SemanticInputs<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> SemanticModelIr {
    let mut references = Vec::new();
    let mut conditions = Vec::new();
    let mut evaluates = Vec::new();
    let data_index = DataReferenceIndex::new(inputs.data_items, &inputs.storage.condition_names);
    validate_occurs_ranges(inputs.data_items, diagnostics);
    validate_odo_counters(inputs.data_items, &data_index, diagnostics);
    let env = SemanticEnv {
        program_name: inputs.program_name,
        paragraphs: inputs.paragraphs,
        data_items: inputs.data_items,
        files: inputs.files,
        indexes: inputs.indexes,
        catalog: inputs.catalog,
        data_index: &data_index,
    };

    for paragraph in inputs.paragraphs {
        for (statement_index, statement) in paragraph.statements.iter().enumerate() {
            for (reference, role) in data_references(statement) {
                let resolution = resolve_reference_for_role(
                    &data_index,
                    inputs.indexes,
                    &reference,
                    role,
                    paragraph,
                    statement_index,
                    diagnostics,
                );
                references.push(resolution);
            }
            analyze_statement_semantics(statement, paragraph, statement_index, &env, diagnostics);
            analyze_statement_control_conditions(
                statement,
                paragraph,
                statement_index,
                &env,
                &mut references,
                &mut conditions,
                &mut evaluates,
                diagnostics,
            );
        }
    }
    for declarative in inputs.declaratives {
        let paragraph = ParagraphIr {
            name: declarative.name.clone(),
            rust_name: rust_ident(&declarative.name),
            statements: declarative.statements.clone(),
            statement_spans: vec![declarative.span.clone(); declarative.statements.len()],
            sentences: Vec::new(),
            span: declarative.span.clone(),
        };
        for (statement_index, statement) in declarative.statements.iter().enumerate() {
            for (reference, role) in data_references(statement) {
                let resolution = resolve_reference_for_role(
                    &data_index,
                    inputs.indexes,
                    &reference,
                    role,
                    &paragraph,
                    statement_index,
                    diagnostics,
                );
                references.push(resolution);
            }
            analyze_statement_semantics(statement, &paragraph, statement_index, &env, diagnostics);
            analyze_statement_control_conditions(
                statement,
                &paragraph,
                statement_index,
                &env,
                &mut references,
                &mut conditions,
                &mut evaluates,
                diagnostics,
            );
        }
    }
    validate_paragraph_reachability(inputs.paragraphs, diagnostics);
    validate_statement_reachability(inputs.paragraphs, diagnostics);

    let resolved_data_refs = references
        .iter()
        .map(|reference| resolved_data_ref(reference, &data_index))
        .collect();

    SemanticModelIr {
        references,
        resolved_data_refs,
        conditions,
        evaluates,
    }
}

fn analyze_if_condition(
    raw_condition: &str,
    paragraph: &ParagraphIr,
    statement_index: usize,
    data_index: &DataReferenceIndex<'_>,
    indexes: &[IndexItemIr],
    references: &mut Vec<ReferenceResolutionIr>,
    diagnostics: &mut Vec<Diagnostic>,
) -> ConditionAnalysisIr {
    let condition_text = strip_if_keyword(raw_condition);
    match parse_condition(&condition_text) {
        Ok(tree) => {
            let before = diagnostics.len();
            resolve_condition_tree(
                &tree,
                paragraph,
                statement_index,
                data_index,
                indexes,
                references,
                diagnostics,
            );
            ConditionAnalysisIr {
                raw: condition_text,
                paragraph: paragraph.name.clone(),
                statement_index,
                status: if diagnostics.len() == before {
                    ConditionStatusIr::Parsed
                } else {
                    ConditionStatusIr::SemanticError
                },
                tree: Some(tree),
                span: paragraph.span.clone(),
            }
        }
        Err(message) => {
            if let Some(function) = parse_function_operand(&condition_text) {
                analyze_function_operand(&function, paragraph, diagnostics);
            }
            diagnostics.push(Diagnostic::error(
                "E_CONDITION_PARSE",
                format!("failed to parse IF condition: {message}"),
                paragraph.span.clone(),
            ));
            ConditionAnalysisIr {
                raw: condition_text,
                paragraph: paragraph.name.clone(),
                statement_index,
                status: ConditionStatusIr::ParseError,
                tree: None,
                span: paragraph.span.clone(),
            }
        }
    }
}

fn analyze_evaluate(
    evaluate: &EvaluateIr,
    paragraph: &ParagraphIr,
    statement_index: usize,
    env: &SemanticEnv<'_>,
    references: &mut Vec<ReferenceResolutionIr>,
    diagnostics: &mut Vec<Diagnostic>,
) -> EvaluateAnalysisIr {
    let before = diagnostics.len();
    for subject in &evaluate.subjects {
        resolve_evaluate_subject(
            subject,
            paragraph,
            statement_index,
            env.data_index,
            env.indexes,
            references,
            diagnostics,
        );
    }
    for arm in &evaluate.arms {
        if arm.patterns.len() != evaluate.subjects.len() {
            diagnostics.push(Diagnostic::error(
                "E_EVALUATE_ARITY",
                format!(
                    "EVALUATE arm has {} patterns for {} subjects",
                    arm.patterns.len(),
                    evaluate.subjects.len()
                ),
                paragraph.span.clone(),
            ));
        }
        for pattern in &arm.patterns {
            resolve_evaluate_pattern(
                pattern,
                paragraph,
                statement_index,
                env.data_index,
                env.indexes,
                references,
                diagnostics,
            );
        }
        for statement in &arm.statements {
            analyze_statement_semantics(statement, paragraph, statement_index, env, diagnostics);
        }
    }
    EvaluateAnalysisIr {
        raw: evaluate.raw.clone(),
        paragraph: paragraph.name.clone(),
        statement_index,
        status: if diagnostics.len() == before {
            ConditionStatusIr::Parsed
        } else {
            ConditionStatusIr::SemanticError
        },
        evaluate: Some(evaluate.clone()),
        span: paragraph.span.clone(),
    }
}

fn analyze_statement_control_conditions(
    statement: &StatementIr,
    paragraph: &ParagraphIr,
    statement_index: usize,
    env: &SemanticEnv<'_>,
    references: &mut Vec<ReferenceResolutionIr>,
    conditions: &mut Vec<ConditionAnalysisIr>,
    evaluates: &mut Vec<EvaluateAnalysisIr>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match statement {
        StatementIr::If {
            condition,
            then_statements,
            else_statements,
            ..
        } => {
            conditions.push(analyze_if_condition(
                condition,
                paragraph,
                statement_index,
                env.data_index,
                env.indexes,
                references,
                diagnostics,
            ));
            analyze_statement_list_control_conditions(
                then_statements,
                paragraph,
                statement_index,
                env,
                references,
                conditions,
                evaluates,
                diagnostics,
            );
            analyze_statement_list_control_conditions(
                else_statements,
                paragraph,
                statement_index,
                env,
                references,
                conditions,
                evaluates,
                diagnostics,
            );
        }
        StatementIr::Evaluate(evaluate) => {
            evaluates.push(analyze_evaluate(
                evaluate,
                paragraph,
                statement_index,
                env,
                references,
                diagnostics,
            ));
            for arm in &evaluate.arms {
                analyze_statement_list_control_conditions(
                    &arm.statements,
                    paragraph,
                    statement_index,
                    env,
                    references,
                    conditions,
                    evaluates,
                    diagnostics,
                );
            }
        }
        StatementIr::Compute {
            on_size_error_ops,
            not_on_size_error_ops,
            ..
        } => {
            analyze_statement_list_control_conditions(
                on_size_error_ops,
                paragraph,
                statement_index,
                env,
                references,
                conditions,
                evaluates,
                diagnostics,
            );
            analyze_statement_list_control_conditions(
                not_on_size_error_ops,
                paragraph,
                statement_index,
                env,
                references,
                conditions,
                evaluates,
                diagnostics,
            );
        }
        StatementIr::Perform { until_tree, .. } => {
            if let Some(until_tree) = until_tree {
                resolve_condition_tree(
                    until_tree,
                    paragraph,
                    statement_index,
                    env.data_index,
                    env.indexes,
                    references,
                    diagnostics,
                );
            }
        }
        StatementIr::Search(search) => {
            analyze_statement_list_control_conditions(
                &search.at_end,
                paragraph,
                statement_index,
                env,
                references,
                conditions,
                evaluates,
                diagnostics,
            );
            for when in &search.whens {
                resolve_condition_tree(
                    &when.condition,
                    paragraph,
                    statement_index,
                    env.data_index,
                    env.indexes,
                    references,
                    diagnostics,
                );
                analyze_statement_list_control_conditions(
                    &when.statements,
                    paragraph,
                    statement_index,
                    env,
                    references,
                    conditions,
                    evaluates,
                    diagnostics,
                );
            }
        }
        StatementIr::SearchAll(search) => {
            resolve_condition_tree(
                &search.key_condition,
                paragraph,
                statement_index,
                env.data_index,
                env.indexes,
                references,
                diagnostics,
            );
            analyze_statement_list_control_conditions(
                &search.at_end,
                paragraph,
                statement_index,
                env,
                references,
                conditions,
                evaluates,
                diagnostics,
            );
            analyze_statement_list_control_conditions(
                &search.statements,
                paragraph,
                statement_index,
                env,
                references,
                conditions,
                evaluates,
                diagnostics,
            );
        }
        StatementIr::ReturnSortRecord(ret) => {
            analyze_statement_list_control_conditions(
                &ret.at_end_ops,
                paragraph,
                statement_index,
                env,
                references,
                conditions,
                evaluates,
                diagnostics,
            );
            analyze_statement_list_control_conditions(
                &ret.not_at_end_ops,
                paragraph,
                statement_index,
                env,
                references,
                conditions,
                evaluates,
                diagnostics,
            );
        }
        StatementIr::StartFile(start) => {
            analyze_statement_list_control_conditions(
                &start.invalid_key_ops,
                paragraph,
                statement_index,
                env,
                references,
                conditions,
                evaluates,
                diagnostics,
            );
            analyze_statement_list_control_conditions(
                &start.not_invalid_key_ops,
                paragraph,
                statement_index,
                env,
                references,
                conditions,
                evaluates,
                diagnostics,
            );
        }
        StatementIr::ReadFile(read) => {
            analyze_statement_list_control_conditions(
                &read.at_end_ops,
                paragraph,
                statement_index,
                env,
                references,
                conditions,
                evaluates,
                diagnostics,
            );
            analyze_statement_list_control_conditions(
                &read.not_at_end_ops,
                paragraph,
                statement_index,
                env,
                references,
                conditions,
                evaluates,
                diagnostics,
            );
            analyze_statement_list_control_conditions(
                &read.on_exception_ops,
                paragraph,
                statement_index,
                env,
                references,
                conditions,
                evaluates,
                diagnostics,
            );
        }
        StatementIr::WriteFile(write) => {
            analyze_statement_list_control_conditions(
                &write.invalid_key_ops,
                paragraph,
                statement_index,
                env,
                references,
                conditions,
                evaluates,
                diagnostics,
            );
            analyze_statement_list_control_conditions(
                &write.not_invalid_key_ops,
                paragraph,
                statement_index,
                env,
                references,
                conditions,
                evaluates,
                diagnostics,
            );
            analyze_statement_list_control_conditions(
                &write.on_exception_ops,
                paragraph,
                statement_index,
                env,
                references,
                conditions,
                evaluates,
                diagnostics,
            );
            analyze_statement_list_control_conditions(
                &write.not_on_exception_ops,
                paragraph,
                statement_index,
                env,
                references,
                conditions,
                evaluates,
                diagnostics,
            );
        }
        StatementIr::RewriteFile(rewrite) => {
            analyze_statement_list_control_conditions(
                &rewrite.invalid_key_ops,
                paragraph,
                statement_index,
                env,
                references,
                conditions,
                evaluates,
                diagnostics,
            );
            analyze_statement_list_control_conditions(
                &rewrite.not_invalid_key_ops,
                paragraph,
                statement_index,
                env,
                references,
                conditions,
                evaluates,
                diagnostics,
            );
        }
        StatementIr::DeleteFile(delete) => {
            analyze_statement_list_control_conditions(
                &delete.invalid_key_ops,
                paragraph,
                statement_index,
                env,
                references,
                conditions,
                evaluates,
                diagnostics,
            );
            analyze_statement_list_control_conditions(
                &delete.not_invalid_key_ops,
                paragraph,
                statement_index,
                env,
                references,
                conditions,
                evaluates,
                diagnostics,
            );
        }
        StatementIr::StringOp(string) => {
            analyze_statement_list_control_conditions(
                &string.on_overflow_ops,
                paragraph,
                statement_index,
                env,
                references,
                conditions,
                evaluates,
                diagnostics,
            );
            analyze_statement_list_control_conditions(
                &string.not_on_overflow_ops,
                paragraph,
                statement_index,
                env,
                references,
                conditions,
                evaluates,
                diagnostics,
            );
        }
        StatementIr::UnstringOp(unstring) => {
            analyze_statement_list_control_conditions(
                &unstring.on_overflow_ops,
                paragraph,
                statement_index,
                env,
                references,
                conditions,
                evaluates,
                diagnostics,
            );
            analyze_statement_list_control_conditions(
                &unstring.not_on_overflow_ops,
                paragraph,
                statement_index,
                env,
                references,
                conditions,
                evaluates,
                diagnostics,
            );
        }
        _ => {}
    }
}

fn analyze_statement_list_control_conditions(
    statements: &[StatementIr],
    paragraph: &ParagraphIr,
    statement_index: usize,
    env: &SemanticEnv<'_>,
    references: &mut Vec<ReferenceResolutionIr>,
    conditions: &mut Vec<ConditionAnalysisIr>,
    evaluates: &mut Vec<EvaluateAnalysisIr>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for statement in statements {
        analyze_statement_control_conditions(
            statement,
            paragraph,
            statement_index,
            env,
            references,
            conditions,
            evaluates,
            diagnostics,
        );
    }
}

fn resolve_evaluate_subject(
    subject: &EvaluateSubjectIr,
    paragraph: &ParagraphIr,
    statement_index: usize,
    data_index: &DataReferenceIndex<'_>,
    indexes: &[IndexItemIr],
    references: &mut Vec<ReferenceResolutionIr>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match subject {
        EvaluateSubjectIr::Operand(operand) => {
            let _ = resolve_condition_operand(
                operand,
                paragraph,
                statement_index,
                data_index,
                indexes,
                references,
                diagnostics,
            );
        }
        EvaluateSubjectIr::Condition(condition) => resolve_condition_tree(
            condition,
            paragraph,
            statement_index,
            data_index,
            indexes,
            references,
            diagnostics,
        ),
    }
}

fn resolve_evaluate_pattern(
    pattern: &EvaluatePatternIr,
    paragraph: &ParagraphIr,
    statement_index: usize,
    data_index: &DataReferenceIndex<'_>,
    indexes: &[IndexItemIr],
    references: &mut Vec<ReferenceResolutionIr>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match pattern {
        EvaluatePatternIr::Any => {}
        EvaluatePatternIr::Operand(operand) => {
            let _ = resolve_condition_operand(
                operand,
                paragraph,
                statement_index,
                data_index,
                indexes,
                references,
                diagnostics,
            );
        }
        EvaluatePatternIr::Range { start, end } => {
            let _ = resolve_condition_operand(
                start,
                paragraph,
                statement_index,
                data_index,
                indexes,
                references,
                diagnostics,
            );
            let _ = resolve_condition_operand(
                end,
                paragraph,
                statement_index,
                data_index,
                indexes,
                references,
                diagnostics,
            );
        }
        EvaluatePatternIr::Condition(condition) => resolve_condition_tree(
            condition,
            paragraph,
            statement_index,
            data_index,
            indexes,
            references,
            diagnostics,
        ),
    }
}

fn resolved_data_ref(
    reference: &ReferenceResolutionIr,
    data_index: &DataReferenceIndex<'_>,
) -> ResolvedDataRefIr {
    let data_ref = parse_data_ref(&reference.raw);
    let mut byte_range = None;
    let mut layout_id = None;
    let mut in_occurs = false;
    let mut in_odo = false;
    let mut in_redefines = false;
    let mut condition_name_target = None;

    match data_index.resolve_ref(&data_ref) {
        DataResolution::Resolved(item) => {
            if let (Some(offset), Some(length)) = (item.offset, item.byte_len) {
                byte_range = Some(ByteRangeIr { offset, length });
            }
            layout_id = item.layout_id.clone();
            in_occurs = data_index.has_occurs_context(item);
            in_odo = data_index.has_dynamic_occurs_context(item);
            in_redefines = data_index.has_redefines_context(item);
        }
        DataResolution::Condition(condition) => {
            condition_name_target = Some(condition_qualified_name(condition));
            if let DataResolution::Resolved(parent) = data_index.resolve(&condition.parent) {
                if let (Some(offset), Some(length)) = (parent.offset, parent.byte_len) {
                    byte_range = Some(ByteRangeIr { offset, length });
                }
                layout_id = parent.layout_id.clone();
                in_occurs = data_index.has_occurs_context(parent);
                in_odo = data_index.has_dynamic_occurs_context(parent);
                in_redefines = data_index.has_redefines_context(parent);
            }
        }
        DataResolution::Special { name, .. } => {
            layout_id = Some(name);
        }
        DataResolution::Missing | DataResolution::Ambiguous(_) => {}
    }

    ResolvedDataRefIr {
        raw: reference.raw.clone(),
        normalized: reference.normalized.clone(),
        target: reference.target.clone(),
        condition_name_target,
        subscripts: data_ref.subscripts,
        reference_modifier: data_ref.reference_modifier,
        category: reference.category,
        byte_range,
        layout_id,
        in_redefines,
        in_occurs,
        in_odo,
        status: reference.status,
    }
}

fn condition_qualified_name(condition: &ConditionNameIr) -> String {
    format!("{}.{}", condition.parent, condition.name)
}

fn strip_if_keyword(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('.');
    let upper = trimmed.to_ascii_uppercase();
    let condition = if upper == "IF" {
        ""
    } else if upper.starts_with("IF ") {
        trimmed[2..].trim()
    } else {
        trimmed
    };
    trim_if_condition_before_action(condition)
}

fn trim_if_condition_before_action(condition: &str) -> String {
    let tokens = tokenize_condition(condition);
    if tokens.is_empty() {
        return String::new();
    }
    let mut depth = 0usize;
    for (idx, token) in tokens.iter().enumerate() {
        if token == "(" {
            depth += 1;
        } else if token == ")" {
            depth = depth.saturating_sub(1);
        }
        if idx > 0 && depth == 0 && is_if_action_boundary(token) {
            return tokens[..idx].join(" ");
        }
    }
    tokens.join(" ")
}

fn is_if_action_boundary(token: &str) -> bool {
    matches!(
        token.to_ascii_uppercase().as_str(),
        "THEN"
            | "DISPLAY"
            | "MOVE"
            | "ADD"
            | "SUBTRACT"
            | "MULTIPLY"
            | "DIVIDE"
            | "COMPUTE"
            | "PERFORM"
            | "GO"
            | "GOBACK"
            | "STOP"
            | "OPEN"
            | "READ"
            | "WRITE"
            | "CLOSE"
            | "EXEC"
            | "CALL"
            | "SORT"
            | "MERGE"
            | "ENTER"
            | "ALTER"
            | "ELSE"
            | "END-IF"
            | "NEXT"
            | "CONTINUE"
    )
}

fn resolve_condition_tree(
    condition: &ConditionIr,
    paragraph: &ParagraphIr,
    statement_index: usize,
    data_index: &DataReferenceIndex<'_>,
    indexes: &[IndexItemIr],
    references: &mut Vec<ReferenceResolutionIr>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match condition {
        ConditionIr::Relation { left, right, .. } => {
            let left_category = resolve_condition_operand(
                left,
                paragraph,
                statement_index,
                data_index,
                indexes,
                references,
                diagnostics,
            );
            let right_category = resolve_condition_operand(
                right,
                paragraph,
                statement_index,
                data_index,
                indexes,
                references,
                diagnostics,
            );
            if !relation_categories_compatible(left_category, right_category) {
                diagnostics.push(Diagnostic::error(
                    "E_CONDITION_TYPE_MISMATCH",
                    format!(
                        "relational condition compares incompatible categories {:?} and {:?}",
                        left_category, right_category
                    ),
                    paragraph.span.clone(),
                ));
            }
        }
        ConditionIr::ClassTest { operand, class, .. } => {
            let category = resolve_condition_operand(
                operand,
                paragraph,
                statement_index,
                data_index,
                indexes,
                references,
                diagnostics,
            );
            if !class_test_supported(category, *class) {
                diagnostics.push(Diagnostic::error(
                    "E_CONDITION_CLASS_UNSUPPORTED",
                    format!(
                        "class test {:?} is not enabled for operand category {:?}",
                        class, category
                    ),
                    paragraph.span.clone(),
                ));
            }
        }
        ConditionIr::SignTest { operand, sign, .. } => {
            let category = resolve_condition_operand(
                operand,
                paragraph,
                statement_index,
                data_index,
                indexes,
                references,
                diagnostics,
            );
            if !category.map(category_is_numeric).unwrap_or(false) {
                diagnostics.push(Diagnostic::error(
                    "E_CONDITION_SIGN_UNSUPPORTED",
                    format!(
                        "sign test {:?} requires a numeric operand; got {:?}",
                        sign, category
                    ),
                    paragraph.span.clone(),
                ));
            }
        }
        ConditionIr::ConditionName { reference } => {
            let resolution = resolve_reference_for_role(
                data_index,
                indexes,
                reference,
                ReferenceRoleIr::ConditionOperand,
                paragraph,
                statement_index,
                diagnostics,
            );
            if resolution.category != Some(ValueCategoryIr::ConditionName) {
                diagnostics.push(Diagnostic::error(
                    "E_CONDITION_EXPECTED",
                    format!(
                        "bare condition operand {} must resolve to an 88-level condition-name",
                        reference.raw
                    ),
                    paragraph.span.clone(),
                ));
            }
            references.push(resolution);
        }
        ConditionIr::Not(inner) => resolve_condition_tree(
            inner,
            paragraph,
            statement_index,
            data_index,
            indexes,
            references,
            diagnostics,
        ),
        ConditionIr::And(left, right) | ConditionIr::Or(left, right) => {
            resolve_condition_tree(
                left,
                paragraph,
                statement_index,
                data_index,
                indexes,
                references,
                diagnostics,
            );
            resolve_condition_tree(
                right,
                paragraph,
                statement_index,
                data_index,
                indexes,
                references,
                diagnostics,
            );
        }
    }
}

fn resolve_condition_operand(
    operand: &ConditionOperandIr,
    paragraph: &ParagraphIr,
    statement_index: usize,
    data_index: &DataReferenceIndex<'_>,
    indexes: &[IndexItemIr],
    references: &mut Vec<ReferenceResolutionIr>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<ValueCategoryIr> {
    match operand {
        ConditionOperandIr::Identifier(reference) => {
            let resolution = resolve_reference_for_role(
                data_index,
                indexes,
                reference,
                ReferenceRoleIr::ConditionOperand,
                paragraph,
                statement_index,
                diagnostics,
            );
            let category = resolution.category;
            references.push(resolution);
            category
        }
        ConditionOperandIr::Literal(_) => Some(ValueCategoryIr::Alphanumeric),
        ConditionOperandIr::Number(_) => Some(ValueCategoryIr::NumericDisplay),
        ConditionOperandIr::Figurative(FigurativeConstantIr::Zero) => {
            Some(ValueCategoryIr::NumericDisplay)
        }
        ConditionOperandIr::Figurative(
            FigurativeConstantIr::Space
            | FigurativeConstantIr::HighValue
            | FigurativeConstantIr::LowValue
            | FigurativeConstantIr::Quote,
        ) => Some(ValueCategoryIr::Alphanumeric),
        ConditionOperandIr::AllLiteral(_) => Some(ValueCategoryIr::Alphanumeric),
        ConditionOperandIr::Bool(_) => Some(ValueCategoryIr::ConditionName),
        ConditionOperandIr::Function(function) => {
            for reference in function_references(function) {
                let resolution = resolve_reference_for_role(
                    data_index,
                    indexes,
                    &reference.0,
                    ReferenceRoleIr::ConditionOperand,
                    paragraph,
                    statement_index,
                    diagnostics,
                );
                references.push(resolution);
            }
            match function {
                FunctionOperandIr::Length(_)
                | FunctionOperandIr::Ord(_)
                | FunctionOperandIr::Numval(_) => Some(ValueCategoryIr::NumericDisplay),
                FunctionOperandIr::UserDefined {
                    name, raw, args, ..
                } => {
                    push_function_operand_blocker(name, raw, args.len(), paragraph, diagnostics);
                    None
                }
            }
        }
    }
}

fn relation_categories_compatible(
    left: Option<ValueCategoryIr>,
    right: Option<ValueCategoryIr>,
) -> bool {
    let (Some(left), Some(right)) = (left, right) else {
        return true;
    };
    if category_is_numeric(left) && category_is_numeric(right) {
        return true;
    }
    if category_is_nonnumeric(left) && category_is_nonnumeric(right) {
        return true;
    }
    false
}

fn category_is_nonnumeric(category: ValueCategoryIr) -> bool {
    matches!(
        category,
        ValueCategoryIr::Group
            | ValueCategoryIr::Alphanumeric
            | ValueCategoryIr::Alphabetic
            | ValueCategoryIr::NumericEdited
    )
}

fn class_test_supported(category: Option<ValueCategoryIr>, class: ClassTestIr) -> bool {
    let Some(category) = category else {
        return true;
    };
    match class {
        ClassTestIr::Numeric => matches!(
            category,
            ValueCategoryIr::Alphanumeric
                | ValueCategoryIr::Alphabetic
                | ValueCategoryIr::NumericDisplay
                | ValueCategoryIr::PackedDecimal
                | ValueCategoryIr::Binary
                | ValueCategoryIr::NativeBinary
        ),
        ClassTestIr::Alphabetic | ClassTestIr::AlphabeticUpper | ClassTestIr::AlphabeticLower => {
            matches!(
                category,
                ValueCategoryIr::Alphanumeric | ValueCategoryIr::Alphabetic
            )
        }
    }
}

fn resolve_reference_for_role(
    data_index: &DataReferenceIndex<'_>,
    indexes: &[IndexItemIr],
    reference: &DataRefIr,
    role: ReferenceRoleIr,
    paragraph: &ParagraphIr,
    statement_index: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> ReferenceResolutionIr {
    let mut status = ReferenceResolutionStatusIr::Resolved;
    let mut target = None;
    let mut candidates = Vec::new();
    let mut category = None;

    match data_index.resolve_ref(reference) {
        DataResolution::Missing => {
            diagnostics.push(Diagnostic::error(
                "E_UNRESOLVED_DATA",
                format!(
                    "data reference {} does not resolve to a Data Division item or condition name",
                    reference.raw
                ),
                paragraph.span.clone(),
            ));
            status = ReferenceResolutionStatusIr::Missing;
        }
        DataResolution::Ambiguous(matches) => {
            candidates = matches.clone();
            diagnostics.push(Diagnostic::error(
                "E_AMBIGUOUS_DATA",
                format!(
                    "data reference {} is ambiguous; candidates: {}",
                    reference.raw,
                    matches.join(", ")
                ),
                paragraph.span.clone(),
            ));
            status = ReferenceResolutionStatusIr::Ambiguous;
        }
        DataResolution::Condition(condition) => {
            target = Some(condition_qualified_name(condition));
            category = Some(ValueCategoryIr::ConditionName);
            if reference.has_reference_modifier() {
                diagnostics.push(Diagnostic::error(
                    "E_INVALID_REFERENCE_MODIFICATION",
                    format!(
                        "condition-name {} cannot be reference-modified because 88-levels are predicates",
                        reference.raw
                    ),
                    paragraph.span.clone(),
                ));
                status = ReferenceResolutionStatusIr::UnsupportedReferenceModification;
            }
            if matches!(
                role,
                ReferenceRoleIr::Target
                    | ReferenceRoleIr::ArithmeticTarget
                    | ReferenceRoleIr::ComputeTarget
            ) {
                diagnostics.push(Diagnostic::error(
                    "E_INVALID_MOVE_TARGET",
                    format!(
                        "condition name {} cannot be used as a receiving storage item",
                        condition.name
                    ),
                    paragraph.span.clone(),
                ));
                status = ReferenceResolutionStatusIr::UnsupportedCategory;
            }
            if status == ReferenceResolutionStatusIr::Resolved
                && !role_supported_for_category(role, ValueCategoryIr::ConditionName)
            {
                diagnostics.push(Diagnostic::error(
                    "E_CODEGEN_FIELD_CODEC",
                    format!(
                        "condition-name {} is a predicate, not a storage item; this procedure role is not enabled for generated Rust yet",
                        reference.raw
                    ),
                    paragraph.span.clone(),
                ));
                status = ReferenceResolutionStatusIr::UnsupportedCategory;
            }
            if let DataResolution::Resolved(parent) = data_index.resolve(&condition.parent) {
                if reference.is_subscripted() {
                    validate_subscripts(
                        reference,
                        parent,
                        data_index,
                        indexes,
                        paragraph,
                        diagnostics,
                    );
                    let occurs_depth = occurs_chain(parent, data_index).len();
                    if reference.subscripts.len() != occurs_depth {
                        status = ReferenceResolutionStatusIr::InvalidSubscript;
                    }
                } else {
                    let occurs_depth = occurs_chain(parent, data_index).len();
                    if occurs_depth > 0 {
                        diagnostics.push(Diagnostic::error(
                            "E_MISSING_SUBSCRIPT",
                            format!(
                                "condition-name {} predicates {} and requires {} subscript(s)",
                                reference.raw, parent.qualified_name, occurs_depth
                            ),
                            paragraph.span.clone(),
                        ));
                        status = ReferenceResolutionStatusIr::InvalidSubscript;
                    }
                }
            }
        }
        DataResolution::Special {
            name,
            category: special_category,
            ..
        } => {
            target = Some(name);
            category = Some(special_category);
            if reference.has_reference_modifier() {
                diagnostics.push(Diagnostic::error(
                    "E_CODEGEN_REFERENCE_MODIFICATION",
                    format!(
                        "special register {} cannot be reference-modified in generated Rust yet",
                        reference.raw
                    ),
                    paragraph.span.clone(),
                ));
                status = ReferenceResolutionStatusIr::UnsupportedReferenceModification;
            }
            if reference.is_subscripted() {
                diagnostics.push(Diagnostic::error(
                    "E_CODEGEN_SUBSCRIPT",
                    format!("special register {} cannot be subscripted", reference.raw),
                    paragraph.span.clone(),
                ));
                status = ReferenceResolutionStatusIr::InvalidSubscript;
            }
            if !role_supported_for_category(role, special_category) {
                diagnostics.push(Diagnostic::error(
                    "E_CODEGEN_FIELD_CODEC",
                    format!(
                        "special register {} has semantic category {:?}; this procedure role is not enabled for generated Rust yet",
                        reference.raw, special_category
                    ),
                    paragraph.span.clone(),
                ));
                status = ReferenceResolutionStatusIr::UnsupportedCategory;
            }
        }
        DataResolution::Resolved(item) => {
            target = Some(item.qualified_name.clone());
            category = Some(item.value_category);
            if reference.has_reference_modifier()
                && !validate_reference_modifier(reference, item, data_index, paragraph, diagnostics)
            {
                status = ReferenceResolutionStatusIr::UnsupportedReferenceModification;
            }
            if reference.is_subscripted() {
                validate_subscripts(reference, item, data_index, indexes, paragraph, diagnostics);
                let occurs_depth = occurs_chain(item, data_index).len();
                let subscript_aware_context = role_allows_subscripted_occurs(role);
                if !subscript_aware_context || reference.subscripts.len() != occurs_depth {
                    diagnostics.push(Diagnostic::error(
                        "E_CODEGEN_SUBSCRIPT",
                        format!(
                            "data reference {} uses subscripts; only fully-subscripted condition/evaluate operands and CALL USING arguments are enabled for generated Rust",
                            reference.raw
                        ),
                        paragraph.span.clone(),
                    ));
                    status = ReferenceResolutionStatusIr::InvalidSubscript;
                }
            } else {
                let occurs_depth = occurs_chain(item, data_index).len();
                if occurs_depth > 0 {
                    diagnostics.push(Diagnostic::error(
                        "E_MISSING_SUBSCRIPT",
                        format!(
                            "data reference {} resolves to {} and requires {} subscript(s)",
                            reference.raw, item.qualified_name, occurs_depth
                        ),
                        paragraph.span.clone(),
                    ));
                    status = ReferenceResolutionStatusIr::InvalidSubscript;
                }
            }
            let has_dynamic_occurs_context = data_index.has_dynamic_occurs_context(item);
            let unsupported_occurs_context = if has_dynamic_occurs_context {
                !role_allows_subscripted_dynamic_occurs(role) || !reference.is_subscripted()
            } else {
                !role_allows_subscripted_occurs(role) || !reference.is_subscripted()
            };
            if has_dynamic_occurs_context && unsupported_occurs_context {
                diagnostics.push(Diagnostic::error(
                        "E_CODEGEN_ODO_REFERENCE",
                        format!(
                        "field {} is inside OCCURS DEPENDING ON storage; only fully-subscripted display and checked condition/evaluate reads are enabled",
                        item.qualified_name
                    ),
                        paragraph.span.clone(),
                    ));
                status = ReferenceResolutionStatusIr::UnsupportedDynamic;
            } else if data_index.has_occurs_context(item) && unsupported_occurs_context {
                diagnostics.push(Diagnostic::error(
                    "E_CODEGEN_OCCURS_REFERENCE",
                    format!(
                        "field {} is inside OCCURS storage; only subscript-aware condition/evaluate reads are enabled",
                        item.qualified_name
                    ),
                    paragraph.span.clone(),
                ));
                status = ReferenceResolutionStatusIr::UnsupportedDynamic;
            }
            if data_index.has_redefines_context(item)
                && !matches!(role, ReferenceRoleIr::ConditionOperand)
            {
                diagnostics.push(Diagnostic::error(
                    "E_CODEGEN_REDEFINES_REFERENCE",
                    format!(
                        "field {} participates in REDEFINES storage; non-condition procedure code still needs active-view semantics",
                        item.qualified_name
                    ),
                    paragraph.span.clone(),
                ));
                status = ReferenceResolutionStatusIr::UnsupportedRedefines;
            }
            if !role_supported_for_category(role, item.value_category) {
                diagnostics.push(Diagnostic::error(
                    "E_CODEGEN_FIELD_CODEC",
                    format!(
                        "field {} has semantic category {:?}; this procedure role is not enabled for generated Rust yet",
                        item.qualified_name, item.value_category
                    ),
                    paragraph.span.clone(),
                ));
                status = ReferenceResolutionStatusIr::UnsupportedCategory;
            }
        }
    }

    ReferenceResolutionIr {
        raw: reference.raw.clone(),
        normalized: reference.normalized.clone(),
        paragraph: paragraph.name.clone(),
        statement_index,
        role,
        status,
        target,
        candidates,
        category,
        span: paragraph.span.clone(),
    }
}

fn analyze_statement_semantics(
    statement: &StatementIr,
    paragraph: &ParagraphIr,
    _statement_index: usize,
    env: &SemanticEnv<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match statement {
        StatementIr::Display(values) => {
            for value in values {
                analyze_operand_functions(value, paragraph, diagnostics);
            }
        }
        StatementIr::Move { source, target } => {
            analyze_operand_functions(source, paragraph, diagnostics);
            analyze_move_semantics(source, target, env.data_index, paragraph, diagnostics);
        }
        StatementIr::MoveCorresponding { source, target } => {
            analyze_move_corresponding_semantics(
                source,
                target,
                env.data_index,
                paragraph,
                diagnostics,
            );
        }
        StatementIr::Add { source, target }
        | StatementIr::Subtract { source, target }
        | StatementIr::Multiply { source, target }
        | StatementIr::Divide { source, target } => {
            analyze_operand_functions(source, paragraph, diagnostics);
            analyze_arithmetic_semantics(source, target, env.data_index, paragraph, diagnostics);
        }
        StatementIr::Compute {
            expression,
            on_size_error_ops,
            not_on_size_error_ops,
            ..
        } => {
            analyze_compute_expression_functions(expression, paragraph, diagnostics);
            for branch_statement in on_size_error_ops.iter().chain(not_on_size_error_ops) {
                analyze_statement_semantics(
                    branch_statement,
                    paragraph,
                    _statement_index,
                    env,
                    diagnostics,
                );
            }
        }
        StatementIr::SetCondition { condition, value } => {
            if !*value {
                diagnostics.push(Diagnostic::error(
                    "E_CODEGEN_SET_FALSE",
                    "SET condition-name TO FALSE requires alternate value semantics and is not enabled",
                    paragraph.span.clone(),
                ));
            }
            let resolution = env.data_index.resolve_ref(condition);
            if !matches!(resolution, DataResolution::Condition(_)) {
                diagnostics.push(Diagnostic::error(
                    "E_INVALID_SET_CONDITION",
                    format!("SET target {} is not a condition-name", condition.raw),
                    paragraph.span.clone(),
                ));
            }
            if let DataResolution::Condition(condition_name) = resolution {
                if let DataResolution::Resolved(parent) =
                    env.data_index.resolve(&condition_name.parent)
                {
                    if env.data_index.has_dynamic_occurs_context(parent) {
                        diagnostics.push(Diagnostic::error(
                            "E_CODEGEN_ODO_REFERENCE",
                            format!(
                                "SET condition-name {} writes parent field {} inside OCCURS DEPENDING ON storage; ODO condition-name updates are not executable yet",
                                condition.raw, parent.qualified_name
                            ),
                            paragraph.span.clone(),
                        ));
                    }
                    if env.data_index.has_redefines_context(parent) {
                        diagnostics.push(Diagnostic::error(
                            "E_CODEGEN_REDEFINES_REFERENCE",
                            format!(
                                "SET condition-name {} writes parent field {} participating in REDEFINES storage; active-view condition-name updates are not executable yet",
                                condition.raw, parent.qualified_name
                            ),
                            paragraph.span.clone(),
                        ));
                    }
                }
            }
        }
        StatementIr::SetIndex { index, operation } => {
            if !env
                .indexes
                .iter()
                .any(|item| item.name.eq_ignore_ascii_case(index))
            {
                diagnostics.push(Diagnostic::error(
                    "E_INVALID_SET_INDEX",
                    format!("SET target {index} is not a resolved INDEXED BY item"),
                    paragraph.span.clone(),
                ));
            }
            validate_set_index_expr(operation, env.data_index, paragraph, diagnostics);
        }
        StatementIr::Accept(accept) => {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_ACCEPT",
                "ACCEPT is represented in IR but runtime/environment input lowering is not executable yet",
                paragraph.span.clone(),
            ));
            if !matches!(
                env.data_index.resolve_ref(&accept.target),
                DataResolution::Resolved(_)
            ) {
                diagnostics.push(Diagnostic::error(
                    "E_UNRESOLVED_DATA",
                    format!("ACCEPT target {} does not resolve", accept.target.raw),
                    paragraph.span.clone(),
                ));
            }
        }
        StatementIr::Initialize(initialize) => {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_INITIALIZE",
                "INITIALIZE is represented in IR but data-category defaulting and REPLACING semantics are not executable yet",
                paragraph.span.clone(),
            ));
            for target in &initialize.targets {
                if !matches!(
                    env.data_index.resolve_ref(target),
                    DataResolution::Resolved(_)
                ) {
                    diagnostics.push(Diagnostic::error(
                        "E_UNRESOLVED_DATA",
                        format!("INITIALIZE target {} does not resolve", target.raw),
                        paragraph.span.clone(),
                    ));
                }
            }
        }
        StatementIr::GenerateReport(generate) => {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_GENERATE_REPORT",
                format!(
                    "GENERATE {} is represented in IR but report writer rendering and control-break semantics are not executable yet",
                    generate.target
                ),
                paragraph.span.clone(),
            ));
        }
        StatementIr::InitiateReport(initiate) => {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_INITIATE_REPORT",
                format!(
                    "INITIATE {} is represented in IR but report writer initialization semantics are not executable yet",
                    initiate.targets.join(", ")
                ),
                paragraph.span.clone(),
            ));
        }
        StatementIr::TerminateReport(terminate) => {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_TERMINATE_REPORT",
                format!(
                    "TERMINATE {} is represented in IR but report writer finalization semantics are not executable yet",
                    terminate.targets.join(", ")
                ),
                paragraph.span.clone(),
            ));
        }
        StatementIr::SuppressReport(suppress) => {
            let target = suppress
                .target
                .as_deref()
                .unwrap_or("<current report group>");
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_SUPPRESS_REPORT",
                format!(
                    "SUPPRESS {target} is represented in IR but report writer suppression semantics are not executable yet"
                ),
                paragraph.span.clone(),
            ));
        }
        StatementIr::PurgeQueue(purge) => {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_PURGE_QUEUE",
                format!(
                    "PURGE {} is represented in IR but queue/message purge runtime semantics are not executable yet",
                    purge.target
                ),
                paragraph.span.clone(),
            ));
        }
        StatementIr::EnableCommunication(enable) => {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_ENABLE_COMMUNICATION",
                format!(
                    "ENABLE {} is represented in IR but communications runtime enable semantics are not executable yet",
                    enable.target
                ),
                paragraph.span.clone(),
            ));
        }
        StatementIr::DisableCommunication(disable) => {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_DISABLE_COMMUNICATION",
                format!(
                    "DISABLE {} is represented in IR but communications runtime disable semantics are not executable yet",
                    disable.target
                ),
                paragraph.span.clone(),
            ));
        }
        StatementIr::SendCommunication(send) => {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_SEND_COMMUNICATION",
                format!(
                    "SEND {} is represented in IR but communications runtime send semantics are not executable yet",
                    send.target
                ),
                paragraph.span.clone(),
            ));
        }
        StatementIr::ReceiveCommunication(receive) => {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_RECEIVE_COMMUNICATION",
                format!(
                    "RECEIVE {} is represented in IR but communications runtime receive semantics are not executable yet",
                    receive.target
                ),
                paragraph.span.clone(),
            ));
        }
        StatementIr::EnterLanguage(enter) => {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_ENTER_LANGUAGE",
                format!(
                    "ENTER {} is represented in IR but alternate language execution semantics are not executable yet",
                    enter.language
                ),
                paragraph.span.clone(),
            ));
        }
        StatementIr::MergeFile(merge) => {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_MERGE_FILE",
                format!(
                    "MERGE {} is represented in IR but file merge runtime semantics are not executable yet",
                    merge.file
                ),
                paragraph.span.clone(),
            ));
        }
        StatementIr::Cancel(cancel) => {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_CANCEL",
                "CANCEL is represented in IR but subprogram lifecycle reset semantics are not executable yet",
                paragraph.span.clone(),
            ));
            for target in &cancel.targets {
                if let CallTargetIr::Identifier(reference) = target {
                    if !matches!(
                        env.data_index.resolve_ref(reference),
                        DataResolution::Resolved(_)
                    ) {
                        diagnostics.push(Diagnostic::error(
                            "E_UNRESOLVED_DATA",
                            format!("CANCEL target {} does not resolve", reference.raw),
                            paragraph.span.clone(),
                        ));
                    }
                }
            }
        }
        StatementIr::Entry(entry) => {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_ENTRY",
                match &entry.name {
                    CallTargetIr::Literal(name) => format!(
                        "ENTRY {name} declares an alternate entry point; alternate entry dispatch is represented in IR but not executable yet"
                    ),
                    CallTargetIr::Identifier(reference) => format!(
                        "ENTRY {} declares a dynamic alternate entry point; alternate entry dispatch is represented in IR but not executable yet",
                        reference.raw
                    ),
                },
                paragraph.span.clone(),
            ));
            for using in &entry.using {
                if !matches!(
                    env.data_index.resolve_ref(using),
                    DataResolution::Resolved(_)
                ) {
                    diagnostics.push(Diagnostic::error(
                        "E_UNRESOLVED_DATA",
                        format!("ENTRY USING item {} does not resolve", using.raw),
                        paragraph.span.clone(),
                    ));
                }
            }
        }
        StatementIr::Chain(chain) => {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_CHAIN",
                "CHAIN is represented in IR but runtime transfer and replacement program state semantics are not executable yet",
                paragraph.span.clone(),
            ));
            if let CallTargetIr::Identifier(reference) = &chain.target {
                if !matches!(
                    env.data_index.resolve_ref(reference),
                    DataResolution::Resolved(_)
                ) {
                    diagnostics.push(Diagnostic::error(
                        "E_UNRESOLVED_DATA",
                        format!("CHAIN target {} does not resolve", reference.raw),
                        paragraph.span.clone(),
                    ));
                }
            }
            for using in &chain.using {
                if !matches!(
                    env.data_index.resolve_ref(using),
                    DataResolution::Resolved(_)
                ) {
                    diagnostics.push(Diagnostic::error(
                        "E_UNRESOLVED_DATA",
                        format!("CHAIN USING item {} does not resolve", using.raw),
                        paragraph.span.clone(),
                    ));
                }
            }
        }
        StatementIr::If {
            then_statements,
            else_statements,
            ..
        } => {
            for branch_statement in then_statements.iter().chain(else_statements) {
                analyze_statement_semantics(
                    branch_statement,
                    paragraph,
                    _statement_index,
                    env,
                    diagnostics,
                );
            }
        }
        StatementIr::Evaluate(evaluate) => {
            for arm in &evaluate.arms {
                for branch_statement in &arm.statements {
                    analyze_statement_semantics(
                        branch_statement,
                        paragraph,
                        _statement_index,
                        env,
                        diagnostics,
                    );
                }
            }
        }
        StatementIr::Search(search) => {
            validate_search_semantics(search, env, paragraph, diagnostics);
            for branch_statement in &search.at_end {
                analyze_statement_semantics(
                    branch_statement,
                    paragraph,
                    _statement_index,
                    env,
                    diagnostics,
                );
            }
            for when in &search.whens {
                for branch_statement in &when.statements {
                    analyze_statement_semantics(
                        branch_statement,
                        paragraph,
                        _statement_index,
                        env,
                        diagnostics,
                    );
                }
            }
        }
        StatementIr::SearchAll(search) => {
            validate_search_all_semantics(search, env, paragraph, diagnostics);
            for branch_statement in &search.at_end {
                analyze_statement_semantics(
                    branch_statement,
                    paragraph,
                    _statement_index,
                    env,
                    diagnostics,
                );
            }
            for branch_statement in &search.statements {
                analyze_statement_semantics(
                    branch_statement,
                    paragraph,
                    _statement_index,
                    env,
                    diagnostics,
                );
            }
        }
        StatementIr::OpenFile(open) => {
            if let Err(message) = validate_open_file(open, env.files, env.data_index) {
                diagnostics.push(Diagnostic::error(
                    "E_CODEGEN_FILE_IO",
                    message,
                    paragraph.span.clone(),
                ));
            }
        }
        StatementIr::StartFile(start) => {
            if let Err(message) = validate_start_file(start, env.files, env.data_index) {
                diagnostics.push(Diagnostic::error(
                    "E_CODEGEN_FILE_IO",
                    message,
                    paragraph.span.clone(),
                ));
            }
            for branch_statement in start
                .invalid_key_ops
                .iter()
                .chain(&start.not_invalid_key_ops)
            {
                analyze_statement_semantics(
                    branch_statement,
                    paragraph,
                    _statement_index,
                    env,
                    diagnostics,
                );
            }
        }
        StatementIr::ReadFile(read) => {
            if let Err(message) = validate_read_file(read, env.files, env.data_index) {
                diagnostics.push(Diagnostic::error(
                    "E_CODEGEN_FILE_IO",
                    message,
                    paragraph.span.clone(),
                ));
            }
            for branch_statement in read
                .at_end_ops
                .iter()
                .chain(&read.not_at_end_ops)
                .chain(&read.on_exception_ops)
            {
                analyze_statement_semantics(
                    branch_statement,
                    paragraph,
                    _statement_index,
                    env,
                    diagnostics,
                );
            }
        }
        StatementIr::WriteFile(write) => {
            if let Err(message) = validate_write_file(write, env.files, env.data_index) {
                diagnostics.push(Diagnostic::error(
                    "E_CODEGEN_FILE_IO",
                    message,
                    paragraph.span.clone(),
                ));
            }
            if !write.branch_phrases.is_empty() {
                diagnostics.push(Diagnostic::error(
                    "E_UNSUPPORTED_WRITE_BRANCH",
                    format!(
                        "WRITE {} branch phrases are not executable yet: {}",
                        write.record.raw,
                        write.branch_phrases.join(", ")
                    ),
                    paragraph.span.clone(),
                ));
            }
            for branch_statement in write
                .invalid_key_ops
                .iter()
                .chain(&write.not_invalid_key_ops)
                .chain(&write.on_exception_ops)
                .chain(&write.not_on_exception_ops)
            {
                analyze_statement_semantics(
                    branch_statement,
                    paragraph,
                    _statement_index,
                    env,
                    diagnostics,
                );
            }
        }
        StatementIr::RewriteFile(rewrite) => {
            if let Err(message) = validate_rewrite_file(rewrite, env.files, env.data_index) {
                diagnostics.push(Diagnostic::error(
                    "E_CODEGEN_FILE_IO",
                    message,
                    paragraph.span.clone(),
                ));
            }
            for branch_statement in rewrite
                .invalid_key_ops
                .iter()
                .chain(&rewrite.not_invalid_key_ops)
            {
                analyze_statement_semantics(
                    branch_statement,
                    paragraph,
                    _statement_index,
                    env,
                    diagnostics,
                );
            }
        }
        StatementIr::DeleteFile(delete) => {
            if let Err(message) = validate_delete_file(delete, env.files, env.data_index) {
                diagnostics.push(Diagnostic::error(
                    "E_CODEGEN_FILE_IO",
                    message,
                    paragraph.span.clone(),
                ));
            }
            for branch_statement in delete
                .invalid_key_ops
                .iter()
                .chain(&delete.not_invalid_key_ops)
            {
                analyze_statement_semantics(
                    branch_statement,
                    paragraph,
                    _statement_index,
                    env,
                    diagnostics,
                );
            }
        }
        StatementIr::UnlockFile(unlock) => {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_UNLOCK",
                "UNLOCK is represented in IR but record-lock release semantics are not executable yet",
                paragraph.span.clone(),
            ));
            if let Err(message) = validate_unlock_file(unlock, env.files, env.data_index) {
                diagnostics.push(Diagnostic::error(
                    "E_CODEGEN_FILE_IO",
                    message,
                    paragraph.span.clone(),
                ));
            }
        }
        StatementIr::CloseFile(close) => {
            if let Err(message) = validate_close_file(close, env.files, env.data_index) {
                diagnostics.push(Diagnostic::error(
                    "E_CODEGEN_FILE_IO",
                    message,
                    paragraph.span.clone(),
                ));
            }
        }
        StatementIr::SortProcedure(sort) => {
            validate_sort_procedure_semantics(sort, env, paragraph, diagnostics);
        }
        StatementIr::ReleaseSortRecord(release) => {
            validate_sort_release_semantics(release, env, paragraph, diagnostics);
        }
        StatementIr::ReturnSortRecord(ret) => {
            validate_sort_return_semantics(ret, env, paragraph, diagnostics);
            for branch_statement in ret.at_end_ops.iter().chain(&ret.not_at_end_ops) {
                analyze_statement_semantics(
                    branch_statement,
                    paragraph,
                    _statement_index,
                    env,
                    diagnostics,
                );
            }
        }
        StatementIr::Perform {
            target,
            through,
            varying,
            varying_ir,
            until,
            until_tree,
            times,
            ..
        } => {
            if varying.is_some() && varying_ir.is_none() {
                diagnostics.push(Diagnostic::error(
                    "E_UNSUPPORTED_PERFORM_VARYING",
                    "PERFORM VARYING must be an out-of-line form with VARYING identifier FROM value BY value",
                    paragraph.span.clone(),
                ));
            }
            if until.is_some() && until_tree.is_none() {
                diagnostics.push(Diagnostic::error(
                    "E_UNSUPPORTED_PERFORM_UNTIL",
                    "PERFORM UNTIL condition could not be lowered to executable VM condition IR",
                    paragraph.span.clone(),
                ));
            }
            if paragraph_index(env.paragraphs, target).is_none() {
                let dynamic_target = matches!(
                    env.data_index.resolve(target),
                    DataResolution::Resolved(_) | DataResolution::Special { .. }
                );
                if dynamic_target {
                    if through.is_some() || varying.is_some() || until.is_some() || times.is_some()
                    {
                        diagnostics.push(Diagnostic::error(
                            "E_UNSUPPORTED_DYNAMIC_PERFORM",
                            format!(
                                "dynamic PERFORM target {target} is only executable for a simple paragraph-name data item"
                            ),
                            paragraph.span.clone(),
                        ));
                    }
                } else {
                    diagnostics.push(Diagnostic::error(
                        "E_UNRESOLVED_PARAGRAPH",
                        format!("PERFORM target {target} does not resolve to a paragraph"),
                        paragraph.span.clone(),
                    ));
                }
            }
            if let Some(through) = through {
                if paragraph_index(env.paragraphs, through).is_none() {
                    diagnostics.push(Diagnostic::error(
                        "E_UNRESOLVED_PARAGRAPH",
                        format!("PERFORM THRU target {through} does not resolve to a paragraph"),
                        paragraph.span.clone(),
                    ));
                } else if let (Some(start_idx), Some(end_idx)) = (
                    paragraph_index(env.paragraphs, target),
                    paragraph_index(env.paragraphs, through),
                ) {
                    if start_idx > end_idx {
                        diagnostics.push(Diagnostic::error(
                            "E_INVALID_PERFORM_THRU_RANGE",
                            format!(
                                "PERFORM {target} THRU {through} names a reversed paragraph range"
                            ),
                            paragraph.span.clone(),
                        ));
                    } else {
                        validate_perform_thru_range_integrity(
                            target,
                            through,
                            start_idx,
                            end_idx,
                            env.paragraphs,
                            paragraph,
                            diagnostics,
                        );
                    }
                }
            }
        }
        StatementIr::GoTo(target) => {
            if target == "." {
                if !paragraph_has_altered_goto_slot(env.paragraphs, &paragraph.name) {
                    diagnostics.push(Diagnostic::error(
                        "E_INVALID_GO_TO_TARGET",
                        "GO TO statement has no paragraph target",
                        paragraph.span.clone(),
                    ));
                }
            } else if paragraph_index(env.paragraphs, target).is_none() {
                diagnostics.push(Diagnostic::error(
                    "E_UNRESOLVED_PARAGRAPH",
                    format!("GO TO target {target} does not resolve to a paragraph"),
                    paragraph.span.clone(),
                ));
            }
        }
        StatementIr::ComputedGoTo {
            targets,
            depending_on,
        } => {
            for target in targets {
                if paragraph_index(env.paragraphs, target).is_none() {
                    diagnostics.push(Diagnostic::error(
                        "E_UNRESOLVED_PARAGRAPH",
                        format!(
                            "GO TO DEPENDING ON target {target} does not resolve to a paragraph"
                        ),
                        paragraph.span.clone(),
                    ));
                }
            }
            if let OperandIr::Identifier(reference) = depending_on {
                match env.data_index.resolve(&reference.normalized) {
                    DataResolution::Resolved(item) if category_is_numeric(item.value_category) => {
                    }
                    DataResolution::Resolved(item) => diagnostics.push(Diagnostic::error(
                        "E_INVALID_GO_TO_DEPENDING",
                        format!(
                            "GO TO DEPENDING ON value {} must reference a numeric item, found {:?}",
                            reference.raw, item.value_category
                        ),
                        paragraph.span.clone(),
                    )),
                    DataResolution::Special { category, .. } if category_is_numeric(category) => {}
                    DataResolution::Special { name, category, .. } => diagnostics.push(
                        Diagnostic::error(
                            "E_INVALID_GO_TO_DEPENDING",
                            format!(
                                "GO TO DEPENDING ON value {} must reference a numeric item, but resolved to special register {} ({:?})",
                                reference.raw, name, category
                            ),
                            paragraph.span.clone(),
                        ),
                    ),
                    DataResolution::Condition(_) => diagnostics.push(Diagnostic::error(
                        "E_INVALID_GO_TO_DEPENDING",
                        format!(
                            "GO TO DEPENDING ON value {} resolves to a condition-name",
                            reference.raw
                        ),
                        paragraph.span.clone(),
                    )),
                    DataResolution::Ambiguous(candidates) => diagnostics.push(Diagnostic::error(
                        "E_AMBIGUOUS_DATA",
                        format!(
                            "GO TO DEPENDING ON value {} is ambiguous: {}",
                            reference.raw,
                            candidates.join(", ")
                        ),
                        paragraph.span.clone(),
                    )),
                    DataResolution::Missing => diagnostics.push(Diagnostic::error(
                        "E_UNRESOLVED_DATA",
                        format!(
                            "GO TO DEPENDING ON value {} does not resolve to data storage",
                            reference.raw
                        ),
                        paragraph.span.clone(),
                    )),
                }
            }
        }
        StatementIr::Alter {
            paragraph: alter_paragraph,
            target,
        } => {
            if paragraph_index(env.paragraphs, alter_paragraph).is_none() {
                diagnostics.push(Diagnostic::error(
                    "E_UNRESOLVED_PARAGRAPH",
                    format!("ALTER paragraph {alter_paragraph} does not resolve to a paragraph"),
                    paragraph.span.clone(),
                ));
            }
            if paragraph_index(env.paragraphs, target).is_none() {
                diagnostics.push(Diagnostic::error(
                    "E_UNRESOLVED_PARAGRAPH",
                    format!("ALTER target {target} does not resolve to a paragraph"),
                    paragraph.span.clone(),
                ));
            }
        }
        StatementIr::Call(call) => match &call.target {
            CallTargetIr::Literal(name) => {
                if let Some(linkage) = env.catalog.linkage_params_for(name) {
                    if call.using.len() != linkage.len() {
                        diagnostics.push(Diagnostic::error(
                            "E_WRONG_NUMBER_OF_USING",
                            format!(
                                "CALL target {name} expects {} USING arguments but got {}",
                                linkage.len(),
                                call.using.len()
                            ),
                            paragraph.span.clone(),
                        ));
                    }
                    for (idx, argument) in call.using.iter().enumerate() {
                        let Some(formal) = linkage.get(idx) else {
                            continue;
                        };
                        let Some(actual_category) = resolved_category(env.data_index, argument)
                        else {
                            continue;
                        };
                        if call_using_requires_conversion(actual_category, formal.category)
                            && !call_using_conversion_supported(actual_category, formal.category)
                        {
                            diagnostics.push(Diagnostic::error(
                                "E_UNSUPPORTED_CALL_USING_CONVERSION",
                                format!(
                                    "CALL target {name} USING argument {} {} ({actual_category:?}) does not match LINKAGE parameter {} ({:?}); implicit call-site conversion is not executable yet",
                                    idx + 1,
                                    argument.normalized,
                                    formal.name,
                                    formal.category
                                ),
                                paragraph.span.clone(),
                            ));
                        }
                    }
                } else if normalize_name(name) != normalize_name(env.program_name) {
                    diagnostics.push(Diagnostic::error(
                        "E_UNRESOLVED_CALL_TARGET",
                        format!(
                            "literal CALL target {name} is not registered in this compilation unit"
                        ),
                        paragraph.span.clone(),
                    ));
                    if !call.using.is_empty() {
                        diagnostics.push(Diagnostic::error(
                                "E_UNSUPPORTED_CALL_USING",
                                "CALL USING cannot be lowered without a resolved callee LINKAGE signature",
                                paragraph.span.clone(),
                            ));
                    }
                }
            }
            CallTargetIr::Identifier(reference) => {
                diagnostics.push(Diagnostic::warning(
                    "W_DYNAMIC_CALL_RUNTIME_CHECK",
                    format!(
                        "dynamic CALL target {} is resolved at runtime through the linked program registry; USING argument count and linkage compatibility are checked during dispatch",
                        reference.raw
                    ),
                    paragraph.span.clone(),
                ));
            }
        },
        StatementIr::StringOp(string) => {
            for branch_statement in string
                .on_overflow_ops
                .iter()
                .chain(&string.not_on_overflow_ops)
            {
                analyze_statement_semantics(
                    branch_statement,
                    paragraph,
                    _statement_index,
                    env,
                    diagnostics,
                );
            }
        }
        StatementIr::UnstringOp(unstring) => {
            for branch_statement in unstring
                .on_overflow_ops
                .iter()
                .chain(&unstring.not_on_overflow_ops)
            {
                analyze_statement_semantics(
                    branch_statement,
                    paragraph,
                    _statement_index,
                    env,
                    diagnostics,
                );
            }
        }
        StatementIr::BlockedNextSentence
        | StatementIr::ReadyTrace
        | StatementIr::ResetTrace
        | StatementIr::Continue
        | StatementIr::InspectLike(_)
        | StatementIr::Goback
        | StatementIr::StopRun
        | StatementIr::Unsupported { .. } => {}
    }
}

fn analyze_operand_functions(
    operand: &OperandIr,
    paragraph: &ParagraphIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let OperandIr::Function(function) = operand {
        analyze_function_operand(function, paragraph, diagnostics);
    }
}

fn analyze_condition_operand_functions(
    operand: &ConditionOperandIr,
    paragraph: &ParagraphIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let ConditionOperandIr::Function(function) = operand {
        analyze_function_operand(function, paragraph, diagnostics);
    }
}

fn analyze_function_operand(
    function: &FunctionOperandIr,
    paragraph: &ParagraphIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match function {
        FunctionOperandIr::Length(arg)
        | FunctionOperandIr::Ord(arg)
        | FunctionOperandIr::Numval(arg) => {
            analyze_condition_operand_functions(arg, paragraph, diagnostics);
        }
        FunctionOperandIr::UserDefined { name, raw, args } => {
            push_function_operand_blocker(name, raw, args.len(), paragraph, diagnostics);
            for arg in args {
                analyze_condition_operand_functions(arg, paragraph, diagnostics);
            }
        }
    }
}

fn push_function_operand_blocker(
    name: &str,
    raw: &str,
    arg_count: usize,
    paragraph: &ParagraphIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some(expected) = intrinsic_function_expected_arity(name) {
        if function_call_parentheses_malformed(raw) || arg_count == expected {
            diagnostics.push(Diagnostic::error(
                "E_INVALID_FUNCTION_ARGUMENT",
                format!("FUNCTION {name} has unsupported or ambiguous argument syntax: {raw}"),
                paragraph.span.clone(),
            ));
        } else {
            diagnostics.push(Diagnostic::error(
                "E_INVALID_FUNCTION_ARITY",
                format!(
                    "FUNCTION {name} expects {expected} argument(s) but got {arg_count}: {raw}"
                ),
                paragraph.span.clone(),
            ));
        }
    } else {
        diagnostics.push(Diagnostic::error(
            "E_UNSUPPORTED_FUNCTION_OPERAND",
            format!("FUNCTION {name} in {raw} is not enabled for converter codegen"),
            paragraph.span.clone(),
        ));
    }
}

fn analyze_compute_expression_functions(
    expression: &str,
    paragraph: &ParagraphIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let clean = strip_outer_parens(expression).trim();
    if clean.is_empty()
        || is_numeric_literal(clean)
        || ((clean.starts_with('"') && clean.ends_with('"'))
            || (clean.starts_with('\'') && clean.ends_with('\'')))
    {
        return;
    }
    if let Some((left, _, right)) = split_subscript_binary(clean, &['+', '-']) {
        analyze_compute_expression_functions(left, paragraph, diagnostics);
        analyze_compute_expression_functions(right, paragraph, diagnostics);
        return;
    }
    if let Some((left, _, right)) = split_subscript_binary(clean, &['*', '/']) {
        analyze_compute_expression_functions(left, paragraph, diagnostics);
        analyze_compute_expression_functions(right, paragraph, diagnostics);
        return;
    }
    if let Some(function) = parse_function_operand(clean) {
        analyze_function_operand(&function, paragraph, diagnostics);
    }
}

fn intrinsic_function_expected_arity(name: &str) -> Option<usize> {
    match name {
        "LENGTH" | "ORD" | "NUMVAL" => Some(1),
        _ => None,
    }
}

fn function_call_parentheses_malformed(raw: &str) -> bool {
    let clean = raw.trim();
    let upper = clean.to_ascii_uppercase();
    if !upper.starts_with("FUNCTION") {
        return false;
    }
    let rest = clean["FUNCTION".len()..].trim();
    let Some((_, tail)) = split_function_name_tail(rest) else {
        return false;
    };
    if !tail.starts_with('(') {
        return false;
    }
    parenthesized_function_arg_text(tail, 0).is_none()
}

fn single_function_arg_text_is_supported(arg_text: &str) -> bool {
    let clean = strip_outer_parens(arg_text).trim();
    if clean.is_empty() {
        return false;
    }
    if parse_function_operand(clean).is_some()
        || is_numeric_literal(clean)
        || is_complete_quoted_literal(clean)
    {
        return true;
    }

    let words = cobol_text::split_cobol_words(clean);
    if words.is_empty() {
        return false;
    }
    if words.len() == 1 {
        let upper = words[0].to_ascii_uppercase();
        return matches!(
            upper.as_str(),
            "ZERO"
                | "ZEROES"
                | "ZEROS"
                | "SPACE"
                | "SPACES"
                | "HIGH-VALUE"
                | "HIGH-VALUES"
                | "LOW-VALUE"
                | "LOW-VALUES"
                | "QUOTE"
                | "QUOTES"
                | "TRUE"
                | "FALSE"
        ) || !upper.is_empty();
    }
    if words.len() == 2 && words[0].eq_ignore_ascii_case("ALL") {
        let value = words[1].trim();
        return is_complete_quoted_literal(value);
    }
    if words.len() >= 3 && words.len() % 2 == 1 {
        return words.iter().enumerate().all(|(idx, word)| {
            if idx % 2 == 1 {
                word.eq_ignore_ascii_case("OF") || word.eq_ignore_ascii_case("IN")
            } else {
                !word.eq_ignore_ascii_case("OF")
                    && !word.eq_ignore_ascii_case("IN")
                    && !word.trim().is_empty()
            }
        });
    }

    false
}

fn is_complete_quoted_literal(value: &str) -> bool {
    let clean = value.trim();
    if !(clean.starts_with('"') || clean.starts_with('\'')) {
        return false;
    }
    cobol_text::quoted_literal_end(clean, 0)
        .map(|end| clean[end..].trim().is_empty())
        .unwrap_or(false)
}

fn validate_sort_procedure_semantics(
    sort: &SortProcedureIr,
    env: &SemanticEnv<'_>,
    paragraph: &ParagraphIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(file) = env
        .files
        .iter()
        .find(|file| file.name.eq_ignore_ascii_case(&sort.file))
    else {
        diagnostics.push(Diagnostic::error(
            "E_INVALID_SORT_FILE",
            format!("SORT file {} is not declared as an SD file", sort.file),
            paragraph.span.clone(),
        ));
        return;
    };
    if file.kind != FileKindIr::Sd {
        diagnostics.push(Diagnostic::error(
            "E_INVALID_SORT_FILE",
            format!("SORT file {} is not an SD sort file", sort.file),
            paragraph.span.clone(),
        ));
    }
    let Some(record_name) = &file.record_name else {
        diagnostics.push(Diagnostic::error(
            "E_INVALID_SORT_FILE",
            format!("SORT file {} has no resolved 01 SD record", sort.file),
            paragraph.span.clone(),
        ));
        return;
    };
    validate_sort_record_is_fixed(record_name, env, paragraph, diagnostics);
    if let Some(key) = &sort.key {
        validate_sort_key(key, record_name, env, paragraph, diagnostics);
    }
    if let Some(input) = &sort.input_range {
        validate_procedure_range("SORT INPUT PROCEDURE", input, env, paragraph, diagnostics);
    }
    validate_procedure_range(
        "SORT OUTPUT PROCEDURE",
        &sort.output_range,
        env,
        paragraph,
        diagnostics,
    );
}

fn validate_sort_release_semantics(
    release: &ReleaseSortRecordIr,
    env: &SemanticEnv<'_>,
    paragraph: &ParagraphIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !resolved_ref_is_data(env.data_index, &release.record) {
        diagnostics.push(Diagnostic::error(
            "E_UNRESOLVED_DATA",
            format!(
                "RELEASE record {} does not resolve to data storage",
                release.record.raw
            ),
            paragraph.span.clone(),
        ));
    }
    if let Some(source) = &release.from {
        if !resolved_ref_is_data(env.data_index, source) {
            diagnostics.push(Diagnostic::error(
                "E_UNRESOLVED_DATA",
                format!(
                    "RELEASE FROM source {} does not resolve to data storage",
                    source.raw
                ),
                paragraph.span.clone(),
            ));
        }
    }
}

fn validate_sort_return_semantics(
    ret: &ReturnSortRecordIr,
    env: &SemanticEnv<'_>,
    paragraph: &ParagraphIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match env
        .files
        .iter()
        .find(|file| file.name.eq_ignore_ascii_case(&ret.file))
    {
        Some(file) if file.kind == FileKindIr::Sd => {}
        Some(_) => diagnostics.push(Diagnostic::error(
            "E_INVALID_SORT_FILE",
            format!("RETURN file {} is not an SD sort file", ret.file),
            paragraph.span.clone(),
        )),
        None => diagnostics.push(Diagnostic::error(
            "E_INVALID_SORT_FILE",
            format!("RETURN file {} is not declared as an SD file", ret.file),
            paragraph.span.clone(),
        )),
    }
    if let Some(target) = &ret.into {
        if !resolved_ref_is_data(env.data_index, target) {
            diagnostics.push(Diagnostic::error(
                "E_UNRESOLVED_DATA",
                format!(
                    "RETURN INTO target {} does not resolve to data storage",
                    target.raw
                ),
                paragraph.span.clone(),
            ));
        }
    }
}

fn validate_procedure_range(
    context: &str,
    range: &ProcedureRangeIr,
    env: &SemanticEnv<'_>,
    paragraph: &ParagraphIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if paragraph_index(env.paragraphs, &range.target).is_none() {
        diagnostics.push(Diagnostic::error(
            "E_UNRESOLVED_PARAGRAPH",
            format!(
                "{context} target {} does not resolve to a paragraph",
                range.target
            ),
            paragraph.span.clone(),
        ));
    }
    if let Some(through) = &range.through {
        if paragraph_index(env.paragraphs, through).is_none() {
            diagnostics.push(Diagnostic::error(
                "E_UNRESOLVED_PARAGRAPH",
                format!("{context} THRU target {through} does not resolve to a paragraph"),
                paragraph.span.clone(),
            ));
        }
    }
}

fn validate_paragraph_reachability(paragraphs: &[ParagraphIr], diagnostics: &mut Vec<Diagnostic>) {
    if paragraphs.is_empty() {
        return;
    }

    let mut reachable = vec![false; paragraphs.len()];
    let mut stack = vec![0usize];
    while let Some(idx) = stack.pop() {
        if idx >= paragraphs.len() || reachable[idx] {
            continue;
        }
        reachable[idx] = true;
        for target_idx in paragraph_successors(paragraphs, idx) {
            if !reachable[target_idx] {
                stack.push(target_idx);
            }
        }
    }

    for (idx, paragraph) in paragraphs.iter().enumerate().skip(1) {
        if !reachable[idx] {
            diagnostics.push(Diagnostic::warning(
                "W_UNREACHABLE_PARAGRAPH",
                format!(
                    "paragraph {} is not reachable from procedure entry",
                    paragraph.name
                ),
                paragraph.span.clone(),
            ));
        }
    }
}

fn validate_statement_reachability(paragraphs: &[ParagraphIr], diagnostics: &mut Vec<Diagnostic>) {
    for paragraph in paragraphs {
        let mut terminal_seen = None;
        for (statement_idx, statement) in paragraph.statements.iter().enumerate() {
            if let Some(terminal_idx) = terminal_seen {
                diagnostics.push(Diagnostic::warning(
                    "W_UNREACHABLE_STATEMENT",
                    format!(
                        "statement {} in paragraph {} is unreachable after terminal statement {}",
                        statement_idx + 1,
                        paragraph.name,
                        terminal_idx + 1
                    ),
                    paragraph.span.clone(),
                ));
                continue;
            }
            if statement_is_terminal_transfer(statement, paragraphs) {
                terminal_seen = Some(statement_idx);
            }
        }
    }
}

fn statement_is_terminal_transfer(statement: &StatementIr, paragraphs: &[ParagraphIr]) -> bool {
    match statement {
        StatementIr::Goback | StatementIr::StopRun => true,
        StatementIr::GoTo(target) => paragraph_index(paragraphs, target).is_some(),
        _ => false,
    }
}

fn validate_perform_thru_range_integrity(
    target: &str,
    through: &str,
    start_idx: usize,
    end_idx: usize,
    paragraphs: &[ParagraphIr],
    callsite: &ParagraphIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for paragraph in &paragraphs[start_idx..=end_idx] {
        for statement in &paragraph.statements {
            match statement {
                StatementIr::GoTo(goto_target)
                    if !paragraph_target_in_range(paragraphs, goto_target, start_idx, end_idx) =>
                {
                    diagnostics.push(Diagnostic::error(
                        "E_PERFORM_THRU_ESCAPES",
                        format!(
                            "PERFORM {target} THRU {through} range contains GO TO {goto_target} in paragraph {}, which leaves the performed range",
                            paragraph.name
                        ),
                        callsite.span.clone(),
                    ));
                }
                StatementIr::ComputedGoTo { targets, .. } => {
                    for goto_target in targets {
                        if !paragraph_target_in_range(paragraphs, goto_target, start_idx, end_idx) {
                            diagnostics.push(Diagnostic::error(
                                "E_PERFORM_THRU_ESCAPES",
                                format!(
                                    "PERFORM {target} THRU {through} range contains computed GO TO target {goto_target} in paragraph {}, which leaves the performed range",
                                    paragraph.name
                                ),
                                callsite.span.clone(),
                            ));
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn paragraph_target_in_range(
    paragraphs: &[ParagraphIr],
    target: &str,
    start_idx: usize,
    end_idx: usize,
) -> bool {
    paragraph_index(paragraphs, target)
        .map(|idx| (start_idx..=end_idx).contains(&idx))
        .unwrap_or(false)
}

fn paragraph_successors(paragraphs: &[ParagraphIr], idx: usize) -> Vec<usize> {
    let paragraph = &paragraphs[idx];
    let mut successors = Vec::new();
    let mut terminal = false;
    for statement in &paragraph.statements {
        collect_statement_successors(paragraphs, statement, &mut successors);
        if statement_is_terminal_transfer(statement, paragraphs) {
            terminal = true;
            break;
        }
    }
    if !terminal && idx + 1 < paragraphs.len() {
        successors.push(idx + 1);
    }
    successors.sort_unstable();
    successors.dedup();
    successors
}

fn collect_statement_successors(
    paragraphs: &[ParagraphIr],
    statement: &StatementIr,
    successors: &mut Vec<usize>,
) {
    match statement {
        StatementIr::Perform {
            target, through, ..
        } => push_paragraph_range_successors(paragraphs, target, through.as_deref(), successors),
        StatementIr::GoTo(target) => push_paragraph_successor(paragraphs, target, successors),
        StatementIr::ComputedGoTo { targets, .. } => {
            for target in targets {
                push_paragraph_successor(paragraphs, target, successors);
            }
        }
        StatementIr::Alter { target, .. } => {
            push_paragraph_successor(paragraphs, target, successors)
        }
        StatementIr::SortProcedure(sort) => {
            if let Some(input_range) = &sort.input_range {
                push_paragraph_range_successors(
                    paragraphs,
                    &input_range.target,
                    input_range.through.as_deref(),
                    successors,
                );
            }
            push_paragraph_range_successors(
                paragraphs,
                &sort.output_range.target,
                sort.output_range.through.as_deref(),
                successors,
            );
        }
        StatementIr::Compute {
            on_size_error_ops,
            not_on_size_error_ops,
            ..
        } => {
            collect_statement_list_successors(paragraphs, on_size_error_ops, successors);
            collect_statement_list_successors(paragraphs, not_on_size_error_ops, successors);
        }
        StatementIr::If {
            then_statements,
            else_statements,
            ..
        } => {
            collect_statement_list_successors(paragraphs, then_statements, successors);
            collect_statement_list_successors(paragraphs, else_statements, successors);
        }
        StatementIr::Evaluate(evaluate) => {
            for arm in &evaluate.arms {
                collect_statement_list_successors(paragraphs, &arm.statements, successors);
            }
        }
        StatementIr::Search(search) => {
            collect_statement_list_successors(paragraphs, &search.at_end, successors);
            for when in &search.whens {
                collect_statement_list_successors(paragraphs, &when.statements, successors);
            }
        }
        StatementIr::SearchAll(search) => {
            collect_statement_list_successors(paragraphs, &search.at_end, successors);
            collect_statement_list_successors(paragraphs, &search.statements, successors);
        }
        StatementIr::ReturnSortRecord(ret) => {
            collect_statement_list_successors(paragraphs, &ret.at_end_ops, successors);
            collect_statement_list_successors(paragraphs, &ret.not_at_end_ops, successors);
        }
        StatementIr::ReadFile(read) => {
            collect_statement_list_successors(paragraphs, &read.at_end_ops, successors);
            collect_statement_list_successors(paragraphs, &read.not_at_end_ops, successors);
            collect_statement_list_successors(paragraphs, &read.on_exception_ops, successors);
        }
        StatementIr::StartFile(start) => {
            collect_statement_list_successors(paragraphs, &start.invalid_key_ops, successors);
            collect_statement_list_successors(paragraphs, &start.not_invalid_key_ops, successors);
        }
        StatementIr::WriteFile(write) => {
            collect_statement_list_successors(paragraphs, &write.invalid_key_ops, successors);
            collect_statement_list_successors(paragraphs, &write.not_invalid_key_ops, successors);
            collect_statement_list_successors(paragraphs, &write.on_exception_ops, successors);
            collect_statement_list_successors(paragraphs, &write.not_on_exception_ops, successors);
        }
        StatementIr::RewriteFile(rewrite) => {
            collect_statement_list_successors(paragraphs, &rewrite.invalid_key_ops, successors);
            collect_statement_list_successors(paragraphs, &rewrite.not_invalid_key_ops, successors);
        }
        StatementIr::DeleteFile(delete) => {
            collect_statement_list_successors(paragraphs, &delete.invalid_key_ops, successors);
            collect_statement_list_successors(paragraphs, &delete.not_invalid_key_ops, successors);
        }
        StatementIr::StringOp(string) => {
            collect_statement_list_successors(paragraphs, &string.on_overflow_ops, successors);
            collect_statement_list_successors(paragraphs, &string.not_on_overflow_ops, successors);
        }
        StatementIr::UnstringOp(unstring) => {
            collect_statement_list_successors(paragraphs, &unstring.on_overflow_ops, successors);
            collect_statement_list_successors(
                paragraphs,
                &unstring.not_on_overflow_ops,
                successors,
            );
        }
        _ => {}
    }
}

fn collect_statement_list_successors(
    paragraphs: &[ParagraphIr],
    statements: &[StatementIr],
    successors: &mut Vec<usize>,
) {
    for statement in statements {
        collect_statement_successors(paragraphs, statement, successors);
    }
}

fn push_paragraph_range_successors(
    paragraphs: &[ParagraphIr],
    target: &str,
    through: Option<&str>,
    successors: &mut Vec<usize>,
) {
    let Some(target_idx) = paragraph_index(paragraphs, target) else {
        return;
    };
    successors.push(target_idx);
    if let Some(through) = through {
        if let Some(through_idx) = paragraph_index(paragraphs, through) {
            let (start, end) = if target_idx <= through_idx {
                (target_idx, through_idx)
            } else {
                (through_idx, target_idx)
            };
            successors.extend(start..=end);
        }
    }
}

fn push_paragraph_successor(paragraphs: &[ParagraphIr], target: &str, successors: &mut Vec<usize>) {
    if let Some(target_idx) = paragraph_index(paragraphs, target) {
        successors.push(target_idx);
    }
}

fn paragraph_has_altered_goto_slot(paragraphs: &[ParagraphIr], paragraph_name: &str) -> bool {
    paragraphs.iter().any(|paragraph| {
        paragraph
            .statements
            .iter()
            .any(|statement| statement_alters_paragraph(statement, paragraph_name))
    })
}

fn statement_alters_paragraph(statement: &StatementIr, paragraph_name: &str) -> bool {
    match statement {
        StatementIr::Alter { paragraph, .. } => paragraph.eq_ignore_ascii_case(paragraph_name),
        StatementIr::Compute {
            on_size_error_ops,
            not_on_size_error_ops,
            ..
        } => {
            statement_list_alters_paragraph(on_size_error_ops, paragraph_name)
                || statement_list_alters_paragraph(not_on_size_error_ops, paragraph_name)
        }
        StatementIr::If {
            then_statements,
            else_statements,
            ..
        } => {
            statement_list_alters_paragraph(then_statements, paragraph_name)
                || statement_list_alters_paragraph(else_statements, paragraph_name)
        }
        StatementIr::Evaluate(evaluate) => evaluate
            .arms
            .iter()
            .any(|arm| statement_list_alters_paragraph(&arm.statements, paragraph_name)),
        StatementIr::Search(search) => {
            statement_list_alters_paragraph(&search.at_end, paragraph_name)
                || search
                    .whens
                    .iter()
                    .any(|when| statement_list_alters_paragraph(&when.statements, paragraph_name))
        }
        StatementIr::SearchAll(search) => {
            statement_list_alters_paragraph(&search.at_end, paragraph_name)
                || statement_list_alters_paragraph(&search.statements, paragraph_name)
        }
        StatementIr::ReturnSortRecord(ret) => {
            statement_list_alters_paragraph(&ret.at_end_ops, paragraph_name)
                || statement_list_alters_paragraph(&ret.not_at_end_ops, paragraph_name)
        }
        StatementIr::ReadFile(read) => {
            statement_list_alters_paragraph(&read.at_end_ops, paragraph_name)
                || statement_list_alters_paragraph(&read.not_at_end_ops, paragraph_name)
                || statement_list_alters_paragraph(&read.on_exception_ops, paragraph_name)
        }
        StatementIr::StartFile(start) => {
            statement_list_alters_paragraph(&start.invalid_key_ops, paragraph_name)
                || statement_list_alters_paragraph(&start.not_invalid_key_ops, paragraph_name)
        }
        StatementIr::WriteFile(write) => {
            statement_list_alters_paragraph(&write.invalid_key_ops, paragraph_name)
                || statement_list_alters_paragraph(&write.not_invalid_key_ops, paragraph_name)
                || statement_list_alters_paragraph(&write.on_exception_ops, paragraph_name)
                || statement_list_alters_paragraph(&write.not_on_exception_ops, paragraph_name)
        }
        StatementIr::RewriteFile(rewrite) => {
            statement_list_alters_paragraph(&rewrite.invalid_key_ops, paragraph_name)
                || statement_list_alters_paragraph(&rewrite.not_invalid_key_ops, paragraph_name)
        }
        StatementIr::DeleteFile(delete) => {
            statement_list_alters_paragraph(&delete.invalid_key_ops, paragraph_name)
                || statement_list_alters_paragraph(&delete.not_invalid_key_ops, paragraph_name)
        }
        StatementIr::StringOp(string) => {
            statement_list_alters_paragraph(&string.on_overflow_ops, paragraph_name)
                || statement_list_alters_paragraph(&string.not_on_overflow_ops, paragraph_name)
        }
        StatementIr::UnstringOp(unstring) => {
            statement_list_alters_paragraph(&unstring.on_overflow_ops, paragraph_name)
                || statement_list_alters_paragraph(&unstring.not_on_overflow_ops, paragraph_name)
        }
        _ => false,
    }
}

fn statement_list_alters_paragraph(statements: &[StatementIr], paragraph_name: &str) -> bool {
    statements
        .iter()
        .any(|statement| statement_alters_paragraph(statement, paragraph_name))
}

fn validate_sort_record_is_fixed(
    record_name: &str,
    env: &SemanticEnv<'_>,
    paragraph: &ParagraphIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let record_key = normalize_name(record_name);
    for item in env.data_items {
        let qualified = item.qualified_name.to_ascii_uppercase();
        if (qualified == record_key || qualified.starts_with(&format!("{record_key}.")))
            && item
                .occurs
                .as_ref()
                .and_then(|occurs| occurs.depending_on.as_ref())
                .is_some()
        {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_SORT_RECORD_ODO",
                format!(
                    "SD record {record_name} contains OCCURS DEPENDING ON item {}; in-memory SORT requires a fixed-length SD record",
                    item.qualified_name
                ),
                paragraph.span.clone(),
            ));
        }
    }
}

fn validate_sort_key(
    key: &SortKeyIr,
    record_name: &str,
    env: &SemanticEnv<'_>,
    paragraph: &ParagraphIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let key_ref = parse_data_ref(&key.name);
    let key_item = match env.data_index.resolve_ref(&key_ref) {
        DataResolution::Resolved(item) => item,
        DataResolution::Condition(_) => {
            diagnostics.push(Diagnostic::error(
                "E_INVALID_SORT_KEY",
                format!("SORT KEY {} resolves to a condition-name", key.name),
                paragraph.span.clone(),
            ));
            return;
        }
        DataResolution::Ambiguous(candidates) => {
            diagnostics.push(Diagnostic::error(
                "E_AMBIGUOUS_DATA",
                format!(
                    "SORT KEY {} is ambiguous: {}",
                    key.name,
                    candidates.join(", ")
                ),
                paragraph.span.clone(),
            ));
            return;
        }
        DataResolution::Missing | DataResolution::Special { .. } => {
            diagnostics.push(Diagnostic::error(
                "E_UNRESOLVED_DATA",
                format!("SORT KEY {} does not resolve to data storage", key.name),
                paragraph.span.clone(),
            ));
            return;
        }
    };
    let record_key = normalize_name(record_name);
    let key_qualified = key_item.qualified_name.to_ascii_uppercase();
    if key_qualified != record_key && !key_qualified.starts_with(&format!("{record_key}.")) {
        diagnostics.push(Diagnostic::error(
            "E_INVALID_SORT_KEY",
            format!(
                "SORT KEY {} is not contained in SD record {record_name}",
                key.name
            ),
            paragraph.span.clone(),
        ));
    }
    validate_sort_key_numeric_metadata(key, key_item, paragraph, diagnostics);
    if matches!(
        key_item.value_category,
        ValueCategoryIr::Binary | ValueCategoryIr::NativeBinary | ValueCategoryIr::Float
    ) {
        diagnostics.push(Diagnostic::error(
            "E_UNSUPPORTED_SORT_KEY",
            format!(
                "SORT KEY {} uses {:?}; binary/float sort key comparison remains fail-closed",
                key.name, key_item.value_category
            ),
            paragraph.span.clone(),
        ));
    }
}

fn validate_sort_key_numeric_metadata(
    key: &SortKeyIr,
    key_item: &DataItemIr,
    paragraph: &ParagraphIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match key_item.value_category {
        ValueCategoryIr::NumericDisplay | ValueCategoryIr::PackedDecimal => {
            let Some(picture) = &key_item.picture_ast else {
                diagnostics.push(Diagnostic::error(
                    "E_INVALID_SORT_KEY",
                    format!(
                        "SORT KEY {} is numeric but has no resolved PICTURE metadata",
                        key.name
                    ),
                    paragraph.span.clone(),
                ));
                return;
            };
            if picture.digits == 0 {
                diagnostics.push(Diagnostic::error(
                    "E_INVALID_SORT_KEY",
                    format!("SORT KEY {} has zero numeric digits", key.name),
                    paragraph.span.clone(),
                ));
            }
            if key_item.value_category == ValueCategoryIr::PackedDecimal {
                let expected = packed_decimal_len(picture.digits);
                if key_item.byte_len != Some(expected) {
                    diagnostics.push(Diagnostic::error(
                        "E_INVALID_SORT_KEY",
                        format!(
                            "SORT KEY {} packed byte length {:?} does not match expected COMP-3 length {}",
                            key.name, key_item.byte_len, expected
                        ),
                        paragraph.span.clone(),
                    ));
                }
            }
        }
        _ => {}
    }
}

fn resolved_ref_is_data(data_index: &DataReferenceIndex<'_>, reference: &DataRefIr) -> bool {
    matches!(
        data_index.resolve_ref(reference),
        DataResolution::Resolved(_) | DataResolution::Special { .. }
    )
}

fn validate_open_file(
    open: &OpenFileIr,
    files: &[FileIr],
    data_index: &DataReferenceIndex<'_>,
) -> Result<(), String> {
    validate_file_target(&open.file, files, data_index, None)
}

fn validate_start_file(
    start: &StartFileIr,
    files: &[FileIr],
    data_index: &DataReferenceIndex<'_>,
) -> Result<(), String> {
    validate_file_target(&start.file, files, data_index, None)?;
    if !start.unsupported_options.is_empty() {
        return Err(format!(
            "file {} START options are not executable yet: {}",
            start.file,
            start.unsupported_options.join(" ")
        ));
    }
    Ok(())
}

fn validate_read_file(
    read: &ReadFileIr,
    files: &[FileIr],
    data_index: &DataReferenceIndex<'_>,
) -> Result<(), String> {
    validate_file_target(&read.file, files, data_index, read.into.as_ref())
}

fn validate_write_file(
    write: &WriteFileIr,
    files: &[FileIr],
    data_index: &DataReferenceIndex<'_>,
) -> Result<(), String> {
    let target = file_name_for_record_ref(&write.record, files)
        .unwrap_or_else(|| write.record.normalized.clone());
    validate_file_target(&target, files, data_index, None)
}

fn validate_rewrite_file(
    rewrite: &RewriteFileIr,
    files: &[FileIr],
    data_index: &DataReferenceIndex<'_>,
) -> Result<(), String> {
    let target = file_name_for_record_ref(&rewrite.record, files)
        .unwrap_or_else(|| rewrite.record.normalized.clone());
    validate_file_target(&target, files, data_index, None)
}

fn validate_delete_file(
    delete: &DeleteFileIr,
    files: &[FileIr],
    data_index: &DataReferenceIndex<'_>,
) -> Result<(), String> {
    validate_file_target(&delete.file, files, data_index, None)
}

fn validate_unlock_file(
    unlock: &UnlockFileIr,
    files: &[FileIr],
    data_index: &DataReferenceIndex<'_>,
) -> Result<(), String> {
    validate_file_target(&unlock.file, files, data_index, None)
}

fn validate_close_file(
    close: &CloseFileIr,
    files: &[FileIr],
    data_index: &DataReferenceIndex<'_>,
) -> Result<(), String> {
    validate_file_target(&close.file, files, data_index, None)
}

fn validate_file_target(
    target: &str,
    files: &[FileIr],
    data_index: &DataReferenceIndex<'_>,
    read_into: Option<&DataRefIr>,
) -> Result<(), String> {
    let Some(file) = resolve_file_target(target, files) else {
        return Err(format!("file IO target {target} has no SELECT/FD metadata"));
    };
    validate_file_metadata(file, data_index, read_into)
}

fn resolve_file_target<'a>(target: &str, files: &'a [FileIr]) -> Option<&'a FileIr> {
    files
        .iter()
        .find(|file| file.name.eq_ignore_ascii_case(target))
        .or_else(|| {
            files.iter().find(|file| {
                file.record_name
                    .as_ref()
                    .map(|record| record.eq_ignore_ascii_case(target))
                    .unwrap_or(false)
            })
        })
}

fn file_name_for_record_ref(record: &DataRefIr, files: &[FileIr]) -> Option<String> {
    files
        .iter()
        .find(|file| {
            file.record_name
                .as_ref()
                .map(|name| name.eq_ignore_ascii_case(&record.normalized))
                .unwrap_or(false)
        })
        .map(|file| file.name.clone())
}

fn validate_file_metadata(
    file: &FileIr,
    data_index: &DataReferenceIndex<'_>,
    read_into: Option<&DataRefIr>,
) -> Result<(), String> {
    if file.record_name.is_none() {
        return Err(format!("file {} has no FD record description", file.name));
    }
    let mut unsupported_file_metadata = Vec::new();
    if file.assign.is_some() && !file.assign_is_literal {
        unsupported_file_metadata.push(format!(
            "file {} ASSIGN target is not a static literal path; dynamic ASSIGN is not executable yet; runtime platform config supports static file bindings only",
            file.name
        ));
    }
    if let Some(organization) = &file.organization {
        if organization.eq_ignore_ascii_case("LINE SEQUENTIAL") {
            unsupported_file_metadata.push(format!(
                "file {} organization LINE SEQUENTIAL is not executable yet; runtime platform config currently supports fixed-length SEQUENTIAL only",
                file.name
            ));
        } else if !organization.eq_ignore_ascii_case("SEQUENTIAL") {
            unsupported_file_metadata.push(format!(
                "file {} organization {organization} is not executable yet; runtime platform config fails closed for indexed, relative, and VSAM files",
                file.name
            ));
        }
    }
    if let Some(access_mode) = &file.access_mode {
        if !access_mode.eq_ignore_ascii_case("SEQUENTIAL") {
            unsupported_file_metadata.push(format!(
                "file {} access mode {access_mode} is not executable yet",
                file.name
            ));
        }
    }
    if let Some(record_name) = &file.record_name {
        if let DataResolution::Resolved(record) =
            data_index.resolve_ref(&DataRefIr::simple(record_name.clone()))
        {
            if data_index.has_dynamic_occurs_in_subtree(record) {
                unsupported_file_metadata.push(format!(
                    "file {} FD record {} contains OCCURS DEPENDING ON; variable-length file records are not executable yet",
                    file.name, record_name
                ));
            }
        }
    }
    if let Some(target) = read_into {
        match data_index.resolve_ref(target) {
            DataResolution::Resolved(item) => {
                if data_index.has_dynamic_occurs_context(item) {
                    unsupported_file_metadata.push(format!(
                        "file {} READ INTO target {} is inside OCCURS DEPENDING ON storage; nested ODO READ INTO targets are not executable yet",
                        file.name, target.normalized
                    ));
                } else if data_index.has_dynamic_occurs_in_subtree(item) {
                    let record_result = file
                        .record_name
                        .as_ref()
                        .map(|record_name| {
                            data_index.resolve_ref(&DataRefIr::simple(record_name.clone()))
                        })
                        .unwrap_or(DataResolution::Missing);
                    match record_result {
                        DataResolution::Resolved(record) => {
                            if let Err(message) =
                                validate_read_into_odo_shape(file, record, item, data_index)
                            {
                                unsupported_file_metadata.push(message);
                            }
                        }
                        _ => unsupported_file_metadata.push(format!(
                            "file {} READ INTO target {} cannot validate ODO target because the FD record is unresolved",
                            file.name, target.normalized
                        )),
                    }
                }
            }
            DataResolution::Condition(_) => unsupported_file_metadata.push(format!(
                "file {} READ INTO target {} resolves to a condition-name",
                file.name, target.normalized
            )),
            DataResolution::Special { .. } => unsupported_file_metadata.push(format!(
                "file {} READ INTO target {} resolves to a special register",
                file.name, target.normalized
            )),
            DataResolution::Ambiguous(candidates) => unsupported_file_metadata.push(format!(
                "file {} READ INTO target {} is ambiguous: {}",
                file.name,
                target.normalized,
                candidates.join(", ")
            )),
            DataResolution::Missing => unsupported_file_metadata.push(format!(
                "file {} READ INTO target {} does not resolve to data storage",
                file.name, target.normalized
            )),
        }
    }
    if !unsupported_file_metadata.is_empty() {
        return Err(unsupported_file_metadata.join("; "));
    }
    validate_file_status(file, data_index)
}

fn validate_file_status(file: &FileIr, data_index: &DataReferenceIndex<'_>) -> Result<(), String> {
    if let Some(status) = &file.file_status {
        let reference = DataRefIr::simple(status.clone());
        match data_index.resolve_ref(&reference) {
            DataResolution::Resolved(item)
                if item.byte_len.unwrap_or(0) >= 2
                    && item.value_category != ValueCategoryIr::Group => {}
            DataResolution::Special {
                byte_len, category, ..
            } if byte_len >= 2 && category != ValueCategoryIr::Group => {}
            DataResolution::Resolved(item) => {
                return Err(format!(
                    "file {} FILE STATUS item {} must be an elementary data item of at least 2 bytes, got {:?} length {:?}",
                    file.name, status, item.value_category, item.byte_len
                ));
            }
            DataResolution::Special {
                byte_len, category, ..
            } => {
                return Err(format!(
                    "file {} FILE STATUS item {} must be an elementary data item of at least 2 bytes, got {:?} length {:?}",
                    file.name, status, category, byte_len
                ));
            }
            DataResolution::Condition(_) => {
                return Err(format!(
                    "file {} FILE STATUS item {} resolves to a condition-name",
                    file.name, status
                ));
            }
            DataResolution::Ambiguous(candidates) => {
                return Err(format!(
                    "file {} FILE STATUS item {} is ambiguous: {}",
                    file.name,
                    status,
                    candidates.join(", ")
                ));
            }
            DataResolution::Missing => {
                return Err(format!(
                    "file {} FILE STATUS item {} does not resolve to data storage",
                    file.name, status
                ));
            }
        }
    }
    Ok(())
}

fn validate_read_into_odo_shape(
    file: &FileIr,
    record: &DataItemIr,
    target: &DataItemIr,
    data_index: &DataReferenceIndex<'_>,
) -> Result<(), String> {
    if target.value_category != ValueCategoryIr::Group {
        return Err(format!(
            "E_ODO_TARGET_INCOMPATIBLE_LENGTH: file {} READ INTO target {} contains OCCURS DEPENDING ON but is not a group",
            file.name, target.qualified_name
        ));
    }
    let odo_items = data_index.dynamic_occurs_items_in_subtree(target);
    let [odo_item] = odo_items.as_slice() else {
        return Err(format!(
            "E_ODO_TARGET_INCOMPATIBLE_LENGTH: file {} READ INTO target {} must contain exactly one OCCURS DEPENDING ON table, found {}",
            file.name,
            target.qualified_name,
            odo_items.len()
        ));
    };
    let Some(occurs) = &odo_item.occurs else {
        return Err(format!(
            "E_ODO_TARGET_INCOMPATIBLE_LENGTH: file {} READ INTO target {} has invalid ODO metadata",
            file.name, target.qualified_name
        ));
    };
    let Some(_depending_on) = &occurs.depending_on else {
        return Err(format!(
            "E_ODO_TARGET_INCOMPATIBLE_LENGTH: file {} READ INTO target {} has invalid DEPENDING ON metadata",
            file.name, target.qualified_name
        ));
    };
    let record_len = record.byte_len.unwrap_or(0);
    let target_len = target.byte_len.unwrap_or(0);
    let odo_len = odo_item.byte_len.unwrap_or(0);
    let element_len = odo_len / occurs.max.max(1);
    if record_len == 0 || target_len == 0 || odo_len == 0 || element_len == 0 {
        return Err(format!(
            "E_ODO_TARGET_INCOMPATIBLE_LENGTH: file {} READ INTO target {} has zero-length record or ODO layout",
            file.name, target.qualified_name
        ));
    }
    if target_len < odo_len {
        return Err(format!(
            "E_ODO_TARGET_INCOMPATIBLE_LENGTH: file {} READ INTO target {} has inconsistent ODO layout length",
            file.name, target.qualified_name
        ));
    }
    let fixed_len = target_len - odo_len;
    let Some(variable_len) = record_len.checked_sub(fixed_len) else {
        return Err(format!(
            "E_ODO_TARGET_INCOMPATIBLE_LENGTH: file {} fixed record length {record_len} is smaller than READ INTO target {} fixed prefix length {fixed_len}",
            file.name, target.qualified_name
        ));
    };
    if variable_len % element_len != 0 {
        return Err(format!(
            "E_ODO_TARGET_INCOMPATIBLE_LENGTH: file {} fixed record length {record_len} does not align with READ INTO target {} ODO element length {element_len}",
            file.name, target.qualified_name
        ));
    }
    let active = variable_len / element_len;
    if active < occurs.min || active > occurs.max {
        return Err(format!(
            "E_ODO_TARGET_INCOMPATIBLE_LENGTH: file {} fixed record length {record_len} maps READ INTO target {} to ODO count {active}, outside {}..={}",
            file.name, target.qualified_name, occurs.min, occurs.max
        ));
    }
    Ok(())
}

fn validate_set_index_expr(
    operation: &SetIndexOperationIr,
    data_index: &DataReferenceIndex<'_>,
    paragraph: &ParagraphIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for reference in set_index_expr_references(operation)
        .into_iter()
        .map(|(reference, _)| reference)
    {
        match data_index.resolve_ref(&reference) {
            DataResolution::Resolved(item) if category_is_numeric(item.value_category) => {}
            DataResolution::Resolved(item) => diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_SET_INDEX_OPERAND",
                format!(
                    "SET index expression {} resolves to nonnumeric category {:?}",
                    reference.raw, item.value_category
                ),
                paragraph.span.clone(),
            )),
            DataResolution::Condition(_) => diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_SET_INDEX_OPERAND",
                format!(
                    "SET index expression {} resolves to a condition-name, not a numeric item",
                    reference.raw
                ),
                paragraph.span.clone(),
            )),
            DataResolution::Special { category, .. } => diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_SET_INDEX_OPERAND",
                format!(
                    "SET index expression {} resolves to special-register category {:?}, not a numeric item",
                    reference.raw, category
                ),
                paragraph.span.clone(),
            )),
            DataResolution::Ambiguous(candidates) => diagnostics.push(Diagnostic::error(
                "E_AMBIGUOUS_SET_INDEX_OPERAND",
                format!(
                    "SET index expression {} is ambiguous: {}",
                    reference.raw,
                    candidates.join(", ")
                ),
                paragraph.span.clone(),
            )),
            DataResolution::Missing => diagnostics.push(Diagnostic::error(
                "E_UNRESOLVED_SET_INDEX_OPERAND",
                format!("SET index expression {} cannot be resolved", reference.raw),
                paragraph.span.clone(),
            )),
        }
    }
}

fn resolve_search_all_declared_keys(
    paragraphs: &mut [ParagraphIr],
    data_items: &[DataItemIr],
    conditions: &[ConditionNameIr],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let data_index = DataReferenceIndex::new(data_items, conditions);
    for paragraph in paragraphs {
        resolve_search_all_declared_keys_in_statements(
            &mut paragraph.statements,
            data_items,
            &data_index,
            &paragraph.span,
            diagnostics,
        );
    }
}

fn resolve_search_all_declared_keys_in_statements(
    statements: &mut [StatementIr],
    data_items: &[DataItemIr],
    data_index: &DataReferenceIndex<'_>,
    span: &cobol_ir::SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for statement in statements {
        match statement {
            StatementIr::Compute {
                on_size_error_ops,
                not_on_size_error_ops,
                ..
            } => {
                resolve_search_all_declared_keys_in_statements(
                    on_size_error_ops,
                    data_items,
                    data_index,
                    span,
                    diagnostics,
                );
                resolve_search_all_declared_keys_in_statements(
                    not_on_size_error_ops,
                    data_items,
                    data_index,
                    span,
                    diagnostics,
                );
            }
            StatementIr::If {
                then_statements,
                else_statements,
                ..
            } => {
                resolve_search_all_declared_keys_in_statements(
                    then_statements,
                    data_items,
                    data_index,
                    span,
                    diagnostics,
                );
                resolve_search_all_declared_keys_in_statements(
                    else_statements,
                    data_items,
                    data_index,
                    span,
                    diagnostics,
                );
            }
            StatementIr::Evaluate(evaluate) => {
                for arm in &mut evaluate.arms {
                    resolve_search_all_declared_keys_in_statements(
                        &mut arm.statements,
                        data_items,
                        data_index,
                        span,
                        diagnostics,
                    );
                }
            }
            StatementIr::Search(search) => {
                resolve_search_all_declared_keys_in_statements(
                    &mut search.at_end,
                    data_items,
                    data_index,
                    span,
                    diagnostics,
                );
                for when in &mut search.whens {
                    resolve_search_all_declared_keys_in_statements(
                        &mut when.statements,
                        data_items,
                        data_index,
                        span,
                        diagnostics,
                    );
                }
            }
            StatementIr::SearchAll(search) => {
                resolve_search_all_declared_key(search, data_items, data_index, span, diagnostics);
                resolve_search_all_declared_keys_in_statements(
                    &mut search.at_end,
                    data_items,
                    data_index,
                    span,
                    diagnostics,
                );
                resolve_search_all_declared_keys_in_statements(
                    &mut search.statements,
                    data_items,
                    data_index,
                    span,
                    diagnostics,
                );
            }
            StatementIr::ReturnSortRecord(ret) => {
                resolve_search_all_declared_keys_in_statements(
                    &mut ret.at_end_ops,
                    data_items,
                    data_index,
                    span,
                    diagnostics,
                );
                resolve_search_all_declared_keys_in_statements(
                    &mut ret.not_at_end_ops,
                    data_items,
                    data_index,
                    span,
                    diagnostics,
                );
            }
            StatementIr::StartFile(start) => {
                resolve_search_all_declared_keys_in_statements(
                    &mut start.invalid_key_ops,
                    data_items,
                    data_index,
                    span,
                    diagnostics,
                );
                resolve_search_all_declared_keys_in_statements(
                    &mut start.not_invalid_key_ops,
                    data_items,
                    data_index,
                    span,
                    diagnostics,
                );
            }
            StatementIr::ReadFile(read) => {
                resolve_search_all_declared_keys_in_statements(
                    &mut read.at_end_ops,
                    data_items,
                    data_index,
                    span,
                    diagnostics,
                );
                resolve_search_all_declared_keys_in_statements(
                    &mut read.not_at_end_ops,
                    data_items,
                    data_index,
                    span,
                    diagnostics,
                );
                resolve_search_all_declared_keys_in_statements(
                    &mut read.on_exception_ops,
                    data_items,
                    data_index,
                    span,
                    diagnostics,
                );
            }
            StatementIr::WriteFile(write) => {
                resolve_search_all_declared_keys_in_statements(
                    &mut write.invalid_key_ops,
                    data_items,
                    data_index,
                    span,
                    diagnostics,
                );
                resolve_search_all_declared_keys_in_statements(
                    &mut write.not_invalid_key_ops,
                    data_items,
                    data_index,
                    span,
                    diagnostics,
                );
                resolve_search_all_declared_keys_in_statements(
                    &mut write.on_exception_ops,
                    data_items,
                    data_index,
                    span,
                    diagnostics,
                );
                resolve_search_all_declared_keys_in_statements(
                    &mut write.not_on_exception_ops,
                    data_items,
                    data_index,
                    span,
                    diagnostics,
                );
            }
            StatementIr::RewriteFile(rewrite) => {
                resolve_search_all_declared_keys_in_statements(
                    &mut rewrite.invalid_key_ops,
                    data_items,
                    data_index,
                    span,
                    diagnostics,
                );
                resolve_search_all_declared_keys_in_statements(
                    &mut rewrite.not_invalid_key_ops,
                    data_items,
                    data_index,
                    span,
                    diagnostics,
                );
            }
            StatementIr::DeleteFile(delete) => {
                resolve_search_all_declared_keys_in_statements(
                    &mut delete.invalid_key_ops,
                    data_items,
                    data_index,
                    span,
                    diagnostics,
                );
                resolve_search_all_declared_keys_in_statements(
                    &mut delete.not_invalid_key_ops,
                    data_items,
                    data_index,
                    span,
                    diagnostics,
                );
            }
            StatementIr::StringOp(string) => {
                resolve_search_all_declared_keys_in_statements(
                    &mut string.on_overflow_ops,
                    data_items,
                    data_index,
                    span,
                    diagnostics,
                );
                resolve_search_all_declared_keys_in_statements(
                    &mut string.not_on_overflow_ops,
                    data_items,
                    data_index,
                    span,
                    diagnostics,
                );
            }
            StatementIr::UnstringOp(unstring) => {
                resolve_search_all_declared_keys_in_statements(
                    &mut unstring.on_overflow_ops,
                    data_items,
                    data_index,
                    span,
                    diagnostics,
                );
                resolve_search_all_declared_keys_in_statements(
                    &mut unstring.not_on_overflow_ops,
                    data_items,
                    data_index,
                    span,
                    diagnostics,
                );
            }
            _ => {}
        }
    }
}

fn resolve_search_all_declared_key(
    search: &mut SearchAllIr,
    data_items: &[DataItemIr],
    data_index: &DataReferenceIndex<'_>,
    span: &cobol_ir::SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let table_ref = DataRefIr::simple(search.table.clone());
    let DataResolution::Resolved(table_item) = data_index.resolve_ref(&table_ref) else {
        return;
    };
    let Some(occurs) = table_item.occurs.as_ref() else {
        return;
    };
    let Some(key) = occurs.keys.first() else {
        return;
    };
    let Some(key_item) = resolve_occurs_key_item(table_item, key, data_index) else {
        diagnostics.push(Diagnostic::error(
            "E_SEARCH_ALL_KEY_UNRESOLVED",
            format!(
                "SEARCH ALL table {} declares key {} but the key field cannot be resolved under the table's declared layout",
                table_item.qualified_name, key.name
            ),
            span.clone(),
        ));
        return;
    };
    search.declared_key = Some(SearchAllKeyIr {
        direction: key.direction,
        name: key.name.clone(),
        qualified_name: key_item.qualified_name.clone(),
        children: declared_key_children(key_item, data_items),
    });
}

fn resolve_occurs_key_item<'a>(
    table_item: &DataItemIr,
    key: &OccursKeyIr,
    data_index: &DataReferenceIndex<'a>,
) -> Option<&'a DataItemIr> {
    let candidate = if key.name.contains('.') {
        key.name.clone()
    } else {
        format!("{}.{}", table_item.qualified_name, key.name)
    };
    let resolved = match data_index.resolve(&candidate) {
        DataResolution::Resolved(item) => Some(item),
        _ => match data_index.resolve(&key.name) {
            DataResolution::Resolved(item) => Some(item),
            _ => None,
        },
    }?;
    resolved
        .path
        .starts_with(&table_item.path)
        .then_some(resolved)
}

fn declared_key_children(key_item: &DataItemIr, data_items: &[DataItemIr]) -> Vec<String> {
    let prefix = format!("{}.", key_item.qualified_name);
    data_items
        .iter()
        .filter(|item| item.addressable && item.qualified_name.starts_with(&prefix))
        .map(|item| item.qualified_name.clone())
        .collect()
}

fn validate_search_semantics(
    search: &SearchIr,
    env: &SemanticEnv<'_>,
    paragraph: &ParagraphIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let table_ref = DataRefIr::simple(search.table.clone());
    let table_item = match env.data_index.resolve_ref(&table_ref) {
        DataResolution::Resolved(item) => Some(item),
        DataResolution::Ambiguous(candidates) => {
            diagnostics.push(Diagnostic::error(
                "E_AMBIGUOUS_SEARCH_TABLE",
                format!(
                    "SEARCH table {} is ambiguous: {}",
                    search.table,
                    candidates.join(", ")
                ),
                paragraph.span.clone(),
            ));
            None
        }
        DataResolution::Condition(_) => {
            diagnostics.push(Diagnostic::error(
                "E_INVALID_SEARCH_TABLE",
                format!("SEARCH target {} is a condition-name", search.table),
                paragraph.span.clone(),
            ));
            None
        }
        DataResolution::Special { .. } => {
            diagnostics.push(Diagnostic::error(
                "E_INVALID_SEARCH_TABLE",
                format!("SEARCH target {} is a special register", search.table),
                paragraph.span.clone(),
            ));
            None
        }
        DataResolution::Missing => {
            diagnostics.push(Diagnostic::error(
                "E_UNRESOLVED_SEARCH_TABLE",
                format!("SEARCH table {} cannot be resolved", search.table),
                paragraph.span.clone(),
            ));
            None
        }
    };

    if let Some(item) = table_item {
        if item.occurs.is_none() {
            diagnostics.push(Diagnostic::error(
                "E_INVALID_SEARCH_TABLE",
                format!("SEARCH table {} is not an OCCURS item", item.qualified_name),
                paragraph.span.clone(),
            ));
        }
    }

    let inferred_index = table_item
        .and_then(|item| item.occurs.as_ref())
        .and_then(|occurs| occurs.indexed_by.first())
        .cloned();
    let index = search.index.as_ref().or(inferred_index.as_ref());
    let Some(index) = index else {
        diagnostics.push(Diagnostic::error(
            "E_UNSUPPORTED_SEARCH_INDEX",
            format!(
                "SEARCH table {} has no VARYING index and no INDEXED BY item",
                search.table
            ),
            paragraph.span.clone(),
        ));
        return;
    };
    if !env
        .indexes
        .iter()
        .any(|item| item.name.eq_ignore_ascii_case(index))
    {
        diagnostics.push(Diagnostic::error(
            "E_INVALID_SEARCH_INDEX",
            format!("SEARCH index {index} is not a resolved INDEXED BY item"),
            paragraph.span.clone(),
        ));
    }
    if search.whens.is_empty() {
        diagnostics.push(Diagnostic::error(
            "E_INVALID_SEARCH",
            format!("SEARCH table {} has no WHEN branch", search.table),
            paragraph.span.clone(),
        ));
    }
}

fn validate_search_all_semantics(
    search: &SearchAllIr,
    env: &SemanticEnv<'_>,
    paragraph: &ParagraphIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let table_ref = DataRefIr::simple(search.table.clone());
    let table_item = match env.data_index.resolve_ref(&table_ref) {
        DataResolution::Resolved(item) => Some(item),
        DataResolution::Ambiguous(candidates) => {
            diagnostics.push(Diagnostic::error(
                "E_AMBIGUOUS_SEARCH_TABLE",
                format!(
                    "SEARCH ALL table {} is ambiguous: {}",
                    search.table,
                    candidates.join(", ")
                ),
                paragraph.span.clone(),
            ));
            None
        }
        DataResolution::Condition(_) => {
            diagnostics.push(Diagnostic::error(
                "E_INVALID_SEARCH_TABLE",
                format!("SEARCH ALL target {} is a condition-name", search.table),
                paragraph.span.clone(),
            ));
            None
        }
        DataResolution::Special { .. } => {
            diagnostics.push(Diagnostic::error(
                "E_INVALID_SEARCH_TABLE",
                format!("SEARCH ALL target {} is a special register", search.table),
                paragraph.span.clone(),
            ));
            None
        }
        DataResolution::Missing => {
            diagnostics.push(Diagnostic::error(
                "E_UNRESOLVED_SEARCH_TABLE",
                format!("SEARCH ALL table {} cannot be resolved", search.table),
                paragraph.span.clone(),
            ));
            None
        }
    };

    let Some(item) = table_item else {
        return;
    };
    let Some(occurs) = item.occurs.as_ref() else {
        diagnostics.push(Diagnostic::error(
            "E_INVALID_SEARCH_TABLE",
            format!(
                "SEARCH ALL table {} is not an OCCURS item",
                item.qualified_name
            ),
            paragraph.span.clone(),
        ));
        return;
    };
    if occurs.keys.is_empty() {
        diagnostics.push(Diagnostic::error(
            "E_SEARCH_ALL_REQUIRES_KEY",
            format!(
                "SEARCH ALL table {} must declare ASCENDING or DESCENDING KEY metadata",
                item.qualified_name
            ),
            paragraph.span.clone(),
        ));
    }
    if occurs.keys.len() > 1 {
        diagnostics.push(Diagnostic::error(
            "E_UNSUPPORTED_SEARCH_ALL_MULTI_KEY",
            format!(
                "SEARCH ALL table {} declares multiple keys; binary-search lowering currently supports one key",
                item.qualified_name
            ),
            paragraph.span.clone(),
        ));
    }
    if search.declared_key.is_none() && !occurs.keys.is_empty() {
        diagnostics.push(Diagnostic::error(
            "E_SEARCH_ALL_KEY_UNRESOLVED",
            format!(
                "SEARCH ALL table {} has a declared key that cannot be resolved to the table's declared layout",
                item.qualified_name
            ),
            paragraph.span.clone(),
        ));
    }
    if let Some(declared_key) = &search.declared_key {
        if search_all_condition_target(&search.key_condition, declared_key).is_none() {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_SEARCH_ALL_CONDITION",
                format!(
                    "SEARCH ALL table {} must use a single equality relation against declared key {}",
                    item.qualified_name, declared_key.qualified_name
                ),
                paragraph.span.clone(),
            ));
        }
    }

    let inferred_index = occurs.indexed_by.first().cloned();
    let index = search.index.as_ref().or(inferred_index.as_ref());
    let Some(index) = index else {
        diagnostics.push(Diagnostic::error(
            "E_UNSUPPORTED_SEARCH_INDEX",
            format!(
                "SEARCH ALL table {} has no VARYING index and no INDEXED BY item",
                search.table
            ),
            paragraph.span.clone(),
        ));
        return;
    };
    if !env
        .indexes
        .iter()
        .any(|item| item.name.eq_ignore_ascii_case(index))
    {
        diagnostics.push(Diagnostic::error(
            "E_INVALID_SEARCH_INDEX",
            format!("SEARCH ALL index {index} is not a resolved INDEXED BY item"),
            paragraph.span.clone(),
        ));
    }
}

fn search_all_condition_target<'a>(
    condition: &'a ConditionIr,
    declared_key: &SearchAllKeyIr,
) -> Option<&'a ConditionOperandIr> {
    let ConditionIr::Relation { left, op, right } = condition else {
        return None;
    };
    if *op != RelOpIr::Equal {
        return None;
    }
    if search_all_operand_matches_key(left, declared_key) {
        Some(right)
    } else if search_all_operand_matches_key(right, declared_key) {
        Some(left)
    } else {
        None
    }
}

fn search_all_operand_matches_key(
    operand: &ConditionOperandIr,
    declared_key: &SearchAllKeyIr,
) -> bool {
    let ConditionOperandIr::Identifier(reference) = operand else {
        return false;
    };
    let reference_key = reference.normalized.to_ascii_uppercase();
    let key_name = declared_key.name.to_ascii_uppercase();
    let key_qualified = declared_key.qualified_name.to_ascii_uppercase();
    reference_key == key_name
        || reference_key == key_qualified
        || key_qualified.ends_with(&format!(".{reference_key}"))
}

fn analyze_move_semantics(
    source: &OperandIr,
    target: &DataRefIr,
    data_index: &DataReferenceIndex<'_>,
    paragraph: &ParagraphIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let target_category = resolved_category(data_index, target);
    if let Some(ValueCategoryIr::ConditionName) = target_category {
        diagnostics.push(Diagnostic::error(
            "E_INVALID_MOVE_TARGET",
            format!("condition name {} cannot receive MOVE data", target.raw),
            paragraph.span.clone(),
        ));
    }
    let Some(target_category) = target_category else {
        return;
    };
    let source_category = operand_category(data_index, source);
    if !move_compatible(source_category, target_category) {
        diagnostics.push(Diagnostic::error(
            "E_MOVE_CATEGORY_MISMATCH",
            format!(
                "MOVE from {:?} to {:?} is not enabled without an exact COBOL coercion rule",
                source_category, target_category
            ),
            paragraph.span.clone(),
        ));
    }
}

fn analyze_move_corresponding_semantics(
    source: &DataRefIr,
    target: &DataRefIr,
    data_index: &DataReferenceIndex<'_>,
    paragraph: &ParagraphIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let source_group = match data_index.resolve_ref(source) {
        DataResolution::Resolved(item) if item.value_category == ValueCategoryIr::Group => item,
        DataResolution::Resolved(_) => {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_MOVE_CORRESPONDING",
                format!(
                    "MOVE CORRESPONDING source {} must be a group item",
                    source.raw
                ),
                paragraph.span.clone(),
            ));
            return;
        }
        _ => {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_MOVE_CORRESPONDING",
                format!(
                    "MOVE CORRESPONDING source {} could not be resolved",
                    source.raw
                ),
                paragraph.span.clone(),
            ));
            return;
        }
    };
    let target_group = match data_index.resolve_ref(target) {
        DataResolution::Resolved(item) if item.value_category == ValueCategoryIr::Group => item,
        DataResolution::Resolved(_) => {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_MOVE_CORRESPONDING",
                format!(
                    "MOVE CORRESPONDING target {} must be a group item",
                    target.raw
                ),
                paragraph.span.clone(),
            ));
            return;
        }
        _ => {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_MOVE_CORRESPONDING",
                format!(
                    "MOVE CORRESPONDING target {} could not be resolved",
                    target.raw
                ),
                paragraph.span.clone(),
            ));
            return;
        }
    };

    let source_descendants = data_index.corresponding_descendants(source_group);
    let target_descendants = data_index.corresponding_descendants(target_group);
    let mut source_by_name = BTreeMap::<String, Vec<&DataItemIr>>::new();
    let mut target_by_name = BTreeMap::<String, Vec<&DataItemIr>>::new();
    for item in source_descendants {
        source_by_name
            .entry(item.name.to_ascii_uppercase())
            .or_default()
            .push(item);
    }
    for item in target_descendants {
        target_by_name
            .entry(item.name.to_ascii_uppercase())
            .or_default()
            .push(item);
    }

    for (name, source_matches) in &source_by_name {
        let Some(target_matches) = target_by_name.get(name) else {
            continue;
        };
        if source_matches.len() != 1 || target_matches.len() != 1 {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_MOVE_CORRESPONDING",
                format!(
                    "MOVE CORRESPONDING name {name} is ambiguous between source and target groups"
                ),
                paragraph.span.clone(),
            ));
            continue;
        }
        let source_item = source_matches[0];
        let target_item = target_matches[0];
        if move_corresponding_item_is_unsupported(source_item, data_index)
            || move_corresponding_item_is_unsupported(target_item, data_index)
        {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_MOVE_CORRESPONDING",
                format!(
                    "MOVE CORRESPONDING item {name} uses OCCURS or REDEFINES semantics not enabled yet"
                ),
                paragraph.span.clone(),
            ));
            continue;
        }
        if !move_compatible(Some(source_item.value_category), target_item.value_category) {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_MOVE_CORRESPONDING",
                format!(
                    "MOVE CORRESPONDING item {name} requires unsupported {:?} to {:?} coercion",
                    source_item.value_category, target_item.value_category
                ),
                paragraph.span.clone(),
            ));
        }
    }
}

fn move_corresponding_item_is_unsupported(
    item: &DataItemIr,
    data_index: &DataReferenceIndex<'_>,
) -> bool {
    data_index.has_redefines_context(item)
        || data_index.has_occurs_context(item)
        || data_index.has_dynamic_occurs_context(item)
}

fn analyze_arithmetic_semantics(
    source: &OperandIr,
    target: &DataRefIr,
    data_index: &DataReferenceIndex<'_>,
    paragraph: &ParagraphIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let source_category = operand_category(data_index, source);
    let target_category = resolved_category(data_index, target);
    if !source_category.map(category_is_numeric).unwrap_or(false)
        || !target_category.map(category_is_numeric).unwrap_or(false)
    {
        diagnostics.push(Diagnostic::error(
            "E_ARITH_CATEGORY_MISMATCH",
            format!(
                "arithmetic requires numeric operands; got source {:?} and target {:?}",
                source_category, target_category
            ),
            paragraph.span.clone(),
        ));
    }
}

fn analyze_data_items(
    ast_items: Vec<DataDeclAst>,
    dialect: CobolDialect,
    platform_profile: PlatformProfile,
    diagnostics: &mut Vec<Diagnostic>,
) -> (Vec<DataItemIr>, StoragePlanIr) {
    let mut planned = Vec::<PlannedData>::new();
    let mut level_stack: Vec<(u8, usize)> = Vec::new();
    let mut seen_qualified = HashSet::new();
    let mut name_to_index = HashMap::<String, usize>::new();
    let mut scoped_name_to_index = HashMap::<(u8, Option<String>, String), usize>::new();
    let mut cursors = HashMap::<(u8, Option<String>), usize>::new();
    let mut record_length = 0usize;
    let mut filler_seq = 0usize;
    let mut pending_renames = Vec::<PendingRenames>::new();
    let mut active_storage_area = None;

    for ast_item in ast_items {
        let item_storage_area = storage_area_ir(ast_item.storage_area);
        let item_area_key = storage_area_cursor_code(item_storage_area);
        if active_storage_area
            .map(storage_area_cursor_code)
            .is_some_and(|active| active != item_area_key)
        {
            while let Some((_, finished_idx)) = level_stack.pop() {
                record_length = record_length.max(finalize_group_cursor(
                    &mut planned,
                    &mut cursors,
                    finished_idx,
                ));
            }
        }
        active_storage_area = Some(item_storage_area);

        if ast_item.level == 88 {
            if let Some((_, parent_idx)) = level_stack.last().copied() {
                let raw_value_set =
                    extract_condition_value_set_from_clause_ast(&ast_item.clause_ast)
                        .unwrap_or_else(|| extract_condition_value_set(&ast_item.clauses));
                let value_set = expand_all_condition_values(
                    raw_value_set,
                    condition_parent_value_len(&planned[parent_idx].item),
                );
                let values = condition_values_from_set(&value_set);
                let parent = planned[parent_idx].item.qualified_name.clone();
                planned[parent_idx].conditions.push(ConditionNameIr {
                    name: ast_item.name.clone(),
                    rust_name: rust_ident(&ast_item.name),
                    parent,
                    values,
                    value_set,
                    span: ast_item.span,
                });
            } else {
                diagnostics.push(Diagnostic::error(
                    "E_ORPHAN_CONDITION_NAME",
                    format!(
                        "88-level condition {} has no parent data item",
                        ast_item.name
                    ),
                    ast_item.span,
                ));
            }
            continue;
        }
        if ast_item.level == 66 {
            if let Some((first, last)) = extract_renames_clause(&ast_item) {
                pending_renames.push(PendingRenames {
                    name: ast_item.name,
                    first,
                    last,
                    storage_area: item_storage_area,
                    span: ast_item.span,
                });
            } else {
                diagnostics.push(Diagnostic::error(
                    "E_UNSUPPORTED_DATA_CLAUSE",
                    format!(
                        "66-level data item {} requires a RENAMES clause",
                        ast_item.name
                    ),
                    ast_item.span,
                ));
            }
            continue;
        }
        if ast_item.level == 78 {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_DATA_CLAUSE",
                format!(
                    "78-level constant {} is represented as a named constant and does not allocate storage yet",
                    ast_item.name
                ),
                ast_item.span,
            ));
            continue;
        }

        while level_stack
            .last()
            .map(|(level, _)| *level >= ast_item.level)
            .unwrap_or(false)
        {
            if let Some((_, finished_idx)) = level_stack.pop() {
                record_length = record_length.max(finalize_group_cursor(
                    &mut planned,
                    &mut cursors,
                    finished_idx,
                ));
            }
        }

        let parent_idx = level_stack.last().map(|(_, idx)| *idx);
        let parent = parent_idx.map(|idx| planned[idx].item.qualified_name.clone());
        let mut path = parent_idx
            .map(|idx| planned[idx].item.path.clone())
            .unwrap_or_default();
        let mut ast_item = ast_item;
        let addressable = !ast_item.name.eq_ignore_ascii_case("FILLER");
        if !addressable {
            filler_seq += 1;
            ast_item.name = format!("FILLER_{filler_seq}");
        }
        path.push(ast_item.name.clone());
        let qualified_name = path.join(".");
        if addressable && !seen_qualified.insert((item_area_key, qualified_name.clone())) {
            diagnostics.push(Diagnostic::error(
                "E_DUPLICATE_SYMBOL",
                format!("duplicate data item {qualified_name}"),
                ast_item.span.clone(),
            ));
        }

        let mut item = lower_data_item(
            &ast_item,
            parent.clone(),
            qualified_name.clone(),
            path,
            addressable,
            dialect,
            diagnostics,
        );
        if parent_idx
            .map(|idx| planned[idx].item.external)
            .unwrap_or(false)
        {
            item.external = true;
        }
        let base_len = elementary_byte_len(&item);
        let occurs_multiplier = item.occurs.as_ref().map(|occurs| occurs.max).unwrap_or(1);
        let own_len = base_len.saturating_mul(occurs_multiplier);
        let redefines_key = item.redefines.as_ref().map(|name| normalize_name(name));
        let parent_in_redefines_tree = parent_idx
            .map(|idx| planned_item_in_redefines_tree(&planned, idx))
            .unwrap_or(false);
        let area_key = item_area_key;
        let mut offset = *cursors.get(&(area_key, parent.clone())).unwrap_or(&0);

        if let Some(base_name) = &redefines_key {
            if let Some(base_idx) = scoped_name_to_index
                .get(&(area_key, parent.clone(), base_name.clone()))
                .copied()
            {
                offset = planned[base_idx].item.offset.unwrap_or(offset);
            } else {
                diagnostics.push(Diagnostic::error(
                    "E_UNRESOLVED_REDEFINES",
                    format!(
                        "data item {} redefines unknown item {}",
                        item.name,
                        item.redefines.as_deref().unwrap_or("")
                    ),
                    item.span.clone(),
                ));
            }
        } else if item.sync && own_len > 0 {
            if let Some(alignment) =
                sync_alignment(&record_usage(&item.usage), own_len, true, platform_profile)
            {
                offset = record_align_offset(offset, alignment);
            }
        }

        item.offset = Some(offset);
        item.byte_len = if own_len > 0 { Some(own_len) } else { Some(0) };
        item.layout_id = Some(item.qualified_name.clone());

        if item.level == 77 && parent.is_some() {
            diagnostics.push(Diagnostic::error(
                "E_INVALID_LEVEL_HIERARCHY",
                format!("77-level item {} cannot be nested", item.name),
                item.span.clone(),
            ));
        }

        let index = planned.len();
        let normalized = normalize_name(&item.name);
        if item.addressable {
            name_to_index.entry(normalized).or_insert(index);
            name_to_index
                .entry(normalize_name(&item.qualified_name))
                .or_insert(index);
            scoped_name_to_index.insert(
                (area_key, item.parent.clone(), normalize_name(&item.name)),
                index,
            );
        }

        if redefines_key.is_none() {
            cursors.insert((area_key, parent.clone()), offset.saturating_add(own_len));
        }
        if matches!(item.usage, UsageIr::Group) {
            cursors
                .entry((area_key, Some(item.qualified_name.clone())))
                .or_insert(offset);
        }

        planned.push(PlannedData {
            item,
            conditions: Vec::new(),
        });
        level_stack.push((ast_item.level, index));
        let child_end = offset.saturating_add(own_len);
        if parent_in_redefines_tree {
            bump_redefines_ancestors(&mut planned, parent_idx, child_end);
        } else if redefines_key.is_none() {
            bump_ancestors(&mut planned, parent_idx, child_end);
            record_length = record_length.max(child_end);
        }
    }
    while let Some((_, finished_idx)) = level_stack.pop() {
        record_length = record_length.max(finalize_group_cursor(
            &mut planned,
            &mut cursors,
            finished_idx,
        ));
    }

    let mut storage_items = Vec::new();
    let mut redefines = Vec::new();
    let mut renames = Vec::new();
    let mut conditions = Vec::new();
    validate_condition_name_values(&planned, diagnostics);
    for planned_item in &planned {
        let item = &planned_item.item;
        if item.level != 88 {
            if let (Some(offset), Some(byte_len)) = (item.offset, item.byte_len) {
                storage_items.push(StorageItemIr {
                    name: item.name.clone(),
                    qualified_name: item.qualified_name.clone(),
                    path: item.path.clone(),
                    offset,
                    byte_len,
                    usage: item.usage.clone(),
                    storage_area: item.storage_area,
                    external: item.external,
                    value_category: item.value_category,
                    picture: item.picture_ast.clone(),
                    occurs: item.occurs.clone(),
                    redefines: item.redefines.clone(),
                    parent: item.parent.clone(),
                    addressable: item.addressable,
                    layout_id: item
                        .layout_id
                        .clone()
                        .unwrap_or_else(|| item.qualified_name.clone()),
                    sync: item.sync,
                    value: item.value.clone(),
                    span: item.span.clone(),
                });
            }
        }
        conditions.extend(planned_item.conditions.clone());
    }
    for planned_item in &planned {
        let Some(base_name) = &planned_item.item.redefines else {
            continue;
        };
        let area_key = storage_area_cursor_code(planned_item.item.storage_area);
        let Some(base_idx) = scoped_name_to_index
            .get(&(
                area_key,
                planned_item.item.parent.clone(),
                normalize_name(base_name),
            ))
            .copied()
        else {
            continue;
        };
        let base = &planned[base_idx].item;
        redefines.push(RedefinesIr {
            redefining_item: planned_item.item.qualified_name.clone(),
            base_item: base.qualified_name.clone(),
            offset: planned_item.item.offset.unwrap_or(0),
            byte_len: planned_item.item.byte_len.unwrap_or(0),
            base_byte_len: base.byte_len.unwrap_or(0),
        });
    }
    validate_redefines_overlay_views(&planned, &scoped_name_to_index, diagnostics);
    let mut renames_items = Vec::new();
    for pending in pending_renames {
        match resolve_renames(&pending, &planned) {
            Ok((data_item, storage_item, rename)) => {
                renames_items.push(data_item);
                storage_items.push(storage_item);
                renames.push(rename);
            }
            Err(message) => diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_DATA_CLAUSE",
                message,
                pending.span,
            )),
        }
    }

    let mut data_items = planned
        .into_iter()
        .map(|planned| planned.item)
        .collect::<Vec<_>>();
    data_items.extend(renames_items);
    let record_plan = build_record_plan(
        record_length,
        &storage_items,
        &redefines,
        &renames,
        &conditions,
        platform_profile,
        diagnostics,
    );
    let storage_cells = build_storage_cell_ir(&storage_items, dialect);
    let storage_bindings = build_storage_binding_ir(&storage_items, &conditions, &renames);
    let odo_templates = build_odo_template_ir(&storage_items, dialect);
    (
        data_items,
        StoragePlanIr {
            record_length,
            items: storage_items,
            redefines,
            renames,
            condition_names: conditions,
            storage_cells,
            storage_bindings,
            odo_templates,
            record_plan,
        },
    )
}

fn extract_renames_clause(item: &DataDeclAst) -> Option<(String, Option<String>)> {
    item.clause_ast.iter().find_map(|clause| match clause {
        DataClauseAst::Renames { first, last } => Some((first.clone(), last.clone())),
        _ => None,
    })
}

fn resolve_renames(
    pending: &PendingRenames,
    planned: &[PlannedData],
) -> Result<(DataItemIr, StorageItemIr, RenamesIr), String> {
    let first = resolve_unique_planned_data(&pending.first, planned).ok_or_else(|| {
        format!(
            "RENAMES {} references unknown item {}",
            pending.name, pending.first
        )
    })?;
    if first.item.storage_area != pending.storage_area {
        return Err(format!(
            "RENAMES {} is declared in storage area {:?} but target {} is in {:?}",
            pending.name, pending.storage_area, first.item.qualified_name, first.item.storage_area
        ));
    }
    let targets = if let Some(last_name) = &pending.last {
        let last = resolve_unique_planned_data(last_name, planned).ok_or_else(|| {
            format!(
                "RENAMES {} references unknown item {last_name}",
                pending.name
            )
        })?;
        if last.item.storage_area != first.item.storage_area {
            return Err(format!(
                "RENAMES {} crosses storage areas {:?} and {:?}",
                pending.name, first.item.storage_area, last.item.storage_area
            ));
        }
        renames_range_targets(first, last, planned)?
    } else if first.item.value_category == ValueCategoryIr::Group {
        let prefix = format!("{}.", first.item.qualified_name);
        let mut children = planned
            .iter()
            .filter(|candidate| {
                candidate.item.value_category != ValueCategoryIr::Group
                    && candidate.item.qualified_name.starts_with(&prefix)
            })
            .map(|candidate| &candidate.item)
            .collect::<Vec<_>>();
        children.sort_by_key(|item| item.offset.unwrap_or(0));
        if let Some(filler) = children.iter().find(|item| !item.addressable) {
            return Err(format!(
                "RENAMES {} spans non-addressable FILLER storage {}",
                pending.name, filler.qualified_name
            ));
        }
        if let Some(occurs_item) = children
            .iter()
            .find(|item| planned_item_has_occurs_context(item, planned))
        {
            return Err(format!(
                "RENAMES {} spans OCCURS storage {}",
                pending.name, occurs_item.qualified_name
            ));
        }
        if let Some(redefines_item) = children
            .iter()
            .find(|item| planned_item_has_redefines_context(item, planned))
        {
            return Err(format!(
                "RENAMES {} spans REDEFINES storage {}",
                pending.name, redefines_item.qualified_name
            ));
        }
        children
            .into_iter()
            .map(|item| item.qualified_name.clone())
            .collect::<Vec<_>>()
    } else {
        vec![first.item.qualified_name.clone()]
    };

    if targets.is_empty() {
        return Err(format!(
            "RENAMES {} does not select any elementary storage",
            pending.name
        ));
    }

    let mut target_items = targets
        .iter()
        .filter_map(|target| {
            planned
                .iter()
                .find(|candidate| candidate.item.qualified_name == *target)
                .map(|candidate| &candidate.item)
        })
        .collect::<Vec<_>>();
    target_items.sort_by_key(|item| item.offset.unwrap_or(0));
    if let Some(occurs_item) = target_items
        .iter()
        .find(|item| planned_item_has_occurs_context(item, planned))
    {
        return Err(format!(
            "RENAMES {} spans OCCURS storage {}",
            pending.name, occurs_item.qualified_name
        ));
    }
    if let Some(redefines_item) = target_items
        .iter()
        .find(|item| planned_item_has_redefines_context(item, planned))
    {
        return Err(format!(
            "RENAMES {} spans REDEFINES storage {}",
            pending.name, redefines_item.qualified_name
        ));
    }
    let offset = target_items
        .first()
        .and_then(|item| item.offset)
        .unwrap_or(0);
    let end = target_items
        .iter()
        .filter_map(|item| {
            item.offset
                .map(|offset| offset.saturating_add(item.byte_len.unwrap_or(0)))
        })
        .max()
        .unwrap_or(offset);
    let byte_len = end.saturating_sub(offset);
    let data_item = DataItemIr {
        level: 66,
        name: pending.name.clone(),
        rust_name: rust_ident(&pending.name),
        picture: None,
        picture_ast: None,
        usage: UsageIr::Group,
        occurs: None,
        redefines: None,
        parent: None,
        qualified_name: pending.name.clone(),
        path: vec![pending.name.clone()],
        addressable: true,
        storage_area: first.item.storage_area,
        external: false,
        value_category: ValueCategoryIr::Group,
        layout_id: Some(pending.name.clone()),
        offset: Some(offset),
        byte_len: Some(byte_len),
        sync: false,
        value: None,
        span: pending.span.clone(),
    };
    let storage_item = StorageItemIr {
        name: pending.name.clone(),
        qualified_name: pending.name.clone(),
        path: vec![pending.name.clone()],
        offset,
        byte_len,
        usage: UsageIr::Group,
        storage_area: first.item.storage_area,
        external: false,
        value_category: ValueCategoryIr::Group,
        picture: None,
        occurs: None,
        redefines: None,
        parent: None,
        addressable: true,
        layout_id: pending.name.clone(),
        sync: false,
        value: None,
        span: pending.span.clone(),
    };
    let rename = RenamesIr {
        renaming_item: pending.name.clone(),
        targets,
        offset,
        byte_len,
    };
    Ok((data_item, storage_item, rename))
}

fn validate_condition_name_values(planned: &[PlannedData], diagnostics: &mut Vec<Diagnostic>) {
    for planned_item in planned {
        let parent = &planned_item.item;
        for condition in &planned_item.conditions {
            for value in &condition.value_set {
                match parent.value_category {
                    ValueCategoryIr::Alphanumeric
                    | ValueCategoryIr::Alphabetic
                    | ValueCategoryIr::Group => {
                        validate_display_condition_value(parent, condition, value, diagnostics);
                    }
                    ValueCategoryIr::NumericDisplay
                    | ValueCategoryIr::PackedDecimal
                    | ValueCategoryIr::Binary
                    | ValueCategoryIr::NativeBinary => {
                        validate_numeric_condition_value(parent, condition, value, diagnostics);
                    }
                    _ => {}
                }
            }
        }
    }
}

fn validate_display_condition_value(
    parent: &DataItemIr,
    condition: &ConditionNameIr,
    value: &ConditionValueIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let ConditionValueIr::Single(value) = value else {
        return;
    };
    let parent_len = condition_parent_value_len(parent).unwrap_or(0);
    let value_len = expand_all_literal_to_len(value, parent_len)
        .map(|expanded| expanded.len())
        .unwrap_or_else(|| value.len());
    if value_len <= parent_len {
        return;
    }
    diagnostics.push(Diagnostic::error(
        "E_INVALID_CONDITION_VALUE",
        format!(
            "88-level condition {} value {:?} is {} bytes but parent {} is {} bytes",
            condition.name, value, value_len, condition.parent, parent_len
        ),
        condition.span.clone(),
    ));
}

fn validate_numeric_condition_value(
    parent: &DataItemIr,
    condition: &ConditionNameIr,
    value: &ConditionValueIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(picture) = &parent.picture_ast else {
        return;
    };
    for literal in condition_value_literals(value) {
        match parse_condition_numeric_literal(literal) {
            Some(parsed) => {
                if numeric_literal_fits_picture(parsed, picture) {
                    continue;
                }
                diagnostics.push(Diagnostic::error(
                    "E_INVALID_CONDITION_VALUE",
                    format!(
                        "88-level condition {} value {} does not fit parent {} picture {} ({} digit(s), scale {}, signed {})",
                        condition.name,
                        literal,
                        condition.parent,
                        picture.raw,
                        picture.digits,
                        picture.scale,
                        picture.signed
                    ),
                    condition.span.clone(),
                ));
            }
            None => diagnostics.push(Diagnostic::error(
                "E_INVALID_CONDITION_VALUE",
                format!(
                    "88-level condition {} value {} is not a numeric literal for parent {}",
                    condition.name, literal, condition.parent
                ),
                condition.span.clone(),
            )),
        }
    }
}

fn condition_value_literals(value: &ConditionValueIr) -> Vec<&str> {
    match value {
        ConditionValueIr::Single(value) => vec![value.as_str()],
        ConditionValueIr::Range { start, end } => vec![start.as_str(), end.as_str()],
    }
}

fn condition_values_from_set(values: &[ConditionValueIr]) -> Vec<String> {
    values
        .iter()
        .flat_map(|value| match value {
            ConditionValueIr::Single(value) => vec![value.clone()],
            ConditionValueIr::Range { start, end } => vec![start.clone(), end.clone()],
        })
        .collect()
}

fn expand_all_condition_values(
    values: Vec<ConditionValueIr>,
    parent_len: Option<usize>,
) -> Vec<ConditionValueIr> {
    let Some(parent_len) = parent_len else {
        return values;
    };
    values
        .into_iter()
        .map(|value| match value {
            ConditionValueIr::Single(value) => expand_all_literal_to_len(&value, parent_len)
                .map(ConditionValueIr::Single)
                .unwrap_or(ConditionValueIr::Single(value)),
            ConditionValueIr::Range { start, end } => ConditionValueIr::Range {
                start: expand_all_literal_to_len(&start, parent_len).unwrap_or(start),
                end: expand_all_literal_to_len(&end, parent_len).unwrap_or(end),
            },
        })
        .collect()
}

fn condition_parent_value_len(parent: &DataItemIr) -> Option<usize> {
    let byte_len = parent.byte_len?;
    let Some(occurs) = &parent.occurs else {
        return Some(byte_len);
    };
    Some(byte_len / occurs.max.max(1))
}

#[derive(Clone, Copy)]
struct ParsedConditionNumericLiteral {
    negative: bool,
    integer_digits: usize,
    fractional_digits: usize,
}

fn parse_condition_numeric_literal(value: &str) -> Option<ParsedConditionNumericLiteral> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut chars = trimmed.chars().peekable();
    let negative = match chars.peek().copied() {
        Some('-') => {
            chars.next();
            true
        }
        Some('+') => {
            chars.next();
            false
        }
        _ => false,
    };
    let mut seen_digit = false;
    let mut seen_decimal = false;
    let mut integer_digits = String::new();
    let mut fractional_digits = String::new();
    for ch in chars {
        if ch.is_ascii_digit() {
            seen_digit = true;
            if seen_decimal {
                fractional_digits.push(ch);
            } else {
                integer_digits.push(ch);
            }
        } else if ch == '.' && !seen_decimal {
            seen_decimal = true;
        } else {
            return None;
        }
    }
    if !seen_digit {
        return None;
    }
    let integer_digits = integer_digits.trim_start_matches('0').len().max(1);
    let fractional_digits = fractional_digits.trim_end_matches('0').len();
    Some(ParsedConditionNumericLiteral {
        negative,
        integer_digits,
        fractional_digits,
    })
}

fn numeric_literal_fits_picture(value: ParsedConditionNumericLiteral, picture: &PicIr) -> bool {
    if value.negative && !picture.signed {
        return false;
    }
    if value.fractional_digits > picture.scale {
        return false;
    }
    let integer_capacity = picture.digits.saturating_sub(picture.scale);
    value.integer_digits <= integer_capacity
        && value.integer_digits.saturating_add(value.fractional_digits) <= picture.digits
}

fn resolve_unique_planned_data<'a>(
    name: &str,
    planned: &'a [PlannedData],
) -> Option<&'a PlannedData> {
    let normalized = normalize_name(name);
    let matches = planned
        .iter()
        .filter(|candidate| {
            candidate.item.addressable
                && (normalize_name(&candidate.item.name) == normalized
                    || normalize_name(&candidate.item.qualified_name) == normalized)
        })
        .collect::<Vec<_>>();
    (matches.len() == 1).then_some(matches[0])
}

fn validate_redefines_overlay_views(
    planned: &[PlannedData],
    scoped_name_to_index: &HashMap<(u8, Option<String>, String), usize>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for planned_item in planned {
        let Some(base_name) = &planned_item.item.redefines else {
            continue;
        };
        let area_key = storage_area_cursor_code(planned_item.item.storage_area);
        let Some(base_idx) = scoped_name_to_index
            .get(&(
                area_key,
                planned_item.item.parent.clone(),
                normalize_name(base_name),
            ))
            .copied()
        else {
            continue;
        };
        let base_item = &planned[base_idx].item;
        if planned_item.item.byte_len.unwrap_or(0) > base_item.byte_len.unwrap_or(0) {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_REDEFINES_OVERLAY",
                format!(
                    "REDEFINES item {} is {} bytes but base {} is {} bytes; larger overlay views are not executable storage aliases",
                    planned_item.item.qualified_name,
                    planned_item.item.byte_len.unwrap_or(0),
                    base_item.qualified_name,
                    base_item.byte_len.unwrap_or(0)
                ),
                planned_item.item.span.clone(),
            ));
            continue;
        }
        if planned_item_has_occurs_context(base_item, planned)
            || planned_item_has_occurs_context(&planned_item.item, planned)
            || planned_item_subtree_has_occurs(planned, base_item)
            || planned_item_subtree_has_occurs(planned, &planned_item.item)
        {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_REDEFINES_OVERLAY",
                format!(
                    "REDEFINES item {} overlays {} with OCCURS storage; executable storage aliases for repeated overlays are not enabled yet",
                    planned_item.item.qualified_name, base_item.qualified_name
                ),
                planned_item.item.span.clone(),
            ));
            continue;
        }
        let base_ranges = elementary_overlay_ranges(planned, base_item)
            .into_iter()
            .filter_map(|item| Some((item.offset?, item.byte_len?)))
            .collect::<BTreeSet<_>>();
        if base_ranges.is_empty() {
            continue;
        }

        for child in elementary_overlay_ranges(planned, &planned_item.item) {
            let Some(range) = child.offset.zip(child.byte_len) else {
                continue;
            };
            if base_ranges.contains(&range) {
                continue;
            }
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_REDEFINES_OVERLAY",
                format!(
                    "REDEFINES child {} overlays {} with a non-identical elementary byte view; executable storage aliases only exact elementary overlay ranges",
                    child.qualified_name, base_item.qualified_name
                ),
                child.span.clone(),
            ));
        }
    }
}

fn elementary_overlay_ranges<'a>(
    planned: &'a [PlannedData],
    item: &'a DataItemIr,
) -> Vec<&'a DataItemIr> {
    if item.value_category != ValueCategoryIr::Group {
        return vec![item];
    }
    let prefix = format!("{}.", item.qualified_name);
    planned
        .iter()
        .filter(|candidate| {
            candidate.item.value_category != ValueCategoryIr::Group
                && candidate.item.qualified_name.starts_with(&prefix)
        })
        .map(|candidate| &candidate.item)
        .collect()
}

fn planned_item_subtree_has_occurs(planned: &[PlannedData], item: &DataItemIr) -> bool {
    if item.occurs.is_some() {
        return true;
    }
    let prefix = format!("{}.", item.qualified_name);
    planned.iter().any(|candidate| {
        candidate.item.storage_area == item.storage_area
            && candidate.item.qualified_name.starts_with(&prefix)
            && candidate.item.occurs.is_some()
    })
}

fn renames_range_targets(
    first: &PlannedData,
    last: &PlannedData,
    planned: &[PlannedData],
) -> Result<Vec<String>, String> {
    if first.item.parent != last.item.parent {
        return Err(format!(
            "RENAMES range {} THRU {} crosses parent groups",
            first.item.name, last.item.name
        ));
    }
    if first.item.value_category == ValueCategoryIr::Group
        || last.item.value_category == ValueCategoryIr::Group
    {
        return Err("RENAMES THRU ranges over groups are not enabled yet".to_string());
    }
    let first_offset = first.item.offset.unwrap_or(0);
    let last_offset = last.item.offset.unwrap_or(0);
    if first_offset > last_offset {
        return Err(format!(
            "RENAMES range {} THRU {} must follow declaration order",
            first.item.name, last.item.name
        ));
    }
    let start = first_offset;
    let end = first
        .item
        .offset
        .unwrap_or(0)
        .saturating_add(first.item.byte_len.unwrap_or(0))
        .max(
            last.item
                .offset
                .unwrap_or(0)
                .saturating_add(last.item.byte_len.unwrap_or(0)),
        );
    let scoped_span_items = planned
        .iter()
        .filter(|candidate| {
            candidate.item.storage_area == first.item.storage_area
                && planned_item_in_renames_scope(&candidate.item, first.item.parent.as_deref())
                && candidate.item.offset.unwrap_or(usize::MAX) >= start
                && candidate
                    .item
                    .offset
                    .unwrap_or(0)
                    .saturating_add(candidate.item.byte_len.unwrap_or(0))
                    <= end
        })
        .map(|candidate| &candidate.item)
        .collect::<Vec<_>>();
    let parent_prefix = first
        .item
        .parent
        .as_ref()
        .map(|parent| format!("{parent}."));
    let mut span_items = planned
        .iter()
        .filter(|candidate| {
            candidate.item.storage_area == first.item.storage_area
                && candidate.item.value_category != ValueCategoryIr::Group
                && match &parent_prefix {
                    Some(prefix) => candidate.item.qualified_name.starts_with(prefix),
                    None => candidate.item.parent.is_none(),
                }
                && candidate.item.offset.unwrap_or(usize::MAX) >= start
                && candidate
                    .item
                    .offset
                    .unwrap_or(0)
                    .saturating_add(candidate.item.byte_len.unwrap_or(0))
                    <= end
        })
        .map(|candidate| &candidate.item)
        .collect::<Vec<_>>();
    span_items.sort_by_key(|item| item.offset.unwrap_or(0));
    if let Some(filler) = span_items.iter().find(|item| !item.addressable) {
        return Err(format!(
            "RENAMES range {} THRU {} spans non-addressable FILLER storage {}",
            first.item.name, last.item.name, filler.qualified_name
        ));
    }
    if let Some(occurs_item) = span_items
        .iter()
        .find(|item| planned_item_has_occurs_context(item, planned))
    {
        return Err(format!(
            "RENAMES range {} THRU {} spans OCCURS storage {}",
            first.item.name, last.item.name, occurs_item.qualified_name
        ));
    }
    if let Some(redefines_item) = span_items
        .iter()
        .find(|item| planned_item_has_redefines_context(item, planned))
    {
        return Err(format!(
            "RENAMES range {} THRU {} spans REDEFINES storage {}",
            first.item.name, last.item.name, redefines_item.qualified_name
        ));
    }
    if let Some(occurs_item) = scoped_span_items
        .iter()
        .find(|item| planned_item_has_occurs_context(item, planned))
    {
        return Err(format!(
            "RENAMES range {} THRU {} spans OCCURS storage {}",
            first.item.name, last.item.name, occurs_item.qualified_name
        ));
    }
    if let Some(redefines_item) = scoped_span_items
        .iter()
        .find(|item| planned_item_has_redefines_context(item, planned))
    {
        return Err(format!(
            "RENAMES range {} THRU {} spans REDEFINES storage {}",
            first.item.name, last.item.name, redefines_item.qualified_name
        ));
    }
    Ok(span_items
        .into_iter()
        .map(|item| item.qualified_name.clone())
        .collect())
}

fn planned_item_in_renames_scope(item: &DataItemIr, parent: Option<&str>) -> bool {
    match parent {
        Some(parent) => {
            item.parent.as_deref() == Some(parent)
                || item
                    .qualified_name
                    .strip_prefix(parent)
                    .is_some_and(|suffix| suffix.starts_with('.'))
        }
        None => item.parent.is_none(),
    }
}

fn planned_item_has_occurs_context(item: &DataItemIr, planned: &[PlannedData]) -> bool {
    if item.occurs.is_some() {
        return true;
    }
    let storage_area = item.storage_area;
    let mut parent = item.parent.as_ref();
    while let Some(parent_name) = parent {
        let Some(parent_item) = planned.iter().find(|candidate| {
            candidate.item.qualified_name == *parent_name
                && candidate.item.storage_area == storage_area
        }) else {
            return false;
        };
        if parent_item.item.occurs.is_some() {
            return true;
        }
        parent = parent_item.item.parent.as_ref();
    }
    false
}

fn planned_item_has_redefines_context(item: &DataItemIr, planned: &[PlannedData]) -> bool {
    if item.redefines.is_some() || planned_item_is_redefined_base(item, planned) {
        return true;
    }
    let storage_area = item.storage_area;
    let mut parent = item.parent.as_ref();
    while let Some(parent_name) = parent {
        let Some(parent_item) = planned.iter().find(|candidate| {
            candidate.item.qualified_name == *parent_name
                && candidate.item.storage_area == storage_area
        }) else {
            return false;
        };
        if parent_item.item.redefines.is_some()
            || planned_item_is_redefined_base(&parent_item.item, planned)
        {
            return true;
        }
        parent = parent_item.item.parent.as_ref();
    }
    false
}

fn planned_item_is_redefined_base(item: &DataItemIr, planned: &[PlannedData]) -> bool {
    let item_name = normalize_name(&item.name);
    planned.iter().any(|candidate| {
        candidate.item.storage_area == item.storage_area
            && candidate.item.parent == item.parent
            && candidate
                .item
                .redefines
                .as_ref()
                .is_some_and(|base| normalize_name(base) == item_name)
    })
}

fn build_storage_cell_ir(items: &[StorageItemIr], dialect: CobolDialect) -> Vec<StorageCellIr> {
    items
        .iter()
        .filter(|item| {
            item.addressable
                && item.value_category != ValueCategoryIr::Group
                && !storage_item_in_redefines_view(item, items)
        })
        .map(|item| StorageCellIr {
            key: item.qualified_name.clone(),
            item_id: item.qualified_name.clone(),
            byte_len: item.byte_len,
            usage: item.usage.clone(),
            category: item.value_category,
            picture: item.picture.clone(),
            initial_bytes: storage_template_bytes(item, item.byte_len, dialect),
        })
        .collect()
}

fn build_storage_binding_ir(
    items: &[StorageItemIr],
    conditions: &[ConditionNameIr],
    renames: &[RenamesIr],
) -> Vec<(String, StorageBindingIr)> {
    let mut bindings = Vec::new();
    for item in items
        .iter()
        .filter(|item| item.addressable && !storage_item_in_redefines_view(item, items))
    {
        let binding = if item.value_category == ValueCategoryIr::Group {
            let renames_targets = renames
                .iter()
                .find(|rename| rename.renaming_item == item.qualified_name)
                .map(|rename| rename.targets.clone());
            let children = if let Some(targets) = renames_targets {
                targets
            } else {
                let prefix = format!("{}.", item.qualified_name);
                let mut children = items
                    .iter()
                    .filter(|child| {
                        child.addressable
                            && child.value_category != ValueCategoryIr::Group
                            && child.qualified_name.starts_with(&prefix)
                            && !storage_item_in_redefines_view(child, items)
                    })
                    .collect::<Vec<_>>();
                children.sort_by_key(|child| child.offset);
                children
                    .into_iter()
                    .map(|child| child.qualified_name.clone())
                    .collect()
            };
            StorageBindingIr::Group { children }
        } else if item.occurs.is_some() || item_has_occurs_parent(item, items) {
            StorageBindingIr::OccursCell {
                program: String::new(),
                item: item.qualified_name.clone(),
                subscripts: Vec::new(),
            }
        } else {
            StorageBindingIr::Cell {
                key: item.qualified_name.clone(),
            }
        };
        bindings.push((item.qualified_name.clone(), binding));
    }
    for condition in conditions {
        if let Some((_, parent)) = bindings
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(&condition.parent))
            .cloned()
        {
            bindings.push((
                format!("{}.{}", condition.parent, condition.name),
                StorageBindingIr::ConditionName {
                    parent: Box::new(parent),
                    condition: cobol_ir::ConditionNameTargetIr {
                        name: condition.name.clone(),
                        parent: condition.parent.clone(),
                        qualified_name: format!("{}.{}", condition.parent, condition.name),
                    },
                },
            ));
        }
    }
    bindings
}

fn storage_item_in_redefines_view(item: &StorageItemIr, items: &[StorageItemIr]) -> bool {
    if item.redefines.is_some() {
        return true;
    }
    let mut parent = item.parent.as_deref();
    while let Some(parent_name) = parent {
        let Some(parent_item) = items
            .iter()
            .find(|candidate| candidate.qualified_name == parent_name)
        else {
            return false;
        };
        if parent_item.redefines.is_some() {
            return true;
        }
        parent = parent_item.parent.as_deref();
    }
    false
}

fn build_odo_template_ir(items: &[StorageItemIr], dialect: CobolDialect) -> Vec<OdoTemplateIr> {
    items
        .iter()
        .filter_map(|item| {
            let occurs = item.occurs.as_ref()?;
            if !occurs_range_is_valid(occurs) {
                return None;
            }
            let depending_on = occurs.depending_on.clone()?;
            let fields = if item.value_category == ValueCategoryIr::Group {
                let prefix = format!("{}.", item.qualified_name);
                let mut children = items
                    .iter()
                    .filter(|child| {
                        child.addressable
                            && child.value_category != ValueCategoryIr::Group
                            && child.qualified_name.starts_with(&prefix)
                    })
                    .collect::<Vec<_>>();
                children.sort_by_key(|child| child.offset);
                children
                    .into_iter()
                    .map(|child| {
                        (
                            child.qualified_name.clone(),
                            storage_template_bytes(child, child.byte_len, dialect),
                        )
                    })
                    .collect()
            } else {
                vec![(
                    item.qualified_name.clone(),
                    storage_template_bytes(item, item.byte_len / occurs.max.max(1), dialect),
                )]
            };
            Some(OdoTemplateIr {
                table: item.qualified_name.clone(),
                depending_on,
                min: occurs.min,
                max: occurs.max,
                fields,
            })
        })
        .collect()
}

fn occurs_range_is_valid(occurs: &OccursIr) -> bool {
    occurs.max >= occurs.min
}

fn item_has_occurs_parent(item: &StorageItemIr, items: &[StorageItemIr]) -> bool {
    let mut parent = item.parent.as_deref();
    while let Some(parent_name) = parent {
        let Some(parent_item) = items
            .iter()
            .find(|candidate| candidate.qualified_name == parent_name)
        else {
            return false;
        };
        if parent_item.occurs.is_some() {
            return true;
        }
        parent = parent_item.parent.as_deref();
    }
    false
}

fn storage_template_bytes(item: &StorageItemIr, len: usize, dialect: CobolDialect) -> Vec<u8> {
    if let Some(occurs) = &item.occurs {
        if occurs.max > 1 && item.byte_len == len {
            let occurrence_len = len / occurs.max.max(1);
            let occurrence = storage_template_bytes(item, occurrence_len, dialect);
            let mut bytes = Vec::with_capacity(len);
            for _ in 0..occurs.max {
                bytes.extend_from_slice(&occurrence);
            }
            bytes.truncate(len);
            return bytes;
        }
    }
    let mut bytes = match item.value_category {
        ValueCategoryIr::NumericDisplay => {
            let mut text = item.value.clone().unwrap_or_else(|| "0".to_string());
            if text.len() > len {
                text = text[text.len() - len..].to_string();
            }
            while text.len() < len {
                text.insert(0, '0');
            }
            text.into_bytes()
        }
        ValueCategoryIr::Alphanumeric
        | ValueCategoryIr::Alphabetic
        | ValueCategoryIr::NumericEdited => {
            let mut bytes = vec![b' '; len];
            if let Some(value) = &item.value {
                if let Some(expanded) = expand_all_literal_to_len(value, len) {
                    bytes = expanded.into_bytes();
                } else {
                    for (idx, byte) in value.as_bytes().iter().take(len).enumerate() {
                        bytes[idx] = *byte;
                    }
                }
            }
            bytes
        }
        ValueCategoryIr::PackedDecimal => packed_decimal_storage_template(item, len),
        ValueCategoryIr::Float => float_storage_template(item, len, dialect),
        _ => vec![0u8; len],
    };
    bytes.resize(len, b' ');
    bytes.truncate(len);
    bytes
}

fn packed_decimal_storage_template(item: &StorageItemIr, len: usize) -> Vec<u8> {
    packed_decimal_initial_bytes(item.picture.as_ref(), item.value.as_deref())
        .map(|bytes| fit_template_bytes(bytes, len, 0u8))
        .unwrap_or_else(|_| {
            item.picture
                .as_ref()
                .and_then(|picture| {
                    encode_packed_decimal(
                        Decimal::ZERO,
                        picture.digits,
                        picture.scale as u32,
                        picture.signed,
                    )
                    .ok()
                })
                .map(|bytes| fit_template_bytes(bytes, len, 0u8))
                .unwrap_or_else(|| vec![0u8; len])
        })
}

fn packed_decimal_initial_bytes(
    picture: Option<&PicIr>,
    value: Option<&str>,
) -> Result<Vec<u8>, String> {
    let picture = picture.ok_or_else(|| "missing PIC metadata".to_string())?;
    let value = parse_initial_decimal(value.unwrap_or("0"))?;
    encode_packed_decimal(value, picture.digits, picture.scale as u32, picture.signed)
        .map_err(|err| err.to_string())
}

fn parse_initial_decimal(value: &str) -> Result<Decimal, String> {
    let trimmed = value.trim();
    let normalized = trimmed.strip_prefix('+').unwrap_or(trimmed);
    let normalized = if normalized.is_empty() {
        "0"
    } else {
        normalized
    };
    Decimal::from_str(normalized).map_err(|_| format!("VALUE {value:?} is not a decimal literal"))
}

fn float_storage_template(item: &StorageItemIr, len: usize, dialect: CobolDialect) -> Vec<u8> {
    float_initial_bytes(&item.usage, item.value.as_deref(), dialect)
        .map(|bytes| fit_template_bytes(bytes, len, 0u8))
        .unwrap_or_else(|_| vec![0u8; len])
}

fn float_initial_bytes(
    usage: &UsageIr,
    value: Option<&str>,
    dialect: CobolDialect,
) -> Result<Vec<u8>, String> {
    let value = parse_initial_float(value.unwrap_or("0"))?;
    match (dialect, usage) {
        (CobolDialect::Ibm, UsageIr::Float32) => {
            cobol_record::encode_ibm_float32(value, cobol_record::Endian::Big)
        }
        (CobolDialect::Ibm, UsageIr::Float64) => {
            cobol_record::encode_ibm_float64(value, cobol_record::Endian::Big)
        }
        (CobolDialect::GnuCobol | CobolDialect::MicroFocus, UsageIr::Float32) => {
            cobol_record::encode_ieee_float32(value, cobol_record::Endian::Big)
        }
        (CobolDialect::GnuCobol | CobolDialect::MicroFocus, UsageIr::Float64) => {
            cobol_record::encode_ieee_float64(value, cobol_record::Endian::Big)
        }
        other => return Err(format!("unsupported float usage {other:?}")),
    }
    .map_err(|err| err.to_string())
}

fn parse_initial_float(value: &str) -> Result<f64, String> {
    value
        .trim()
        .parse::<f64>()
        .map_err(|_| format!("invalid float literal {value:?}"))
}

fn expand_all_literal_to_len(value: &str, len: usize) -> Option<String> {
    let clean = value.trim();
    let rest = clean.strip_prefix("ALL ").or_else(|| {
        clean
            .get(..4)
            .filter(|prefix| prefix.eq_ignore_ascii_case("ALL "))
            .map(|_| &clean[4..])
    })?;
    let pattern = normalize_value_literal(rest);
    if pattern.is_empty() {
        return Some(String::new());
    }
    if !pattern.is_ascii() {
        return None;
    }
    let mut expanded = String::new();
    while expanded.len() < len {
        expanded.push_str(&pattern);
    }
    expanded.truncate(len);
    Some(expanded)
}

fn fit_template_bytes(mut bytes: Vec<u8>, len: usize, pad: u8) -> Vec<u8> {
    bytes.resize(len, pad);
    bytes.truncate(len);
    bytes
}

fn lower_data_item(
    item: &DataDeclAst,
    parent: Option<String>,
    qualified_name: String,
    path: Vec<String>,
    addressable: bool,
    dialect: CobolDialect,
    diagnostics: &mut Vec<Diagnostic>,
) -> DataItemIr {
    let picture = extract_picture_from_clause_ast(&item.clause_ast)
        .or_else(|| extract_picture(&item.clauses));
    let picture_ast = picture.as_ref().map(|picture| parse_picture(picture));
    let usage = if let Some(usage) = extract_usage_from_clause_ast(&item.clause_ast) {
        usage
    } else if picture
        .as_ref()
        .map(|pic| pic.trim_start().to_ascii_uppercase().starts_with('N'))
        .unwrap_or(false)
    {
        UsageIr::National
    } else if picture
        .as_ref()
        .map(|pic| pic.trim_start().to_ascii_uppercase().starts_with('G'))
        .unwrap_or(false)
    {
        UsageIr::Dbcs
    } else if picture_ast
        .as_ref()
        .map(|pic| {
            matches!(
                pic.category,
                PicCategoryIr::Alphanumeric | PicCategoryIr::Alphabetic
            )
        })
        .unwrap_or(false)
    {
        UsageIr::Alphanumeric
    } else if picture.is_some() {
        UsageIr::Display
    } else if picture.is_none()
        && !clause_ast_has_usage_clause(&item.clause_ast)
        && !clause_ast_has_other_word(&item.clause_ast, "USAGE")
        && !clause_ast_has_other_word(&item.clause_ast, "PIC")
        && !clause_ast_has_other_word(&item.clause_ast, "PICTURE")
    {
        UsageIr::Group
    } else {
        UsageIr::Unknown(item.clauses.clone())
    };
    emit_data_clause_diagnostics(item, diagnostics);
    if matches!(usage, UsageIr::National | UsageIr::Dbcs) {
        diagnostics.push(Diagnostic::error(
            "E_UNSUPPORTED_NATIONAL_DBCS",
            format!(
                "data item {} uses {:?}; national/DBCS storage is represented but executable semantics remain fail-closed",
                item.name, usage
            ),
            item.span.clone(),
        ));
    }
    let storage_area = storage_area_ir(item.storage_area);
    if matches!(storage_area, StorageAreaIr::LocalStorage) {
        diagnostics.push(Diagnostic::error(
            "E_UNSUPPORTED_SECTION",
            format!(
                "{:?} data item {} is represented in semantic IR but executable storage for that area is not enabled yet",
                storage_area, item.name
            ),
            item.span.clone(),
        ));
    }
    let storage_value_category = value_category_for(&usage, picture_ast.as_ref());
    let value_category = if data_clause_requires_fail_closed_category(&item.clause_ast) {
        ValueCategoryIr::Unsupported
    } else {
        storage_value_category
    };
    let value = extract_initial_value_from_clause_ast(&item.clause_ast)
        .or_else(|| extract_value(&item.clauses));
    if storage_value_category == ValueCategoryIr::PackedDecimal {
        if let Err(message) = packed_decimal_initial_bytes(picture_ast.as_ref(), value.as_deref()) {
            diagnostics.push(Diagnostic::error(
                "E_INVALID_PACKED_DECIMAL_VALUE",
                format!(
                    "packed decimal item {} has an invalid initial VALUE: {message}",
                    item.name
                ),
                item.span.clone(),
            ));
        }
    }
    if storage_value_category == ValueCategoryIr::Float {
        if let Err(message) = float_initial_bytes(&usage, value.as_deref(), dialect) {
            diagnostics.push(Diagnostic::error(
                "E_INVALID_FLOAT_VALUE",
                format!(
                    "float item {} has an invalid initial VALUE: {message}",
                    item.name
                ),
                item.span.clone(),
            ));
        }
    }

    DataItemIr {
        level: item.level,
        rust_name: rust_ident(&item.name),
        name: item.name.clone(),
        picture,
        picture_ast,
        usage,
        occurs: extract_occurs_from_clause_ast(&item.clause_ast)
            .or_else(|| extract_occurs(&item.clauses)),
        redefines: extract_after_keyword(&item.clauses, "REDEFINES").map(|value| value.to_string()),
        parent,
        qualified_name,
        path,
        addressable,
        storage_area,
        external: clause_ast_has_external(&item.clause_ast),
        value_category,
        layout_id: None,
        offset: None,
        byte_len: None,
        sync: clause_ast_has_sync(&item.clause_ast),
        value,
        span: item.span.clone(),
    }
}

fn storage_area_ir(area: StorageAreaAst) -> StorageAreaIr {
    match area {
        StorageAreaAst::WorkingStorage => StorageAreaIr::WorkingStorage,
        StorageAreaAst::LocalStorage => StorageAreaIr::LocalStorage,
        StorageAreaAst::Linkage => StorageAreaIr::Linkage,
        StorageAreaAst::FileSection => StorageAreaIr::FileSection,
        StorageAreaAst::Unknown => StorageAreaIr::Unknown,
    }
}

fn storage_area_cursor_code(area: StorageAreaIr) -> u8 {
    match area {
        StorageAreaIr::WorkingStorage => 0,
        StorageAreaIr::LocalStorage => 1,
        StorageAreaIr::Linkage => 2,
        StorageAreaIr::FileSection => 3,
        StorageAreaIr::Unknown => 4,
    }
}

fn value_category_for(usage: &UsageIr, picture: Option<&PicIr>) -> ValueCategoryIr {
    match usage {
        UsageIr::Group => ValueCategoryIr::Group,
        UsageIr::PackedDecimal => ValueCategoryIr::PackedDecimal,
        UsageIr::Binary => ValueCategoryIr::Binary,
        UsageIr::NativeBinary => ValueCategoryIr::NativeBinary,
        UsageIr::Float32 | UsageIr::Float64 => ValueCategoryIr::Float,
        UsageIr::National => ValueCategoryIr::National,
        UsageIr::Dbcs => ValueCategoryIr::Dbcs,
        UsageIr::Alphanumeric => match picture.map(|pic| pic.category) {
            Some(PicCategoryIr::Alphabetic) => ValueCategoryIr::Alphabetic,
            _ => ValueCategoryIr::Alphanumeric,
        },
        UsageIr::Display => match picture.map(|pic| pic.category) {
            Some(PicCategoryIr::NumericDisplay) => ValueCategoryIr::NumericDisplay,
            Some(PicCategoryIr::NumericEdited) => ValueCategoryIr::NumericEdited,
            Some(PicCategoryIr::Alphabetic) => ValueCategoryIr::Alphabetic,
            Some(PicCategoryIr::Alphanumeric) => ValueCategoryIr::Alphanumeric,
            _ => ValueCategoryIr::Unsupported,
        },
        UsageIr::Unknown(_) => ValueCategoryIr::Unsupported,
    }
}

fn data_clause_requires_fail_closed_category(clauses: &[DataClauseAst]) -> bool {
    clause_ast_has_value_range(clauses)
        || [
            "SIGN",
            "JUSTIFIED",
            "BLANK",
            "GLOBAL",
            "POINTER",
            "PROCEDURE-POINTER",
            "BASED",
            "ANY LENGTH",
        ]
        .iter()
        .any(|word| clause_ast_has_unsupported_data_clause(clauses, word))
}

fn emit_data_clause_diagnostics(item: &DataDeclAst, diagnostics: &mut Vec<Diagnostic>) {
    if clause_ast_has_value_range(&item.clause_ast) {
        diagnostics.push(Diagnostic::error(
            "E_UNSUPPORTED_DATA_CLAUSE",
            format!(
                "Data Division VALUE range on {} requires condition-name semantics or exact runtime validation and remains fail-closed",
                item.name
            ),
            item.span.clone(),
        ));
    }
    for word in ["SIGN"] {
        if clause_ast_has_unsupported_data_clause(&item.clause_ast, word) {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_DATA_CLAUSE",
                format!(
                    "Data Division clause {word} requires real layout/runtime semantics and is not lowered yet"
                ),
                item.span.clone(),
            ));
        }
    }
    for word in [
        "JUSTIFIED",
        "BLANK",
        "GLOBAL",
        "POINTER",
        "PROCEDURE-POINTER",
        "BASED",
        "ANY LENGTH",
    ] {
        if clause_ast_has_unsupported_data_clause(&item.clause_ast, word) {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_DATA_CLAUSE",
                format!(
                    "Data Division clause {word} requires exact storage/runtime semantics and is not lowered yet"
                ),
                item.span.clone(),
            ));
        }
    }
    for word in ["NATIONAL", "DISPLAY-1", "DBCS", "KANJI"] {
        if clause_ast_has_usage_word(&item.clause_ast, word) {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_NATIONAL_DBCS",
                format!(
                    "Data Division clause {word} requires national/DBCS runtime semantics and remains fail-closed"
                ),
                item.span.clone(),
            ));
        }
    }
}

fn clause_ast_has_value_range(clauses: &[DataClauseAst]) -> bool {
    clauses.iter().any(|clause| {
        matches!(
            clause,
            DataClauseAst::Values(values)
                if values
                    .iter()
                    .any(|value| matches!(value, DataValueAst::Range { .. }))
        )
    })
}

fn clause_ast_has_unsupported_data_clause(clauses: &[DataClauseAst], needle: &str) -> bool {
    clauses.iter().any(|clause| match clause {
        DataClauseAst::Sign { .. } => needle.eq_ignore_ascii_case("SIGN"),
        DataClauseAst::Justified { .. } => needle.eq_ignore_ascii_case("JUSTIFIED"),
        DataClauseAst::BlankWhenZero => needle.eq_ignore_ascii_case("BLANK"),
        DataClauseAst::Global => needle.eq_ignore_ascii_case("GLOBAL"),
        DataClauseAst::Based { .. } => needle.eq_ignore_ascii_case("BASED"),
        DataClauseAst::AnyLength => needle.eq_ignore_ascii_case("ANY LENGTH"),
        DataClauseAst::Usage(value)
            if needle.eq_ignore_ascii_case("POINTER")
                || needle.eq_ignore_ascii_case("PROCEDURE-POINTER") =>
        {
            value.eq_ignore_ascii_case(needle)
        }
        DataClauseAst::Other(value) => value
            .trim()
            .trim_end_matches('.')
            .eq_ignore_ascii_case(needle),
        _ => false,
    })
}

fn clause_ast_has_other_word(clauses: &[DataClauseAst], needle: &str) -> bool {
    clauses.iter().any(|clause| match clause {
        DataClauseAst::Other(value) => value
            .trim()
            .trim_end_matches('.')
            .eq_ignore_ascii_case(needle),
        _ => false,
    })
}

fn clause_ast_has_usage_word(clauses: &[DataClauseAst], needle: &str) -> bool {
    clauses.iter().any(|clause| match clause {
        DataClauseAst::Usage(value) => value.eq_ignore_ascii_case(needle),
        _ => false,
    })
}

fn clause_ast_has_usage_clause(clauses: &[DataClauseAst]) -> bool {
    clauses
        .iter()
        .any(|clause| matches!(clause, DataClauseAst::Usage(_)))
}

fn clause_ast_has_sync(clauses: &[DataClauseAst]) -> bool {
    clauses
        .iter()
        .any(|clause| matches!(clause, DataClauseAst::Sync))
}

fn clause_ast_has_external(clauses: &[DataClauseAst]) -> bool {
    clauses
        .iter()
        .any(|clause| matches!(clause, DataClauseAst::External))
}

fn lower_paragraphs(
    paragraphs: Vec<cobol_syntax::ParagraphAst>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<ParagraphIr> {
    paragraphs
        .into_iter()
        .map(|paragraph| {
            let name = paragraph.name;
            let span = paragraph.span;
            let lowered_sentences = paragraph
                .sentences
                .into_iter()
                .map(|sentence| {
                    let statement_spans = sentence
                        .statements
                        .iter()
                        .map(|statement| statement.span.clone())
                        .collect::<Vec<_>>();
                    (
                        ProcedureSentenceIr {
                            statements: sentence
                                .statements
                                .into_iter()
                                .map(|statement| {
                                    lower_statement_with_diagnostics(statement, diagnostics)
                                })
                                .collect(),
                            span: sentence.span,
                        },
                        statement_spans,
                    )
                })
                .collect::<Vec<_>>();
            let sentence_statement_spans = lowered_sentences
                .iter()
                .flat_map(|(_, spans)| spans.iter().cloned())
                .collect::<Vec<_>>();
            let sentences = lowered_sentences
                .into_iter()
                .map(|(sentence, _)| sentence)
                .collect::<Vec<_>>();
            let statement_spans = if sentences.is_empty() {
                paragraph
                    .statements
                    .iter()
                    .map(|statement| statement.span.clone())
                    .collect()
            } else {
                sentence_statement_spans
            };
            let statements = if sentences.is_empty() {
                paragraph
                    .statements
                    .into_iter()
                    .map(|statement| lower_statement_with_diagnostics(statement, diagnostics))
                    .collect()
            } else {
                sentences
                    .iter()
                    .flat_map(|sentence| sentence.statements.iter().cloned())
                    .collect()
            };
            ParagraphIr {
                rust_name: rust_ident(&name),
                name,
                span,
                sentences,
                statements,
                statement_spans,
            }
        })
        .collect()
}

fn lower_declaratives(
    declaratives: Vec<cobol_syntax::DeclarativeAst>,
    files: &[FileIr],
    paragraphs: &[ParagraphIr],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<DeclarativeIr> {
    declaratives
        .into_iter()
        .map(|declarative| {
            let trigger = match declarative.trigger {
                DeclarativeTriggerAst::FileError(file) => {
                    let resolved = files
                        .iter()
                        .find(|candidate| candidate.name.eq_ignore_ascii_case(&file))
                        .map(|candidate| candidate.name.clone());
                    match resolved {
                        Some(file) => DeclarativeTriggerIr::FileError { file },
                        None => {
                            diagnostics.push(Diagnostic::error(
                                "E_UNSUPPORTED_SECTION",
                                format!(
                                    "DECLARATIVE {} USE AFTER ERROR target {file} is not a resolved SELECT file",
                                    declarative.name
                                ),
                                declarative.span.clone(),
                            ));
                            DeclarativeTriggerIr::Unsupported {
                                raw: format!("USE AFTER ERROR ON {file}"),
                            }
                        }
                    }
                }
                DeclarativeTriggerAst::Debugging(paragraph) => {
                    let resolved = paragraphs
                        .iter()
                        .find(|candidate| candidate.name.eq_ignore_ascii_case(&paragraph))
                        .map(|candidate| candidate.name.clone());
                    match resolved {
                        Some(paragraph) => DeclarativeTriggerIr::Debugging { paragraph },
                        None => {
                            diagnostics.push(Diagnostic::error(
                                "E_UNSUPPORTED_SECTION",
                                format!(
                                    "DECLARATIVE {} USE FOR DEBUGGING target {paragraph} is not a resolved paragraph",
                                    declarative.name
                                ),
                                declarative.span.clone(),
                            ));
                            DeclarativeTriggerIr::Unsupported {
                                raw: format!("USE FOR DEBUGGING ON {paragraph}"),
                            }
                        }
                    }
                }
                DeclarativeTriggerAst::Unsupported(raw) => {
                    diagnostics.push(Diagnostic::error(
                        "E_UNSUPPORTED_SECTION",
                        format!(
                            "unsupported DECLARATIVES USE phrase in {}: {raw}",
                            declarative.name
                        ),
                        declarative.span.clone(),
                    ));
                    DeclarativeTriggerIr::Unsupported { raw }
                }
                DeclarativeTriggerAst::Missing => {
                    diagnostics.push(Diagnostic::error(
                        "E_UNSUPPORTED_SECTION",
                        format!(
                            "DECLARATIVE {} has no supported USE AFTER ERROR ON file trigger",
                            declarative.name
                        ),
                        declarative.span.clone(),
                    ));
                    DeclarativeTriggerIr::Missing
                }
            };
            DeclarativeIr {
                name: declarative.name,
                trigger,
                statements: declarative
                    .statements
                    .into_iter()
                    .map(|statement| lower_statement_with_diagnostics(statement, diagnostics))
                    .collect(),
                span: declarative.span,
            }
        })
        .collect()
}

fn lower_statement_with_diagnostics(
    statement: cobol_syntax::StatementAst,
    diagnostics: &mut Vec<Diagnostic>,
) -> StatementIr {
    let span = statement.span.clone();
    let lowered = lower_statement_ast(statement);
    if let StatementIr::Unsupported { keyword, .. } = &lowered {
        let code = unsupported_statement_code(keyword);
        diagnostics.push(Diagnostic::error(
            code,
            format!("unsupported or not-yet-lowered COBOL statement: {keyword}"),
            span,
        ));
    } else if let StatementIr::Entry(entry) = &lowered {
        diagnostics.push(Diagnostic::error(
            "E_UNSUPPORTED_ENTRY",
            match &entry.name {
                CallTargetIr::Literal(name) => format!(
                    "ENTRY {name} declares an alternate entry point; alternate entry dispatch is represented in IR but not executable yet"
                ),
                CallTargetIr::Identifier(reference) => format!(
                    "ENTRY {} declares a dynamic alternate entry point; alternate entry dispatch is represented in IR but not executable yet",
                    reference.raw
                ),
            },
            span,
        ));
    }
    lowered
}

fn unsupported_statement_code(keyword: &str) -> &'static str {
    let upper = keyword.to_ascii_uppercase();
    if upper.contains("PERFORM VARYING") {
        "E_UNSUPPORTED_PERFORM_VARYING"
    } else if upper.contains("PERFORM UNTIL") || upper.contains("PERFORM WITH") {
        "E_UNSUPPORTED_PERFORM_UNTIL"
    } else if upper.contains("INLINE PERFORM") {
        "E_UNSUPPORTED_INLINE_PERFORM"
    } else if matches!(upper.as_str(), "SORT" | "RELEASE" | "RETURN") {
        "E_UNSUPPORTED_SORT"
    } else if matches!(
        upper.as_str(),
        "INSPECT" | "EXAMINE" | "STRING" | "UNSTRING"
    ) {
        "E_UNSUPPORTED_VERB"
    } else if upper.contains("DISPLAY") && upper.contains("NO ADVANCING") {
        "E_UNSUPPORTED_DISPLAY_NO_ADVANCING"
    } else if upper.contains("DISPLAY") {
        "E_UNSUPPORTED_DISPLAY"
    } else {
        "E_UNSUPPORTED_STATEMENT"
    }
}

fn diagnose_nested_unsupported_statements(
    paragraphs: &[ParagraphIr],
    declaratives: &[DeclarativeIr],
    diagnostics: &mut Vec<Diagnostic>,
) {
    for paragraph in paragraphs {
        for statement in &paragraph.statements {
            diagnose_unsupported_in_statement_children(statement, &paragraph.span, diagnostics);
        }
    }
    for declarative in declaratives {
        for statement in &declarative.statements {
            diagnose_unsupported_in_statement_children(statement, &declarative.span, diagnostics);
        }
    }
}

fn diagnose_unsupported_statements(
    statements: &[StatementIr],
    span: &SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for statement in statements {
        if let StatementIr::Unsupported { keyword, .. } = statement {
            diagnostics.push(Diagnostic::error(
                unsupported_statement_code(keyword),
                format!("unsupported or not-yet-lowered nested COBOL statement: {keyword}"),
                span.clone(),
            ));
        }
        diagnose_unsupported_in_statement_children(statement, span, diagnostics);
    }
}

fn diagnose_unsupported_in_statement_children(
    statement: &StatementIr,
    span: &SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match statement {
        StatementIr::Compute {
            on_size_error_ops,
            not_on_size_error_ops,
            ..
        } => {
            diagnose_unsupported_statements(on_size_error_ops, span, diagnostics);
            diagnose_unsupported_statements(not_on_size_error_ops, span, diagnostics);
        }
        StatementIr::If {
            then_statements,
            else_statements,
            ..
        } => {
            diagnose_unsupported_statements(then_statements, span, diagnostics);
            diagnose_unsupported_statements(else_statements, span, diagnostics);
        }
        StatementIr::Evaluate(evaluate) => {
            for arm in &evaluate.arms {
                diagnose_unsupported_statements(&arm.statements, span, diagnostics);
            }
        }
        StatementIr::Search(search) => {
            diagnose_unsupported_statements(&search.at_end, span, diagnostics);
            for when in &search.whens {
                diagnose_unsupported_statements(&when.statements, span, diagnostics);
            }
        }
        StatementIr::SearchAll(search) => {
            diagnose_unsupported_statements(&search.at_end, span, diagnostics);
            diagnose_unsupported_statements(&search.statements, span, diagnostics);
        }
        StatementIr::ReturnSortRecord(ret) => {
            diagnose_unsupported_statements(&ret.at_end_ops, span, diagnostics);
            diagnose_unsupported_statements(&ret.not_at_end_ops, span, diagnostics);
        }
        StatementIr::StartFile(start) => {
            diagnose_unsupported_statements(&start.invalid_key_ops, span, diagnostics);
            diagnose_unsupported_statements(&start.not_invalid_key_ops, span, diagnostics);
        }
        StatementIr::ReadFile(read) => {
            diagnose_unsupported_statements(&read.at_end_ops, span, diagnostics);
            diagnose_unsupported_statements(&read.not_at_end_ops, span, diagnostics);
            diagnose_unsupported_statements(&read.on_exception_ops, span, diagnostics);
        }
        StatementIr::WriteFile(write) => {
            diagnose_unsupported_statements(&write.invalid_key_ops, span, diagnostics);
            diagnose_unsupported_statements(&write.not_invalid_key_ops, span, diagnostics);
            diagnose_unsupported_statements(&write.on_exception_ops, span, diagnostics);
            diagnose_unsupported_statements(&write.not_on_exception_ops, span, diagnostics);
        }
        StatementIr::RewriteFile(rewrite) => {
            diagnose_unsupported_statements(&rewrite.invalid_key_ops, span, diagnostics);
            diagnose_unsupported_statements(&rewrite.not_invalid_key_ops, span, diagnostics);
        }
        StatementIr::DeleteFile(delete) => {
            diagnose_unsupported_statements(&delete.invalid_key_ops, span, diagnostics);
            diagnose_unsupported_statements(&delete.not_invalid_key_ops, span, diagnostics);
        }
        StatementIr::StringOp(string) => {
            diagnose_unsupported_statements(&string.on_overflow_ops, span, diagnostics);
            diagnose_unsupported_statements(&string.not_on_overflow_ops, span, diagnostics);
        }
        StatementIr::UnstringOp(unstring) => {
            diagnose_unsupported_statements(&unstring.on_overflow_ops, span, diagnostics);
            diagnose_unsupported_statements(&unstring.not_on_overflow_ops, span, diagnostics);
        }
        _ => {}
    }
}

fn lower_statement_ast(statement: cobol_syntax::StatementAst) -> StatementIr {
    lower_statement(statement.kind, statement.raw)
}

fn lower_statement_list(statements: Vec<cobol_syntax::StatementAst>) -> Vec<StatementIr> {
    statements.into_iter().map(lower_statement_ast).collect()
}

fn lower_statement(kind: StatementKindAst, raw: String) -> StatementIr {
    match kind {
        StatementKindAst::Display(values) => StatementIr::Display(
            values
                .into_iter()
                .map(|value| parse_operand(&value))
                .collect(),
        ),
        StatementKindAst::Move { source, target } => StatementIr::Move {
            source: parse_operand(&source),
            target: parse_data_ref(&target),
        },
        StatementKindAst::MoveCorresponding { source, target } => StatementIr::MoveCorresponding {
            source: parse_data_ref(&source),
            target: parse_data_ref(&target),
        },
        StatementKindAst::Add { source, target } => StatementIr::Add {
            source: parse_operand(&source),
            target: parse_data_ref(&target),
        },
        StatementKindAst::Subtract { source, target } => StatementIr::Subtract {
            source: parse_operand(&source),
            target: parse_data_ref(&target),
        },
        StatementKindAst::Multiply { source, target } => StatementIr::Multiply {
            source: parse_operand(&source),
            target: parse_data_ref(&target),
        },
        StatementKindAst::Divide { source, target } => StatementIr::Divide {
            source: parse_operand(&source),
            target: parse_data_ref(&target),
        },
        StatementKindAst::Compute(compute) => StatementIr::Compute {
            target: parse_data_ref(&compute.target),
            expression: compute.expression.clone(),
            rounded: compute.rounded,
            on_size_error_ops: lower_statement_list(compute.on_size_error.clone()),
            not_on_size_error_ops: lower_statement_list(compute.not_on_size_error.clone()),
        },
        StatementKindAst::BlockedNextSentence => StatementIr::BlockedNextSentence,
        StatementKindAst::Perform {
            target,
            through,
            varying,
            until,
            times,
            test_position: _,
        } => {
            let varying_ir = varying.as_deref().and_then(parse_perform_varying_clause);
            let until_tree = until
                .as_deref()
                .and_then(|condition| parse_condition(condition).ok());
            StatementIr::Perform {
                target,
                through,
                varying,
                varying_ir: varying_ir.map(Box::new),
                until,
                until_tree: until_tree.map(Box::new),
                times: times.map(|value| parse_operand(&value)),
            }
        }
        StatementKindAst::GoTo(target) => StatementIr::GoTo(target),
        StatementKindAst::ComputedGoTo {
            targets,
            depending_on,
        } => StatementIr::ComputedGoTo {
            targets,
            depending_on: parse_operand(&depending_on),
        },
        StatementKindAst::Alter { paragraph, target } => StatementIr::Alter { paragraph, target },
        StatementKindAst::Accept {
            target,
            source,
            options,
        } => StatementIr::Accept(AcceptIr {
            target: parse_data_ref(&target),
            source,
            options,
        }),
        StatementKindAst::Initialize { targets, options } => {
            StatementIr::Initialize(InitializeIr {
                targets: targets
                    .into_iter()
                    .map(|target| parse_data_ref(&target))
                    .collect(),
                options,
            })
        }
        StatementKindAst::Cancel { targets } => StatementIr::Cancel(CancelIr {
            targets: targets
                .into_iter()
                .map(|target| parse_call_target(&target))
                .collect(),
        }),
        StatementKindAst::Chain { target, using } => StatementIr::Chain(Box::new(ChainIr {
            target: parse_call_target(&target),
            using: using
                .into_iter()
                .map(|reference| parse_data_ref(&reference))
                .collect(),
        })),
        StatementKindAst::Unlock { file, options } => StatementIr::UnlockFile(UnlockFileIr {
            file: normalize_name(&file),
            options,
        }),
        StatementKindAst::Generate { target, options } => {
            StatementIr::GenerateReport(GenerateReportIr {
                target: normalize_name(&target),
                options,
            })
        }
        StatementKindAst::Initiate { targets } => StatementIr::InitiateReport(ReportLifecycleIr {
            targets: targets
                .into_iter()
                .map(|target| normalize_name(&target))
                .collect(),
        }),
        StatementKindAst::Terminate { targets } => {
            StatementIr::TerminateReport(ReportLifecycleIr {
                targets: targets
                    .into_iter()
                    .map(|target| normalize_name(&target))
                    .collect(),
            })
        }
        StatementKindAst::Purge { target, options } => StatementIr::PurgeQueue(PurgeQueueIr {
            target: normalize_name(&target),
            options,
        }),
        StatementKindAst::Suppress { target, options } => {
            StatementIr::SuppressReport(SuppressReportIr {
                target: target.map(|target| normalize_name(&target)),
                options,
            })
        }
        StatementKindAst::Enable { target, options } => {
            StatementIr::EnableCommunication(CommunicationControlIr {
                target: normalize_name(&target),
                options,
            })
        }
        StatementKindAst::Disable { target, options } => {
            StatementIr::DisableCommunication(CommunicationControlIr {
                target: normalize_name(&target),
                options,
            })
        }
        StatementKindAst::Send { target, options } => {
            StatementIr::SendCommunication(CommunicationMessageIr {
                target: normalize_name(&target),
                options,
            })
        }
        StatementKindAst::Receive { target, options } => {
            StatementIr::ReceiveCommunication(CommunicationMessageIr {
                target: normalize_name(&target),
                options,
            })
        }
        StatementKindAst::Enter { language, options } => {
            StatementIr::EnterLanguage(EnterLanguageIr {
                language: normalize_name(&language),
                options,
            })
        }
        StatementKindAst::Merge { file, options } => StatementIr::MergeFile(MergeFileIr {
            file: normalize_name(&file),
            options,
        }),
        StatementKindAst::Entry { name, using } => StatementIr::Entry(Box::new(EntryIr {
            name: parse_call_target(&name),
            using: using
                .into_iter()
                .map(|reference| parse_data_ref(&reference))
                .collect(),
        })),
        StatementKindAst::Start {
            file,
            options,
            invalid_key,
            not_invalid_key,
        } => {
            let (position, unsupported_options) = lower_start_options(&options);
            StatementIr::StartFile(StartFileIr {
                file: normalize_name(&file),
                position,
                raw_options: options,
                unsupported_options,
                invalid_key_ops: lower_statement_list(invalid_key),
                not_invalid_key_ops: lower_statement_list(not_invalid_key),
            })
        }
        StatementKindAst::Call { target, using } => {
            let target = parse_call_target(&target);
            StatementIr::Call(Box::new(CallIr {
                target,
                using: using
                    .into_iter()
                    .map(|reference| parse_data_ref(&reference))
                    .collect(),
            }))
        }
        StatementKindAst::Open(open) => StatementIr::OpenFile(OpenFileIr {
            file: normalize_name(&open.file),
            mode: lower_file_open_mode(open.mode),
        }),
        StatementKindAst::Read(read) => StatementIr::ReadFile(ReadFileIr {
            file: normalize_name(&read.file),
            into: read.into.as_deref().map(parse_data_ref),
            at_end_ops: lower_statement_list(read.at_end),
            not_at_end_ops: lower_statement_list(read.not_at_end),
            on_exception_ops: lower_statement_list(read.on_exception),
        }),
        StatementKindAst::Write(write) => StatementIr::WriteFile(WriteFileIr {
            record: parse_data_ref(&write.record),
            advancing: lower_write_advancing(write.advancing),
            invalid_key_ops: lower_statement_list(write.invalid_key),
            not_invalid_key_ops: lower_statement_list(write.not_invalid_key),
            on_exception_ops: lower_statement_list(write.on_exception),
            not_on_exception_ops: lower_statement_list(write.not_on_exception),
            branch_phrases: write.branch_phrases,
        }),
        StatementKindAst::Rewrite(rewrite) => StatementIr::RewriteFile(RewriteFileIr {
            record: parse_data_ref(&rewrite.record),
            invalid_key_ops: lower_statement_list(rewrite.invalid_key),
            not_invalid_key_ops: lower_statement_list(rewrite.not_invalid_key),
        }),
        StatementKindAst::Delete(delete) => StatementIr::DeleteFile(DeleteFileIr {
            file: normalize_name(&delete.file),
            invalid_key_ops: lower_statement_list(delete.invalid_key),
            not_invalid_key_ops: lower_statement_list(delete.not_invalid_key),
        }),
        StatementKindAst::Close(close) => StatementIr::CloseFile(CloseFileIr {
            file: normalize_name(&close.file),
        }),
        StatementKindAst::Sort(sort) => StatementIr::SortProcedure(SortProcedureIr {
            file: normalize_name(&sort.file),
            key: sort.key.map(|key| SortKeyIr {
                direction: match key.direction {
                    SortDirectionAst::Ascending => SortDirectionIr::Ascending,
                    SortDirectionAst::Descending => SortDirectionIr::Descending,
                },
                name: normalize_name(&key.name),
            }),
            input_range: sort.input_range.map(|range| ProcedureRangeIr {
                target: normalize_name(&range.target),
                through: range.through.map(|name| normalize_name(&name)),
            }),
            output_range: ProcedureRangeIr {
                target: normalize_name(&sort.output_range.target),
                through: sort.output_range.through.map(|name| normalize_name(&name)),
            },
        }),
        StatementKindAst::Release(release) => StatementIr::ReleaseSortRecord(ReleaseSortRecordIr {
            record: parse_data_ref(&release.record),
            from: release.from.as_deref().map(parse_data_ref),
        }),
        StatementKindAst::Return(ret) => StatementIr::ReturnSortRecord(ReturnSortRecordIr {
            file: normalize_name(&ret.file),
            into: ret.into.as_deref().map(parse_data_ref),
            at_end_ops: lower_statement_list(ret.at_end),
            not_at_end_ops: lower_statement_list(ret.not_at_end),
        }),
        StatementKindAst::Inspect(inspect) => lower_inspect_like_ast(inspect),
        StatementKindAst::Examine(examine) => lower_inspect_like_ast(examine),
        StatementKindAst::String(string) => StatementIr::StringOp(StringOpIr {
            pieces: string
                .pieces
                .into_iter()
                .map(|piece| StringPieceIr {
                    source: parse_operand(&piece.source),
                    delimiter: lower_string_delimiter(piece.delimiter),
                })
                .collect(),
            target: parse_data_ref(&string.target),
            pointer: string.pointer.as_deref().map(parse_data_ref),
            on_overflow_ops: lower_statement_list(string.on_overflow),
            not_on_overflow_ops: lower_statement_list(string.not_on_overflow),
        }),
        StatementKindAst::Unstring(unstring) => StatementIr::UnstringOp(UnstringOpIr {
            source: parse_operand(&unstring.source),
            delimiter: lower_string_delimiter(unstring.delimiter),
            targets: unstring
                .targets
                .into_iter()
                .map(|target| UnstringTargetIr {
                    target: parse_data_ref(&target.target),
                    count: target.count.as_deref().map(parse_data_ref),
                })
                .collect(),
            pointer: unstring.pointer.as_deref().map(parse_data_ref),
            tallying: unstring.tallying.as_deref().map(parse_data_ref),
            on_overflow_ops: lower_statement_list(unstring.on_overflow),
            not_on_overflow_ops: lower_statement_list(unstring.not_on_overflow),
        }),
        StatementKindAst::ReadyTrace => StatementIr::ReadyTrace,
        StatementKindAst::ResetTrace => StatementIr::ResetTrace,
        StatementKindAst::Continue => StatementIr::Continue,
        StatementKindAst::ExitProgram | StatementKindAst::Goback => StatementIr::Goback,
        StatementKindAst::Stop(_) => StatementIr::Unsupported {
            keyword: "STOP".to_string(),
            raw,
        },
        StatementKindAst::StopRun => StatementIr::StopRun,
        StatementKindAst::If {
            condition,
            then_statements,
            else_statements,
        } => lower_if_statement(&condition, then_statements, else_statements),
        StatementKindAst::Evaluate(evaluate) => StatementIr::Evaluate(lower_evaluate_ast(evaluate)),
        StatementKindAst::Search(search) => {
            lower_search_ast(&search).unwrap_or_else(|| StatementIr::Unsupported {
                keyword: "SEARCH".to_string(),
                raw: search.raw,
            })
        }
        StatementKindAst::SetCondition { condition, value } => StatementIr::SetCondition {
            condition: parse_data_ref(&condition),
            value,
        },
        StatementKindAst::SetIndex { index, operation } => StatementIr::SetIndex {
            index,
            operation: match operation {
                cobol_syntax::SetIndexAst::To(expr) => {
                    SetIndexOperationIr::To(parse_subscript_expr(&expr))
                }
                cobol_syntax::SetIndexAst::UpBy(expr) => {
                    SetIndexOperationIr::UpBy(parse_subscript_expr(&expr))
                }
                cobol_syntax::SetIndexAst::DownBy(expr) => {
                    SetIndexOperationIr::DownBy(parse_subscript_expr(&expr))
                }
            },
        },
        StatementKindAst::Unsupported(keyword) => StatementIr::Unsupported { keyword, raw },
    }
}

fn lower_file_open_mode(mode: FileOpenModeAst) -> FileOpenModeIr {
    match mode {
        FileOpenModeAst::Input => FileOpenModeIr::Input,
        FileOpenModeAst::Output => FileOpenModeIr::Output,
        FileOpenModeAst::Io => FileOpenModeIr::Io,
        FileOpenModeAst::Extend => FileOpenModeIr::Extend,
    }
}

fn lower_start_options(options: &[String]) -> (Option<StartPositionIr>, Vec<String>) {
    let mut position = None;
    let mut unsupported = Vec::new();
    for option in options {
        if option
            .split_whitespace()
            .next()
            .is_some_and(|word| word.eq_ignore_ascii_case("KEY"))
        {
            if position.is_none() {
                match parse_start_position(option) {
                    Some(parsed) => position = Some(parsed),
                    None => unsupported.push(option.clone()),
                }
            } else {
                unsupported.push(option.clone());
            }
        } else {
            unsupported.push(option.clone());
        }
    }
    (position, unsupported)
}

fn parse_start_position(raw: &str) -> Option<StartPositionIr> {
    let tokens = tokenize_condition(raw);
    if tokens.is_empty() || !tokens[0].eq_ignore_ascii_case("KEY") {
        return None;
    }
    let mut pos = 1usize;
    if tokens
        .get(pos)
        .is_some_and(|token| token.eq_ignore_ascii_case("IS"))
    {
        pos += 1;
    }
    if pos >= tokens.len() {
        return None;
    }

    let mut op = RelOpIr::Equal;
    if start_position_has_rel_op(&tokens, pos) {
        let mut parser = ConditionParser {
            tokens: tokens[pos..].to_vec(),
            pos: 0,
            last_subject: None,
            last_rel_op: None,
            allow_bare_abbrev: false,
        };
        op = parser.parse_rel_op().ok()?;
        pos += parser.pos;
    }
    if pos >= tokens.len() {
        return None;
    }
    Some(StartPositionIr {
        op,
        key: parse_data_ref(&tokens[pos..].join(" ")),
        raw: raw.to_string(),
    })
}

fn start_position_has_rel_op(tokens: &[String], pos: usize) -> bool {
    let Some(token) = tokens.get(pos) else {
        return false;
    };
    matches!(
        token.to_ascii_uppercase().as_str(),
        "=" | "<>" | "!=" | ">" | ">=" | "<" | "<=" | "GREATER" | "LESS" | "EQUAL" | "NOT"
    )
}

fn lower_write_advancing(advancing: WriteAdvancingAst) -> WriteAdvancingIr {
    match advancing {
        WriteAdvancingAst::None => WriteAdvancingIr::None,
        WriteAdvancingAst::BeforeLines(lines) => WriteAdvancingIr::BeforeLines(lines),
        WriteAdvancingAst::AfterLines(lines) => WriteAdvancingIr::AfterLines(lines),
        WriteAdvancingAst::BeforePage => WriteAdvancingIr::BeforePage,
        WriteAdvancingAst::AfterPage => WriteAdvancingIr::AfterPage,
    }
}

fn lower_inspect_like_ast(inspect: InspectLikeAst) -> StatementIr {
    StatementIr::InspectLike(InspectLikeIr {
        subject: parse_data_ref(&inspect.subject),
        tally: inspect.tally.map(|tally| InspectTallyIr {
            target: parse_data_ref(&tally.target),
            pattern: normalize_value_literal(&tally.pattern),
        }),
        replacing: inspect.replacing.map(|replacing| InspectReplacingIr {
            pattern: normalize_value_literal(&replacing.pattern),
            replacement: normalize_value_literal(&replacing.replacement),
        }),
        converting: inspect.converting.map(|converting| InspectConvertingIr {
            from: normalize_value_literal(&converting.from),
            to: normalize_value_literal(&converting.to),
        }),
    })
}

fn lower_string_delimiter(delimiter: StringDelimiterAst) -> StringDelimiterIr {
    match delimiter {
        StringDelimiterAst::Size => StringDelimiterIr::Size,
        StringDelimiterAst::Literal { value, all } => StringDelimiterIr::Literal {
            value: normalize_value_literal(&value),
            all,
        },
    }
}

fn lower_if_statement(
    condition: &str,
    then_statements: Vec<cobol_syntax::StatementAst>,
    else_statements: Vec<cobol_syntax::StatementAst>,
) -> StatementIr {
    let condition_tree = parse_condition(condition).ok();
    StatementIr::If {
        condition: condition.to_string(),
        condition_tree,
        then_statements: lower_statement_list(then_statements),
        else_statements: lower_statement_list(else_statements),
    }
}

fn lower_evaluate_ast(evaluate: cobol_syntax::EvaluateAst) -> EvaluateIr {
    let subjects = evaluate
        .subjects
        .iter()
        .filter_map(|subject| parse_evaluate_subject(&tokenize_condition(subject)))
        .collect::<Vec<_>>();
    let subject_count = subjects.len().max(1);
    let arms = evaluate
        .arms
        .into_iter()
        .map(|arm| lower_evaluate_arm_ast(arm, subject_count))
        .collect::<Vec<_>>();
    EvaluateIr {
        raw: evaluate.raw,
        subjects,
        arms,
    }
}

fn lower_evaluate_arm_ast(
    arm: cobol_syntax::EvaluateArmAst,
    subject_count: usize,
) -> EvaluateArmIr {
    let patterns = if arm.patterns.len() == 1 && arm.patterns[0].eq_ignore_ascii_case("OTHER") {
        vec![EvaluatePatternIr::Any; subject_count]
    } else {
        arm.patterns
            .iter()
            .filter_map(|pattern| parse_evaluate_pattern(&tokenize_condition(pattern)))
            .collect::<Vec<_>>()
    };
    EvaluateArmIr {
        raw: arm.raw,
        patterns,
        statements: lower_statement_list(arm.statements),
    }
}

fn lower_search_ast(search: &cobol_syntax::SearchAst) -> Option<StatementIr> {
    if search.all {
        let when = search.whens.first()?;
        return Some(StatementIr::SearchAll(SearchAllIr {
            table: normalize_reference(&search.table),
            index: search.index.as_deref().map(normalize_name),
            declared_key: None,
            key_condition: parse_condition(&when.condition).ok()?,
            at_end: lower_statement_list(search.at_end.clone()),
            statements: lower_statement_list(when.statements.clone()),
        }));
    }
    Some(StatementIr::Search(SearchIr {
        table: normalize_reference(&search.table),
        index: search.index.as_deref().map(normalize_name),
        at_end: lower_statement_list(search.at_end.clone()),
        whens: search
            .whens
            .iter()
            .map(|when| {
                Some(SearchWhenIr {
                    condition: parse_condition(&when.condition).ok()?,
                    statements: lower_statement_list(when.statements.clone()),
                })
            })
            .collect::<Option<Vec<_>>>()?,
    }))
}

fn parse_evaluate_subject(tokens: &[String]) -> Option<EvaluateSubjectIr> {
    if tokens.is_empty() {
        return None;
    }
    let raw = tokens.join(" ");
    if raw.eq_ignore_ascii_case("TRUE") {
        return Some(EvaluateSubjectIr::Operand(ConditionOperandIr::Bool(true)));
    }
    if raw.eq_ignore_ascii_case("FALSE") {
        return Some(EvaluateSubjectIr::Operand(ConditionOperandIr::Bool(false)));
    }
    if tokens_form_condition(tokens) {
        parse_condition(&raw).map(EvaluateSubjectIr::Condition).ok()
    } else {
        Some(EvaluateSubjectIr::Operand(parse_condition_operand(&raw)))
    }
}

fn parse_evaluate_pattern(tokens: &[String]) -> Option<EvaluatePatternIr> {
    if tokens.is_empty() {
        return None;
    }
    if tokens.len() == 1 && tokens[0].eq_ignore_ascii_case("ANY") {
        return Some(EvaluatePatternIr::Any);
    }
    if tokens.len() == 1 && tokens[0].eq_ignore_ascii_case("OTHER") {
        return Some(EvaluatePatternIr::Any);
    }
    if let Some(thru_idx) = tokens.iter().position(|token| {
        token.eq_ignore_ascii_case("THRU") || token.eq_ignore_ascii_case("THROUGH")
    }) {
        let start = parse_condition_operand(&tokens[..thru_idx].join(" "));
        let end = parse_condition_operand(&tokens[thru_idx + 1..].join(" "));
        return Some(EvaluatePatternIr::Range { start, end });
    }
    let raw = tokens.join(" ");
    if raw.eq_ignore_ascii_case("TRUE") {
        return Some(EvaluatePatternIr::Operand(ConditionOperandIr::Bool(true)));
    }
    if raw.eq_ignore_ascii_case("FALSE") {
        return Some(EvaluatePatternIr::Operand(ConditionOperandIr::Bool(false)));
    }
    if tokens_form_condition(tokens) {
        parse_condition(&raw).map(EvaluatePatternIr::Condition).ok()
    } else {
        Some(EvaluatePatternIr::Operand(parse_condition_operand(&raw)))
    }
}

fn tokens_form_condition(tokens: &[String]) -> bool {
    tokens.iter().any(|token| {
        token.eq_ignore_ascii_case("AND")
            || token.eq_ignore_ascii_case("OR")
            || token.eq_ignore_ascii_case("NOT")
            || token.eq_ignore_ascii_case("IS")
            || is_rel_token(token)
    })
}

fn build_control_flow(paragraphs: &[ParagraphIr]) -> ControlFlowIr {
    let mut flows = Vec::new();
    for (idx, paragraph) in paragraphs.iter().enumerate() {
        let mut edges = Vec::new();
        let mut terminal = false;
        for statement in &paragraph.statements {
            match statement {
                StatementIr::Perform {
                    target, through, ..
                } => {
                    edges.push(ControlFlowEdgeIr {
                        kind: ControlFlowEdgeKindIr::Perform,
                        target: Some(
                            through
                                .as_ref()
                                .map(|end| format!("{target} THRU {end}"))
                                .unwrap_or_else(|| target.clone()),
                        ),
                    });
                }
                StatementIr::GoTo(target) => {
                    edges.push(ControlFlowEdgeIr {
                        kind: ControlFlowEdgeKindIr::GoTo,
                        target: Some(target.clone()),
                    });
                    terminal = true;
                    break;
                }
                StatementIr::ComputedGoTo { targets, .. } => {
                    for target in targets {
                        edges.push(ControlFlowEdgeIr {
                            kind: ControlFlowEdgeKindIr::GoTo,
                            target: Some(target.clone()),
                        });
                    }
                }
                StatementIr::Goback => {
                    edges.push(ControlFlowEdgeIr {
                        kind: ControlFlowEdgeKindIr::Goback,
                        target: None,
                    });
                    terminal = true;
                    break;
                }
                StatementIr::StopRun => {
                    edges.push(ControlFlowEdgeIr {
                        kind: ControlFlowEdgeKindIr::StopRun,
                        target: None,
                    });
                    terminal = true;
                    break;
                }
                _ => {}
            }
        }
        if !terminal {
            if let Some(next) = paragraphs.get(idx + 1) {
                edges.push(ControlFlowEdgeIr {
                    kind: ControlFlowEdgeKindIr::FallThrough,
                    target: Some(next.name.clone()),
                });
            }
        }
        flows.push(ParagraphFlowIr {
            name: paragraph.name.clone(),
            index: idx,
            can_fall_through: !terminal && idx + 1 < paragraphs.len(),
            edges,
        });
    }
    ControlFlowIr { paragraphs: flows }
}

fn build_procedure_cfg(paragraphs: &[ParagraphIr]) -> ProcedureCfgIr {
    let mut blocks = Vec::new();
    let mut next_sentence_targets = Vec::new();
    for (paragraph_idx, paragraph) in paragraphs.iter().enumerate() {
        if paragraph.sentences.is_empty() {
            let statements = paragraph.statements.clone();
            let label = procedure_block_label(&paragraph.name, 0);
            let next_target = next_paragraph_block_label(paragraphs, paragraph_idx);
            collect_next_sentence_targets(
                &statements,
                &label,
                next_target.clone(),
                &mut next_sentence_targets,
            );
            let transfer = statement_transfer(&statements, next_target.clone())
                .unwrap_or(ControlTransferIr::FallThrough(next_target));
            blocks.push(BasicBlockIr {
                id: blocks.len(),
                label,
                paragraph: paragraph.name.clone(),
                sentence_index: 0,
                statements,
                transfer,
            });
            continue;
        }

        for (sentence_index, sentence) in paragraph.sentences.iter().enumerate() {
            let statements = sentence.statements.clone();
            let label = procedure_block_label(&paragraph.name, sentence_index);
            let next_target = next_procedure_block_label(paragraphs, paragraph_idx, sentence_index);
            collect_next_sentence_targets(
                &statements,
                &label,
                next_target.clone(),
                &mut next_sentence_targets,
            );
            let transfer = statement_transfer(&statements, next_target.clone())
                .unwrap_or(ControlTransferIr::FallThrough(next_target));
            blocks.push(BasicBlockIr {
                id: blocks.len(),
                label,
                paragraph: paragraph.name.clone(),
                sentence_index,
                statements,
                transfer,
            });
        }
    }
    ProcedureCfgIr {
        entry: paragraphs.first().map(|paragraph| paragraph.name.clone()),
        blocks,
        next_sentence_targets,
    }
}

fn procedure_block_label(paragraph: &str, sentence_index: usize) -> String {
    if sentence_index == 0 {
        paragraph.to_string()
    } else {
        format!("{paragraph}#{}", sentence_index + 1)
    }
}

fn next_procedure_block_label(
    paragraphs: &[ParagraphIr],
    paragraph_idx: usize,
    sentence_index: usize,
) -> Option<String> {
    let paragraph = paragraphs.get(paragraph_idx)?;
    if sentence_index + 1 < paragraph.sentences.len() {
        Some(procedure_block_label(&paragraph.name, sentence_index + 1))
    } else {
        next_paragraph_block_label(paragraphs, paragraph_idx)
    }
}

fn next_paragraph_block_label(paragraphs: &[ParagraphIr], paragraph_idx: usize) -> Option<String> {
    paragraphs
        .get(paragraph_idx + 1)
        .map(|next| procedure_block_label(&next.name, 0))
}

fn statement_transfer(
    statements: &[StatementIr],
    next_sentence_target: Option<String>,
) -> Option<ControlTransferIr> {
    statements.iter().find_map(|statement| match statement {
        StatementIr::BlockedNextSentence => Some(ControlTransferIr::NextSentence {
            target: next_sentence_target.clone(),
        }),
        StatementIr::Perform {
            target,
            through,
            varying,
            until,
            times,
            ..
        } => Some(ControlTransferIr::Perform(Box::new(PerformIr {
            target: target.clone(),
            through: through.clone(),
            varying: varying.clone(),
            until: until.clone(),
            times: times.clone(),
        }))),
        StatementIr::GoTo(target) => Some(ControlTransferIr::GoTo(GoToIr {
            target: target.clone(),
        })),
        StatementIr::Goback => Some(ControlTransferIr::Goback),
        StatementIr::StopRun => Some(ControlTransferIr::StopRun),
        _ => None,
    })
}

fn collect_next_sentence_targets(
    statements: &[StatementIr],
    source_block: &str,
    target: Option<String>,
    out: &mut Vec<NextSentenceTargetIr>,
) {
    let mut path = Vec::new();
    collect_next_sentence_targets_at_path(statements, source_block, target, &mut path, out);
}

fn collect_next_sentence_targets_at_path(
    statements: &[StatementIr],
    source_block: &str,
    target: Option<String>,
    path: &mut Vec<StatementPathElementIr>,
    out: &mut Vec<NextSentenceTargetIr>,
) {
    for (idx, statement) in statements.iter().enumerate() {
        path.push(StatementPathElementIr::Statement(idx));
        match statement {
            StatementIr::BlockedNextSentence => out.push(NextSentenceTargetIr {
                source_block: source_block.to_string(),
                target: target.clone(),
                path: path.clone(),
            }),
            StatementIr::Compute {
                on_size_error_ops,
                not_on_size_error_ops,
                ..
            } => {
                collect_next_sentence_branch(
                    on_size_error_ops,
                    StatementBranchIr::OnSizeError,
                    source_block,
                    target.clone(),
                    path,
                    out,
                );
                collect_next_sentence_branch(
                    not_on_size_error_ops,
                    StatementBranchIr::NotOnSizeError,
                    source_block,
                    target.clone(),
                    path,
                    out,
                );
            }
            StatementIr::If {
                then_statements,
                else_statements,
                ..
            } => {
                collect_next_sentence_branch(
                    then_statements,
                    StatementBranchIr::Then,
                    source_block,
                    target.clone(),
                    path,
                    out,
                );
                collect_next_sentence_branch(
                    else_statements,
                    StatementBranchIr::Else,
                    source_block,
                    target.clone(),
                    path,
                    out,
                );
            }
            StatementIr::Evaluate(evaluate) => {
                for (arm_idx, arm) in evaluate.arms.iter().enumerate() {
                    collect_next_sentence_branch(
                        &arm.statements,
                        StatementBranchIr::EvaluateArm(arm_idx),
                        source_block,
                        target.clone(),
                        path,
                        out,
                    );
                }
            }
            StatementIr::Search(search) => {
                collect_next_sentence_branch(
                    &search.at_end,
                    StatementBranchIr::AtEnd,
                    source_block,
                    target.clone(),
                    path,
                    out,
                );
                for (when_idx, when) in search.whens.iter().enumerate() {
                    collect_next_sentence_branch(
                        &when.statements,
                        StatementBranchIr::SearchWhen(when_idx),
                        source_block,
                        target.clone(),
                        path,
                        out,
                    );
                }
            }
            StatementIr::SearchAll(search) => {
                collect_next_sentence_branch(
                    &search.at_end,
                    StatementBranchIr::AtEnd,
                    source_block,
                    target.clone(),
                    path,
                    out,
                );
                collect_next_sentence_branch(
                    &search.statements,
                    StatementBranchIr::SearchAllBody,
                    source_block,
                    target.clone(),
                    path,
                    out,
                );
            }
            StatementIr::ReadFile(read) => {
                collect_next_sentence_branch(
                    &read.at_end_ops,
                    StatementBranchIr::AtEnd,
                    source_block,
                    target.clone(),
                    path,
                    out,
                );
                collect_next_sentence_branch(
                    &read.not_at_end_ops,
                    StatementBranchIr::NotAtEnd,
                    source_block,
                    target.clone(),
                    path,
                    out,
                );
                collect_next_sentence_branch(
                    &read.on_exception_ops,
                    StatementBranchIr::OnException,
                    source_block,
                    target.clone(),
                    path,
                    out,
                );
            }
            StatementIr::WriteFile(write) => {
                collect_next_sentence_branch(
                    &write.invalid_key_ops,
                    StatementBranchIr::InvalidKey,
                    source_block,
                    target.clone(),
                    path,
                    out,
                );
                collect_next_sentence_branch(
                    &write.not_invalid_key_ops,
                    StatementBranchIr::NotInvalidKey,
                    source_block,
                    target.clone(),
                    path,
                    out,
                );
                collect_next_sentence_branch(
                    &write.on_exception_ops,
                    StatementBranchIr::OnException,
                    source_block,
                    target.clone(),
                    path,
                    out,
                );
                collect_next_sentence_branch(
                    &write.not_on_exception_ops,
                    StatementBranchIr::NotOnException,
                    source_block,
                    target.clone(),
                    path,
                    out,
                );
            }
            StatementIr::RewriteFile(rewrite) => {
                collect_next_sentence_branch(
                    &rewrite.invalid_key_ops,
                    StatementBranchIr::InvalidKey,
                    source_block,
                    target.clone(),
                    path,
                    out,
                );
                collect_next_sentence_branch(
                    &rewrite.not_invalid_key_ops,
                    StatementBranchIr::NotInvalidKey,
                    source_block,
                    target.clone(),
                    path,
                    out,
                );
            }
            StatementIr::StartFile(start) => {
                collect_next_sentence_branch(
                    &start.invalid_key_ops,
                    StatementBranchIr::InvalidKey,
                    source_block,
                    target.clone(),
                    path,
                    out,
                );
                collect_next_sentence_branch(
                    &start.not_invalid_key_ops,
                    StatementBranchIr::NotInvalidKey,
                    source_block,
                    target.clone(),
                    path,
                    out,
                );
            }
            StatementIr::DeleteFile(delete) => {
                collect_next_sentence_branch(
                    &delete.invalid_key_ops,
                    StatementBranchIr::InvalidKey,
                    source_block,
                    target.clone(),
                    path,
                    out,
                );
                collect_next_sentence_branch(
                    &delete.not_invalid_key_ops,
                    StatementBranchIr::NotInvalidKey,
                    source_block,
                    target.clone(),
                    path,
                    out,
                );
            }
            StatementIr::ReturnSortRecord(ret) => {
                collect_next_sentence_branch(
                    &ret.at_end_ops,
                    StatementBranchIr::AtEnd,
                    source_block,
                    target.clone(),
                    path,
                    out,
                );
                collect_next_sentence_branch(
                    &ret.not_at_end_ops,
                    StatementBranchIr::NotAtEnd,
                    source_block,
                    target.clone(),
                    path,
                    out,
                );
            }
            StatementIr::StringOp(string) => {
                collect_next_sentence_branch(
                    &string.on_overflow_ops,
                    StatementBranchIr::OnOverflow,
                    source_block,
                    target.clone(),
                    path,
                    out,
                );
                collect_next_sentence_branch(
                    &string.not_on_overflow_ops,
                    StatementBranchIr::NotOnOverflow,
                    source_block,
                    target.clone(),
                    path,
                    out,
                );
            }
            StatementIr::UnstringOp(unstring) => {
                collect_next_sentence_branch(
                    &unstring.on_overflow_ops,
                    StatementBranchIr::OnOverflow,
                    source_block,
                    target.clone(),
                    path,
                    out,
                );
                collect_next_sentence_branch(
                    &unstring.not_on_overflow_ops,
                    StatementBranchIr::NotOnOverflow,
                    source_block,
                    target.clone(),
                    path,
                    out,
                );
            }
            _ => {}
        }
        path.pop();
    }
}

fn collect_next_sentence_branch(
    statements: &[StatementIr],
    branch: StatementBranchIr,
    source_block: &str,
    target: Option<String>,
    path: &mut Vec<StatementPathElementIr>,
    out: &mut Vec<NextSentenceTargetIr>,
) {
    path.push(StatementPathElementIr::Branch(branch));
    collect_next_sentence_targets_at_path(statements, source_block, target, path, out);
    path.pop();
}

fn collect_valid_odo_tables(
    items: &[DataItemIr],
    data_index: &DataReferenceIndex<'_>,
) -> BTreeSet<String> {
    items
        .iter()
        .filter_map(|item| {
            let occurs = item.occurs.as_ref()?;
            if occurs_range_is_valid(occurs) && odo_counter_resolves_to_numeric(occurs, data_index)
            {
                Some(item.qualified_name.clone())
            } else {
                None
            }
        })
        .collect()
}

fn collect_odo_descriptors(
    items: &[DataItemIr],
    data_index: &DataReferenceIndex<'_>,
) -> Vec<OdoDescriptorIr> {
    items
        .iter()
        .filter_map(|item| {
            let occurs = item.occurs.as_ref()?;
            if !occurs_range_is_valid(occurs) {
                return None;
            }
            if !odo_counter_resolves_to_numeric(occurs, data_index) {
                return None;
            }
            let depending_on = occurs.depending_on.as_ref()?;
            let stride = item.byte_len.unwrap_or(0) / occurs.max.max(1);
            Some(OdoDescriptorIr {
                table: item.qualified_name.clone(),
                depending_on: depending_on.clone(),
                min: occurs.min,
                max: occurs.max,
                stride,
            })
        })
        .collect()
}

fn odo_counter_resolves_to_numeric(occurs: &OccursIr, data_index: &DataReferenceIndex<'_>) -> bool {
    let Some(depending_on) = occurs.depending_on.as_ref() else {
        return false;
    };
    match data_index.resolve_ref(&DataRefIr::simple(depending_on.clone())) {
        DataResolution::Resolved(counter) => category_is_numeric(counter.value_category),
        DataResolution::Missing
        | DataResolution::Ambiguous(_)
        | DataResolution::Condition(_)
        | DataResolution::Special { .. } => false,
    }
}

fn validate_occurs_ranges(data_items: &[DataItemIr], diagnostics: &mut Vec<Diagnostic>) {
    for item in data_items {
        let Some(occurs) = &item.occurs else {
            continue;
        };
        if occurs_range_is_valid(occurs) {
            continue;
        }
        diagnostics.push(Diagnostic::error(
            "E_INVALID_OCCURS_RANGE",
            format!(
                "OCCURS range for {} has minimum {} greater than maximum {}",
                item.qualified_name, occurs.min, occurs.max
            ),
            item.span.clone(),
        ));
    }
}

enum DataResolution<'a> {
    Missing,
    Ambiguous(Vec<String>),
    Resolved(&'a DataItemIr),
    Condition(&'a ConditionNameIr),
    Special {
        name: String,
        category: ValueCategoryIr,
        byte_len: usize,
    },
}

fn validate_odo_counters(
    data_items: &[DataItemIr],
    data_index: &DataReferenceIndex<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for item in data_items {
        let Some(depending_on) = item
            .occurs
            .as_ref()
            .and_then(|occurs| occurs.depending_on.as_ref())
        else {
            continue;
        };
        match data_index.resolve_ref(&DataRefIr::simple(depending_on.clone())) {
            DataResolution::Resolved(counter) if category_is_numeric(counter.value_category) => {}
            DataResolution::Resolved(counter) => diagnostics.push(Diagnostic::error(
                "E_INVALID_ODO_DEPENDING_ON",
                format!(
                    "OCCURS DEPENDING ON {} for {} must reference a numeric data item, found {:?}",
                    depending_on, item.qualified_name, counter.value_category
                ),
                item.span.clone(),
            )),
            DataResolution::Missing => diagnostics.push(Diagnostic::error(
                "E_UNRESOLVED_ODO_DEPENDING_ON",
                format!(
                    "OCCURS DEPENDING ON {} for {} references an unknown counter",
                    depending_on, item.qualified_name
                ),
                item.span.clone(),
            )),
            DataResolution::Ambiguous(matches) => diagnostics.push(Diagnostic::error(
                "E_AMBIGUOUS_ODO_DEPENDING_ON",
                format!(
                    "OCCURS DEPENDING ON {} for {} is ambiguous: {}",
                    depending_on,
                    item.qualified_name,
                    matches.join(", ")
                ),
                item.span.clone(),
            )),
            DataResolution::Condition(condition) => diagnostics.push(Diagnostic::error(
                "E_INVALID_ODO_DEPENDING_ON",
                format!(
                    "OCCURS DEPENDING ON {} for {} resolved to condition-name {}.{}",
                    depending_on, item.qualified_name, condition.parent, condition.name
                ),
                item.span.clone(),
            )),
            DataResolution::Special {
                name,
                category,
                byte_len,
            } => diagnostics.push(Diagnostic::error(
                "E_INVALID_ODO_DEPENDING_ON",
                format!(
                    "OCCURS DEPENDING ON {} for {} must reference a numeric data item, but resolved to special register {} ({:?}, {} bytes)",
                    depending_on, item.qualified_name, name, category, byte_len
                ),
                item.span.clone(),
            )),
        }
    }
}

struct DataReferenceIndex<'a> {
    by_name: BTreeMap<String, Vec<&'a DataItemIr>>,
    by_qualified: BTreeMap<String, &'a DataItemIr>,
    conditions_by_name: BTreeMap<String, Vec<&'a ConditionNameIr>>,
    conditions_by_qualified: BTreeMap<String, &'a ConditionNameIr>,
    redefined_bases: BTreeSet<String>,
}

impl<'a> DataReferenceIndex<'a> {
    fn new(items: &'a [DataItemIr], conditions: &'a [ConditionNameIr]) -> Self {
        let mut by_name = BTreeMap::<String, Vec<&DataItemIr>>::new();
        let mut by_qualified = BTreeMap::<String, &DataItemIr>::new();
        let mut conditions_by_name = BTreeMap::<String, Vec<&ConditionNameIr>>::new();
        let mut conditions_by_qualified = BTreeMap::<String, &ConditionNameIr>::new();
        let mut redefined_bases = BTreeSet::new();
        for item in items {
            if !item.addressable {
                continue;
            }
            by_name
                .entry(item.name.to_ascii_uppercase())
                .or_default()
                .push(item);
            by_qualified.insert(item.qualified_name.to_ascii_uppercase(), item);
        }
        for condition in conditions {
            conditions_by_name
                .entry(condition.name.to_ascii_uppercase())
                .or_default()
                .push(condition);
            conditions_by_qualified.insert(
                format!(
                    "{}.{}",
                    condition.parent.to_ascii_uppercase(),
                    condition.name.to_ascii_uppercase()
                ),
                condition,
            );
        }
        for item in items {
            let Some(base) = &item.redefines else {
                continue;
            };
            let base_key = normalize_data_key(base);
            if let Some(parent) = &item.parent {
                redefined_bases.insert(format!("{}.{}", parent.to_ascii_uppercase(), base_key));
            } else {
                redefined_bases.insert(base_key);
            }
        }
        Self {
            by_name,
            by_qualified,
            conditions_by_name,
            conditions_by_qualified,
            redefined_bases,
        }
    }

    fn resolve_ref(&self, reference: &DataRefIr) -> DataResolution<'a> {
        self.resolve(&reference.normalized)
    }

    fn resolve(&self, reference: &str) -> DataResolution<'a> {
        let key = reference.to_ascii_uppercase();
        if let Some(condition) = self.conditions_by_qualified.get(&key).copied() {
            return DataResolution::Condition(condition);
        }
        if let Some(item) = self.by_qualified.get(&key).copied() {
            return DataResolution::Resolved(item);
        }
        if key.contains('.') {
            let suffix = format!(".{key}");
            let condition_matches = self
                .conditions_by_qualified
                .iter()
                .filter_map(|(qualified, condition)| {
                    if qualified.ends_with(&suffix) {
                        Some(*condition)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            match condition_matches.as_slice() {
                [condition] => return DataResolution::Condition(condition),
                [] => {}
                matches => {
                    return DataResolution::Ambiguous(
                        matches
                            .iter()
                            .map(|condition| format!("{}.{}", condition.parent, condition.name))
                            .collect(),
                    )
                }
            }

            let matches = self
                .by_qualified
                .iter()
                .filter_map(|(qualified, item)| {
                    if qualified.ends_with(&suffix) {
                        Some(*item)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            return match matches.as_slice() {
                [item] => DataResolution::Resolved(item),
                [] => DataResolution::Missing,
                items => DataResolution::Ambiguous(
                    items
                        .iter()
                        .map(|item| item.qualified_name.clone())
                        .collect(),
                ),
            };
        }
        if let Some(conditions) = self.conditions_by_name.get(&key).map(Vec::as_slice) {
            return match conditions {
                [condition] => DataResolution::Condition(condition),
                matches => DataResolution::Ambiguous(
                    matches
                        .iter()
                        .map(|condition| format!("{}.{}", condition.parent, condition.name))
                        .collect(),
                ),
            };
        }
        match self.by_name.get(&key).map(Vec::as_slice) {
            Some([item]) => DataResolution::Resolved(item),
            Some(items) => DataResolution::Ambiguous(
                items
                    .iter()
                    .map(|item| item.qualified_name.clone())
                    .collect(),
            ),
            None if key == PROGRAM_STATUS_REGISTER => DataResolution::Special {
                name: PROGRAM_STATUS_REGISTER.to_string(),
                category: ValueCategoryIr::Alphanumeric,
                byte_len: 2,
            },
            None if key == TALLY_REGISTER => DataResolution::Special {
                name: TALLY_REGISTER.to_string(),
                category: ValueCategoryIr::NumericDisplay,
                byte_len: 5,
            },
            None if key == DEBUG_ITEM_REGISTER => DataResolution::Special {
                name: DEBUG_ITEM_REGISTER.to_string(),
                category: ValueCategoryIr::Alphanumeric,
                byte_len: 64,
            },
            None if key == DEBUG_CONTENTS_REGISTER => DataResolution::Special {
                name: DEBUG_CONTENTS_REGISTER.to_string(),
                category: ValueCategoryIr::Alphanumeric,
                byte_len: 16,
            },
            None => DataResolution::Missing,
        }
    }

    fn has_occurs_context(&self, item: &DataItemIr) -> bool {
        if item.occurs.is_some() {
            return true;
        }
        let mut parent = item.parent.as_ref().map(|value| value.to_ascii_uppercase());
        while let Some(parent_key) = parent {
            let Some(parent_item) = self.by_qualified.get(&parent_key).copied() else {
                return false;
            };
            if parent_item.occurs.is_some() {
                return true;
            }
            parent = parent_item
                .parent
                .as_ref()
                .map(|value| value.to_ascii_uppercase());
        }
        false
    }

    fn has_dynamic_occurs_context(&self, item: &DataItemIr) -> bool {
        if item
            .occurs
            .as_ref()
            .and_then(|occurs| occurs.depending_on.as_ref())
            .is_some()
        {
            return true;
        }
        let mut parent = item.parent.as_ref().map(|value| value.to_ascii_uppercase());
        while let Some(parent_key) = parent {
            let Some(parent_item) = self.by_qualified.get(&parent_key).copied() else {
                return false;
            };
            if parent_item
                .occurs
                .as_ref()
                .and_then(|occurs| occurs.depending_on.as_ref())
                .is_some()
            {
                return true;
            }
            parent = parent_item
                .parent
                .as_ref()
                .map(|value| value.to_ascii_uppercase());
        }
        false
    }

    fn has_dynamic_occurs_in_subtree(&self, item: &DataItemIr) -> bool {
        fn has_odo(item: &DataItemIr) -> bool {
            item.occurs
                .as_ref()
                .and_then(|occurs| occurs.depending_on.as_ref())
                .is_some()
        }

        if has_odo(item) {
            return true;
        }
        let prefix = format!("{}.", item.qualified_name.to_ascii_uppercase());
        self.by_qualified
            .iter()
            .any(|(qualified, child)| qualified.starts_with(&prefix) && has_odo(child))
    }

    fn dynamic_occurs_items_in_subtree(&self, item: &DataItemIr) -> Vec<&'a DataItemIr> {
        fn has_odo(item: &DataItemIr) -> bool {
            item.occurs
                .as_ref()
                .and_then(|occurs| occurs.depending_on.as_ref())
                .is_some()
        }

        let item_key = item.qualified_name.to_ascii_uppercase();
        let prefix = format!("{}.", item_key);
        self.by_qualified
            .iter()
            .filter_map(|(qualified, child)| {
                if (qualified == &item_key || qualified.starts_with(&prefix)) && has_odo(child) {
                    Some(*child)
                } else {
                    None
                }
            })
            .collect()
    }

    fn has_redefines_context(&self, item: &DataItemIr) -> bool {
        if item.redefines.is_some() || self.is_redefined_base(item) {
            return true;
        }
        let mut parent = item.parent.as_ref().map(|value| value.to_ascii_uppercase());
        while let Some(parent_key) = parent {
            let Some(parent_item) = self.by_qualified.get(&parent_key).copied() else {
                return false;
            };
            if parent_item.redefines.is_some() || self.is_redefined_base(parent_item) {
                return true;
            }
            parent = parent_item
                .parent
                .as_ref()
                .map(|value| value.to_ascii_uppercase());
        }
        false
    }

    fn corresponding_descendants(&self, item: &DataItemIr) -> Vec<&'a DataItemIr> {
        let prefix = format!("{}.", item.qualified_name.to_ascii_uppercase());
        self.by_qualified
            .iter()
            .filter_map(|(qualified, child)| {
                if qualified.starts_with(&prefix)
                    && child.addressable
                    && child.value_category != ValueCategoryIr::Group
                {
                    Some(*child)
                } else {
                    None
                }
            })
            .collect()
    }

    fn is_redefined_base(&self, item: &DataItemIr) -> bool {
        self.redefined_bases
            .contains(&item.qualified_name.to_ascii_uppercase())
    }
}

fn data_references(statement: &StatementIr) -> Vec<(DataRefIr, ReferenceRoleIr)> {
    let mut references = Vec::new();
    match statement {
        StatementIr::Display(values) => {
            for value in values {
                references.extend(operand_references(value, ReferenceRoleIr::Display));
            }
        }
        StatementIr::Move { source, target } => {
            references.extend(operand_references(source, ReferenceRoleIr::Source));
            references.push((target.clone(), ReferenceRoleIr::Target));
        }
        StatementIr::MoveCorresponding { source, target } => {
            references.push((source.clone(), ReferenceRoleIr::Source));
            references.push((target.clone(), ReferenceRoleIr::Target));
        }
        StatementIr::Add { source, target }
        | StatementIr::Subtract { source, target }
        | StatementIr::Multiply { source, target }
        | StatementIr::Divide { source, target } => {
            references.extend(operand_references(
                source,
                ReferenceRoleIr::ArithmeticSource,
            ));
            references.push((target.clone(), ReferenceRoleIr::ArithmeticTarget));
        }
        StatementIr::Compute {
            target,
            expression,
            on_size_error_ops,
            not_on_size_error_ops,
            ..
        } => {
            references.extend(
                compute_expression_references(expression)
                    .into_iter()
                    .map(|reference| (reference, ReferenceRoleIr::ArithmeticSource)),
            );
            references.push((target.clone(), ReferenceRoleIr::ComputeTarget));
            for statement in on_size_error_ops.iter().chain(not_on_size_error_ops) {
                references.extend(data_references(statement));
            }
        }
        StatementIr::SetCondition { condition, .. } => {
            references.push((condition.clone(), ReferenceRoleIr::ConditionOperand));
        }
        StatementIr::SetIndex { operation, .. } => {
            references.extend(set_index_expr_references(operation));
        }
        StatementIr::Perform {
            varying_ir,
            times,
            until_tree,
            ..
        } => {
            if let Some(varying) = varying_ir {
                references.push((varying.target.clone(), ReferenceRoleIr::ArithmeticTarget));
                references.extend(operand_references(
                    &varying.from,
                    ReferenceRoleIr::ArithmeticSource,
                ));
                references.extend(operand_references(
                    &varying.by,
                    ReferenceRoleIr::ArithmeticSource,
                ));
            }
            if let Some(times) = times {
                references.extend(operand_references(times, ReferenceRoleIr::ArithmeticSource));
            }
            if let Some(until_tree) = until_tree {
                references.extend(condition_references(until_tree));
            }
        }
        StatementIr::If {
            then_statements,
            else_statements,
            ..
        } => {
            for statement in then_statements.iter().chain(else_statements) {
                references.extend(data_references(statement));
            }
        }
        StatementIr::Evaluate(evaluate) => {
            for subject in &evaluate.subjects {
                references.extend(evaluate_subject_references(subject));
            }
            for arm in &evaluate.arms {
                for pattern in &arm.patterns {
                    references.extend(evaluate_pattern_references(pattern));
                }
                for statement in &arm.statements {
                    references.extend(data_references(statement));
                }
            }
        }
        StatementIr::Search(search) => {
            for statement in &search.at_end {
                references.extend(data_references(statement));
            }
            for when in &search.whens {
                for statement in &when.statements {
                    references.extend(data_references(statement));
                }
            }
        }
        StatementIr::SearchAll(search) => {
            for statement in &search.at_end {
                references.extend(data_references(statement));
            }
            for statement in &search.statements {
                references.extend(data_references(statement));
            }
        }
        StatementIr::Call(call) => {
            if let CallTargetIr::Identifier(reference) = &call.target {
                references.push((reference.clone(), ReferenceRoleIr::ProcedureTarget));
            }
            references.extend(
                call.using
                    .iter()
                    .cloned()
                    .map(|reference| (reference, ReferenceRoleIr::ProcedureArgument)),
            );
        }
        StatementIr::Chain(chain) => {
            if let CallTargetIr::Identifier(reference) = &chain.target {
                references.push((reference.clone(), ReferenceRoleIr::ProcedureTarget));
            }
            references.extend(
                chain
                    .using
                    .iter()
                    .cloned()
                    .map(|reference| (reference, ReferenceRoleIr::ProcedureArgument)),
            );
        }
        StatementIr::Accept(accept) => {
            references.push((accept.target.clone(), ReferenceRoleIr::Target));
        }
        StatementIr::Initialize(initialize) => {
            references.extend(
                initialize
                    .targets
                    .iter()
                    .cloned()
                    .map(|target| (target, ReferenceRoleIr::Target)),
            );
        }
        StatementIr::Cancel(cancel) => {
            for target in &cancel.targets {
                if let CallTargetIr::Identifier(reference) = target {
                    references.push((reference.clone(), ReferenceRoleIr::ProcedureTarget));
                }
            }
        }
        StatementIr::Entry(entry) => {
            if let CallTargetIr::Identifier(reference) = &entry.name {
                references.push((reference.clone(), ReferenceRoleIr::ProcedureTarget));
            }
            references.extend(
                entry
                    .using
                    .iter()
                    .cloned()
                    .map(|reference| (reference, ReferenceRoleIr::ProcedureArgument)),
            );
        }
        StatementIr::ComputedGoTo { depending_on, .. } => {
            references.extend(operand_references(
                depending_on,
                ReferenceRoleIr::ArithmeticSource,
            ));
        }
        StatementIr::SortProcedure(sort) => {
            if let Some(key) = &sort.key {
                references.push((parse_data_ref(&key.name), ReferenceRoleIr::Source));
            }
        }
        StatementIr::ReleaseSortRecord(release) => {
            references.push((release.record.clone(), ReferenceRoleIr::Source));
            if let Some(source) = &release.from {
                references.push((source.clone(), ReferenceRoleIr::Source));
            }
        }
        StatementIr::ReturnSortRecord(ret) => {
            if let Some(target) = &ret.into {
                references.push((target.clone(), ReferenceRoleIr::Target));
            }
            for statement in ret.at_end_ops.iter().chain(&ret.not_at_end_ops) {
                references.extend(data_references(statement));
            }
        }
        StatementIr::OpenFile(_) => {}
        StatementIr::StartFile(start) => {
            if let Some(position) = &start.position {
                references.push((position.key.clone(), ReferenceRoleIr::Source));
            }
            for statement in start
                .invalid_key_ops
                .iter()
                .chain(&start.not_invalid_key_ops)
            {
                references.extend(data_references(statement));
            }
        }
        StatementIr::ReadFile(read) => {
            if let Some(target) = &read.into {
                references.push((target.clone(), ReferenceRoleIr::Target));
            }
            for statement in read
                .at_end_ops
                .iter()
                .chain(&read.not_at_end_ops)
                .chain(&read.on_exception_ops)
            {
                references.extend(data_references(statement));
            }
        }
        StatementIr::WriteFile(write) => {
            references.push((write.record.clone(), ReferenceRoleIr::Source));
            for statement in write
                .invalid_key_ops
                .iter()
                .chain(&write.not_invalid_key_ops)
                .chain(&write.on_exception_ops)
                .chain(&write.not_on_exception_ops)
            {
                references.extend(data_references(statement));
            }
        }
        StatementIr::RewriteFile(rewrite) => {
            references.push((rewrite.record.clone(), ReferenceRoleIr::Source));
            for statement in rewrite
                .invalid_key_ops
                .iter()
                .chain(&rewrite.not_invalid_key_ops)
            {
                references.extend(data_references(statement));
            }
        }
        StatementIr::DeleteFile(delete) => {
            for statement in delete
                .invalid_key_ops
                .iter()
                .chain(&delete.not_invalid_key_ops)
            {
                references.extend(data_references(statement));
            }
        }
        StatementIr::UnlockFile(_) => {}
        StatementIr::CloseFile(_) => {}
        StatementIr::InspectLike(inspect) => {
            references.push((inspect.subject.clone(), ReferenceRoleIr::Target));
            if let Some(tally) = &inspect.tally {
                references.push((tally.target.clone(), ReferenceRoleIr::ArithmeticTarget));
            }
        }
        StatementIr::StringOp(string) => {
            for piece in &string.pieces {
                references.extend(operand_references(&piece.source, ReferenceRoleIr::Source));
            }
            references.push((string.target.clone(), ReferenceRoleIr::Target));
            if let Some(pointer) = &string.pointer {
                references.push((pointer.clone(), ReferenceRoleIr::ArithmeticTarget));
            }
            for statement in string
                .on_overflow_ops
                .iter()
                .chain(&string.not_on_overflow_ops)
            {
                references.extend(data_references(statement));
            }
        }
        StatementIr::UnstringOp(unstring) => {
            references.extend(operand_references(
                &unstring.source,
                ReferenceRoleIr::Source,
            ));
            for target in &unstring.targets {
                references.push((target.target.clone(), ReferenceRoleIr::Target));
                if let Some(count) = &target.count {
                    references.push((count.clone(), ReferenceRoleIr::ArithmeticTarget));
                }
            }
            if let Some(pointer) = &unstring.pointer {
                references.push((pointer.clone(), ReferenceRoleIr::ArithmeticTarget));
            }
            if let Some(tallying) = &unstring.tallying {
                references.push((tallying.clone(), ReferenceRoleIr::ArithmeticTarget));
            }
            for statement in unstring
                .on_overflow_ops
                .iter()
                .chain(&unstring.not_on_overflow_ops)
            {
                references.extend(data_references(statement));
            }
        }
        StatementIr::GenerateReport(_)
        | StatementIr::InitiateReport(_)
        | StatementIr::TerminateReport(_)
        | StatementIr::SuppressReport(_)
        | StatementIr::PurgeQueue(_)
        | StatementIr::EnableCommunication(_)
        | StatementIr::DisableCommunication(_)
        | StatementIr::SendCommunication(_)
        | StatementIr::ReceiveCommunication(_)
        | StatementIr::EnterLanguage(_)
        | StatementIr::MergeFile(_)
        | StatementIr::Alter { .. }
        | StatementIr::GoTo(_)
        | StatementIr::BlockedNextSentence
        | StatementIr::ReadyTrace
        | StatementIr::ResetTrace
        | StatementIr::Continue
        | StatementIr::Goback
        | StatementIr::StopRun
        | StatementIr::Unsupported { .. } => {}
    }
    references
}

fn set_index_expr_references(operation: &SetIndexOperationIr) -> Vec<(DataRefIr, ReferenceRoleIr)> {
    let expr = match operation {
        SetIndexOperationIr::To(expr)
        | SetIndexOperationIr::UpBy(expr)
        | SetIndexOperationIr::DownBy(expr) => expr,
    };
    subscript_expr_references(expr)
        .into_iter()
        .map(|reference| (reference, ReferenceRoleIr::ArithmeticSource))
        .collect()
}

fn subscript_expr_references(expr: &SubscriptExprIr) -> Vec<DataRefIr> {
    match expr {
        SubscriptExprIr::Literal(_) => Vec::new(),
        SubscriptExprIr::DataRef(reference) => vec![reference.clone()],
        SubscriptExprIr::Add(left, right)
        | SubscriptExprIr::Subtract(left, right)
        | SubscriptExprIr::Multiply(left, right)
        | SubscriptExprIr::Divide(left, right) => {
            let mut refs = subscript_expr_references(left);
            refs.extend(subscript_expr_references(right));
            refs
        }
    }
}

fn compute_expression_references(expression: &str) -> Vec<DataRefIr> {
    let clean = strip_outer_parens(expression).trim();
    if clean.is_empty()
        || is_numeric_literal(clean)
        || ((clean.starts_with('"') && clean.ends_with('"'))
            || (clean.starts_with('\'') && clean.ends_with('\'')))
    {
        return Vec::new();
    }
    if let Some((left, _, right)) = split_subscript_binary(clean, &['+', '-']) {
        let mut refs = compute_expression_references(left);
        refs.extend(compute_expression_references(right));
        return refs;
    }
    if let Some((left, _, right)) = split_subscript_binary(clean, &['*', '/']) {
        let mut refs = compute_expression_references(left);
        refs.extend(compute_expression_references(right));
        return refs;
    }
    if let Some(function) = parse_function_operand(clean) {
        return function_references(&function)
            .into_iter()
            .map(|(reference, _)| reference)
            .collect();
    }
    vec![parse_data_ref(clean)]
}

fn evaluate_subject_references(subject: &EvaluateSubjectIr) -> Vec<(DataRefIr, ReferenceRoleIr)> {
    match subject {
        EvaluateSubjectIr::Operand(operand) => condition_operand_references(operand),
        EvaluateSubjectIr::Condition(condition) => condition_references(condition),
    }
}

fn evaluate_pattern_references(pattern: &EvaluatePatternIr) -> Vec<(DataRefIr, ReferenceRoleIr)> {
    match pattern {
        EvaluatePatternIr::Any => Vec::new(),
        EvaluatePatternIr::Operand(operand) => condition_operand_references(operand),
        EvaluatePatternIr::Range { start, end } => {
            let mut refs = condition_operand_references(start);
            refs.extend(condition_operand_references(end));
            refs
        }
        EvaluatePatternIr::Condition(condition) => condition_references(condition),
    }
}

fn condition_references(condition: &ConditionIr) -> Vec<(DataRefIr, ReferenceRoleIr)> {
    match condition {
        ConditionIr::Relation { left, right, .. } => {
            let mut refs = condition_operand_references(left);
            refs.extend(condition_operand_references(right));
            refs
        }
        ConditionIr::ClassTest { operand, .. } | ConditionIr::SignTest { operand, .. } => {
            condition_operand_references(operand)
        }
        ConditionIr::ConditionName { reference } => {
            vec![(reference.clone(), ReferenceRoleIr::ConditionOperand)]
        }
        ConditionIr::Not(inner) => condition_references(inner),
        ConditionIr::And(left, right) | ConditionIr::Or(left, right) => {
            let mut refs = condition_references(left);
            refs.extend(condition_references(right));
            refs
        }
    }
}

fn condition_operand_references(operand: &ConditionOperandIr) -> Vec<(DataRefIr, ReferenceRoleIr)> {
    match operand {
        ConditionOperandIr::Identifier(reference) => {
            vec![(reference.clone(), ReferenceRoleIr::ConditionOperand)]
        }
        ConditionOperandIr::Literal(_)
        | ConditionOperandIr::Number(_)
        | ConditionOperandIr::Figurative(_)
        | ConditionOperandIr::AllLiteral(_)
        | ConditionOperandIr::Bool(_) => Vec::new(),
        ConditionOperandIr::Function(function) => function_references(function),
    }
}

fn function_references(function: &FunctionOperandIr) -> Vec<(DataRefIr, ReferenceRoleIr)> {
    match function {
        FunctionOperandIr::Length(arg)
        | FunctionOperandIr::Ord(arg)
        | FunctionOperandIr::Numval(arg) => condition_operand_references(arg),
        FunctionOperandIr::UserDefined { args, .. } => {
            args.iter().flat_map(condition_operand_references).collect()
        }
    }
}

fn operand_references(
    operand: &OperandIr,
    role: ReferenceRoleIr,
) -> Vec<(DataRefIr, ReferenceRoleIr)> {
    match operand {
        OperandIr::Identifier(reference) => vec![(reference.clone(), role)],
        OperandIr::Literal(_) | OperandIr::Number(_) => Vec::new(),
        OperandIr::Function(function) => function_references(function)
            .into_iter()
            .map(|(reference, _)| (reference, role))
            .collect(),
    }
}

fn paragraph_index(paragraphs: &[ParagraphIr], target: &str) -> Option<usize> {
    let normalized_target = normalize_name(target);
    paragraphs
        .iter()
        .position(|paragraph| normalize_name(&paragraph.name) == normalized_target)
}

fn role_allows_subscripted_occurs(role: ReferenceRoleIr) -> bool {
    matches!(
        role,
        ReferenceRoleIr::Display
            | ReferenceRoleIr::Source
            | ReferenceRoleIr::Target
            | ReferenceRoleIr::ArithmeticSource
            | ReferenceRoleIr::ArithmeticTarget
            | ReferenceRoleIr::ComputeTarget
            | ReferenceRoleIr::ConditionOperand
            | ReferenceRoleIr::ProcedureArgument
    )
}

fn role_allows_subscripted_dynamic_occurs(role: ReferenceRoleIr) -> bool {
    matches!(
        role,
        ReferenceRoleIr::Display | ReferenceRoleIr::ConditionOperand
    )
}

fn role_supported_for_category(role: ReferenceRoleIr, category: ValueCategoryIr) -> bool {
    match role {
        ReferenceRoleIr::Display => matches!(
            category,
            ValueCategoryIr::Group
                | ValueCategoryIr::Alphanumeric
                | ValueCategoryIr::Alphabetic
                | ValueCategoryIr::NumericDisplay
                | ValueCategoryIr::PackedDecimal
                | ValueCategoryIr::Binary
                | ValueCategoryIr::NativeBinary
                | ValueCategoryIr::Float
        ),
        ReferenceRoleIr::Source => matches!(
            category,
            ValueCategoryIr::Group
                | ValueCategoryIr::Alphanumeric
                | ValueCategoryIr::Alphabetic
                | ValueCategoryIr::NumericDisplay
                | ValueCategoryIr::PackedDecimal
        ),
        ReferenceRoleIr::Target => matches!(
            category,
            ValueCategoryIr::Group
                | ValueCategoryIr::Alphanumeric
                | ValueCategoryIr::Alphabetic
                | ValueCategoryIr::NumericDisplay
                | ValueCategoryIr::PackedDecimal
        ),
        ReferenceRoleIr::ArithmeticSource | ReferenceRoleIr::ArithmeticTarget => {
            category_is_numeric(category)
        }
        ReferenceRoleIr::ComputeTarget => category_is_numeric(category),
        ReferenceRoleIr::ConditionOperand => true,
        ReferenceRoleIr::ProcedureTarget => matches!(
            category,
            ValueCategoryIr::Alphanumeric | ValueCategoryIr::Alphabetic
        ),
        ReferenceRoleIr::ProcedureArgument => !matches!(
            category,
            ValueCategoryIr::National
                | ValueCategoryIr::Dbcs
                | ValueCategoryIr::ConditionName
                | ValueCategoryIr::Unsupported
        ),
    }
}

fn validate_subscripts(
    reference: &DataRefIr,
    item: &DataItemIr,
    data_index: &DataReferenceIndex<'_>,
    indexes: &[IndexItemIr],
    paragraph: &ParagraphIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let occurs_depth = occurs_chain(item, data_index).len();
    if occurs_depth == 0 {
        diagnostics.push(Diagnostic::error(
            "E_INVALID_SUBSCRIPT",
            format!(
                "data reference {} has subscripts but target {} is not in OCCURS storage",
                reference.raw, item.qualified_name
            ),
            paragraph.span.clone(),
        ));
    }
    if reference.subscripts.len() != occurs_depth {
        diagnostics.push(Diagnostic::error(
            "E_INVALID_SUBSCRIPT",
            format!(
                "data reference {} has {} subscripts but target has OCCURS depth {}",
                reference.raw,
                reference.subscripts.len(),
                occurs_depth
            ),
            paragraph.span.clone(),
        ));
    }
    for (idx, subscript) in reference.subscripts.iter().enumerate() {
        let Some(occurs_item) = occurs_chain(item, data_index).get(idx).copied() else {
            continue;
        };
        match parse_subscript_literal(subscript) {
            SubscriptLiteral::Positive(value) => {
                let Some(occurs) = &occurs_item.occurs else {
                    continue;
                };
                if value > occurs.max {
                    diagnostics.push(Diagnostic::error(
                        "E_INVALID_SUBSCRIPT",
                        format!(
                            "data reference {} subscript {} is outside 1..={}",
                            reference.raw, value, occurs.max
                        ),
                        paragraph.span.clone(),
                    ));
                }
            }
            SubscriptLiteral::NonPositive => {
                diagnostics.push(Diagnostic::error(
                    "E_INVALID_SUBSCRIPT",
                    format!(
                        "data reference {} subscript {} must be a positive nonzero integer",
                        reference.raw, subscript
                    ),
                    paragraph.span.clone(),
                ));
            }
            SubscriptLiteral::Fractional => {
                diagnostics.push(Diagnostic::error(
                    "E_INVALID_SUBSCRIPT",
                    format!(
                        "data reference {} subscript {} must be an integer",
                        reference.raw, subscript
                    ),
                    paragraph.span.clone(),
                ));
            }
            SubscriptLiteral::NotLiteral => validate_subscript_expression(
                subscript,
                reference,
                data_index,
                indexes,
                paragraph,
                diagnostics,
            ),
        }
    }
}

fn validate_subscript_expression(
    subscript: &str,
    owner: &DataRefIr,
    data_index: &DataReferenceIndex<'_>,
    indexes: &[IndexItemIr],
    paragraph: &ParagraphIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let expr = parse_subscript_expr(subscript);
    if !expr.is_some_supported() {
        diagnostics.push(Diagnostic::error(
            "E_UNSUPPORTED_SUBSCRIPT",
            format!(
                "data reference {} uses unsupported subscript expression {}; only literals, data items, and simple +, -, *, / integer expressions are enabled",
                owner.raw, subscript
            ),
            paragraph.span.clone(),
        ));
        return;
    }
    validate_subscript_expr_numeric(&expr, owner, data_index, indexes, paragraph, diagnostics);
}

fn validate_subscript_expr_numeric(
    expr: &SubscriptExprIr,
    owner: &DataRefIr,
    data_index: &DataReferenceIndex<'_>,
    indexes: &[IndexItemIr],
    paragraph: &ParagraphIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match expr {
        SubscriptExprIr::Literal(value) if is_numeric_literal(value) => {
            if value.parse::<isize>().map(|value| value < 1).unwrap_or(false) {
                diagnostics.push(Diagnostic::error(
                    "E_INVALID_SUBSCRIPT",
                    format!(
                        "data reference {} subscript {} must be a positive nonzero integer",
                        owner.raw, value
                    ),
                    paragraph.span.clone(),
                ));
            }
        }
        SubscriptExprIr::Literal(value) => diagnostics.push(Diagnostic::error(
            "E_INVALID_SUBSCRIPT",
            format!(
                "data reference {} subscript {} is not a numeric subscript expression",
                owner.raw, value
            ),
            paragraph.span.clone(),
        )),
        SubscriptExprIr::DataRef(reference) if resolve_index_name(indexes, reference).is_some() => {
        }
        SubscriptExprIr::DataRef(reference) => match data_index.resolve_ref(reference) {
            DataResolution::Resolved(item) if category_is_numeric(item.value_category) => {
                validate_subscript_data_ref_storage_context(
                    reference,
                    item,
                    data_index,
                    indexes,
                    paragraph,
                    diagnostics,
                );
            }
            DataResolution::Resolved(item) => diagnostics.push(Diagnostic::error(
                "E_INVALID_SUBSCRIPT",
                format!(
                    "data reference {} subscript {} resolves to {}, which is {:?} and not numeric",
                    owner.raw, reference.raw, item.qualified_name, item.value_category
                ),
                paragraph.span.clone(),
            )),
            DataResolution::Condition(condition) => diagnostics.push(Diagnostic::error(
                "E_INVALID_SUBSCRIPT",
                format!(
                    "data reference {} subscript {} resolves to condition-name {}.{}",
                    owner.raw, reference.raw, condition.parent, condition.name
                ),
                paragraph.span.clone(),
            )),
            DataResolution::Special { category, .. } if category_is_numeric(category) => {}
            DataResolution::Special { name, category, .. } => diagnostics.push(Diagnostic::error(
                "E_INVALID_SUBSCRIPT",
                format!(
                    "data reference {} subscript {} resolves to special register {}, which is {:?} and not numeric",
                    owner.raw, reference.raw, name, category
                ),
                paragraph.span.clone(),
            )),
            DataResolution::Missing => diagnostics.push(Diagnostic::error(
                "E_UNRESOLVED_DATA",
                format!(
                    "data reference {} subscript {} does not resolve to a numeric Data Division item",
                    owner.raw, reference.raw
                ),
                paragraph.span.clone(),
            )),
            DataResolution::Ambiguous(candidates) => diagnostics.push(Diagnostic::error(
                "E_AMBIGUOUS_DATA",
                format!(
                    "data reference {} subscript {} is ambiguous; candidates: {}",
                    owner.raw,
                    reference.raw,
                    candidates.join(", ")
                ),
                paragraph.span.clone(),
            )),
        },
        SubscriptExprIr::Add(left, right)
        | SubscriptExprIr::Subtract(left, right)
        | SubscriptExprIr::Multiply(left, right)
        | SubscriptExprIr::Divide(left, right) => {
            validate_subscript_expr_numeric(left, owner, data_index, indexes, paragraph, diagnostics);
            validate_subscript_expr_numeric(right, owner, data_index, indexes, paragraph, diagnostics);
        }
    }
}

fn validate_subscript_data_ref_storage_context(
    reference: &DataRefIr,
    item: &DataItemIr,
    data_index: &DataReferenceIndex<'_>,
    indexes: &[IndexItemIr],
    paragraph: &ParagraphIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if reference.is_subscripted() {
        validate_subscripts(reference, item, data_index, indexes, paragraph, diagnostics);
    } else {
        let occurs_depth = occurs_chain(item, data_index).len();
        if occurs_depth > 0 {
            diagnostics.push(Diagnostic::error(
                "E_MISSING_SUBSCRIPT",
                format!(
                    "subscript expression {} resolves to {} and requires {} subscript(s)",
                    reference.raw, item.qualified_name, occurs_depth
                ),
                paragraph.span.clone(),
            ));
        }
    }
    if data_index.has_dynamic_occurs_context(item) {
        diagnostics.push(Diagnostic::error(
            "E_CODEGEN_ODO_REFERENCE",
            format!(
                "subscript expression {} uses field {} inside OCCURS DEPENDING ON storage; ODO subscript sources are not executable yet",
                reference.raw, item.qualified_name
            ),
            paragraph.span.clone(),
        ));
    }
    if data_index.has_redefines_context(item) {
        diagnostics.push(Diagnostic::error(
            "E_CODEGEN_REDEFINES_REFERENCE",
            format!(
                "subscript expression {} uses field {} participating in REDEFINES storage; active-view subscript sources are not executable yet",
                reference.raw, item.qualified_name
            ),
            paragraph.span.clone(),
        ));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubscriptLiteral {
    Positive(usize),
    NonPositive,
    Fractional,
    NotLiteral,
}

fn parse_subscript_literal(value: &str) -> SubscriptLiteral {
    let clean = value.trim().trim_end_matches('.');
    if clean.is_empty() {
        return SubscriptLiteral::NotLiteral;
    }
    if is_numeric_literal(clean) && clean.contains('.') {
        return SubscriptLiteral::Fractional;
    }
    match clean.parse::<isize>() {
        Ok(value) if value > 0 => SubscriptLiteral::Positive(value as usize),
        Ok(_) => SubscriptLiteral::NonPositive,
        Err(_) => SubscriptLiteral::NotLiteral,
    }
}

fn resolve_index_name<'a>(
    indexes: &'a [IndexItemIr],
    reference: &DataRefIr,
) -> Option<&'a IndexItemIr> {
    if reference.is_subscripted()
        || reference.has_reference_modifier()
        || reference.parts.len() != 1
    {
        return None;
    }
    let name = reference.normalized.to_ascii_uppercase();
    indexes
        .iter()
        .find(|index| index.name.eq_ignore_ascii_case(&name))
}

fn validate_reference_modifier(
    reference: &DataRefIr,
    item: &DataItemIr,
    data_index: &DataReferenceIndex<'_>,
    paragraph: &ParagraphIr,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let Some(modifier) = &reference.reference_modifier else {
        return true;
    };
    let mut valid = true;
    if !reference_modifiable_item(item, data_index) {
        diagnostics.push(Diagnostic::error(
            "E_INVALID_REFERENCE_MODIFICATION",
            format!(
                "data reference {} uses reference modification on {}, which is {:?} and not character-addressable",
                reference.raw, item.qualified_name, item.value_category
            ),
            paragraph.span.clone(),
        ));
        valid = false;
    }

    let start = parse_positive_integer_text(&modifier.start);
    match start {
        IntegerText::Positive(_) => {}
        IntegerText::NonPositive(_) => {
            diagnostics.push(Diagnostic::error(
                "E_INVALID_REFERENCE_MODIFICATION",
                format!(
                    "data reference {} reference-modifier start {} must be positive",
                    reference.raw, modifier.start
                ),
                paragraph.span.clone(),
            ));
            valid = false;
        }
        IntegerText::NotLiteral => {}
    }

    let length = modifier
        .length
        .as_ref()
        .map(|length| (length, parse_positive_integer_text(length)));
    if let Some((length_text, parsed)) = &length {
        match parsed {
            IntegerText::Positive(_) => {}
            IntegerText::NonPositive(_) => {
                diagnostics.push(Diagnostic::error(
                    "E_INVALID_REFERENCE_MODIFICATION",
                    format!(
                        "data reference {} reference-modifier length {} must be positive",
                        reference.raw, length_text
                    ),
                    paragraph.span.clone(),
                ));
                valid = false;
            }
            IntegerText::NotLiteral => {}
        }
    }

    if let (IntegerText::Positive(start), Some(IntegerText::Positive(length)), Some(byte_len)) = (
        start,
        length.as_ref().map(|(_, parsed)| *parsed),
        item.byte_len,
    ) {
        if start.saturating_add(length).saturating_sub(1) > byte_len {
            diagnostics.push(Diagnostic::error(
                "E_INVALID_REFERENCE_MODIFICATION",
                format!(
                    "data reference {} reference-modifier range is outside {} byte(s)",
                    reference.raw, byte_len
                ),
                paragraph.span.clone(),
            ));
            valid = false;
        }
    }
    if let (IntegerText::Positive(start), None, Some(byte_len)) =
        (start, length.as_ref(), item.byte_len)
    {
        if start > byte_len {
            diagnostics.push(Diagnostic::error(
                "E_INVALID_REFERENCE_MODIFICATION",
                format!(
                    "data reference {} reference-modifier start is outside {} byte(s)",
                    reference.raw, byte_len
                ),
                paragraph.span.clone(),
            ));
            valid = false;
        }
    }

    if item.value_category == ValueCategoryIr::Group
        && data_index.has_dynamic_occurs_in_subtree(item)
    {
        diagnostics.push(Diagnostic::error(
            "E_CODEGEN_REFERENCE_MODIFICATION",
            format!(
                "data reference {} reference-modifies a group with OCCURS DEPENDING ON storage; dynamic group slicing remains fail-closed",
                reference.raw
            ),
            paragraph.span.clone(),
        ));
        valid = false;
    }

    valid
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IntegerText {
    Positive(usize),
    NonPositive(isize),
    NotLiteral,
}

fn parse_positive_integer_text(value: &str) -> IntegerText {
    let clean = value.trim();
    let Ok(parsed) = clean.parse::<isize>() else {
        return IntegerText::NotLiteral;
    };
    if parsed < 1 {
        IntegerText::NonPositive(parsed)
    } else {
        IntegerText::Positive(parsed as usize)
    }
}

fn reference_modifiable_item(item: &DataItemIr, data_index: &DataReferenceIndex<'_>) -> bool {
    match item.value_category {
        ValueCategoryIr::Group => !data_index.has_dynamic_occurs_in_subtree(item),
        ValueCategoryIr::Alphanumeric
        | ValueCategoryIr::Alphabetic
        | ValueCategoryIr::National
        | ValueCategoryIr::Dbcs
        | ValueCategoryIr::NumericDisplay
        | ValueCategoryIr::NumericEdited => true,
        ValueCategoryIr::PackedDecimal
        | ValueCategoryIr::Binary
        | ValueCategoryIr::NativeBinary
        | ValueCategoryIr::Float
        | ValueCategoryIr::ConditionName
        | ValueCategoryIr::Unsupported => false,
    }
}

fn occurs_chain<'a>(
    item: &'a DataItemIr,
    data_index: &'a DataReferenceIndex<'a>,
) -> Vec<&'a DataItemIr> {
    let mut chain = Vec::new();
    if item.occurs.is_some() {
        chain.push(item);
    }
    let mut parent = item.parent.as_ref().map(|value| value.to_ascii_uppercase());
    while let Some(parent_key) = parent {
        let Some(parent_item) = data_index.by_qualified.get(&parent_key).copied() else {
            break;
        };
        if parent_item.occurs.is_some() {
            chain.push(parent_item);
        }
        parent = parent_item
            .parent
            .as_ref()
            .map(|value| value.to_ascii_uppercase());
    }
    chain.reverse();
    chain
}

trait SubscriptExprSupport {
    fn is_some_supported(&self) -> bool;
}

impl SubscriptExprSupport for SubscriptExprIr {
    fn is_some_supported(&self) -> bool {
        match self {
            SubscriptExprIr::Literal(value) => {
                is_numeric_literal(value)
                    || value
                        .chars()
                        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
            }
            SubscriptExprIr::DataRef(reference) => !reference.normalized.is_empty(),
            SubscriptExprIr::Add(left, right)
            | SubscriptExprIr::Subtract(left, right)
            | SubscriptExprIr::Multiply(left, right)
            | SubscriptExprIr::Divide(left, right) => {
                left.is_some_supported() && right.is_some_supported()
            }
        }
    }
}

fn resolved_category(
    data_index: &DataReferenceIndex<'_>,
    reference: &DataRefIr,
) -> Option<ValueCategoryIr> {
    match data_index.resolve_ref(reference) {
        DataResolution::Resolved(item) => Some(item.value_category),
        DataResolution::Condition(_) => Some(ValueCategoryIr::ConditionName),
        DataResolution::Special { category, .. } => Some(category),
        DataResolution::Missing | DataResolution::Ambiguous(_) => None,
    }
}

fn operand_category(
    data_index: &DataReferenceIndex<'_>,
    operand: &OperandIr,
) -> Option<ValueCategoryIr> {
    match operand {
        OperandIr::Identifier(reference) => resolved_category(data_index, reference),
        OperandIr::Literal(_) => Some(ValueCategoryIr::Alphanumeric),
        OperandIr::Number(_) => Some(ValueCategoryIr::NumericDisplay),
        OperandIr::Function(FunctionOperandIr::Length(_))
        | OperandIr::Function(FunctionOperandIr::Ord(_))
        | OperandIr::Function(FunctionOperandIr::Numval(_)) => {
            Some(ValueCategoryIr::NumericDisplay)
        }
        OperandIr::Function(FunctionOperandIr::UserDefined { .. }) => None,
    }
}

fn category_is_numeric(category: ValueCategoryIr) -> bool {
    matches!(
        category,
        ValueCategoryIr::NumericDisplay
            | ValueCategoryIr::PackedDecimal
            | ValueCategoryIr::Binary
            | ValueCategoryIr::NativeBinary
            | ValueCategoryIr::Float
    )
}

fn call_using_requires_conversion(actual: ValueCategoryIr, formal: ValueCategoryIr) -> bool {
    if actual == formal {
        return false;
    }
    !matches!(
        (actual, formal),
        (ValueCategoryIr::Alphabetic, ValueCategoryIr::Alphanumeric)
            | (ValueCategoryIr::Alphanumeric, ValueCategoryIr::Alphabetic)
    )
}

fn call_using_conversion_supported(actual: ValueCategoryIr, formal: ValueCategoryIr) -> bool {
    let actual_scalar = matches!(
        actual,
        ValueCategoryIr::Alphanumeric
            | ValueCategoryIr::Alphabetic
            | ValueCategoryIr::NumericEdited
            | ValueCategoryIr::NumericDisplay
            | ValueCategoryIr::PackedDecimal
            | ValueCategoryIr::Binary
            | ValueCategoryIr::NativeBinary
            | ValueCategoryIr::Float
    );
    // Only display-like LINKAGE formals are currently executable as implicit
    // call-site conversions. Binary, native-binary, packed, and float formals
    // need representation-aware conversion metadata at the IR/VM boundary.
    let formal_supported = matches!(
        formal,
        ValueCategoryIr::Alphanumeric
            | ValueCategoryIr::Alphabetic
            | ValueCategoryIr::NumericEdited
            | ValueCategoryIr::NumericDisplay
    );
    actual_scalar && formal_supported
}

fn move_compatible(source: Option<ValueCategoryIr>, target: ValueCategoryIr) -> bool {
    let Some(source) = source else {
        return true;
    };
    match target {
        ValueCategoryIr::Group | ValueCategoryIr::Alphanumeric | ValueCategoryIr::Alphabetic => {
            true
        }
        ValueCategoryIr::NumericDisplay
        | ValueCategoryIr::PackedDecimal
        | ValueCategoryIr::Binary
        | ValueCategoryIr::NativeBinary => category_is_numeric(source),
        ValueCategoryIr::Float => category_is_numeric(source),
        ValueCategoryIr::NumericEdited
        | ValueCategoryIr::National
        | ValueCategoryIr::Dbcs
        | ValueCategoryIr::ConditionName
        | ValueCategoryIr::Unsupported => false,
    }
}

fn parse_condition(raw: &str) -> Result<ConditionIr, String> {
    let tokens = tokenize_condition(raw);
    if tokens.is_empty() {
        return Err("empty condition".to_string());
    }
    let mut parser = ConditionParser {
        tokens,
        pos: 0,
        last_subject: None,
        last_rel_op: None,
        allow_bare_abbrev: true,
    };
    let condition = parser.parse_or()?;
    if parser.peek().is_some() {
        return Err(format!(
            "unexpected token {}",
            parser.peek().unwrap_or_default()
        ));
    }
    Ok(condition)
}

fn tokenize_condition(raw: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut idx = 0usize;
    while idx < raw.len() {
        let Some(ch) = raw[idx..].chars().next() else {
            break;
        };
        if ch.is_whitespace() || ch == ',' {
            idx += ch.len_utf8();
            continue;
        }
        if ch == '"' || ch == '\'' {
            let end = cobol_text::quoted_literal_end(raw, idx).unwrap_or(idx + ch.len_utf8());
            tokens.push(raw[idx..end].to_string());
            idx = end;
            continue;
        }
        if matches!(ch, '(' | ')') {
            tokens.push(ch.to_string());
            idx += ch.len_utf8();
            continue;
        }
        if matches!(ch, '=' | '>' | '<' | '!') {
            let mut token = ch.to_string();
            let next_idx = idx + ch.len_utf8();
            if let Some(next) = raw[next_idx..].chars().next() {
                if matches!(
                    (ch, next),
                    ('>', '=') | ('<', '=') | ('<', '>') | ('!', '=')
                ) {
                    token.push(next);
                    idx = next_idx + next.len_utf8();
                } else {
                    idx = next_idx;
                }
            } else {
                idx = next_idx;
            }
            tokens.push(token);
            continue;
        }
        let mut token = String::new();
        while idx < raw.len() {
            let Some(next) = raw[idx..].chars().next() else {
                break;
            };
            if next.is_whitespace()
                || next == ','
                || matches!(next, '=' | '>' | '<' | '!')
                || matches!(next, '(' | ')')
                || matches!(next, '"' | '\'')
            {
                break;
            }
            token.push(next);
            idx += next.len_utf8();
        }
        if raw[idx..].starts_with('(') && !token.is_empty() {
            let mut depth = 0usize;
            while idx < raw.len() {
                let Some(next) = raw[idx..].chars().next() else {
                    break;
                };
                if next == '"' || next == '\'' {
                    let end =
                        cobol_text::quoted_literal_end(raw, idx).unwrap_or(idx + next.len_utf8());
                    token.push_str(&raw[idx..end]);
                    idx = end;
                    continue;
                }
                token.push(next);
                if next == '(' {
                    depth += 1;
                } else if next == ')' {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        idx += next.len_utf8();
                        break;
                    }
                }
                idx += next.len_utf8();
            }
        }
        if !token.is_empty() {
            tokens.push(token);
        } else {
            idx += ch.len_utf8();
        }
    }
    tokens
}

struct ConditionParser {
    tokens: Vec<String>,
    pos: usize,
    last_subject: Option<ConditionOperandIr>,
    last_rel_op: Option<RelOpIr>,
    allow_bare_abbrev: bool,
}

impl ConditionParser {
    fn parse_or(&mut self) -> Result<ConditionIr, String> {
        let mut left = self.parse_and()?;
        while self.eat_word("OR") {
            let right = self.parse_and()?;
            left = ConditionIr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<ConditionIr, String> {
        let mut left = self.parse_not()?;
        while self.eat_word("AND") {
            let right = self.parse_not()?;
            left = ConditionIr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<ConditionIr, String> {
        if self.eat_word("NOT") {
            let previous = self.allow_bare_abbrev;
            if !self.starts_relation_operator() {
                self.allow_bare_abbrev = false;
            }
            let condition = self.parse_not();
            self.allow_bare_abbrev = previous;
            Ok(ConditionIr::Not(Box::new(condition?)))
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Result<ConditionIr, String> {
        if self.eat("(") {
            let condition = self.parse_or()?;
            self.expect(")")?;
            return Ok(condition);
        }
        self.parse_relation_or_condition_name()
    }

    fn parse_relation_or_condition_name(&mut self) -> Result<ConditionIr, String> {
        if self.starts_relation_operator() {
            let Some(left) = self.last_subject.clone() else {
                return Err("abbreviated relation has no previous subject".to_string());
            };
            let op = self.parse_rel_op()?;
            let right = self.parse_operand_until_boundary()?;
            self.last_subject = Some(left.clone());
            self.last_rel_op = Some(op);
            return Ok(ConditionIr::Relation { left, op, right });
        }

        let left = self.parse_operand_until_operator()?;
        if self.eat_word("IS") {
            let negated = self.eat_word("NOT");
            if let Some(class) = self.parse_class_test() {
                return Ok(ConditionIr::ClassTest {
                    operand: left,
                    class,
                    negated,
                });
            }
            if let Some(sign) = self.parse_sign_test() {
                return Ok(ConditionIr::SignTest {
                    operand: left,
                    sign,
                    negated,
                });
            }
            let op = self.parse_rel_op()?;
            let op = if negated { invert_rel_op(op) } else { op };
            let right = self.parse_operand_until_boundary()?;
            self.last_subject = Some(left.clone());
            self.last_rel_op = Some(op);
            return Ok(ConditionIr::Relation { left, op, right });
        }

        if self.starts_relation_operator() {
            let op = self.parse_rel_op()?;
            let right = self.parse_operand_until_boundary()?;
            self.last_subject = Some(left.clone());
            self.last_rel_op = Some(op);
            return Ok(ConditionIr::Relation { left, op, right });
        }

        if self.allow_bare_abbrev {
            if let (Some(subject), Some(op)) = (self.last_subject.clone(), self.last_rel_op) {
                return Ok(ConditionIr::Relation {
                    left: subject,
                    op,
                    right: left,
                });
            }
        }

        match left {
            ConditionOperandIr::Identifier(reference) => {
                Ok(ConditionIr::ConditionName { reference })
            }
            other => Err(format!(
                "condition operand {:?} is not a relation or condition-name",
                other
            )),
        }
    }

    fn parse_operand_until_operator(&mut self) -> Result<ConditionOperandIr, String> {
        let start = self.pos;
        let mut function_depth = 0usize;
        while let Some(token) = self.peek() {
            if token == "("
                && (function_depth > 0 || self.tokens[start].eq_ignore_ascii_case("FUNCTION"))
            {
                function_depth += 1;
                self.pos += 1;
                continue;
            }
            if token == ")" && function_depth > 0 {
                function_depth = function_depth.saturating_sub(1);
                self.pos += 1;
                continue;
            }
            if function_depth == 0
                && (self.is_boundary(token)
                    || token.eq_ignore_ascii_case("IS")
                    || is_rel_token(token))
            {
                break;
            }
            if function_depth == 0
                && token.eq_ignore_ascii_case("NOT")
                && self
                    .tokens
                    .get(self.pos + 1)
                    .map(|next| is_rel_token(next))
                    .unwrap_or(false)
            {
                break;
            }
            if function_depth == 0
                && (token.eq_ignore_ascii_case("GREATER")
                    || token.eq_ignore_ascii_case("LESS")
                    || token.eq_ignore_ascii_case("EQUAL"))
            {
                break;
            }
            self.pos += 1;
        }
        self.operand_from_range(start, self.pos)
    }

    fn parse_operand_until_boundary(&mut self) -> Result<ConditionOperandIr, String> {
        let start = self.pos;
        let mut function_depth = 0usize;
        while let Some(token) = self.peek() {
            if token == "("
                && (function_depth > 0 || self.tokens[start].eq_ignore_ascii_case("FUNCTION"))
            {
                function_depth += 1;
                self.pos += 1;
                continue;
            }
            if token == ")" && function_depth > 0 {
                function_depth = function_depth.saturating_sub(1);
                self.pos += 1;
                continue;
            }
            if function_depth == 0 && self.is_boundary(token) {
                break;
            }
            self.pos += 1;
        }
        self.operand_from_range(start, self.pos)
    }

    fn operand_from_range(&self, start: usize, end: usize) -> Result<ConditionOperandIr, String> {
        if start >= end {
            return Err("missing condition operand".to_string());
        }
        let raw = self.tokens[start..end].join(" ");
        Ok(parse_condition_operand(&raw))
    }

    fn parse_rel_op(&mut self) -> Result<RelOpIr, String> {
        let negated = self.eat_word("NOT");
        let Some(token) = self.next() else {
            return Err("missing relational operator".to_string());
        };
        let mut op = match token.to_ascii_uppercase().as_str() {
            "=" => RelOpIr::Equal,
            "<>" | "!=" => RelOpIr::NotEqual,
            ">" => RelOpIr::Greater,
            ">=" => RelOpIr::GreaterOrEqual,
            "<" => RelOpIr::Less,
            "<=" => RelOpIr::LessOrEqual,
            "GREATER" => {
                let _ = self.eat_word("THAN");
                if self.eat_word("OR") {
                    self.expect_word("EQUAL")?;
                    let _ = self.eat_word("TO");
                    RelOpIr::GreaterOrEqual
                } else {
                    RelOpIr::Greater
                }
            }
            "LESS" => {
                let _ = self.eat_word("THAN");
                if self.eat_word("OR") {
                    self.expect_word("EQUAL")?;
                    let _ = self.eat_word("TO");
                    RelOpIr::LessOrEqual
                } else {
                    RelOpIr::Less
                }
            }
            "EQUAL" => {
                let _ = self.eat_word("TO");
                RelOpIr::Equal
            }
            other => return Err(format!("unsupported relational operator {other}")),
        };
        if negated {
            op = invert_rel_op(op);
        }
        Ok(op)
    }

    fn parse_class_test(&mut self) -> Option<ClassTestIr> {
        let token = self.peek()?.to_ascii_uppercase();
        let class = match token.as_str() {
            "NUMERIC" => ClassTestIr::Numeric,
            "ALPHABETIC" => ClassTestIr::Alphabetic,
            "ALPHABETIC-UPPER" => ClassTestIr::AlphabeticUpper,
            "ALPHABETIC-LOWER" => ClassTestIr::AlphabeticLower,
            _ => return None,
        };
        self.pos += 1;
        Some(class)
    }

    fn parse_sign_test(&mut self) -> Option<SignTestIr> {
        let token = self.peek()?.to_ascii_uppercase();
        let sign = match token.as_str() {
            "POSITIVE" => SignTestIr::Positive,
            "NEGATIVE" => SignTestIr::Negative,
            "ZERO" => SignTestIr::Zero,
            _ => return None,
        };
        self.pos += 1;
        Some(sign)
    }

    fn starts_relation_operator(&self) -> bool {
        self.peek()
            .map(|token| {
                is_rel_token(token)
                    || token.eq_ignore_ascii_case("NOT")
                        && self
                            .tokens
                            .get(self.pos + 1)
                            .map(|next| is_rel_token(next))
                            .unwrap_or(false)
            })
            .unwrap_or(false)
    }

    fn is_boundary(&self, token: &str) -> bool {
        token == ")" || token.eq_ignore_ascii_case("AND") || token.eq_ignore_ascii_case("OR")
    }

    fn eat(&mut self, expected: &str) -> bool {
        if self.peek() == Some(expected) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: &str) -> Result<(), String> {
        if self.eat(expected) {
            Ok(())
        } else {
            Err(format!("expected {expected}"))
        }
    }

    fn eat_word(&mut self, expected: &str) -> bool {
        if self
            .peek()
            .map(|token| token.eq_ignore_ascii_case(expected))
            .unwrap_or(false)
        {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect_word(&mut self, expected: &str) -> Result<(), String> {
        if self.eat_word(expected) {
            Ok(())
        } else {
            Err(format!("expected {expected}"))
        }
    }

    fn next(&mut self) -> Option<String> {
        let token = self.tokens.get(self.pos).cloned()?;
        self.pos += 1;
        Some(token)
    }

    fn peek(&self) -> Option<&str> {
        self.tokens.get(self.pos).map(String::as_str)
    }
}

fn is_rel_token(token: &str) -> bool {
    matches!(
        token.to_ascii_uppercase().as_str(),
        "=" | "<>" | "!=" | ">" | ">=" | "<" | "<=" | "GREATER" | "LESS" | "EQUAL"
    )
}

fn invert_rel_op(op: RelOpIr) -> RelOpIr {
    match op {
        RelOpIr::Equal => RelOpIr::NotEqual,
        RelOpIr::NotEqual => RelOpIr::Equal,
        RelOpIr::Greater => RelOpIr::LessOrEqual,
        RelOpIr::GreaterOrEqual => RelOpIr::Less,
        RelOpIr::Less => RelOpIr::GreaterOrEqual,
        RelOpIr::LessOrEqual => RelOpIr::Greater,
    }
}

fn parse_condition_operand(raw: &str) -> ConditionOperandIr {
    let clean = raw.trim().trim_end_matches('.');
    if let Some(function) = parse_function_operand(clean) {
        return ConditionOperandIr::Function(function);
    }
    if clean.eq_ignore_ascii_case("TRUE") {
        return ConditionOperandIr::Bool(true);
    }
    if clean.eq_ignore_ascii_case("FALSE") {
        return ConditionOperandIr::Bool(false);
    }
    if let Some(rest) = clean
        .strip_prefix("ALL ")
        .or_else(|| clean.strip_prefix("all "))
    {
        let value = rest.trim().trim_matches('"').trim_matches('\'').to_string();
        return ConditionOperandIr::AllLiteral(value);
    }
    if (clean.starts_with('"') && clean.ends_with('"'))
        || (clean.starts_with('\'') && clean.ends_with('\''))
    {
        return ConditionOperandIr::Literal(clean.trim_matches('"').trim_matches('\'').to_string());
    }
    if is_numeric_literal(clean) {
        return ConditionOperandIr::Number(clean.to_string());
    }
    match clean.to_ascii_uppercase().as_str() {
        "ZERO" | "ZEROES" | "ZEROS" => ConditionOperandIr::Figurative(FigurativeConstantIr::Zero),
        "SPACE" | "SPACES" => ConditionOperandIr::Figurative(FigurativeConstantIr::Space),
        "HIGH-VALUE" | "HIGH-VALUES" => {
            ConditionOperandIr::Figurative(FigurativeConstantIr::HighValue)
        }
        "LOW-VALUE" | "LOW-VALUES" => {
            ConditionOperandIr::Figurative(FigurativeConstantIr::LowValue)
        }
        "QUOTE" | "QUOTES" => ConditionOperandIr::Figurative(FigurativeConstantIr::Quote),
        _ => ConditionOperandIr::Identifier(parse_data_ref(clean)),
    }
}

fn parse_function_operand(clean: &str) -> Option<FunctionOperandIr> {
    let upper = clean.to_ascii_uppercase();
    if !upper.starts_with("FUNCTION") {
        return None;
    }
    let rest = &clean["FUNCTION".len()..];
    if !rest.is_empty()
        && !rest
            .chars()
            .next()
            .map(char::is_whitespace)
            .unwrap_or(false)
    {
        return None;
    }
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }
    let (name, tail) = split_function_name_tail(rest)?;
    let name = name.to_ascii_uppercase();
    let arg_text = if tail.starts_with('(') {
        let Some(arg_text) = parenthesized_function_arg_text(tail, 0) else {
            return Some(FunctionOperandIr::UserDefined {
                name,
                args: Vec::new(),
                raw: clean.to_string(),
            });
        };
        arg_text.trim()
    } else {
        tail
    };
    let arg_text = arg_text
        .strip_prefix("OF ")
        .or_else(|| arg_text.strip_prefix("of "))
        .unwrap_or(arg_text)
        .trim();
    let arg_text = arg_text.trim();
    let args = if arg_text.is_empty() {
        Vec::new()
    } else {
        split_function_args(arg_text)
            .into_iter()
            .map(|arg| parse_condition_operand(&arg))
            .collect::<Vec<_>>()
    };
    Some(match name.as_str() {
        "LENGTH" if args.len() == 1 && single_function_arg_text_is_supported(arg_text) => {
            FunctionOperandIr::Length(Box::new(args[0].clone()))
        }
        "ORD" if args.len() == 1 && single_function_arg_text_is_supported(arg_text) => {
            FunctionOperandIr::Ord(Box::new(args[0].clone()))
        }
        "NUMVAL" if args.len() == 1 && single_function_arg_text_is_supported(arg_text) => {
            FunctionOperandIr::Numval(Box::new(args[0].clone()))
        }
        _ => FunctionOperandIr::UserDefined {
            name,
            args,
            raw: clean.to_string(),
        },
    })
}

fn split_function_name_tail(rest: &str) -> Option<(&str, &str)> {
    let rest = rest.trim();
    if rest.is_empty() {
        return None;
    }
    let name_end = rest
        .char_indices()
        .find_map(|(idx, ch)| (ch.is_whitespace() || ch == '(').then_some(idx))
        .unwrap_or(rest.len());
    if name_end == 0 {
        return None;
    }
    Some((&rest[..name_end], rest[name_end..].trim()))
}

fn parenthesized_function_arg_text(text: &str, open_idx: usize) -> Option<&str> {
    if text[open_idx..].chars().next()? != '(' {
        return None;
    }
    let mut depth = 0usize;
    for item in cobol_text::literal_aware_char_indices(text) {
        if item.byte_idx < open_idx || item.inside_literal {
            continue;
        }
        match item.ch {
            '(' => depth = depth.saturating_add(1),
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    let close_end = item.byte_idx;
                    let trailing_idx = item.byte_idx + item.ch.len_utf8();
                    if text[trailing_idx..].trim().is_empty() {
                        return Some(&text[open_idx + 1..close_end]);
                    }
                    return None;
                }
            }
            _ => {}
        }
    }
    None
}

fn split_function_args(text: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    for item in cobol_text::literal_aware_char_indices(text) {
        match item.ch {
            '(' if !item.inside_literal => {
                depth += 1;
                current.push(item.ch);
            }
            ')' if !item.inside_literal => {
                depth = depth.saturating_sub(1);
                current.push(item.ch);
            }
            ',' if depth == 0 && !item.inside_literal => {
                if !current.trim().is_empty() {
                    args.push(current.trim().to_string());
                }
                current.clear();
            }
            _ => current.push(item.ch),
        }
    }
    if !current.trim().is_empty() {
        args.push(current.trim().to_string());
    }
    args
}

fn parse_operand(value: &str) -> OperandIr {
    let clean = value.trim().trim_end_matches('.');
    if let Some(function) = parse_function_operand(clean) {
        return OperandIr::Function(function);
    }
    if (clean.starts_with('"') && clean.ends_with('"'))
        || (clean.starts_with('\'') && clean.ends_with('\''))
    {
        OperandIr::Literal(unquote_cobol_literal(clean))
    } else if is_numeric_literal(clean) {
        OperandIr::Number(clean.to_string())
    } else {
        OperandIr::Identifier(parse_data_ref(clean))
    }
}

fn unquote_cobol_literal(value: &str) -> String {
    let clean = value.trim();
    let Some(quote) = clean.chars().next().filter(|ch| *ch == '"' || *ch == '\'') else {
        return clean.to_string();
    };
    if !clean.ends_with(quote) || clean.len() < 2 {
        return clean.to_string();
    }
    let inner = &clean[quote.len_utf8()..clean.len() - quote.len_utf8()];
    inner.replace(&format!("{quote}{quote}"), &quote.to_string())
}

fn parse_call_target(value: &str) -> CallTargetIr {
    let clean = value.trim().trim_end_matches('.');
    if (clean.starts_with('"') && clean.ends_with('"'))
        || (clean.starts_with('\'') && clean.ends_with('\''))
    {
        CallTargetIr::Literal(clean.trim_matches('"').trim_matches('\'').to_string())
    } else {
        CallTargetIr::Identifier(parse_data_ref(clean))
    }
}

pub fn parse_data_ref(value: &str) -> DataRefIr {
    let raw = value.trim().trim_end_matches('.').to_string();
    let mut base = raw.clone();
    let mut subscript_groups = Vec::new();
    let mut reference_modifier = None;

    while let Some(open) = base.find('(') {
        if let Some(close) = base[open + 1..].find(')').map(|idx| idx + open + 1) {
            let inner = base[open + 1..close].trim().to_string();
            if let Some((start, length)) = inner.split_once(':') {
                reference_modifier = Some(ReferenceModifierIr {
                    start: start.trim().to_string(),
                    length: {
                        let length = length.trim();
                        if length.is_empty() {
                            None
                        } else {
                            Some(length.to_string())
                        }
                    },
                });
            } else if !inner.is_empty() {
                subscript_groups.push(
                    inner
                        .split(',')
                        .map(|part| part.trim().to_string())
                        .collect::<Vec<_>>(),
                );
            }
            base.replace_range(open..=close, "");
        } else {
            break;
        }
    }

    let normalized = normalize_reference(&base);
    let qualified = base
        .split_whitespace()
        .any(|word| word.eq_ignore_ascii_case("OF") || word.eq_ignore_ascii_case("IN"));
    let subscripts = if qualified {
        subscript_groups
            .into_iter()
            .rev()
            .flatten()
            .collect::<Vec<_>>()
    } else {
        subscript_groups.into_iter().flatten().collect::<Vec<_>>()
    };
    let parts = normalized
        .split('.')
        .filter(|part| !part.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    DataRefIr {
        raw,
        normalized,
        parts,
        subscripts,
        reference_modifier,
    }
}

fn parse_subscript_expr(value: &str) -> SubscriptExprIr {
    let clean = value.trim().trim_end_matches('.');
    if let Some((left, op, right)) = split_subscript_binary(clean, &['+', '-']) {
        let left = Box::new(parse_subscript_expr(left));
        let right = Box::new(parse_subscript_expr(right));
        return match op {
            '+' => SubscriptExprIr::Add(left, right),
            '-' => SubscriptExprIr::Subtract(left, right),
            _ => unreachable!(),
        };
    }
    if let Some((left, op, right)) = split_subscript_binary(clean, &['*', '/']) {
        let left = Box::new(parse_subscript_expr(left));
        let right = Box::new(parse_subscript_expr(right));
        return match op {
            '*' => SubscriptExprIr::Multiply(left, right),
            '/' => SubscriptExprIr::Divide(left, right),
            _ => unreachable!(),
        };
    }
    let clean = strip_outer_parens(clean).trim();
    if is_numeric_literal(clean) {
        SubscriptExprIr::Literal(clean.to_string())
    } else {
        SubscriptExprIr::DataRef(parse_data_ref(clean))
    }
}

fn parse_perform_varying_clause(raw: &str) -> Option<PerformVaryingIr> {
    let words = split_clause_tokens(raw);
    if words.iter().any(|word| word.eq_ignore_ascii_case("AFTER")) {
        return None;
    }
    let from_idx = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("FROM"))?;
    let by_idx = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("BY"))?;
    if from_idx == 0 || by_idx <= from_idx + 1 || by_idx + 1 >= words.len() {
        return None;
    }
    let by_end = words[by_idx + 1..]
        .iter()
        .position(|word| word.eq_ignore_ascii_case("AFTER"))
        .map(|offset| by_idx + 1 + offset)
        .unwrap_or(words.len());
    if by_idx + 1 >= by_end {
        return None;
    }

    let target = words[..from_idx].join(" ");
    let from = words[from_idx + 1..by_idx].join(" ");
    let by = words[by_idx + 1..by_end].join(" ");
    if target.trim().is_empty() || from.trim().is_empty() || by.trim().is_empty() {
        return None;
    }

    Some(PerformVaryingIr {
        target: parse_data_ref(&target),
        from: parse_operand(&from),
        by: parse_operand(&by),
    })
}

fn split_subscript_binary<'a>(value: &'a str, ops: &[char]) -> Option<(&'a str, char, &'a str)> {
    let value = strip_outer_parens(value).trim();
    let mut depth = 0usize;
    let chars = cobol_text::literal_aware_char_indices(value).collect::<Vec<_>>();
    for item in chars.iter().rev() {
        let idx = item.byte_idx;
        let ch = item.ch;
        match ch {
            ')' if !item.inside_literal => depth = depth.saturating_add(1),
            '(' if !item.inside_literal => depth = depth.saturating_sub(1),
            _ => {}
        }
        if item.inside_literal || depth != 0 || !ops.contains(&ch) || idx == 0 {
            continue;
        }
        if ch == '-' && !operator_has_space_around(value, idx) {
            continue;
        }
        let left = value[..idx].trim();
        let right = value[idx + ch.len_utf8()..].trim();
        if !left.is_empty() && !right.is_empty() {
            return Some((left, ch, right));
        }
    }
    None
}

fn operator_has_space_around(value: &str, idx: usize) -> bool {
    let before = value[..idx].chars().next_back();
    let after = value[idx + 1..].chars().next();
    before.map(char::is_whitespace).unwrap_or(false)
        || after.map(char::is_whitespace).unwrap_or(false)
}

fn strip_outer_parens(value: &str) -> &str {
    let trimmed = value.trim();
    if !(trimmed.starts_with('(') && trimmed.ends_with(')')) {
        return trimmed;
    }
    let mut depth = 0usize;
    for (idx, ch) in trimmed.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && idx != trimmed.len() - 1 {
                    return trimmed;
                }
            }
            _ => {}
        }
    }
    &trimmed[1..trimmed.len() - 1]
}

fn parse_picture(raw: &str) -> PicIr {
    let compact = raw
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase();
    let signed = compact.starts_with('S') || compact.contains('S');
    let mut before_v = true;
    let mut digits = 0usize;
    let mut scale = 0usize;
    let mut char_len = 0usize;
    let mut has_x = false;
    let mut has_a = false;
    let mut has_numeric = false;
    let chars = compact.chars().collect::<Vec<_>>();
    let mut idx = 0usize;
    while idx < chars.len() {
        let ch = chars[idx];
        match ch {
            'V' => {
                before_v = false;
                idx += 1;
            }
            'X' | 'A' | '9' => {
                let repeat = parse_repeat(&chars, idx + 1).unwrap_or(1);
                match ch {
                    'X' => {
                        has_x = true;
                        char_len = char_len.saturating_add(repeat);
                    }
                    'A' => {
                        has_a = true;
                        char_len = char_len.saturating_add(repeat);
                    }
                    '9' => {
                        has_numeric = true;
                        digits = digits.saturating_add(repeat);
                        if !before_v {
                            scale = scale.saturating_add(repeat);
                        }
                        char_len = char_len.saturating_add(repeat);
                    }
                    _ => {}
                }
                idx = skip_repeat(&chars, idx + 1);
            }
            'S' | '(' | ')' => idx += 1,
            _ => {
                if matches!(ch, 'Z' | '*' | '+' | '-' | ',' | '.') {
                    has_numeric = true;
                    char_len = char_len.saturating_add(1);
                }
                idx += 1;
            }
        }
    }

    let category = if has_x {
        PicCategoryIr::Alphanumeric
    } else if has_a && !has_numeric {
        PicCategoryIr::Alphabetic
    } else if has_numeric
        && compact
            .chars()
            .all(|ch| matches!(ch, 'S' | '9' | 'V' | '(' | ')' | '0'..='9'))
    {
        PicCategoryIr::NumericDisplay
    } else if has_numeric {
        PicCategoryIr::NumericEdited
    } else {
        PicCategoryIr::Unknown
    };

    PicIr {
        raw: raw.to_string(),
        category,
        signed,
        digits,
        scale,
        char_len,
    }
}

fn elementary_byte_len(item: &DataItemIr) -> usize {
    record_byte_len(
        &record_usage(&item.usage),
        item.picture_ast.as_ref().map(record_picture).as_ref(),
    )
}

fn build_record_plan(
    record_length: usize,
    storage_items: &[StorageItemIr],
    redefines: &[RedefinesIr],
    renames: &[RenamesIr],
    conditions: &[ConditionNameIr],
    platform_profile: PlatformProfile,
    diagnostics: &mut Vec<Diagnostic>,
) -> RecordPlan {
    let rename_aliases = renames
        .iter()
        .map(|rename| rename.renaming_item.as_str())
        .collect::<BTreeSet<_>>();
    let fields = storage_items
        .iter()
        .filter(|item| !rename_aliases.contains(item.qualified_name.as_str()))
        .map(|item| {
            let usage = record_usage(&item.usage);
            let picture = item.picture.as_ref().map(record_picture);
            let expected_len = record_byte_len(&usage, picture.as_ref());
            if expected_len != item.byte_len
                && !matches!(usage, RecordUsage::Group)
                && item.occurs.is_none()
            {
                diagnostics.push(Diagnostic::error(
                    "E_LAYOUT_MISMATCH",
                    format!(
                        "converter layout for {} computed {} bytes but shared record engine computed {} bytes",
                        item.qualified_name, item.byte_len, expected_len
                    ),
                    item.span.clone(),
                ));
            }
            RecordField {
                layout_id: item.layout_id.clone(),
                name: item.name.clone(),
                qualified_name: item.qualified_name.clone(),
                path: item.path.clone(),
                offset: item.offset,
                byte_len: item.byte_len,
                usage,
                picture,
                occurs: item.occurs.as_ref().map(record_occurs),
                redefines: item.redefines.clone(),
                parent: item.parent.clone(),
                addressable: item.addressable,
                sync: item.sync,
                value: item.value.clone(),
                source: source_ref(&item.span),
            }
        })
        .collect::<Vec<_>>();

    let ranges = fields
        .iter()
        .filter(|field| {
            !record_field_in_redefines_view(field, &fields)
                && !matches!(field.usage, RecordUsage::Group)
        })
        .map(|field| CoverageRange {
            kind: if field.occurs.is_some() {
                CoverageKind::Occurs
            } else if field.addressable {
                CoverageKind::Field
            } else {
                CoverageKind::Filler
            },
            name: field.qualified_name.clone(),
            offset: field.offset,
            length: field.byte_len,
        })
        .collect::<Vec<_>>();

    RecordPlan {
        layout_mode: LayoutMode::Sequential,
        platform_profile,
        record_length,
        fields,
        redefines: redefines
            .iter()
            .map(|redefines| RecordRedefines {
                redefining_item: redefines.redefining_item.clone(),
                base_item: redefines.base_item.clone(),
                offset: redefines.offset,
                byte_len: redefines.byte_len,
                base_byte_len: redefines.base_byte_len,
            })
            .collect(),
        condition_names: conditions
            .iter()
            .map(|condition| RecordConditionName {
                name: condition.name.clone(),
                rust_name: condition.rust_name.clone(),
                parent: condition.parent.clone(),
                values: condition.values.clone(),
                value_set: condition
                    .value_set
                    .iter()
                    .map(|value| match value {
                        ConditionValueIr::Single(value) => {
                            RecordConditionValue::Single(value.clone())
                        }
                        ConditionValueIr::Range { start, end } => RecordConditionValue::Range {
                            start: start.clone(),
                            end: end.clone(),
                        },
                    })
                    .collect(),
                source: source_ref(&condition.span),
            })
            .collect(),
        coverage: coverage_summary(record_length, &ranges),
    }
}

fn record_field_in_redefines_view(field: &RecordField, fields: &[RecordField]) -> bool {
    if field.redefines.is_some() {
        return true;
    }
    let mut parent = field.parent.as_deref();
    while let Some(parent_name) = parent {
        let Some(parent_field) = fields
            .iter()
            .find(|candidate| candidate.qualified_name == parent_name)
        else {
            return false;
        };
        if parent_field.redefines.is_some() {
            return true;
        }
        parent = parent_field.parent.as_deref();
    }
    false
}

fn record_usage(usage: &UsageIr) -> RecordUsage {
    match usage {
        UsageIr::Display => RecordUsage::Display,
        UsageIr::PackedDecimal => RecordUsage::PackedDecimal,
        UsageIr::Binary => RecordUsage::Binary,
        UsageIr::NativeBinary => RecordUsage::NativeBinary,
        UsageIr::Float32 => RecordUsage::IbmFloat32,
        UsageIr::Float64 => RecordUsage::IbmFloat64,
        UsageIr::National => RecordUsage::Unknown("NATIONAL".to_string()),
        UsageIr::Dbcs => RecordUsage::Unknown("DBCS".to_string()),
        UsageIr::Alphanumeric => RecordUsage::Alphanumeric,
        UsageIr::Group => RecordUsage::Group,
        UsageIr::Unknown(value) => RecordUsage::Unknown(value.clone()),
    }
}

fn record_picture(pic: &PicIr) -> RecordPicture {
    RecordPicture {
        raw: pic.raw.clone(),
        category: match pic.category {
            PicCategoryIr::Alphanumeric => PicCategory::Alphanumeric,
            PicCategoryIr::Alphabetic => PicCategory::Alphabetic,
            PicCategoryIr::NumericDisplay => PicCategory::NumericDisplay,
            PicCategoryIr::NumericEdited => PicCategory::NumericEdited,
            PicCategoryIr::Unknown => PicCategory::Unknown,
        },
        signed: pic.signed,
        digits: pic.digits,
        scale: pic.scale,
        char_len: pic.char_len,
    }
}

fn record_occurs(occurs: &OccursIr) -> RecordOccurs {
    RecordOccurs {
        min: occurs.min,
        max: occurs.max,
        depending_on: occurs.depending_on.clone(),
    }
}

fn source_ref(span: &cobol_ir::SourceSpan) -> SourceRef {
    SourceRef {
        file: span.file.clone(),
        line: span.line,
        column: span.column,
    }
}

fn bump_ancestors(planned: &mut [PlannedData], mut parent_idx: Option<usize>, child_end: usize) {
    while let Some(idx) = parent_idx {
        let storage_area = planned[idx].item.storage_area;
        let start = planned[idx].item.offset.unwrap_or(child_end);
        let len = child_end.saturating_sub(start);
        if planned[idx].item.byte_len.unwrap_or(0) < len {
            planned[idx].item.byte_len = Some(len);
        }
        parent_idx = planned[idx].item.parent.as_ref().and_then(|parent| {
            planned.iter().position(|item| {
                item.item.qualified_name == *parent && item.item.storage_area == storage_area
            })
        });
    }
}

fn bump_redefines_ancestors(
    planned: &mut [PlannedData],
    mut parent_idx: Option<usize>,
    child_end: usize,
) {
    while let Some(idx) = parent_idx {
        let storage_area = planned[idx].item.storage_area;
        let start = planned[idx].item.offset.unwrap_or(child_end);
        let len = child_end.saturating_sub(start);
        if planned[idx].item.byte_len.unwrap_or(0) < len {
            planned[idx].item.byte_len = Some(len);
        }
        if planned[idx].item.redefines.is_some() {
            break;
        }
        parent_idx = planned[idx].item.parent.as_ref().and_then(|parent| {
            planned.iter().position(|item| {
                item.item.qualified_name == *parent && item.item.storage_area == storage_area
            })
        });
    }
}

fn planned_item_in_redefines_tree(planned: &[PlannedData], idx: usize) -> bool {
    if planned[idx].item.redefines.is_some() {
        return true;
    }
    let storage_area = planned[idx].item.storage_area;
    let mut parent = planned[idx].item.parent.as_ref();
    while let Some(parent_name) = parent {
        let Some(parent_idx) = planned.iter().position(|item| {
            item.item.qualified_name == *parent_name && item.item.storage_area == storage_area
        }) else {
            return false;
        };
        if planned[parent_idx].item.redefines.is_some() {
            return true;
        }
        parent = planned[parent_idx].item.parent.as_ref();
    }
    false
}

fn finalize_group_cursor(
    planned: &mut [PlannedData],
    cursors: &mut HashMap<(u8, Option<String>), usize>,
    idx: usize,
) -> usize {
    let in_redefines_tree = planned_item_in_redefines_tree(planned, idx);
    let redefines = planned[idx].item.redefines.is_some();
    if redefines {
        return 0;
    }
    let Some(offset) = planned[idx].item.offset else {
        return 0;
    };
    let Some(mut byte_len) = planned[idx].item.byte_len else {
        return 0;
    };
    if matches!(planned[idx].item.usage, UsageIr::Group) {
        let occurs_multiplier = planned[idx]
            .item
            .occurs
            .as_ref()
            .map(|occurs| occurs.max)
            .unwrap_or(1);
        byte_len = byte_len.saturating_mul(occurs_multiplier);
        planned[idx].item.byte_len = Some(byte_len);
    }
    let end = offset.saturating_add(byte_len);
    let parent = planned[idx].item.parent.clone();
    let area_key = storage_area_cursor_code(planned[idx].item.storage_area);
    let cursor = cursors.entry((area_key, parent.clone())).or_insert(0);
    if *cursor < end {
        *cursor = end;
    }
    if let Some(parent_name) = &parent {
        if let Some(parent_idx) = planned.iter().position(|planned_item| {
            planned_item.item.qualified_name == *parent_name
                && planned_item.item.storage_area == planned[idx].item.storage_area
        }) {
            let parent_start = planned[parent_idx].item.offset.unwrap_or(end);
            let parent_len = end.saturating_sub(parent_start);
            if planned[parent_idx].item.byte_len.unwrap_or(0) < parent_len {
                planned[parent_idx].item.byte_len = Some(parent_len);
            }
        }
    }
    if in_redefines_tree {
        0
    } else {
        end
    }
}

fn parse_repeat(chars: &[char], start: usize) -> Option<usize> {
    if chars.get(start) != Some(&'(') {
        return None;
    }
    let mut idx = start + 1;
    let mut digits = String::new();
    while let Some(ch) = chars.get(idx) {
        if *ch == ')' {
            return digits.parse::<usize>().ok();
        }
        if !ch.is_ascii_digit() {
            return None;
        }
        digits.push(*ch);
        idx += 1;
    }
    None
}

fn skip_repeat(chars: &[char], start: usize) -> usize {
    if chars.get(start) != Some(&'(') {
        return start;
    }
    let mut idx = start + 1;
    while idx < chars.len() {
        if chars[idx] == ')' {
            return idx + 1;
        }
        idx += 1;
    }
    start
}

fn is_numeric_literal(value: &str) -> bool {
    let mut seen_digit = false;
    let mut seen_decimal = false;
    for (idx, ch) in value.chars().enumerate() {
        if ch.is_ascii_digit() {
            seen_digit = true;
        } else if ch == '.' && !seen_decimal {
            seen_decimal = true;
        } else if (ch == '-' || ch == '+') && idx == 0 {
        } else {
            return false;
        }
    }
    seen_digit
}

fn extract_picture(clauses: &str) -> Option<String> {
    let parts: Vec<&str> = clauses.split_whitespace().collect();
    for idx in 0..parts.len() {
        if parts[idx].eq_ignore_ascii_case("PIC") || parts[idx].eq_ignore_ascii_case("PICTURE") {
            return parts
                .get(idx + 1)
                .map(|value| value.trim_end_matches('.').to_string());
        }
    }
    None
}

fn extract_picture_from_clause_ast(clauses: &[DataClauseAst]) -> Option<String> {
    clauses.iter().find_map(|clause| match clause {
        DataClauseAst::Picture(value) => Some(value.clone()),
        _ => None,
    })
}

fn extract_usage_from_clause_ast(clauses: &[DataClauseAst]) -> Option<UsageIr> {
    clauses.iter().find_map(|clause| {
        let DataClauseAst::Usage(value) = clause else {
            return None;
        };
        Some(match value.as_str() {
            "COMP-3" | "PACKED-DECIMAL" => UsageIr::PackedDecimal,
            "COMP-5" => UsageIr::NativeBinary,
            "COMP-1" => UsageIr::Float32,
            "COMP-2" => UsageIr::Float64,
            "COMP" | "COMP-4" | "BINARY" => UsageIr::Binary,
            "NATIONAL" | "DISPLAY-1" => UsageIr::National,
            "DBCS" | "KANJI" => UsageIr::Dbcs,
            "DISPLAY" => UsageIr::Display,
            "ALPHANUMERIC" => UsageIr::Alphanumeric,
            other => UsageIr::Unknown(other.to_string()),
        })
    })
}

fn extract_occurs(clauses: &str) -> Option<OccursIr> {
    let parts: Vec<&str> = clauses.split_whitespace().collect();
    let occurs_idx = parts
        .iter()
        .position(|part| part.eq_ignore_ascii_case("OCCURS"))?;
    let min = parts
        .get(occurs_idx + 1)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1);
    let mut max = min;
    if parts
        .get(occurs_idx + 2)
        .map(|value| value.eq_ignore_ascii_case("TO"))
        .unwrap_or(false)
    {
        max = parts
            .get(occurs_idx + 3)
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(min);
    }
    let depending_on = parts
        .iter()
        .position(|part| part.eq_ignore_ascii_case("DEPENDING"))
        .and_then(|idx| {
            if parts
                .get(idx + 1)
                .map(|value| value.eq_ignore_ascii_case("ON"))
                .unwrap_or(false)
            {
                parts.get(idx + 2)
            } else {
                None
            }
        })
        .map(|value| normalize_name(value));
    let keys = extract_occurs_keys(&split_clause_tokens(clauses));
    let indexed_by = parts
        .iter()
        .position(|part| part.eq_ignore_ascii_case("INDEXED"))
        .and_then(|idx| {
            if parts
                .get(idx + 1)
                .map(|value| value.eq_ignore_ascii_case("BY"))
                .unwrap_or(false)
            {
                Some(
                    parts
                        .iter()
                        .skip(idx + 2)
                        .take_while(|value| {
                            !matches!(
                                value.to_ascii_uppercase().as_str(),
                                "PIC" | "PICTURE" | "VALUE" | "VALUES" | "USAGE" | "REDEFINES"
                            )
                        })
                        .map(|value| normalize_name(value))
                        .collect::<Vec<_>>(),
                )
            } else {
                None
            }
        })
        .unwrap_or_default();
    Some(OccursIr {
        min,
        max,
        depending_on,
        indexed_by,
        keys,
    })
}

fn extract_occurs_keys(tokens: &[String]) -> Vec<OccursKeyIr> {
    let mut keys = Vec::new();
    let mut idx = 0usize;
    while idx < tokens.len() {
        let direction = match tokens[idx].to_ascii_uppercase().as_str() {
            "ASCENDING" => Some(OccursKeyDirectionIr::Ascending),
            "DESCENDING" => Some(OccursKeyDirectionIr::Descending),
            _ => None,
        };
        let Some(direction) = direction else {
            idx += 1;
            continue;
        };
        let mut cursor = idx + 1;
        if tokens
            .get(cursor)
            .map(|token| token.eq_ignore_ascii_case("KEY"))
            .unwrap_or(false)
        {
            cursor += 1;
        }
        if tokens
            .get(cursor)
            .map(|token| token.eq_ignore_ascii_case("IS") || token.eq_ignore_ascii_case("ARE"))
            .unwrap_or(false)
        {
            cursor += 1;
        }
        while cursor < tokens.len() {
            let upper = tokens[cursor]
                .trim_end_matches('.')
                .trim_end_matches(',')
                .to_ascii_uppercase();
            if matches!(
                upper.as_str(),
                "ASCENDING"
                    | "DESCENDING"
                    | "INDEXED"
                    | "DEPENDING"
                    | "PIC"
                    | "PICTURE"
                    | "VALUE"
                    | "VALUES"
                    | "USAGE"
                    | "REDEFINES"
                    | "OCCURS"
            ) {
                break;
            }
            if !matches!(upper.as_str(), "KEY" | "IS" | "ARE" | "BY" | "ON" | "TIMES") {
                keys.push(OccursKeyIr {
                    direction,
                    name: normalize_reference(&tokens[cursor]),
                });
            }
            cursor += 1;
        }
        idx = cursor.max(idx + 1);
    }
    keys
}

fn extract_occurs_from_clause_ast(clauses: &[DataClauseAst]) -> Option<OccursIr> {
    clauses.iter().find_map(|clause| match clause {
        DataClauseAst::Occurs {
            min,
            max,
            depending_on,
            indexed_by,
            keys,
        } => Some(OccursIr {
            min: *min,
            max: *max,
            depending_on: depending_on.clone(),
            indexed_by: indexed_by.clone(),
            keys: keys
                .iter()
                .map(|key| OccursKeyIr {
                    direction: match key.direction {
                        DataOccursKeyDirectionAst::Ascending => OccursKeyDirectionIr::Ascending,
                        DataOccursKeyDirectionAst::Descending => OccursKeyDirectionIr::Descending,
                    },
                    name: normalize_reference(&key.name),
                })
                .collect(),
        }),
        _ => None,
    })
}

fn extract_after_keyword<'a>(clauses: &'a str, keyword: &str) -> Option<&'a str> {
    let parts: Vec<&str> = clauses.split_whitespace().collect();
    parts
        .iter()
        .position(|part| part.eq_ignore_ascii_case(keyword))
        .and_then(|idx| parts.get(idx + 1).copied())
        .map(|value| value.trim_end_matches('.'))
}

fn extract_value(clauses: &str) -> Option<String> {
    let tokens = split_clause_tokens(clauses);
    let value_idx = tokens.iter().position(|part| {
        part.eq_ignore_ascii_case("VALUE") || part.eq_ignore_ascii_case("VALUES")
    })?;
    let value = tokens.get(value_idx + 1)?;
    if value.eq_ignore_ascii_case("ALL") {
        let repeated = tokens.get(value_idx + 2)?;
        return Some(format!("ALL {}", normalize_value_literal(repeated)));
    }
    Some(normalize_value_literal(value))
}

fn extract_initial_value_from_clause_ast(clauses: &[DataClauseAst]) -> Option<String> {
    clauses.iter().find_map(|clause| match clause {
        DataClauseAst::Value(value) => Some(normalize_value_literal(value)),
        DataClauseAst::Values(_) => None,
        _ => None,
    })
}

fn extract_condition_value_set(clauses: &str) -> Vec<ConditionValueIr> {
    let tokens = split_clause_tokens(clauses);
    let Some(value_idx) = tokens
        .iter()
        .position(|part| part.eq_ignore_ascii_case("VALUE") || part.eq_ignore_ascii_case("VALUES"))
    else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let mut idx = value_idx + 1;
    while idx < tokens.len() {
        let token = tokens[idx].as_str();
        if token.eq_ignore_ascii_case("THRU") || token.eq_ignore_ascii_case("THROUGH") {
            idx += 1;
            continue;
        }

        let (start, next_idx) = if token.eq_ignore_ascii_case("ALL") {
            let Some(repeated) = tokens.get(idx + 1) else {
                idx += 1;
                continue;
            };
            (
                format!("ALL {}", normalize_value_literal(repeated)),
                idx + 2,
            )
        } else {
            (normalize_value_literal(token), idx + 1)
        };
        if start.is_empty() {
            idx = next_idx;
            continue;
        }

        if tokens
            .get(next_idx)
            .map(|next| next.eq_ignore_ascii_case("THRU") || next.eq_ignore_ascii_case("THROUGH"))
            .unwrap_or(false)
        {
            if let Some(end) = tokens.get(next_idx + 1) {
                out.push(ConditionValueIr::Range {
                    start,
                    end: normalize_value_literal(end),
                });
                idx = next_idx + 2;
                continue;
            }
        }

        out.push(ConditionValueIr::Single(start));
        idx = next_idx;
    }
    out
}

fn extract_condition_value_set_from_clause_ast(
    clauses: &[DataClauseAst],
) -> Option<Vec<ConditionValueIr>> {
    clauses.iter().find_map(|clause| match clause {
        DataClauseAst::Value(value) => Some(vec![ConditionValueIr::Single(
            normalize_value_literal(value),
        )]),
        DataClauseAst::Values(values) => Some(
            values
                .iter()
                .map(|value| match value {
                    DataValueAst::Single(value) => {
                        ConditionValueIr::Single(normalize_value_literal(value))
                    }
                    DataValueAst::Range { start, end } => ConditionValueIr::Range {
                        start: normalize_value_literal(start),
                        end: normalize_value_literal(end),
                    },
                })
                .collect(),
        ),
        _ => None,
    })
}

fn split_clause_tokens(clauses: &str) -> Vec<String> {
    cobol_text::split_cobol_words(clauses)
}

fn normalize_value_literal(value: &str) -> String {
    let clean = value
        .trim()
        .trim_end_matches('.')
        .trim_end_matches(',')
        .trim_matches('"')
        .trim_matches('\'');
    match clean.to_ascii_uppercase().as_str() {
        "SPACE" | "SPACES" => " ".to_string(),
        "ZERO" | "ZEROES" | "ZEROS" => "0".to_string(),
        "QUOTE" | "QUOTES" => "\"".to_string(),
        "HIGH-VALUE" | "HIGH-VALUES" => "\u{FF}".to_string(),
        "LOW-VALUE" | "LOW-VALUES" => "\0".to_string(),
        _ => clean.to_string(),
    }
}

fn normalize_name(name: &str) -> String {
    name.trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_end_matches('.')
        .replace('-', "_")
        .to_ascii_uppercase()
}

fn normalize_data_key(value: &str) -> String {
    normalize_name(value)
}

fn normalize_reference(name: &str) -> String {
    let words = name
        .split_whitespace()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if words
        .iter()
        .any(|word| word.eq_ignore_ascii_case("OF") || word.eq_ignore_ascii_case("IN"))
    {
        let mut parts = Vec::new();
        let mut current = Vec::new();
        for word in words {
            if word.eq_ignore_ascii_case("OF") || word.eq_ignore_ascii_case("IN") {
                if !current.is_empty() {
                    parts.push(normalize_name(&current.join(" ")));
                    current.clear();
                }
            } else {
                current.push(word);
            }
        }
        if !current.is_empty() {
            parts.push(normalize_name(&current.join(" ")));
        }
        parts.reverse();
        parts.join(".")
    } else if name.contains('.') {
        name.split('.')
            .map(normalize_name)
            .collect::<Vec<_>>()
            .join(".")
    } else {
        normalize_name(name)
    }
}

pub fn rust_ident(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }
    while out.contains("__") {
        out = out.replace("__", "_");
    }
    let out = out.trim_matches('_').to_string();
    let mut out = if out.is_empty() {
        "item".to_string()
    } else {
        out
    };
    if out
        .chars()
        .next()
        .map(|ch| ch.is_ascii_digit())
        .unwrap_or(false)
    {
        out.insert_str(0, "n_");
    }
    match out.as_str() {
        "type" | "match" | "move" | "loop" | "fn" | "struct" | "enum" | "crate" | "self" => {
            format!("r#{out}")
        }
        _ => out,
    }
}

fn dedupe_diagnostics(diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for diagnostic in diagnostics {
        let key = format!(
            "{}:{:?}:{}:{}:{}",
            diagnostic.code,
            diagnostic.severity,
            diagnostic.span.file,
            diagnostic.span.line,
            diagnostic.message
        );
        if seen.insert(key) {
            out.push(diagnostic);
        }
    }
    out.sort_by(|left, right| {
        let severity_rank = |severity| match severity {
            Severity::Error => 0,
            Severity::Warning => 1,
            Severity::Info => 2,
        };
        severity_rank(left.severity)
            .cmp(&severity_rank(right.severity))
            .then(left.span.file.cmp(&right.span.file))
            .then(left.span.line.cmp(&right.span.line))
            .then(left.span.column.cmp(&right.span.column))
            .then(left.code.cmp(&right.code))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use cobol_syntax::parse_program;

    fn analyze_src(source_name: &str, src: &str) -> ProgramIr {
        let ast = parse_program(source_name, src).expect("parse");
        analyze(ast, Dialect::Ibm)
    }

    fn analyze_src_with_dialect(source_name: &str, src: &str, dialect: Dialect) -> ProgramIr {
        let ast = parse_program(source_name, src).expect("parse");
        analyze(ast, dialect)
    }

    #[test]
    fn goback_lowers_distinct_from_stop_run() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. RETURNER.\nPROCEDURE DIVISION.\nMAIN.\nGOBACK.\n";
        let ir = analyze_src("goback.cbl", src);

        assert!(matches!(
            ir.paragraphs[0].statements[0],
            StatementIr::Goback
        ));
        assert!(!has_diagnostic(&ir, "E_UNSUPPORTED_GOBACK"));
        assert!(matches!(
            ir.procedure_cfg.blocks[0].transfer,
            ControlTransferIr::Goback
        ));
    }

    #[test]
    fn stop_literal_remains_fail_closed_and_distinct_from_stop_run() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. STOPLIT.\nPROCEDURE DIVISION.\nMAIN.\nSTOP \"PAUSE\".\n";
        let ir = analyze_src("stop-literal.cbl", src);

        let StatementIr::Unsupported { keyword, raw } = &ir.paragraphs[0].statements[0] else {
            panic!("expected STOP literal to remain unsupported");
        };
        assert_eq!(keyword, "STOP");
        assert_eq!(raw, "STOP \"PAUSE\"");
        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_STATEMENT"));
        assert!(!matches!(
            ir.procedure_cfg.blocks[0].transfer,
            ControlTransferIr::StopRun
        ));
    }

    #[test]
    fn exit_program_lowers_like_goback() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. RETURNER.\nPROCEDURE DIVISION.\nMAIN.\nEXIT PROGRAM.\n";
        let ir = analyze_src("exit-program.cbl", src);

        assert!(matches!(
            ir.paragraphs[0].statements[0],
            StatementIr::Goback
        ));
        assert!(!has_diagnostic(&ir, "E_UNSUPPORTED_GOBACK"));
        assert!(matches!(
            ir.procedure_cfg.blocks[0].transfer,
            ControlTransferIr::Goback
        ));
    }

    #[test]
    fn stop_literal_lowers_fail_closed() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. PAUSER.\nPROCEDURE DIVISION.\nMAIN.\nSTOP \"PAUSE\".\n";
        let ir = analyze_src("stop-literal.cbl", src);

        assert!(matches!(
            ir.paragraphs[0].statements[0],
            StatementIr::Unsupported { ref keyword, .. } if keyword == "STOP"
        ));
        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_STATEMENT"));
    }

    fn has_diagnostic(ir: &ProgramIr, code: &str) -> bool {
        ir.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == code)
    }

    fn lower_single_imperative(raw: &str) -> StatementIr {
        let statements = cobol_syntax::parse_imperative_list(raw, SourceSpan::generated());
        assert_eq!(statements.len(), 1, "{statements:?}");
        lower_statement_ast(statements.into_iter().next().expect("statement"))
    }

    #[test]
    fn chain_statement_lowers_to_typed_ir_but_remains_fail_closed() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\nDATA DIVISION.\nLINKAGE SECTION.\n01 LK-A PIC X.\nPROCEDURE DIVISION USING LK-A.\nMAIN.\nCHAIN \"NEXTPROG\" USING LK-A.\nSTOP RUN.\n";
        let ir = analyze_src("hello.cbl", src);
        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_CHAIN"));
        let StatementIr::Chain(chain) = &ir.paragraphs[0].statements[0] else {
            panic!("expected typed CHAIN IR");
        };
        assert!(matches!(
            &chain.target,
            CallTargetIr::Literal(name) if name == "NEXTPROG"
        ));
        assert_eq!(chain.using.len(), 1);
        assert_eq!(chain.using[0].normalized, "LK_A");
        assert!(ir.semantic.references.iter().any(|reference| {
            reference.raw == "LK-A"
                && reference.role == ReferenceRoleIr::ProcedureArgument
                && reference.status == ReferenceResolutionStatusIr::Resolved
        }));
    }

    #[test]
    fn cancel_statement_lowers_to_typed_ir_but_remains_fail_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CANCELIR.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-DYNAMIC PIC X(8).
PROCEDURE DIVISION.
MAIN.
CANCEL "SUBPROG" WS-DYNAMIC.
STOP RUN.
"#;
        let ir = analyze_src("cancel-ir.cbl", src);

        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_CANCEL"));
        let StatementIr::Cancel(cancel) = &ir.paragraphs[0].statements[0] else {
            panic!("expected typed CANCEL IR");
        };
        assert_eq!(cancel.targets.len(), 2);
        assert!(matches!(
            &cancel.targets[0],
            CallTargetIr::Literal(name) if name == "SUBPROG"
        ));
        assert!(matches!(
            &cancel.targets[1],
            CallTargetIr::Identifier(reference) if reference.normalized == "WS_DYNAMIC"
        ));
        assert!(ir.semantic.references.iter().any(|reference| {
            reference.raw == "WS-DYNAMIC"
                && reference.role == ReferenceRoleIr::ProcedureTarget
                && reference.status == ReferenceResolutionStatusIr::Resolved
        }));
    }

    #[test]
    fn unlock_statement_remains_fail_closed_until_record_locking_is_lowered() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\nENVIRONMENT DIVISION.\nINPUT-OUTPUT SECTION.\nFILE-CONTROL.\nSELECT CUSTOMER-FILE ASSIGN TO \"customer.dat\".\nDATA DIVISION.\nFILE SECTION.\nFD CUSTOMER-FILE.\n01 CUSTOMER-REC PIC X.\nPROCEDURE DIVISION.\nMAIN.\nUNLOCK CUSTOMER-FILE RECORD.\nSTOP RUN.\n";
        let ir = analyze_src("hello.cbl", src);
        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_UNLOCK"));
        let StatementIr::UnlockFile(unlock) = &ir.paragraphs[0].statements[0] else {
            panic!("expected typed UNLOCK IR");
        };
        assert_eq!(unlock.file, "CUSTOMER_FILE");
        assert_eq!(unlock.options, vec!["RECORD".to_string()]);
    }

    #[test]
    fn accept_statement_lowers_to_typed_ir_but_remains_fail_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. ACCEPTIR.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-DATE PIC X(8).
PROCEDURE DIVISION.
MAIN.
ACCEPT WS-DATE FROM DATE YYYYMMDD.
STOP RUN.
"#;
        let ir = analyze_src("accept-ir.cbl", src);

        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_ACCEPT"));
        let StatementIr::Accept(accept) = &ir.paragraphs[0].statements[0] else {
            panic!("expected typed ACCEPT IR");
        };
        assert_eq!(accept.target.normalized, "WS_DATE");
        assert_eq!(accept.source.as_deref(), Some("DATE YYYYMMDD"));
        assert!(accept.options.is_empty());
        assert!(ir.semantic.references.iter().any(|reference| {
            reference.raw == "WS-DATE"
                && reference.role == ReferenceRoleIr::Target
                && reference.status == ReferenceResolutionStatusIr::Resolved
        }));
    }

    #[test]
    fn initialize_statement_lowers_to_typed_ir_but_remains_fail_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. INITIR.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC.
   05 WS-A PIC X.
   05 WS-N PIC 9.
PROCEDURE DIVISION.
MAIN.
INITIALIZE WS-A WS-N REPLACING NUMERIC DATA BY ZERO.
STOP RUN.
"#;
        let ir = analyze_src("initialize-ir.cbl", src);

        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_INITIALIZE"));
        let StatementIr::Initialize(initialize) = &ir.paragraphs[0].statements[0] else {
            panic!("expected typed INITIALIZE IR");
        };
        assert_eq!(
            initialize
                .targets
                .iter()
                .map(|target| target.normalized.as_str())
                .collect::<Vec<_>>(),
            vec!["WS_A", "WS_N"]
        );
        assert_eq!(initialize.options, vec!["REPLACING NUMERIC DATA BY ZERO"]);
        for (raw, target) in [("WS-A", "REC.WS_A"), ("WS-N", "REC.WS_N")] {
            assert!(ir.semantic.references.iter().any(|reference| {
                reference.raw == raw
                    && reference.target.as_deref() == Some(target)
                    && reference.role == ReferenceRoleIr::Target
                    && reference.status == ReferenceResolutionStatusIr::Resolved
            }));
        }
    }

    #[test]
    fn initialize_statement_reports_unresolved_targets_from_typed_ir() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. INITBAD.\nPROCEDURE DIVISION.\nMAIN.\nINITIALIZE MISSING-FIELD.\nSTOP RUN.\n";
        let ir = analyze_src("initialize-unresolved.cbl", src);

        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_INITIALIZE"));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNRESOLVED_DATA" && diagnostic.message.contains("MISSING-FIELD")
        }));
        assert!(matches!(
            ir.paragraphs[0].statements[0],
            StatementIr::Initialize(_)
        ));
    }

    #[test]
    fn generate_statement_remains_fail_closed_until_report_writer_is_lowered() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\nPROCEDURE DIVISION.\nMAIN.\nGENERATE SALES-DETAIL REPORT.\nSTOP RUN.\n";
        let ir = analyze_src("hello.cbl", src);
        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_GENERATE_REPORT"));
        let StatementIr::GenerateReport(generate) = &ir.paragraphs[0].statements[0] else {
            panic!("expected typed GENERATE report IR");
        };
        assert_eq!(generate.target, "SALES_DETAIL");
        assert_eq!(generate.options, vec!["REPORT".to_string()]);
    }

    #[test]
    fn initiate_statement_remains_fail_closed_until_report_writer_is_lowered() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\nPROCEDURE DIVISION.\nMAIN.\nINITIATE SALES-REPORT.\nSTOP RUN.\n";
        let ir = analyze_src("hello.cbl", src);
        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_INITIATE_REPORT"));
        let StatementIr::InitiateReport(initiate) = &ir.paragraphs[0].statements[0] else {
            panic!("expected typed INITIATE report IR");
        };
        assert_eq!(initiate.targets, vec!["SALES_REPORT".to_string()]);
    }

    #[test]
    fn terminate_statement_remains_fail_closed_until_report_writer_is_lowered() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\nPROCEDURE DIVISION.\nMAIN.\nTERMINATE SALES-REPORT.\nSTOP RUN.\n";
        let ir = analyze_src("hello.cbl", src);
        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_TERMINATE_REPORT"));
        let StatementIr::TerminateReport(terminate) = &ir.paragraphs[0].statements[0] else {
            panic!("expected typed TERMINATE report IR");
        };
        assert_eq!(terminate.targets, vec!["SALES_REPORT".to_string()]);
    }

    #[test]
    fn purge_statement_remains_fail_closed_until_queue_runtime_is_lowered() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\nPROCEDURE DIVISION.\nMAIN.\nPURGE PRINT-QUEUE MESSAGE.\nSTOP RUN.\n";
        let ir = analyze_src("hello.cbl", src);
        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_PURGE_QUEUE"));
        let StatementIr::PurgeQueue(purge) = &ir.paragraphs[0].statements[0] else {
            panic!("expected typed PURGE queue IR");
        };
        assert_eq!(purge.target, "PRINT_QUEUE");
        assert_eq!(purge.options, vec!["MESSAGE".to_string()]);
    }

    #[test]
    fn suppress_statement_remains_fail_closed_until_report_writer_is_lowered() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\nPROCEDURE DIVISION.\nMAIN.\nSUPPRESS PRINTING DETAIL-LINE.\nSTOP RUN.\n";
        let ir = analyze_src("hello.cbl", src);
        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_SUPPRESS_REPORT"));
        let StatementIr::SuppressReport(suppress) = &ir.paragraphs[0].statements[0] else {
            panic!("expected typed SUPPRESS report IR");
        };
        assert_eq!(suppress.target.as_deref(), Some("DETAIL_LINE"));
        assert_eq!(suppress.options, vec!["PRINTING".to_string()]);
    }

    #[test]
    fn enable_disable_statements_remain_fail_closed_until_communications_is_lowered() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\nPROCEDURE DIVISION.\nMAIN.\nENABLE INPUT TERM-1 WITH KEY.\nDISABLE OUTPUT TERM-1.\nSTOP RUN.\n";
        let ir = analyze_src("hello.cbl", src);
        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_ENABLE_COMMUNICATION"));
        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_DISABLE_COMMUNICATION"));
        let StatementIr::EnableCommunication(enable) = &ir.paragraphs[0].statements[0] else {
            panic!("expected typed ENABLE communication IR");
        };
        assert_eq!(enable.target, "TERM_1");
        assert_eq!(
            enable.options,
            vec!["INPUT".to_string(), "WITH".to_string(), "KEY".to_string()]
        );
        let StatementIr::DisableCommunication(disable) = &ir.paragraphs[0].statements[1] else {
            panic!("expected typed DISABLE communication IR");
        };
        assert_eq!(disable.target, "TERM_1");
        assert_eq!(disable.options, vec!["OUTPUT".to_string()]);
    }

    #[test]
    fn unsupported_statement_inside_file_io_branch_fails_closed() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. FILEBRANCH.\nENVIRONMENT DIVISION.\nINPUT-OUTPUT SECTION.\nFILE-CONTROL.\nSELECT INFILE ASSIGN TO \"in.dat\".\nDATA DIVISION.\nFILE SECTION.\nFD INFILE.\n01 INREC PIC X.\nPROCEDURE DIVISION.\nMAIN.\nREAD INFILE AT END SEND TERM-1 FROM OUT-MSG END-READ.\nSTOP RUN.\n";
        let ir = analyze_src("file-branch.cbl", src);
        let StatementIr::ReadFile(read) = &ir.paragraphs[0].statements[0] else {
            panic!("expected typed READ");
        };
        let Some(StatementIr::SendCommunication(send)) = read.at_end_ops.first() else {
            panic!("expected typed nested SEND communication IR");
        };
        assert_eq!(send.target, "TERM_1");
        assert_eq!(
            send.options,
            vec!["FROM".to_string(), "OUT-MSG".to_string()]
        );
        assert!(
            ir.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "E_UNSUPPORTED_SEND_COMMUNICATION"
                    && diagnostic
                        .message
                        .contains("communications runtime send semantics")
            }),
            "{:?}",
            ir.diagnostics
        );
    }

    #[test]
    fn send_receive_statements_remain_fail_closed_until_communications_is_lowered() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\nPROCEDURE DIVISION.\nMAIN.\nSEND TERM-1 FROM OUT-MSG WITH EGI.\nRECEIVE TERM-1 MESSAGE INTO IN-MSG.\nSTOP RUN.\n";
        let ir = analyze_src("hello.cbl", src);
        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_SEND_COMMUNICATION"));
        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_RECEIVE_COMMUNICATION"));
        let StatementIr::SendCommunication(send) = &ir.paragraphs[0].statements[0] else {
            panic!("expected typed SEND communication IR");
        };
        assert_eq!(send.target, "TERM_1");
        assert_eq!(
            send.options,
            vec![
                "FROM".to_string(),
                "OUT-MSG".to_string(),
                "WITH".to_string(),
                "EGI".to_string()
            ]
        );
        let StatementIr::ReceiveCommunication(receive) = &ir.paragraphs[0].statements[1] else {
            panic!("expected typed RECEIVE communication IR");
        };
        assert_eq!(receive.target, "TERM_1");
        assert_eq!(
            receive.options,
            vec![
                "MESSAGE".to_string(),
                "INTO".to_string(),
                "IN-MSG".to_string()
            ]
        );
    }

    #[test]
    fn merge_statement_remains_fail_closed_until_file_merge_is_lowered() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\nPROCEDURE DIVISION.\nMAIN.\nMERGE SORT-FILE USING INPUT-1 GIVING OUTPUT-FILE.\nSTOP RUN.\n";
        let ir = analyze_src("hello.cbl", src);
        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_MERGE_FILE"));
        let StatementIr::MergeFile(merge) = &ir.paragraphs[0].statements[0] else {
            panic!("expected typed MERGE IR");
        };
        assert_eq!(merge.file, "SORT_FILE");
        assert_eq!(
            merge.options,
            vec![
                "USING".to_string(),
                "INPUT-1".to_string(),
                "GIVING".to_string(),
                "OUTPUT-FILE".to_string()
            ]
        );
    }

    #[test]
    fn enter_statement_remains_fail_closed_until_language_switch_is_lowered() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\nPROCEDURE DIVISION.\nMAIN.\nENTER LANGUAGE ASSEMBLER.\nSTOP RUN.\n";
        let ir = analyze_src("hello.cbl", src);
        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_ENTER_LANGUAGE"));
        let StatementIr::EnterLanguage(enter) = &ir.paragraphs[0].statements[0] else {
            panic!("expected typed ENTER IR");
        };
        assert_eq!(enter.language, "LANGUAGE");
        assert_eq!(enter.options, vec!["ASSEMBLER".to_string()]);
    }

    #[test]
    fn entry_statement_lowers_to_typed_ir_but_remains_fail_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. ENTRYIR.
DATA DIVISION.
LINKAGE SECTION.
01 LK-A PIC X.
PROCEDURE DIVISION USING LK-A.
MAIN.
ENTRY "ALT-ENTRY" USING LK-A.
STOP RUN.
"#;
        let ir = analyze_src("entry-ir.cbl", src);

        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_ENTRY"));
        let StatementIr::Entry(entry) = &ir.paragraphs[0].statements[0] else {
            panic!("expected typed ENTRY IR");
        };
        assert!(matches!(&entry.name, CallTargetIr::Literal(name) if name == "ALT-ENTRY"));
        assert_eq!(entry.using[0].normalized, "LK_A");
        assert!(ir.semantic.references.iter().any(|reference| {
            reference.raw == "LK-A"
                && reference.role == ReferenceRoleIr::ProcedureArgument
                && reference.status == ReferenceResolutionStatusIr::Resolved
        }));
    }

    #[test]
    fn lowers_data_and_display() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 WS-NAME PIC X(10).\nPROCEDURE DIVISION.\nMAIN.\nDISPLAY WS-NAME.\nSTOP RUN.\n";
        let ast = parse_program("hello.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        assert_eq!(ir.data_items.len(), 1);
        assert_eq!(ir.paragraphs.len(), 1);
        assert_eq!(ir.storage.record_length, 10);
        assert!(!ir.has_errors());
    }

    #[test]
    fn move_rejects_unconsumed_target_tail_without_bogus_target_reference() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. MOVETAIL.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-N PIC 9.
PROCEDURE DIVISION.
MAIN.
MOVE 1 TO WS-N GARBAGE.
STOP RUN.
"#;
        let ir = analyze_src("move-tail.cbl", src);

        assert!(matches!(
            ir.paragraphs[0].statements[0],
            StatementIr::Unsupported { ref keyword, .. } if keyword == "MOVE"
        ));
        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_STATEMENT"));
        assert!(ir.diagnostics.iter().all(|diagnostic| {
            !(diagnostic.code == "E_UNRESOLVED_DATA" && diagnostic.message.contains("WS_N_GARBAGE"))
        }));
    }

    #[test]
    fn move_rejects_unconsumed_source_tail_without_bogus_source_reference() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. MOVESRC.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-SRC PIC 9.
01 WS-N PIC 9.
PROCEDURE DIVISION.
MAIN.
MOVE WS-SRC GARBAGE TO WS-N.
STOP RUN.
"#;
        let ir = analyze_src("move-source-tail.cbl", src);

        assert!(matches!(
            ir.paragraphs[0].statements[0],
            StatementIr::Unsupported { ref keyword, .. } if keyword == "MOVE"
        ));
        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_STATEMENT"));
        assert!(ir.diagnostics.iter().all(|diagnostic| {
            !(diagnostic.code == "E_UNRESOLVED_DATA"
                && diagnostic.message.contains("WS_SRC_GARBAGE"))
        }));
    }

    #[test]
    fn computes_group_offsets_packed_binary_redefines_and_conditions() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. DATA.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-NAME PIC X(10) VALUE "ALPHA".
   05 WS-AMOUNT PIC S9(7)V99 COMP-3.
   05 WS-FLAG PIC X.
      88 WS-FLAG-YES VALUE "Y".
   05 WS-ALT REDEFINES WS-AMOUNT PIC X(5).
   05 WS-BIN PIC 9(9) COMP SYNC.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ast = parse_program("data.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let amount = ir
            .storage
            .items
            .iter()
            .find(|item| item.name == "WS_AMOUNT")
            .expect("amount");
        assert_eq!(amount.offset, 10);
        assert_eq!(amount.byte_len, 5);
        let alt = ir
            .storage
            .items
            .iter()
            .find(|item| item.name == "WS_ALT")
            .expect("alt");
        assert_eq!(alt.offset, amount.offset);
        assert_eq!(ir.storage.redefines.len(), 1);
        assert_eq!(ir.storage.condition_names.len(), 1);
        let bin = ir
            .storage
            .items
            .iter()
            .find(|item| item.name == "WS_BIN")
            .expect("bin");
        assert_eq!(bin.offset % 4, 0);
        let record_bin = ir
            .storage
            .record_plan
            .fields
            .iter()
            .find(|field| field.name == "WS_BIN")
            .expect("record bin");
        assert_eq!(record_bin.offset, bin.offset);
        assert_eq!(record_bin.byte_len, bin.byte_len);
        assert!(ir.storage.record_plan.coverage.uncovered_bytes <= ir.storage.record_length);
    }

    #[test]
    fn packed_decimal_storage_initial_bytes_are_valid_comp3() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. PACKEDINIT.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-AMT PIC S9(5) COMP-3 VALUE 00123.
   05 WS-ZERO PIC S9(5) COMP-3.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("packed-init.cbl", src);

        let amount = ir
            .storage
            .storage_cells
            .iter()
            .find(|cell| cell.key == "WS_REC.WS_AMT")
            .expect("amount cell");
        assert_eq!(amount.initial_bytes, vec![0x00, 0x12, 0x3c]);

        let zero = ir
            .storage
            .storage_cells
            .iter()
            .find(|cell| cell.key == "WS_REC.WS_ZERO")
            .expect("zero cell");
        assert_eq!(zero.initial_bytes, vec![0x00, 0x00, 0x0c]);
    }

    #[test]
    fn alphanumeric_occurs_initial_value_repeats_per_occurrence_in_storage_ir() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. ALPHAOCCINIT.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TABLE.
   05 WS-ITEM OCCURS 3 TIMES PIC X(2) VALUE "A".
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("alpha-occurs-init.cbl", src);
        let cell = ir
            .storage
            .storage_cells
            .iter()
            .find(|cell| cell.key == "WS_TABLE.WS_ITEM")
            .expect("WS-ITEM storage cell");

        assert_eq!(cell.initial_bytes, b"A A A ".to_vec());
    }

    #[test]
    fn value_all_initial_value_expands_in_storage_ir() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. VALUEALLINIT.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FILL PIC X(5) VALUE ALL "AB".
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("value-all-init.cbl", src);
        let cell = ir
            .storage
            .storage_cells
            .iter()
            .find(|cell| cell.key == "WS_FILL")
            .expect("WS-FILL storage cell");

        assert_eq!(cell.initial_bytes, b"ABABA".to_vec());
    }

    #[test]
    fn condition_name_value_all_expands_to_parent_occurrence_len() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CONDVALUEALL.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FLAG OCCURS 2 TIMES PIC X(3).
   88 WS-ALL-A VALUE ALL "A".
PROCEDURE DIVISION.
MAIN.
IF WS-ALL-A(1) DISPLAY "Y" END-IF.
STOP RUN.
"#;
        let ir = analyze_src("condition-value-all.cbl", src);
        assert!(
            ir.diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "E_INVALID_CONDITION_VALUE"),
            "{:?}",
            ir.diagnostics
        );
        let condition = ir
            .storage
            .condition_names
            .iter()
            .find(|condition| condition.name == "WS_ALL_A")
            .expect("condition name");

        assert_eq!(
            condition.value_set,
            vec![ConditionValueIr::Single("AAA".to_string())]
        );
    }

    #[test]
    fn invalid_packed_decimal_value_fails_closed_in_sema() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. PACKEDBAD.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-AMT PIC 9(2) COMP-3 VALUE -1.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("packed-bad.cbl", src);

        assert!(has_diagnostic(&ir, "E_INVALID_PACKED_DECIMAL_VALUE"));
    }

    #[test]
    fn float_storage_initial_bytes_are_valid_ibm_hex_float() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FLOATINIT.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-FLOAT COMP-1 VALUE 100.
   05 WS-DOUBLE COMP-2 VALUE -1.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("float-init.cbl", src);

        let single = ir
            .storage
            .storage_cells
            .iter()
            .find(|cell| cell.key == "WS_REC.WS_FLOAT")
            .expect("single cell");
        assert_eq!(single.initial_bytes, vec![0x42, 0x64, 0x00, 0x00]);

        let double = ir
            .storage
            .storage_cells
            .iter()
            .find(|cell| cell.key == "WS_REC.WS_DOUBLE")
            .expect("double cell");
        assert_eq!(
            double.initial_bytes,
            vec![0xc1, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn float_storage_initial_bytes_use_ieee_for_gnucobol() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FLOATINIT.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-FLOAT COMP-1 VALUE 100.
   05 WS-DOUBLE COMP-2 VALUE -1.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src_with_dialect("float-init.cbl", src, Dialect::GnuCobol);

        let single = ir
            .storage
            .storage_cells
            .iter()
            .find(|cell| cell.key == "WS_REC.WS_FLOAT")
            .expect("single cell");
        assert_eq!(single.initial_bytes, vec![0x42, 0xc8, 0x00, 0x00]);

        let double = ir
            .storage
            .storage_cells
            .iter()
            .find(|cell| cell.key == "WS_REC.WS_DOUBLE")
            .expect("double cell");
        assert_eq!(
            double.initial_bytes,
            vec![0xbf, 0xf0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
        );
    }

    #[test]
    fn invalid_float_initial_value_fails_closed_in_sema() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FLOATBAD.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FLOAT COMP-1 VALUE "BAD".
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("float-bad.cbl", src);

        assert!(has_diagnostic(&ir, "E_INVALID_FLOAT_VALUE"));
        assert!(
            ir.diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("WS_FLOAT")),
            "{:?}",
            ir.diagnostics
        );
    }

    #[test]
    fn gnucobol_sync_uses_gnucobol_platform_profile_without_ibm_slack() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. GNUSYNC.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC.
   05 WS-FLAG PIC X.
   05 WS-BIN PIC 9(9) COMP SYNC.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ast = parse_program("gnu-sync.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::GnuCobol);
        let bin = ir
            .storage
            .items
            .iter()
            .find(|item| item.qualified_name == "REC.WS_BIN")
            .expect("binary item");

        assert_eq!(bin.offset, 1);
        assert_eq!(ir.storage.record_length, 5);
        assert_eq!(
            ir.storage.record_plan.platform_profile,
            PlatformProfile::GnuCobol
        );
    }

    #[test]
    fn group_occurs_reserves_repeated_storage_and_qualified_refs_lower() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. OCCURS.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-TABLE OCCURS 3 TIMES.
      10 WS-ITEM PIC X(2).
   05 WS-AFTER PIC X.
PROCEDURE DIVISION.
MAIN.
MOVE "A" TO WS-ITEM OF WS-TABLE.
STOP RUN.
"#;
        let ast = parse_program("occurs.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let table = ir
            .storage
            .items
            .iter()
            .find(|item| item.name == "WS_TABLE")
            .expect("table");
        let after = ir
            .storage
            .items
            .iter()
            .find(|item| item.name == "WS_AFTER")
            .expect("after");
        assert_eq!(table.byte_len, 6);
        assert_eq!(after.offset, 6);
        assert!(matches!(
            &ir.paragraphs[0].statements[0],
            StatementIr::Move { target, .. } if target.normalized == "WS_TABLE.WS_ITEM"
        ));
    }

    #[test]
    fn redefines_resolves_within_same_parent_scope() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. REDEF.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 A-REC.
   05 FIELD-A PIC X(2).
01 B-REC.
   05 FIELD-A PIC X(4).
   05 FIELD-B REDEFINES FIELD-A PIC X(4).
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ast = parse_program("redef.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let redefine = ir.storage.redefines.first().expect("redefines");
        assert_eq!(redefine.base_item, "B_REC.FIELD_A");
        assert_eq!(redefine.base_byte_len, 4);
    }

    #[test]
    fn redefined_base_status_does_not_leak_across_same_named_siblings() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. REDEFSCOPE.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 A-REC.
   05 FIELD-A PIC X.
01 B-REC.
   05 FIELD-A PIC X.
   05 FIELD-B REDEFINES FIELD-A PIC X.
PROCEDURE DIVISION.
MAIN.
DISPLAY FIELD-A OF A-REC.
STOP RUN.
"#;
        let ir = analyze_src("redef-scope.cbl", src);

        assert!(ir
            .storage
            .redefines
            .iter()
            .any(|redefine| redefine.base_item == "B_REC.FIELD_A"));
        assert!(ir.semantic.references.iter().any(|reference| {
            reference.raw == "FIELD-A OF A-REC"
                && reference.target.as_deref() == Some("A_REC.FIELD_A")
                && reference.status == ReferenceResolutionStatusIr::Resolved
        }));
        assert!(!ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_CODEGEN_REDEFINES_REFERENCE"
                && diagnostic.message.contains("A_REC.FIELD_A")
        }));
    }

    #[test]
    fn redefines_group_children_remain_fail_closed_as_overlay_context() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. REDEFCHILD.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC.
   05 BASE-GRP.
      10 BASE-FIELD PIC X.
   05 ALT-GRP REDEFINES BASE-GRP.
      10 ALT-FIELD PIC X.
PROCEDURE DIVISION.
MAIN.
DISPLAY BASE-FIELD OF BASE-GRP.
DISPLAY ALT-FIELD OF ALT-GRP.
STOP RUN.
"#;
        let ir = analyze_src("redef-child.cbl", src);

        for (raw, target) in [
            ("BASE-FIELD OF BASE-GRP", "REC.BASE_GRP.BASE_FIELD"),
            ("ALT-FIELD OF ALT-GRP", "REC.ALT_GRP.ALT_FIELD"),
        ] {
            assert!(ir.semantic.references.iter().any(|reference| {
                reference.raw == raw
                    && reference.target.as_deref() == Some(target)
                    && reference.status == ReferenceResolutionStatusIr::UnsupportedRedefines
            }));
            assert!(ir.semantic.resolved_data_refs.iter().any(|reference| {
                reference.raw == raw
                    && reference.target.as_deref() == Some(target)
                    && reference.in_redefines
                    && reference.status == ReferenceResolutionStatusIr::UnsupportedRedefines
            }));
            assert!(ir.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "E_CODEGEN_REDEFINES_REFERENCE"
                    && diagnostic.message.contains(target)
            }));
        }
    }

    #[test]
    fn redefines_overlay_does_not_allocate_independent_storage_cell_or_binding() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. REDEFCELL.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC.
   05 BASE-FIELD PIC X(2).
   05 OVER-FIELD REDEFINES BASE-FIELD PIC X(2).
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("redef-cell.cbl", src);

        assert_eq!(ir.storage.redefines.len(), 1);
        assert!(ir
            .storage
            .items
            .iter()
            .any(|item| item.qualified_name == "REC.OVER_FIELD"));
        assert!(ir
            .storage
            .storage_cells
            .iter()
            .any(|cell| cell.key == "REC.BASE_FIELD"));
        assert!(ir
            .storage
            .storage_cells
            .iter()
            .all(|cell| cell.key != "REC.OVER_FIELD"));
        assert!(ir
            .storage
            .storage_bindings
            .iter()
            .all(|(name, _)| name != "REC.OVER_FIELD"));
    }

    #[test]
    fn redefines_larger_overlay_fails_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. REDEFBIG.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC.
   05 BASE-FIELD PIC X(2).
   05 OVER-FIELD REDEFINES BASE-FIELD PIC X(3).
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("redef-big.cbl", src);

        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_REDEFINES_OVERLAY"
                && diagnostic.message.contains("OVER_FIELD")
                && diagnostic.message.contains("3 bytes")
                && diagnostic.message.contains("BASE_FIELD")
                && diagnostic.message.contains("2 bytes")
        }));
    }

    #[test]
    fn group_redefines_with_non_identical_child_views_fails_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. REDEFGROUP.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC.
   05 BASE-GRP.
      10 BASE-A PIC X.
      10 BASE-B PIC X.
   05 OVER-GRP REDEFINES BASE-GRP.
      10 OVER-ALL PIC X(2).
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ast = parse_program("redef-group.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_REDEFINES_OVERLAY"
                && diagnostic.message.contains("OVER_GRP.OVER_ALL")
        }));
    }

    #[test]
    fn redefines_occurs_storage_fails_closed_even_when_byte_range_matches() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. REDEFOCC.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC.
   05 BASE-TAB OCCURS 2 TIMES PIC X.
   05 OVER-A REDEFINES BASE-TAB PIC X(2).
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("redef-occurs.cbl", src);

        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_REDEFINES_OVERLAY"
                && diagnostic.message.contains("REDEFINES")
                && diagnostic.message.contains("OCCURS")
                && diagnostic.message.contains("REC.BASE_TAB")
        }));
    }

    #[test]
    fn group_redefines_overlay_does_not_extend_primary_record_coverage() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. REDEFPRIMARY.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC.
   05 BASE-GRP.
      10 BASE-A PIC X.
      10 BASE-B PIC X.
   05 OVER-GRP REDEFINES BASE-GRP.
      10 OVER-A PIC X.
      10 OVER-B PIC X.
      10 OVER-C PIC X.
      10 OVER-D PIC X.
   05 TAIL PIC X.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("redef-primary.cbl", src);
        let tail = ir
            .storage
            .items
            .iter()
            .find(|item| item.qualified_name == "REC.TAIL")
            .expect("tail item");
        let over_group = ir
            .storage
            .items
            .iter()
            .find(|item| item.qualified_name == "REC.OVER_GRP")
            .expect("overlay group");

        assert_eq!(tail.offset, 2);
        assert_eq!(over_group.byte_len, 4);
        assert_eq!(ir.storage.record_length, 3);
        assert_eq!(ir.storage.record_plan.coverage.record_length, 3);
        assert!(ir.storage.record_plan.coverage.overlaps.is_empty());
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_REDEFINES_OVERLAY"
                && diagnostic.message.contains("REC.OVER_GRP")
                && diagnostic.message.contains("4 bytes")
                && diagnostic.message.contains("REC.BASE_GRP")
                && diagnostic.message.contains("2 bytes")
        }));
    }

    #[test]
    fn level_78_constants_do_not_allocate_storage() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CONSTS.
DATA DIVISION.
WORKING-STORAGE SECTION.
78 MAX-SIZE VALUE 10.
01 WS-FIELD PIC X.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ast = parse_program("consts.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        assert!(!ir.storage.items.iter().any(|item| item.name == "MAX_SIZE"));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_DATA_CLAUSE"
                && diagnostic.message.contains("78-level constant")
        }));
    }

    #[test]
    fn odo_requires_resolved_counter_and_descriptor_uses_occurrence_stride() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. ODOCHECK.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 ODO-COUNT PIC 9 VALUE 1.
01 GOOD-TABLE OCCURS 0 TO 4 DEPENDING ON ODO-COUNT.
   05 GOOD-A PIC X.
   05 GOOD-B PIC X(2).
01 BAD-TABLE OCCURS 0 TO 3 DEPENDING ON MISSING-CNT.
   05 BAD-A PIC X.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ast = parse_program("odo-check.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let good = ir
            .odo_descriptors
            .iter()
            .find(|odo| odo.table == "GOOD_TABLE")
            .expect("good odo descriptor");
        assert_eq!(good.stride, 3);
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNRESOLVED_ODO_DEPENDING_ON"
                && diagnostic.message.contains("MISSING_CNT")
        }));
    }

    #[test]
    fn reversed_occurs_range_fails_closed_without_odo_descriptor() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. BADODO.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 ODO-COUNT PIC 9.
01 BAD-TABLE OCCURS 5 TO 3 DEPENDING ON ODO-COUNT PIC X.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("bad-odo-range.cbl", src);

        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_OCCURS_RANGE"
                && diagnostic.message.contains("BAD_TABLE")
                && diagnostic.message.contains("5")
                && diagnostic.message.contains("3")
        }));
        assert!(ir
            .odo_descriptors
            .iter()
            .all(|descriptor| descriptor.table != "BAD_TABLE"));
        assert!(ir
            .storage
            .odo_templates
            .iter()
            .all(|template| template.table != "BAD_TABLE"));
    }

    #[test]
    fn odo_counter_requires_resolved_numeric_item() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. ODONUM.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 TEXT-COUNT PIC X.
01 BAD-TABLE OCCURS 0 TO 3 DEPENDING ON TEXT-COUNT PIC X.
01 MISS-TABLE OCCURS 0 TO 3 DEPENDING ON MISSING-COUNT PIC X.
01 GOOD-COUNT PIC 9.
01 GOOD-TABLE OCCURS 0 TO 3 DEPENDING ON GOOD-COUNT PIC X.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("odo-numeric.cbl", src);
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_ODO_DEPENDING_ON"
                && diagnostic.message.contains("TEXT_COUNT")
        }));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNRESOLVED_ODO_DEPENDING_ON"
                && diagnostic.message.contains("MISSING_COUNT")
        }));
        assert!(ir.diagnostics.iter().all(|diagnostic| {
            !(diagnostic.code == "E_INVALID_ODO_DEPENDING_ON"
                && diagnostic.message.contains("GOOD_COUNT"))
        }));
        assert!(ir
            .odo_descriptors
            .iter()
            .any(|descriptor| descriptor.table == "GOOD_TABLE"));
        assert!(ir
            .odo_descriptors
            .iter()
            .all(|descriptor| descriptor.table != "BAD_TABLE" && descriptor.table != "MISS_TABLE"));
        assert!(ir
            .storage
            .odo_templates
            .iter()
            .any(|template| template.table == "GOOD_TABLE"));
        assert!(ir
            .storage
            .odo_templates
            .iter()
            .all(|template| template.table != "BAD_TABLE" && template.table != "MISS_TABLE"));
    }

    #[test]
    fn odo_counter_rejects_special_registers() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. ODOSPECIAL.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 BAD-TABLE OCCURS 0 TO 3 DEPENDING ON TALLY PIC X.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("odo-special.cbl", src);

        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_ODO_DEPENDING_ON"
                && diagnostic.message.contains("TALLY")
                && diagnostic.message.contains("special register")
        }));
        assert!(ir
            .odo_descriptors
            .iter()
            .all(|descriptor| descriptor.table != "BAD_TABLE"));
        assert!(ir
            .storage
            .odo_templates
            .iter()
            .all(|template| template.table != "BAD_TABLE"));
    }

    #[test]
    fn reversed_renames_thru_range_fails_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. BADREN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC.
   05 A PIC X.
   05 B PIC X.
66 BAD-ALIAS RENAMES B THRU A.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ast = parse_program("bad-renames.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_DATA_CLAUSE"
                && diagnostic.message.contains("declaration order")
        }));
    }

    #[test]
    fn renames_range_spanning_filler_fails_closed_without_alias() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FILLREN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC.
   05 A PIC X.
   05 FILLER PIC X.
   05 B PIC X.
66 AB-ALIAS RENAMES A THRU B.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("filler-renames.cbl", src);

        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_DATA_CLAUSE"
                && diagnostic.message.contains("RENAMES")
                && diagnostic.message.contains("FILLER")
        }));
        assert!(ir.storage.renames.is_empty());
        assert!(ir
            .data_items
            .iter()
            .all(|item| item.qualified_name != "AB_ALIAS"));
        assert!(ir
            .storage
            .storage_bindings
            .iter()
            .all(|(name, _)| name != "AB_ALIAS"));
    }

    #[test]
    fn renames_group_spanning_filler_fails_closed_without_alias() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. GRPFILLREN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC.
   05 A PIC X.
   05 FILLER PIC X.
   05 B PIC X.
66 REC-ALIAS RENAMES REC.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("group-filler-renames.cbl", src);

        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_DATA_CLAUSE"
                && diagnostic.message.contains("RENAMES")
                && diagnostic.message.contains("FILLER")
        }));
        assert!(ir.storage.renames.is_empty());
        assert!(ir
            .data_items
            .iter()
            .all(|item| item.qualified_name != "REC_ALIAS"));
        assert!(ir
            .storage
            .storage_bindings
            .iter()
            .all(|(name, _)| name != "REC_ALIAS"));
    }

    #[test]
    fn renames_group_spanning_occurs_fails_closed_without_alias() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. GRPOCCREN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC.
   05 TAB OCCURS 2 TIMES.
      10 ITEM PIC X.
66 REC-ALIAS RENAMES REC.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("group-occurs-renames.cbl", src);

        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_DATA_CLAUSE"
                && diagnostic.message.contains("RENAMES")
                && diagnostic.message.contains("OCCURS")
        }));
        assert!(ir.storage.renames.is_empty());
        assert!(ir
            .data_items
            .iter()
            .all(|item| item.qualified_name != "REC_ALIAS"));
        assert!(ir
            .storage
            .storage_bindings
            .iter()
            .all(|(name, _)| name != "REC_ALIAS"));
    }

    #[test]
    fn renames_direct_elementary_occurs_fails_closed_without_alias() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. DIROCCREN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC.
   05 TAB OCCURS 2 TIMES PIC X.
66 TAB-ALIAS RENAMES TAB.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("direct-occurs-renames.cbl", src);

        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_DATA_CLAUSE"
                && diagnostic.message.contains("RENAMES")
                && diagnostic.message.contains("OCCURS")
                && diagnostic.message.contains("REC.TAB")
        }));
        assert!(ir.storage.renames.is_empty());
        assert!(ir
            .data_items
            .iter()
            .all(|item| item.qualified_name != "TAB_ALIAS"));
        assert!(ir
            .storage
            .storage_bindings
            .iter()
            .all(|(name, _)| name != "TAB_ALIAS"));
    }

    #[test]
    fn renames_direct_redefines_item_fails_closed_without_alias() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. DIRREDEFREN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC.
   05 BASE-FIELD PIC X.
   05 ALT-FIELD REDEFINES BASE-FIELD PIC X.
66 ALT-ALIAS RENAMES ALT-FIELD.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("direct-redefines-renames.cbl", src);

        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_DATA_CLAUSE"
                && diagnostic.message.contains("RENAMES")
                && diagnostic.message.contains("REDEFINES")
        }));
        assert!(ir.storage.renames.is_empty());
        assert!(ir
            .data_items
            .iter()
            .all(|item| item.qualified_name != "ALT_ALIAS"));
        assert!(ir
            .storage
            .storage_bindings
            .iter()
            .all(|(name, _)| name != "ALT_ALIAS"));
    }

    #[test]
    fn renames_redefined_base_item_fails_closed_without_alias() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. BASEREDEFREN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC.
   05 BASE-FIELD PIC X.
   05 ALT-FIELD REDEFINES BASE-FIELD PIC X.
66 BASE-ALIAS RENAMES BASE-FIELD.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("base-redefines-renames.cbl", src);

        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_DATA_CLAUSE"
                && diagnostic.message.contains("RENAMES")
                && diagnostic.message.contains("REDEFINES")
        }));
        assert!(ir.storage.renames.is_empty());
        assert!(ir
            .data_items
            .iter()
            .all(|item| item.qualified_name != "BASE_ALIAS"));
        assert!(ir
            .storage
            .storage_bindings
            .iter()
            .all(|(name, _)| name != "BASE_ALIAS"));
    }

    #[test]
    fn renames_group_spanning_redefines_fails_closed_without_alias() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. GRPREDEFREN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC.
   05 BASE-GRP.
      10 BASE-FIELD PIC X.
   05 ALT-GRP REDEFINES BASE-GRP.
      10 ALT-FIELD PIC X.
66 REC-ALIAS RENAMES REC.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("group-redefines-renames.cbl", src);

        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_DATA_CLAUSE"
                && diagnostic.message.contains("RENAMES")
                && diagnostic.message.contains("REDEFINES")
        }));
        assert!(ir.storage.renames.is_empty());
        assert!(ir
            .data_items
            .iter()
            .all(|item| item.qualified_name != "REC_ALIAS"));
        assert!(ir
            .storage
            .storage_bindings
            .iter()
            .all(|(name, _)| name != "REC_ALIAS"));
    }

    #[test]
    fn renames_group_with_direct_elementary_occurs_fails_closed_without_partial_alias() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. GRPDIRECTOCCREN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC.
   05 A PIC X.
   05 TAB OCCURS 2 TIMES PIC X.
   05 B PIC X.
66 REC-ALIAS RENAMES REC.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("group-direct-occurs-renames.cbl", src);

        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_DATA_CLAUSE"
                && diagnostic.message.contains("RENAMES")
                && diagnostic.message.contains("OCCURS")
                && diagnostic.message.contains("REC.TAB")
        }));
        assert!(ir.storage.renames.is_empty());
        assert!(ir
            .data_items
            .iter()
            .all(|item| item.qualified_name != "REC_ALIAS"));
        assert!(ir
            .storage
            .storage_bindings
            .iter()
            .all(|(name, _)| name != "REC_ALIAS"));
    }

    #[test]
    fn renames_range_spanning_occurs_fails_closed_without_alias() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. RNGOCCREN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC.
   05 A PIC X.
   05 TAB OCCURS 2 TIMES.
      10 ITEM PIC X.
   05 B PIC X.
66 AB-ALIAS RENAMES A THRU B.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("range-occurs-renames.cbl", src);

        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_DATA_CLAUSE"
                && diagnostic.message.contains("RENAMES")
                && diagnostic.message.contains("OCCURS")
        }));
        assert!(ir.storage.renames.is_empty());
        assert!(ir
            .data_items
            .iter()
            .all(|item| item.qualified_name != "AB_ALIAS"));
        assert!(ir
            .storage
            .storage_bindings
            .iter()
            .all(|(name, _)| name != "AB_ALIAS"));
    }

    #[test]
    fn renames_range_spanning_redefines_fails_closed_without_alias() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. RNGREDEFREN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC.
   05 A PIC X.
   05 BASE-FIELD PIC X.
   05 ALT-FIELD REDEFINES BASE-FIELD PIC X.
   05 B PIC X.
66 AB-ALIAS RENAMES A THRU B.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("range-redefines-renames.cbl", src);

        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_DATA_CLAUSE"
                && diagnostic.message.contains("RENAMES")
                && diagnostic.message.contains("REDEFINES")
        }));
        assert!(ir.storage.renames.is_empty());
        assert!(ir
            .data_items
            .iter()
            .all(|item| item.qualified_name != "AB_ALIAS"));
        assert!(ir
            .storage
            .storage_bindings
            .iter()
            .all(|(name, _)| name != "AB_ALIAS"));
    }

    #[test]
    fn renames_declared_in_different_storage_area_fails_closed_without_alias() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. AREADECLREN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-FIELD PIC X.
LINKAGE SECTION.
66 WS-ALIAS RENAMES WS-REC.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("area-decl-renames.cbl", src);

        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_DATA_CLAUSE"
                && diagnostic.message.contains("RENAMES")
                && diagnostic.message.contains("storage area")
        }));
        assert!(ir.storage.renames.is_empty());
        assert!(ir
            .data_items
            .iter()
            .all(|item| item.qualified_name != "WS_ALIAS"));
        assert!(ir
            .storage
            .storage_bindings
            .iter()
            .all(|(name, _)| name != "WS_ALIAS"));
    }

    #[test]
    fn renames_range_crossing_storage_areas_fails_closed_without_alias() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. AREARNGREN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-A PIC X.
66 BAD-ALIAS RENAMES WS-A THRU LK-B.
LINKAGE SECTION.
01 LK-B PIC X.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("area-range-renames.cbl", src);

        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_DATA_CLAUSE"
                && diagnostic.message.contains("RENAMES")
                && diagnostic.message.contains("storage area")
        }));
        assert!(ir.storage.renames.is_empty());
        assert!(ir
            .data_items
            .iter()
            .all(|item| item.qualified_name != "BAD_ALIAS"));
        assert!(ir
            .storage
            .storage_bindings
            .iter()
            .all(|(name, _)| name != "BAD_ALIAS"));
    }

    #[test]
    fn renames_storage_binding_points_to_selected_elementary_targets() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. GOODREN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC.
   05 A PIC X.
   05 B PIC X.
   05 C PIC X.
66 AB-ALIAS RENAMES A THRU B.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("good-renames.cbl", src);
        let rename = ir.storage.renames.first().expect("renames ir");
        assert_eq!(rename.renaming_item, "AB_ALIAS");
        assert_eq!(rename.targets, vec!["REC.A", "REC.B"]);

        let binding = ir
            .storage
            .storage_bindings
            .iter()
            .find_map(|(name, binding)| (name == "AB_ALIAS").then_some(binding))
            .expect("renames storage binding");
        assert!(matches!(
            binding,
            StorageBindingIr::Group { children } if children == &vec!["REC.A".to_string(), "REC.B".to_string()]
        ));
    }

    #[test]
    fn renames_alias_is_not_a_physical_record_plan_field() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. RENPLAN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC.
   05 A PIC X.
   05 B PIC X.
66 AB-ALIAS RENAMES A THRU B.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("renames-record-plan.cbl", src);

        assert!(ir
            .storage
            .renames
            .iter()
            .any(|rename| rename.renaming_item == "AB_ALIAS"));
        assert!(ir
            .storage
            .items
            .iter()
            .any(|item| item.qualified_name == "AB_ALIAS"));
        assert!(ir
            .storage
            .record_plan
            .fields
            .iter()
            .all(|field| field.qualified_name != "AB_ALIAS"));
    }

    #[test]
    fn non_condition_value_range_fails_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. BADVALUE.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FIELD PIC 9 VALUE 1 THRU 5.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ast = parse_program("bad-value.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_DATA_CLAUSE"
                && diagnostic.message.contains("VALUE range")
        }));
    }

    #[test]
    fn repeated_fillers_are_storage_not_symbols_and_values_keep_literals() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FILLERS.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 FILLER PIC X VALUE SPACES.
   05 FILLER PIC X(11) VALUE "HELLO WORLD".
   05 WS-NAME PIC X(3).
PROCEDURE DIVISION.
MAIN.
DISPLAY FILLER.
STOP RUN.
"#;
        let ast = parse_program("fillers.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        assert!(
            ir.storage
                .items
                .iter()
                .filter(|item| !item.addressable)
                .count()
                >= 2
        );
        assert!(ir
            .storage
            .items
            .iter()
            .any(|item| item.value.as_deref() == Some("HELLO WORLD")));
        assert!(ir
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "E_DUPLICATE_SYMBOL"));
    }

    #[test]
    fn initial_value_uses_typed_value_clause_with_optional_is() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. VALUEIS.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FLAG PIC X VALUE IS "Y".
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("value-is.cbl", src);
        let item = ir
            .data_items
            .iter()
            .find(|item| item.name == "WS_FLAG")
            .expect("WS-FLAG item");
        assert_eq!(item.value.as_deref(), Some("Y"));
    }

    #[test]
    fn picture_clause_uses_typed_ast_with_optional_is() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. PICIS.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-NAME PICTURE IS X(3).
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("pic-is.cbl", src);
        let item = ir
            .data_items
            .iter()
            .find(|item| item.name == "WS_NAME")
            .expect("WS-NAME item");
        assert_eq!(item.picture.as_deref(), Some("X(3)"));
        assert_eq!(item.byte_len, Some(3));
        assert_eq!(item.value_category, ValueCategoryIr::Alphanumeric);
    }

    #[test]
    fn usage_classification_ignores_keywords_inside_value_literals() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. USAGELIT.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TEXT PIC X(6) VALUE "COMP-3".
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("usage-literal.cbl", src);
        let item = ir
            .data_items
            .iter()
            .find(|item| item.name == "WS_TEXT")
            .expect("WS-TEXT item");
        assert_eq!(item.usage, UsageIr::Alphanumeric);
        assert_eq!(item.value_category, ValueCategoryIr::Alphanumeric);
        assert_eq!(item.byte_len, Some(6));
        assert_eq!(item.value.as_deref(), Some("COMP-3"));
    }

    #[test]
    fn data_clause_diagnostics_ignore_keywords_inside_value_literals() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. LITDIAG.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TEXT PIC X(4) VALUE "SIGN".
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("literal-diagnostic.cbl", src);
        assert!(ir.diagnostics.iter().all(|diagnostic| {
            !(diagnostic.code == "E_UNSUPPORTED_DATA_CLAUSE" && diagnostic.message.contains("SIGN"))
        }));
        let item = ir
            .data_items
            .iter()
            .find(|item| item.name == "WS_TEXT")
            .expect("WS-TEXT item");
        assert_eq!(item.value.as_deref(), Some("SIGN"));
    }

    #[test]
    fn typed_sign_clause_remains_fail_closed_diagnostic() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SIGNDIAG.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-NUM PIC S9(4) SIGN IS LEADING SEPARATE.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("sign-diagnostic.cbl", src);
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_DATA_CLAUSE" && diagnostic.message.contains("SIGN")
        }));
    }

    #[test]
    fn typed_justified_clause_remains_fail_closed_diagnostic() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. JUSTDIAG.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TEXT PIC X(4) JUSTIFIED RIGHT.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("justified-diagnostic.cbl", src);
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_DATA_CLAUSE"
                && diagnostic.message.contains("JUSTIFIED")
        }));
    }

    #[test]
    fn typed_blank_when_zero_clause_remains_fail_closed_diagnostic() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. BLANKDIAG.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-NUM PIC 9(4) BLANK WHEN ZERO.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("blank-diagnostic.cbl", src);
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_DATA_CLAUSE" && diagnostic.message.contains("BLANK")
        }));
    }

    #[test]
    fn unsupported_data_clause_references_remain_fail_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. UNSUPCLAUSEREF.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-SIGNED PIC S9(4) SIGN IS LEADING SEPARATE.
01 WS-JUST PIC X(4) JUSTIFIED RIGHT.
01 WS-BLANK PIC 9(4) BLANK WHEN ZERO.
PROCEDURE DIVISION.
MAIN.
DISPLAY WS-SIGNED WS-JUST WS-BLANK.
STOP RUN.
"#;
        let ir = analyze_src("unsupported-clause-reference.cbl", src);

        for raw in ["WS-SIGNED", "WS-JUST", "WS-BLANK"] {
            let reference = ir
                .semantic
                .references
                .iter()
                .find(|reference| {
                    reference.raw == raw && reference.role == ReferenceRoleIr::Display
                })
                .expect("expected DISPLAY reference");
            assert_eq!(reference.category, Some(ValueCategoryIr::Unsupported));
            assert_eq!(
                reference.status,
                ReferenceResolutionStatusIr::UnsupportedCategory
            );
            assert!(ir.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "E_CODEGEN_FIELD_CODEC"
                    && diagnostic
                        .message
                        .contains(reference.target.as_deref().unwrap_or(raw))
                    && diagnostic.message.contains("Unsupported")
            }));
        }
    }

    #[test]
    fn typed_global_clause_remains_fail_closed_diagnostic() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. GLOBALDIAG.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FLAG PIC X GLOBAL.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("global-diagnostic.cbl", src);
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_DATA_CLAUSE" && diagnostic.message.contains("GLOBAL")
        }));
    }

    #[test]
    fn typed_based_clause_remains_fail_closed_diagnostic() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. BASEDDIAG.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-PTR USAGE IS POINTER.
01 WS-BASED PIC X BASED ON WS-PTR.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("based-diagnostic.cbl", src);
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_DATA_CLAUSE" && diagnostic.message.contains("BASED")
        }));
    }

    #[test]
    fn typed_any_length_clause_remains_fail_closed_diagnostic() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. ANYLENDIAG.
DATA DIVISION.
LINKAGE SECTION.
01 LK-TEXT PIC X ANY LENGTH.
PROCEDURE DIVISION USING LK-TEXT.
MAIN.
GOBACK.
"#;
        let ir = analyze_src("any-length-diagnostic.cbl", src);
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_DATA_CLAUSE"
                && diagnostic.message.contains("ANY LENGTH")
        }));
    }

    #[test]
    fn national_dbcs_display_references_remain_fail_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. NATDBCSREF.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC.
   05 NAT-TEXT PIC N(4).
   05 DBCS-TEXT PIC G(4).
PROCEDURE DIVISION.
MAIN.
DISPLAY NAT-TEXT DBCS-TEXT.
STOP RUN.
"#;
        let ir = analyze_src("national-dbcs-display.cbl", src);

        for (raw, target, category) in [
            ("NAT-TEXT", "REC.NAT_TEXT", ValueCategoryIr::National),
            ("DBCS-TEXT", "REC.DBCS_TEXT", ValueCategoryIr::Dbcs),
        ] {
            assert!(ir.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "E_UNSUPPORTED_NATIONAL_DBCS"
                    && diagnostic.message.contains(&format!("{category:?}"))
            }));
            assert!(ir.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "E_CODEGEN_FIELD_CODEC"
                    && diagnostic.message.contains(target)
                    && diagnostic.message.contains(&format!("{category:?}"))
            }));

            let reference = ir
                .semantic
                .references
                .iter()
                .find(|reference| {
                    reference.raw == raw && reference.role == ReferenceRoleIr::Display
                })
                .expect("expected DISPLAY reference");
            assert_eq!(reference.target.as_deref(), Some(target));
            assert_eq!(reference.category, Some(category));
            assert_eq!(
                reference.status,
                ReferenceResolutionStatusIr::UnsupportedCategory
            );
        }
    }

    #[test]
    fn national_dbcs_call_using_arguments_remain_fail_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. NATDBCSARG.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 REC.
   05 NAT-TEXT PIC N(4).
   05 DBCS-TEXT PIC G(4).
PROCEDURE DIVISION.
MAIN.
CALL "NATDBCSARG" USING NAT-TEXT DBCS-TEXT.
STOP RUN.
"#;
        let ir = analyze_src("national-dbcs-call-using.cbl", src);

        for (raw, target, category) in [
            ("NAT-TEXT", "REC.NAT_TEXT", ValueCategoryIr::National),
            ("DBCS-TEXT", "REC.DBCS_TEXT", ValueCategoryIr::Dbcs),
        ] {
            assert!(ir.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "E_UNSUPPORTED_NATIONAL_DBCS"
                    && diagnostic.message.contains(&format!("{category:?}"))
            }));
            assert!(ir.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "E_CODEGEN_FIELD_CODEC"
                    && diagnostic.message.contains(target)
                    && diagnostic.message.contains(&format!("{category:?}"))
            }));

            let reference = ir
                .semantic
                .references
                .iter()
                .find(|reference| {
                    reference.raw == raw && reference.role == ReferenceRoleIr::ProcedureArgument
                })
                .expect("expected CALL USING reference");
            assert_eq!(reference.target.as_deref(), Some(target));
            assert_eq!(reference.category, Some(category));
            assert_eq!(
                reference.status,
                ReferenceResolutionStatusIr::UnsupportedCategory
            );
        }
    }

    #[test]
    fn pointer_usage_clause_remains_fail_closed_diagnostic() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. PTRDIAG.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-PTR USAGE IS POINTER.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("pointer-diagnostic.cbl", src);
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_DATA_CLAUSE" && diagnostic.message.contains("POINTER")
        }));
    }

    #[test]
    fn data_storage_flags_ignore_keywords_inside_value_literals() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FLAGLIT.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-EXTERNAL PIC X(8) VALUE "EXTERNAL".
01 WS-SYNC PIC X(12) VALUE "SYNCHRONIZED".
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("flag-literal.cbl", src);
        let external = ir
            .data_items
            .iter()
            .find(|item| item.name == "WS_EXTERNAL")
            .expect("WS-EXTERNAL item");
        assert!(!external.external);
        assert!(!external.sync);
        assert_eq!(external.value.as_deref(), Some("EXTERNAL"));

        let sync = ir
            .data_items
            .iter()
            .find(|item| item.name == "WS_SYNC")
            .expect("WS-SYNC item");
        assert!(!sync.external);
        assert!(!sync.sync);
        assert_eq!(sync.value.as_deref(), Some("SYNCHRONIZED"));
    }

    #[test]
    fn group_classification_ignores_keywords_inside_value_literals() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. GROUPLIT.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-GROUP VALUE "PIC".
   05 WS-CHILD PIC X.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("group-literal.cbl", src);
        let group = ir
            .data_items
            .iter()
            .find(|item| item.name == "WS_GROUP")
            .expect("WS-GROUP item");
        assert_eq!(group.usage, UsageIr::Group);
        assert_eq!(group.value_category, ValueCategoryIr::Group);
        assert_eq!(group.byte_len, Some(1));
    }

    #[test]
    fn data_refs_preserve_qualification_subscripts_and_reference_modification() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. REFS.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-TABLE OCCURS 3 TIMES.
      10 WS-ITEM PIC X(4).
   05 WS-TEXT PIC X(8).
PROCEDURE DIVISION.
MAIN.
MOVE WS-TEXT(1:3) TO WS-ITEM(2) OF WS-TABLE.
STOP RUN.
"#;
        let ast = parse_program("refs.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let StatementIr::Move { source, target } = &ir.paragraphs[0].statements[0] else {
            panic!("expected MOVE");
        };
        let OperandIr::Identifier(source) = source else {
            panic!("expected identifier source");
        };
        assert_eq!(source.normalized, "WS_TEXT");
        assert!(source.has_reference_modifier());
        assert_eq!(target.normalized, "WS_TABLE.WS_ITEM");
        assert_eq!(target.subscripts, vec!["2"]);
    }

    #[test]
    fn data_refs_order_nested_qualified_occurs_subscripts_outer_to_inner() {
        let reference = parse_data_ref("WS-CELL(3) OF WS-ROW(2)");

        assert_eq!(reference.normalized, "WS_ROW.WS_CELL");
        assert_eq!(reference.subscripts, vec!["2", "3"]);
    }

    #[test]
    fn semantic_refs_resolve_deep_qualification_and_reject_ambiguous_simple_names() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. QUALS.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 A-REC.
   05 B-REC.
      10 LEAF PIC X.
01 C-REC.
   05 LEAF PIC X.
PROCEDURE DIVISION.
MAIN.
DISPLAY LEAF OF B-REC OF A-REC.
DISPLAY LEAF.
STOP RUN.
"#;
        let ir = analyze_src("quals.cbl", src);
        assert!(ir.semantic.references.iter().any(|reference| {
            reference.raw == "LEAF OF B-REC OF A-REC"
                && reference.target.as_deref() == Some("A_REC.B_REC.LEAF")
                && reference.status == ReferenceResolutionStatusIr::Resolved
        }));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_AMBIGUOUS_DATA"
                && diagnostic.message.contains("A_REC.B_REC.LEAF")
                && diagnostic.message.contains("C_REC.LEAF")
        }));
    }

    #[test]
    fn control_flow_rejects_empty_goto_and_reversed_perform_thru() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FLOWFAIL.
PROCEDURE DIVISION.
MAIN.
GO TO .
PERFORM C-PARA THRU B-PARA.
ALTER A-PARA TO PROCEED TO B-PARA.
STOP RUN.
A-PARA.
STOP RUN.
B-PARA.
STOP RUN.
C-PARA.
STOP RUN.
"#;
        let ir = analyze_src("flowfail.cbl", src);
        assert!(ir
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E_INVALID_GO_TO_TARGET"));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_PERFORM_THRU_RANGE"
                && diagnostic.message.contains("C_PARA THRU B_PARA")
        }));
        assert!(!ir
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E_UNSUPPORTED_ALTER"));
    }

    #[test]
    fn control_flow_allows_altered_goto_dot_slot() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. ALTEROK.
PROCEDURE DIVISION.
MAIN.
ALTER DIVE-IN TO PROCEED TO END-WORLD.
GO TO DIVE-IN.
DIVE-IN.
GO TO .
END-WORLD.
STOP RUN.
"#;
        let ir = analyze_src("alterok.cbl", src);
        assert!(!ir
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E_INVALID_GO_TO_TARGET"));
        assert!(!ir
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E_UNSUPPORTED_ALTER"));
    }

    #[test]
    fn control_flow_reports_unreachable_paragraphs() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. UNREACH.
PROCEDURE DIVISION.
MAIN.
GO TO EXIT-PARA.
DEAD-PARA.
DISPLAY "DEAD".
EXIT-PARA.
STOP RUN.
"#;
        let ir = analyze_src("unreach.cbl", src);
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "W_UNREACHABLE_PARAGRAPH" && diagnostic.message.contains("DEAD_PARA")
        }));
        assert!(!ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "W_UNREACHABLE_PARAGRAPH" && diagnostic.message.contains("EXIT_PARA")
        }));
    }

    #[test]
    fn control_flow_tracks_goto_inside_start_branches() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. STARTFLOW.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT INFILE ASSIGN TO "INFILE".
DATA DIVISION.
FILE SECTION.
FD INFILE.
01 IN-REC PIC X.
WORKING-STORAGE SECTION.
01 WS-ID PIC X.
PROCEDURE DIVISION.
MAIN.
START INFILE KEY IS EQUAL TO WS-ID
    INVALID KEY GO TO HIT-PARA
END-START.
STOP RUN.
DEAD-PARA.
STOP RUN.
HIT-PARA.
STOP RUN.
"#;
        let ir = analyze_src("start-flow.cbl", src);
        let StatementIr::StartFile(start) = &ir.paragraphs[0].statements[0] else {
            panic!("expected START");
        };
        assert!(matches!(
            start.invalid_key_ops.first(),
            Some(StatementIr::GoTo(target)) if target == "HIT_PARA"
        ));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "W_UNREACHABLE_PARAGRAPH" && diagnostic.message.contains("DEAD_PARA")
        }));
        assert!(!ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "W_UNREACHABLE_PARAGRAPH" && diagnostic.message.contains("HIT_PARA")
        }));
    }

    #[test]
    fn control_flow_reports_unreachable_statements_after_terminal_transfer() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. DEADSTMT.
PROCEDURE DIVISION.
MAIN.
DISPLAY "LIVE".
GO TO EXIT-PARA.
DISPLAY "DEAD".
EXIT-PARA.
STOP RUN.
"#;
        let ir = analyze_src("deadstmt.cbl", src);
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "W_UNREACHABLE_STATEMENT"
                && diagnostic.message.contains("paragraph MAIN")
        }));
    }

    #[test]
    fn control_flow_rejects_perform_thru_that_escapes_range() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. THRUESC.
PROCEDURE DIVISION.
MAIN.
PERFORM A-PARA THRU B-PARA.
STOP RUN.
A-PARA.
GO TO OUTSIDE-PARA.
B-PARA.
DISPLAY "B".
OUTSIDE-PARA.
STOP RUN.
"#;
        let ir = analyze_src("thruesc.cbl", src);
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_PERFORM_THRU_ESCAPES"
                && diagnostic.message.contains("A_PARA THRU B_PARA")
                && diagnostic.message.contains("GO TO OUTSIDE_PARA")
        }));
    }

    #[test]
    fn computed_goto_depending_on_requires_numeric_selector() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. GOTODEPNUM.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TEXT PIC X.
PROCEDURE DIVISION.
MAIN.
GO TO PATH-A PATH-B DEPENDING ON WS-TEXT.
STOP RUN.
PATH-A.
STOP RUN.
PATH-B.
STOP RUN.
"#;
        let ir = analyze_src("computed-goto-nonnumeric.cbl", src);

        assert!(ir.semantic.references.iter().any(|reference| {
            reference.raw == "WS-TEXT"
                && reference.role == ReferenceRoleIr::ArithmeticSource
                && reference.status == ReferenceResolutionStatusIr::UnsupportedCategory
        }));
        assert!(has_diagnostic(&ir, "E_INVALID_GO_TO_DEPENDING"));
    }

    #[test]
    fn filler_reference_is_unresolved_because_fillers_are_not_symbols() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FILLREF.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 FILLER PIC X.
   05 WS-NAME PIC X.
PROCEDURE DIVISION.
MAIN.
DISPLAY FILLER.
STOP RUN.
"#;
        let ir = analyze_src("fillref.cbl", src);
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNRESOLVED_DATA" && diagnostic.message.contains("FILLER")
        }));
    }

    #[test]
    fn subscript_semantics_validate_counts_literals_and_data_name_categories() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SUBS.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-TABLE OCCURS 3 TIMES.
      10 WS-ITEM PIC X.
   05 WS-NONTABLE PIC X.
   05 WS-NUM-IDX PIC 9.
   05 WS-TEXT-IDX PIC X.
PROCEDURE DIVISION.
MAIN.
DISPLAY WS-NONTABLE(1).
DISPLAY WS-ITEM(0) OF WS-TABLE.
DISPLAY WS-ITEM(4) OF WS-TABLE.
DISPLAY WS-ITEM(WS-NUM-IDX) OF WS-TABLE.
DISPLAY WS-ITEM(WS-TEXT-IDX) OF WS-TABLE.
STOP RUN.
"#;
        let ir = analyze_src("subs.cbl", src);
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_SUBSCRIPT"
                && diagnostic.message.contains("WS-NONTABLE")
                && diagnostic.message.contains("not in OCCURS")
        }));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_SUBSCRIPT" && diagnostic.message.contains("subscript 0")
        }));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_SUBSCRIPT"
                && diagnostic.message.contains("subscript 4")
                && diagnostic.message.contains("1..=3")
        }));
        assert!(!ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_SUBSCRIPT" && diagnostic.message.contains("WS-NUM-IDX")
        }));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_SUBSCRIPT"
                && diagnostic.message.contains("WS-TEXT-IDX")
                && diagnostic.message.contains("not numeric")
        }));
    }

    #[test]
    fn subscript_semantics_report_missing_subscripts_and_accept_index_names() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. INDEXSUB.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-TABLE OCCURS 4 TIMES INDEXED BY WS-IDX.
      10 WS-ITEM PIC X.
PROCEDURE DIVISION.
MAIN.
DISPLAY WS-ITEM OF WS-TABLE.
DISPLAY WS-ITEM(WS-IDX) OF WS-TABLE.
STOP RUN.
"#;
        let ir = analyze_src("indexsub.cbl", src);
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_MISSING_SUBSCRIPT"
                && diagnostic.message.contains("WS-ITEM OF WS-TABLE")
                && diagnostic.message.contains("requires 1 subscript")
        }));
        assert!(!ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNRESOLVED_DATA" && diagnostic.message.contains("WS-IDX")
        }));
        assert!(!ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_SUBSCRIPT" && diagnostic.message.contains("WS-IDX")
        }));
    }

    #[test]
    fn subscript_semantics_reject_fractional_and_negative_literals() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. BADSUBLETS.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-TABLE OCCURS 4 TIMES.
      10 WS-ITEM PIC X.
PROCEDURE DIVISION.
MAIN.
DISPLAY WS-ITEM(-1) OF WS-TABLE.
DISPLAY WS-ITEM(1.5) OF WS-TABLE.
STOP RUN.
"#;
        let ir = analyze_src("badsublets.cbl", src);
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_SUBSCRIPT"
                && diagnostic.message.contains("-1")
                && diagnostic.message.contains("positive")
        }));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_SUBSCRIPT"
                && diagnostic.message.contains("1.5")
                && diagnostic.message.contains("integer")
        }));
    }

    #[test]
    fn subscript_expression_redefines_source_remains_fail_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SUBREDEF.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-IDX-BASE PIC 9 VALUE 1.
   05 WS-IDX-ALT REDEFINES WS-IDX-BASE PIC X.
01 WS-TABLE OCCURS 3 TIMES PIC X.
PROCEDURE DIVISION.
MAIN.
DISPLAY WS-TABLE(WS-IDX-BASE).
STOP RUN.
"#;
        let ir = analyze_src("subscript-redefines-source.cbl", src);
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_CODEGEN_REDEFINES_REFERENCE"
                && diagnostic.message.contains("WS_REC.WS_IDX_BASE")
                && diagnostic.message.contains("subscript")
        }));
    }

    #[test]
    fn subscript_expression_odo_source_remains_fail_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SUBODO.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-COUNT PIC 9 VALUE 1.
01 WS-IDX-TABLE OCCURS 0 TO 3 TIMES DEPENDING ON WS-COUNT PIC 9 VALUE 1.
01 WS-TABLE OCCURS 3 TIMES PIC X.
PROCEDURE DIVISION.
MAIN.
DISPLAY WS-TABLE(WS-IDX-TABLE).
STOP RUN.
"#;
        let ir = analyze_src("subscript-odo-source.cbl", src);
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_CODEGEN_ODO_REFERENCE"
                && diagnostic.message.contains("WS_IDX_TABLE")
                && diagnostic.message.contains("subscript")
        }));
    }

    #[test]
    fn condition_name_subscript_is_invalid_but_condition_context_resolves() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CONDSUB.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-FLAG PIC X.
      88 WS-YES VALUE "Y".
   05 WS-TABLE OCCURS 2 TIMES.
      10 WS-ITEM PIC X.
PROCEDURE DIVISION.
MAIN.
DISPLAY WS-ITEM(WS-YES) OF WS-TABLE.
IF WS-YES DISPLAY "Y" END-IF.
STOP RUN.
"#;
        let ir = analyze_src("condsub.cbl", src);
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_SUBSCRIPT"
                && diagnostic.message.contains("WS-YES")
                && diagnostic.message.contains("condition-name")
        }));
        assert!(ir.semantic.references.iter().any(|reference| {
            reference.raw == "WS-YES"
                && reference.role == ReferenceRoleIr::ConditionOperand
                && reference.category == Some(ValueCategoryIr::ConditionName)
                && reference.status == ReferenceResolutionStatusIr::Resolved
        }));
    }

    #[test]
    fn condition_name_inside_occurs_requires_parent_subscript() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CONDOCC.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-TABLE OCCURS 2 TIMES.
      10 WS-FLAG PIC X.
         88 WS-YES VALUE "Y".
PROCEDURE DIVISION.
MAIN.
IF WS-YES DISPLAY "Y" END-IF.
STOP RUN.
"#;
        let ir = analyze_src("condition-occurs.cbl", src);

        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_MISSING_SUBSCRIPT"
                && diagnostic.message.contains("WS-YES")
                && diagnostic.message.contains("WS_REC.WS_TABLE.WS_FLAG")
                && diagnostic.message.contains("requires 1 subscript")
        }));
        assert!(ir.semantic.references.iter().any(|reference| {
            reference.raw == "WS-YES"
                && reference.role == ReferenceRoleIr::ConditionOperand
                && reference.category == Some(ValueCategoryIr::ConditionName)
                && reference.target.as_deref() == Some("WS_REC.WS_TABLE.WS_FLAG.WS_YES")
                && reference.status == ReferenceResolutionStatusIr::InvalidSubscript
        }));
    }

    #[test]
    fn condition_name_inside_occurs_accepts_full_parent_subscript() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CONDOCCOK.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-TABLE OCCURS 2 TIMES.
      10 WS-FLAG PIC X.
         88 WS-YES VALUE "Y".
PROCEDURE DIVISION.
MAIN.
IF WS-YES(1) DISPLAY "Y" END-IF.
STOP RUN.
"#;
        let ir = analyze_src("condition-occurs-ok.cbl", src);

        assert!(!ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_MISSING_SUBSCRIPT" && diagnostic.message.contains("WS-YES(1)")
        }));
        assert!(ir.semantic.references.iter().any(|reference| {
            reference.raw == "WS-YES(1)"
                && reference.role == ReferenceRoleIr::ConditionOperand
                && reference.category == Some(ValueCategoryIr::ConditionName)
                && reference.target.as_deref() == Some("WS_REC.WS_TABLE.WS_FLAG.WS_YES")
                && reference.status == ReferenceResolutionStatusIr::Resolved
        }));
    }

    #[test]
    fn set_condition_redefines_parent_remains_fail_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SETCONDRED.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-FLAG PIC X.
      88 WS-YES VALUE "Y".
   05 WS-ALT REDEFINES WS-FLAG PIC X.
PROCEDURE DIVISION.
MAIN.
SET WS-YES TO TRUE.
STOP RUN.
"#;
        let ir = analyze_src("set-condition-redefines.cbl", src);

        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_CODEGEN_REDEFINES_REFERENCE"
                && diagnostic.message.contains("SET condition-name")
                && diagnostic.message.contains("WS_REC.WS_FLAG")
        }));
    }

    #[test]
    fn set_condition_odo_parent_remains_fail_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SETCONDODO.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-COUNT PIC 9.
01 WS-TABLE OCCURS 0 TO 3 TIMES DEPENDING ON WS-COUNT.
   05 WS-FLAG PIC X.
      88 WS-YES VALUE "Y".
PROCEDURE DIVISION.
MAIN.
SET WS-YES(1) TO TRUE.
STOP RUN.
"#;
        let ir = analyze_src("set-condition-odo.cbl", src);

        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_CODEGEN_ODO_REFERENCE"
                && diagnostic.message.contains("SET condition-name")
                && diagnostic.message.contains("WS_TABLE.WS_FLAG")
                && diagnostic.message.contains("OCCURS DEPENDING ON")
        }));
    }

    #[test]
    fn reference_modification_semantics_validate_category_and_literal_bounds() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. REFMOD.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TEXT PIC X(5).
01 WS-PACKED PIC 9(3) COMP-3.
PROCEDURE DIVISION.
MAIN.
DISPLAY WS-TEXT(2:3).
DISPLAY WS-TEXT(0:1).
DISPLAY WS-TEXT(2:0).
DISPLAY WS-TEXT(2:5).
DISPLAY WS-PACKED(1:1).
STOP RUN.
"#;
        let ir = analyze_src("refmod.cbl", src);
        assert!(!ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_CODEGEN_REFERENCE_MODIFICATION"
                && diagnostic.message.contains("WS-TEXT(2:3)")
        }));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_REFERENCE_MODIFICATION"
                && diagnostic.message.contains("start")
                && diagnostic.message.contains("WS-TEXT(0:1)")
        }));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_REFERENCE_MODIFICATION"
                && diagnostic.message.contains("length")
                && diagnostic.message.contains("WS-TEXT(2:0)")
        }));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_REFERENCE_MODIFICATION"
                && diagnostic.message.contains("outside")
                && diagnostic.message.contains("WS-TEXT(2:5)")
        }));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_REFERENCE_MODIFICATION"
                && diagnostic.message.contains("WS-PACKED")
                && diagnostic.message.contains("PackedDecimal")
        }));
    }

    #[test]
    fn reference_modification_without_length_uses_item_tail_bounds() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. REFMODTAIL.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TEXT PIC X(5).
PROCEDURE DIVISION.
MAIN.
DISPLAY WS-TEXT(5:).
DISPLAY WS-TEXT(6:).
STOP RUN.
"#;
        let ir = analyze_src("refmodtail.cbl", src);
        assert!(!ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_REFERENCE_MODIFICATION"
                && diagnostic.message.contains("WS-TEXT(5:)")
        }));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_REFERENCE_MODIFICATION"
                && diagnostic.message.contains("WS-TEXT(6:)")
                && diagnostic.message.contains("outside")
        }));
    }

    #[test]
    fn reference_modification_on_dynamic_group_remains_fail_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. REFMODODO.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-COUNT PIC 9.
   05 WS-TABLE OCCURS 0 TO 4 TIMES DEPENDING ON WS-COUNT.
      10 WS-ITEM PIC X.
PROCEDURE DIVISION.
MAIN.
DISPLAY WS-REC(1:2).
STOP RUN.
"#;
        let ir = analyze_src("refmododo.cbl", src);
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_CODEGEN_REFERENCE_MODIFICATION"
                && diagnostic.message.contains("OCCURS DEPENDING ON")
        }));
    }

    #[test]
    fn odo_reference_without_checked_subscript_remains_fail_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. ODOREF.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-COUNT PIC 9.
   05 WS-TABLE OCCURS 0 TO 3 TIMES DEPENDING ON WS-COUNT.
      10 WS-ITEM PIC X.
PROCEDURE DIVISION.
MAIN.
DISPLAY WS-ITEM OF WS-TABLE.
STOP RUN.
"#;
        let ir = analyze_src("odoref.cbl", src);
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_CODEGEN_ODO_REFERENCE"
                && diagnostic.message.contains("OCCURS DEPENDING ON")
        }));
    }

    #[test]
    fn subscripted_odo_display_reference_is_enabled_when_fully_subscripted() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. ODODISPLAY.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-COUNT PIC 9.
   05 WS-TABLE OCCURS 0 TO 3 TIMES DEPENDING ON WS-COUNT.
      10 WS-ITEM PIC X.
PROCEDURE DIVISION.
MAIN.
DISPLAY WS-ITEM(1) OF WS-TABLE.
STOP RUN.
"#;
        let ir = analyze_src("odo-display.cbl", src);
        let reference = ir
            .semantic
            .resolved_data_refs
            .iter()
            .find(|reference| reference.raw == "WS-ITEM(1) OF WS-TABLE")
            .expect("resolved display reference");
        assert!(
            reference.in_odo,
            "resolved reference should retain ODO context: {reference:?}"
        );
        assert_eq!(reference.status, ReferenceResolutionStatusIr::Resolved);
        assert!(!ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_CODEGEN_ODO_REFERENCE"
                && diagnostic.message.contains("WS_REC.WS_TABLE.WS_ITEM")
        }));
    }

    #[test]
    fn start_key_odo_reference_remains_fail_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. ODOSTART.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT INFILE ASSIGN TO "INFILE".
DATA DIVISION.
FILE SECTION.
FD INFILE.
01 IN-REC PIC X.
WORKING-STORAGE SECTION.
01 WS-COUNT PIC 9.
01 WS-TABLE OCCURS 0 TO 3 TIMES DEPENDING ON WS-COUNT.
   05 WS-ITEM PIC X.
PROCEDURE DIVISION.
MAIN.
START INFILE KEY IS EQUAL TO WS-ITEM(1) OF WS-TABLE.
STOP RUN.
"#;
        let ir = analyze_src("odo-start.cbl", src);
        assert!(ir.semantic.references.iter().any(|reference| {
            reference.raw == "WS-ITEM(1) OF WS-TABLE"
                && reference.role == ReferenceRoleIr::Source
                && reference.status == ReferenceResolutionStatusIr::UnsupportedDynamic
        }));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_CODEGEN_ODO_REFERENCE"
                && diagnostic.message.contains("WS_TABLE.WS_ITEM")
                && diagnostic.message.contains("OCCURS DEPENDING ON")
        }));
    }

    #[test]
    fn subscripted_odo_call_using_argument_remains_fail_closed() {
        let callee = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLEE.
DATA DIVISION.
LINKAGE SECTION.
01 LK-ITEM PIC X.
PROCEDURE DIVISION USING LK-ITEM.
MAIN.
GOBACK.
"#;
        let caller = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLER.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-COUNT PIC 9.
   05 WS-TABLE OCCURS 0 TO 3 TIMES DEPENDING ON WS-COUNT.
      10 WS-ITEM PIC X.
PROCEDURE DIVISION.
MAIN.
CALL "CALLEE" USING WS-ITEM(1) OF WS-TABLE.
STOP RUN.
"#;
        let callee_ast = parse_program("callee.cbl", callee).expect("callee parses");
        let caller_ast = parse_program("caller.cbl", caller).expect("caller parses");
        let catalog = ProgramCatalog::from_asts(std::slice::from_ref(&callee_ast));
        let ir = analyze_with_catalog(caller_ast, Dialect::Ibm, &catalog);

        assert!(ir.semantic.references.iter().any(|reference| {
            reference.raw == "WS-ITEM(1) OF WS-TABLE"
                && reference.role == ReferenceRoleIr::ProcedureArgument
                && reference.status == ReferenceResolutionStatusIr::UnsupportedDynamic
        }));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_CODEGEN_ODO_REFERENCE"
                && diagnostic.message.contains("WS_REC.WS_TABLE.WS_ITEM")
                && diagnostic.message.contains("OCCURS DEPENDING ON")
        }));
    }

    #[test]
    fn semantic_model_resolves_condition_name_parent_and_blocks_display_storage_role() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. COND.
DATA DIVISION.
LINKAGE SECTION.
01 LK-REC.
   05 LK-FLAG PIC X.
      88 LK-FLAG-YES VALUE "Y".
PROCEDURE DIVISION.
MAIN.
IF LK-FLAG-YES DISPLAY "Y" END-IF.
STOP RUN.
"#;
        let ast = parse_program("cond.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let flag = ir
            .storage
            .items
            .iter()
            .find(|item| item.qualified_name == "LK_REC.LK_FLAG")
            .expect("flag");
        assert_eq!(flag.storage_area, StorageAreaIr::Linkage);
        let condition = ir
            .storage
            .condition_names
            .iter()
            .find(|condition| condition.name == "LK_FLAG_YES")
            .expect("condition name");
        assert_eq!(condition.parent, "LK_REC.LK_FLAG");
        assert!(ir.semantic.references.iter().any(|reference| {
            reference.raw == "LK-FLAG-YES"
                && reference.role == ReferenceRoleIr::ConditionOperand
                && reference.status == ReferenceResolutionStatusIr::Resolved
                && reference.category == Some(ValueCategoryIr::ConditionName)
        }));
    }

    #[test]
    fn qualified_condition_name_reference_records_parent_scoped_target() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. QUALCOND.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 A-REC.
   05 A-FLAG PIC X.
      88 READY VALUE "Y".
01 B-REC.
   05 B-FLAG PIC X.
      88 READY VALUE "Y".
PROCEDURE DIVISION.
MAIN.
IF READY OF A-FLAG OF A-REC DISPLAY "A" END-IF.
STOP RUN.
"#;
        let ir = analyze_src("qualified-condition.cbl", src);
        let reference = ir
            .semantic
            .references
            .iter()
            .find(|reference| reference.raw == "READY OF A-FLAG OF A-REC")
            .expect("qualified condition reference");

        assert_eq!(reference.status, ReferenceResolutionStatusIr::Resolved);
        assert_eq!(reference.category, Some(ValueCategoryIr::ConditionName));
        assert_eq!(reference.target.as_deref(), Some("A_REC.A_FLAG.READY"));

        let resolved = ir
            .semantic
            .resolved_data_refs
            .iter()
            .find(|reference| reference.raw == "READY OF A-FLAG OF A-REC")
            .expect("resolved condition reference");
        assert_eq!(
            resolved.condition_name_target.as_deref(),
            Some("A_REC.A_FLAG.READY")
        );
    }

    #[test]
    fn storage_area_layout_offsets_do_not_bleed_between_sections() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. AREAS.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-FIELD PIC X(2) VALUE "AB".
LINKAGE SECTION.
01 LK-REC.
   05 LK-FIELD PIC X(3).
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ast = parse_program("areas.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let ws = ir
            .storage
            .items
            .iter()
            .find(|item| item.qualified_name == "WS_REC")
            .expect("working-storage record");
        let linkage = ir
            .storage
            .items
            .iter()
            .find(|item| item.qualified_name == "LK_REC")
            .expect("linkage record");

        assert_eq!(ws.offset, 0);
        assert_eq!(linkage.offset, 0);
    }

    #[test]
    fn semantic_model_rejects_ambiguous_references_before_codegen() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. AMBIG.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 A-REC.
   05 SHARED-FIELD PIC X.
01 B-REC.
   05 SHARED-FIELD PIC X.
PROCEDURE DIVISION.
MAIN.
DISPLAY SHARED-FIELD.
STOP RUN.
"#;
        let ast = parse_program("ambig.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        assert!(ir
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E_AMBIGUOUS_DATA"));
        assert!(ir.semantic.references.iter().any(|reference| {
            reference.raw == "SHARED-FIELD"
                && reference.status == ReferenceResolutionStatusIr::Ambiguous
                && reference.candidates.len() == 2
        }));
    }

    #[test]
    fn semantic_nested_typed_if_lowers_to_ir_child_statement_vectors() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. NESTIF.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FLAG PIC X.
01 WS-OUT PIC X.
PROCEDURE DIVISION.
MAIN.
IF WS-FLAG = "Y"
    IF WS-FLAG = "Z"
        MOVE "A" TO WS-OUT
    ELSE
        MOVE "B" TO WS-OUT
    END-IF
ELSE
    MOVE "C" TO WS-OUT
END-IF.
STOP RUN.
"#;
        let ir = analyze_src("nested-typed-if.cbl", src);
        let StatementIr::If {
            condition_tree,
            then_statements,
            else_statements,
            ..
        } = &ir.paragraphs[0].statements[0]
        else {
            panic!("expected outer IF");
        };
        assert!(condition_tree.is_some());
        assert_eq!(then_statements.len(), 1);
        assert_eq!(else_statements.len(), 1);
        assert!(matches!(
            &else_statements[..],
            [StatementIr::Move { target, .. }] if target.normalized == "WS_OUT"
        ));

        let StatementIr::If {
            condition_tree,
            then_statements: inner_then,
            else_statements: inner_else,
            ..
        } = &then_statements[0]
        else {
            panic!("expected nested IF");
        };
        assert!(condition_tree.is_some());
        assert!(matches!(
            &inner_then[..],
            [StatementIr::Move { target, .. }] if target.normalized == "WS_OUT"
        ));
        assert!(matches!(
            &inner_else[..],
            [StatementIr::Move { target, .. }] if target.normalized == "WS_OUT"
        ));
        assert!(!has_diagnostic(&ir, "E_UNRESOLVED_DATA"));
    }

    #[test]
    fn semantic_nested_if_condition_reports_unresolved_reference() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. NESTMISS.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FLAG PIC X.
01 WS-OUT PIC X.
PROCEDURE DIVISION.
MAIN.
IF WS-FLAG = "Y"
    IF MISSING-FIELD = "Z"
        MOVE "A" TO WS-OUT
    END-IF
END-IF.
STOP RUN.
"#;
        let ir = analyze_src("nested-if-missing.cbl", src);

        let reference = ir
            .semantic
            .references
            .iter()
            .find(|reference| reference.raw == "MISSING-FIELD")
            .expect("missing nested IF condition reference");

        assert_eq!(reference.status, ReferenceResolutionStatusIr::Missing);
        assert_eq!(reference.target, None);
        assert!(has_diagnostic(&ir, "E_UNRESOLVED_DATA"));
    }

    #[test]
    fn semantic_unresolved_reference_reports_missing_without_guessed_target() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. MISSREF.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-KNOWN PIC X.
PROCEDURE DIVISION.
MAIN.
DISPLAY MISSING-FIELD.
STOP RUN.
"#;
        let ir = analyze_src("missing-ref.cbl", src);
        let reference = ir
            .semantic
            .references
            .iter()
            .find(|reference| reference.raw == "MISSING-FIELD")
            .expect("missing reference");
        assert_eq!(reference.status, ReferenceResolutionStatusIr::Missing);
        assert_eq!(reference.target, None);
        assert!(reference.candidates.is_empty());
        assert!(has_diagnostic(&ir, "E_UNRESOLVED_DATA"));
    }

    #[test]
    fn semantic_qualified_reference_resolves_when_unambiguous() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. QUALREF.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 GROUP-A.
   05 ITEM PIC X.
01 GROUP-B.
   05 ITEM PIC X.
PROCEDURE DIVISION.
MAIN.
DISPLAY ITEM OF GROUP-B.
STOP RUN.
"#;
        let ir = analyze_src("qualified-ref.cbl", src);
        let reference = ir
            .semantic
            .references
            .iter()
            .find(|reference| reference.normalized == "GROUP_B.ITEM")
            .expect("qualified reference");
        assert_eq!(reference.status, ReferenceResolutionStatusIr::Resolved);
        assert_eq!(reference.target.as_deref(), Some("GROUP_B.ITEM"));
        assert!(reference.candidates.is_empty());
        assert!(!has_diagnostic(&ir, "E_AMBIGUOUS_DATA"));
    }

    #[test]
    fn semantic_ambiguous_unqualified_reference_fails_closed_without_target() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. AMBIGITEM.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 GROUP-A.
   05 ITEM PIC X.
01 GROUP-B.
   05 ITEM PIC X.
PROCEDURE DIVISION.
MAIN.
DISPLAY ITEM.
STOP RUN.
"#;
        let ir = analyze_src("ambiguous-item.cbl", src);
        let reference = ir
            .semantic
            .references
            .iter()
            .find(|reference| reference.normalized == "ITEM")
            .expect("ambiguous reference");
        assert_eq!(reference.status, ReferenceResolutionStatusIr::Ambiguous);
        assert_eq!(reference.target, None);
        assert_eq!(reference.candidates.len(), 2);
        assert!(has_diagnostic(&ir, "E_AMBIGUOUS_DATA"));
    }

    #[test]
    fn semantic_move_corresponding_ambiguous_child_names_remain_blocked() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. MOVECORR.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 SRC-GRP.
   05 ITEM PIC X.
   05 SRC-NEST.
      10 ITEM PIC X.
01 DST-GRP.
   05 ITEM PIC X.
PROCEDURE DIVISION.
MAIN.
MOVE CORRESPONDING SRC-GRP TO DST-GRP.
STOP RUN.
"#;
        let ir = analyze_src("move-corresponding-ambiguous.cbl", src);
        assert!(matches!(
            &ir.paragraphs[0].statements[0],
            StatementIr::MoveCorresponding { .. }
        ));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_MOVE_CORRESPONDING"
                && diagnostic.message.contains("ITEM is ambiguous")
        }));
    }

    #[test]
    fn move_corresponding_child_under_redefined_group_remains_blocked() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. MOVECORRREDEF.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 SRC-GRP.
   05 SRC-BASE.
      10 ITEM PIC X.
   05 SRC-ALT REDEFINES SRC-BASE.
      10 ALT-ITEM PIC X.
01 DST-GRP.
   05 ITEM PIC X.
PROCEDURE DIVISION.
MAIN.
MOVE CORRESPONDING SRC-GRP TO DST-GRP.
STOP RUN.
"#;
        let ir = analyze_src("move-corresponding-redefines-child.cbl", src);
        assert!(matches!(
            &ir.paragraphs[0].statements[0],
            StatementIr::MoveCorresponding { .. }
        ));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_MOVE_CORRESPONDING"
                && diagnostic.message.contains("ITEM")
                && diagnostic.message.contains("REDEFINES")
        }));
    }

    #[test]
    fn semantic_declarative_file_reference_unresolved_is_diagnostic() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. DECLMISS.
PROCEDURE DIVISION.
DECLARATIVES.
ERR-SEC SECTION.
USE AFTER ERROR ON MISSING-FILE.
DISPLAY MISSING-FIELD.
END DECLARATIVES.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("declarative-missing.cbl", src);
        assert!(matches!(
            ir.declaratives[0].trigger,
            DeclarativeTriggerIr::Unsupported { .. }
        ));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_SECTION"
                && diagnostic.message.contains("MISSING_FILE")
        }));
        assert!(ir.semantic.references.iter().any(|reference| {
            reference.raw == "MISSING-FIELD"
                && reference.status == ReferenceResolutionStatusIr::Missing
                && reference.target.is_none()
        }));
        assert!(has_diagnostic(&ir, "E_UNRESOLVED_DATA"));
    }

    #[test]
    fn semantic_model_reports_move_category_mismatch() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. MOVECAT.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-NUM PIC 9(3).
PROCEDURE DIVISION.
MAIN.
MOVE "ABC" TO WS-NUM.
STOP RUN.
"#;
        let ast = parse_program("move-cat.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        assert!(ir
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E_MOVE_CATEGORY_MISMATCH"));
    }

    #[test]
    fn condition_name_move_source_fails_closed_as_non_storage() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CONDMOVESRC.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FLAG PIC X.
   88 WS-YES VALUE "Y".
01 WS-OUT PIC X.
PROCEDURE DIVISION.
MAIN.
MOVE WS-YES TO WS-OUT.
STOP RUN.
"#;
        let ir = analyze_src("condition-move-source.cbl", src);

        assert!(ir.semantic.references.iter().any(|reference| {
            reference.raw == "WS-YES"
                && reference.role == ReferenceRoleIr::Source
                && reference.category == Some(ValueCategoryIr::ConditionName)
                && reference.status == ReferenceResolutionStatusIr::UnsupportedCategory
        }));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_CODEGEN_FIELD_CODEC"
                && diagnostic.message.contains("condition-name")
                && diagnostic.message.contains("WS-YES")
        }));
    }

    #[test]
    fn condition_name_call_using_argument_fails_closed_as_non_storage() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CONDCALLARG.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FLAG PIC X.
   88 WS-YES VALUE "Y".
PROCEDURE DIVISION.
MAIN.
CALL "MISSING" USING WS-YES.
STOP RUN.
"#;
        let ir = analyze_src("condition-call-arg.cbl", src);

        assert!(ir.semantic.references.iter().any(|reference| {
            reference.raw == "WS-YES"
                && reference.role == ReferenceRoleIr::ProcedureArgument
                && reference.category == Some(ValueCategoryIr::ConditionName)
                && reference.status == ReferenceResolutionStatusIr::UnsupportedCategory
        }));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_CODEGEN_FIELD_CODEC"
                && diagnostic.message.contains("condition-name")
                && diagnostic.message.contains("WS-YES")
        }));
    }

    #[test]
    fn comp5_linkage_signature_keeps_native_binary_category() {
        let callee = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLEE.
DATA DIVISION.
LINKAGE SECTION.
01 LK-NATIVE PIC S9(9) COMP-5.
PROCEDURE DIVISION USING LK-NATIVE.
MAIN.
GOBACK.
"#;
        let caller = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLER.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-BINARY PIC S9(9) COMP.
PROCEDURE DIVISION.
MAIN.
CALL "CALLEE" USING WS-BINARY.
STOP RUN.
"#;
        let callee_ast = parse_program("callee.cbl", callee).expect("callee parses");
        let caller_ast = parse_program("caller.cbl", caller).expect("caller parses");
        let catalog = ProgramCatalog::from_asts(std::slice::from_ref(&callee_ast));
        let ir = analyze_with_catalog(caller_ast, Dialect::Ibm, &catalog);

        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_CALL_USING_CONVERSION"
                && diagnostic.message.contains("WS_BINARY")
                && diagnostic.message.contains("NativeBinary")
        }));
    }

    #[test]
    fn numeric_edited_linkage_signature_preserves_category() {
        let callee = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. EDITED.
DATA DIVISION.
LINKAGE SECTION.
01 LK-EDITED PIC ZZ9.
PROCEDURE DIVISION USING LK-EDITED.
MAIN.
GOBACK.
"#;
        let callee_ast = parse_program("edited.cbl", callee).expect("callee parses");
        let catalog = ProgramCatalog::from_asts(std::slice::from_ref(&callee_ast));
        let params = catalog
            .linkage_params_for("EDITED")
            .expect("catalog has linkage params");

        assert_eq!(params[0].category, ValueCategoryIr::NumericEdited);
    }

    #[test]
    fn national_dbcs_linkage_signature_preserves_storage_categories() {
        let callee = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLEE.
DATA DIVISION.
LINKAGE SECTION.
01 LK-NAT PIC N(4).
01 LK-DBCS PIC G(4).
PROCEDURE DIVISION USING LK-NAT LK-DBCS.
MAIN.
GOBACK.
"#;
        let caller = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLER.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TEXT PIC X(4).
PROCEDURE DIVISION.
MAIN.
CALL "CALLEE" USING WS-TEXT WS-TEXT.
STOP RUN.
"#;
        let callee_ast = parse_program("callee.cbl", callee).expect("callee parses");
        let caller_ast = parse_program("caller.cbl", caller).expect("caller parses");
        let catalog = ProgramCatalog::from_asts(std::slice::from_ref(&callee_ast));
        let ir = analyze_with_catalog(caller_ast, Dialect::Ibm, &catalog);

        for category in [ValueCategoryIr::National, ValueCategoryIr::Dbcs] {
            assert!(ir.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "E_UNSUPPORTED_CALL_USING_CONVERSION"
                    && diagnostic.message.contains(&format!("{category:?}"))
            }));
        }
    }

    #[test]
    fn condition_name_display_fails_closed_as_non_storage() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CONDDISPLAY.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FLAG PIC X.
   88 WS-YES VALUE "Y".
PROCEDURE DIVISION.
MAIN.
DISPLAY WS-YES.
STOP RUN.
"#;
        let ir = analyze_src("condition-display.cbl", src);

        assert!(ir.semantic.references.iter().any(|reference| {
            reference.raw == "WS-YES"
                && reference.role == ReferenceRoleIr::Display
                && reference.category == Some(ValueCategoryIr::ConditionName)
                && reference.status == ReferenceResolutionStatusIr::UnsupportedCategory
        }));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_CODEGEN_FIELD_CODEC"
                && diagnostic.message.contains("condition-name")
                && diagnostic.message.contains("WS-YES")
        }));
    }

    #[test]
    fn condition_ir_parses_precedence_class_sign_and_references() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CONDIF.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-A PIC 9(3).
   05 WS-B PIC X(3).
   05 WS-C PIC S9(3).
PROCEDURE DIVISION.
MAIN.
IF WS-A = 1 OR WS-B IS NUMERIC AND NOT WS-C IS ZERO.
STOP RUN.
"#;
        let ast = parse_program("cond-if.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let condition = ir.semantic.conditions.first().expect("condition");
        assert_eq!(condition.status, ConditionStatusIr::Parsed);
        assert!(matches!(condition.tree, Some(ConditionIr::Or(_, _))));
        assert!(ir.semantic.references.iter().any(|reference| {
            reference.raw == "WS-A"
                && reference.role == ReferenceRoleIr::ConditionOperand
                && reference.category == Some(ValueCategoryIr::NumericDisplay)
        }));
        assert!(ir
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "E_CONDITION_PARSE"));
    }

    #[test]
    fn condition_ir_desugars_abbreviated_relations() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. ABBREV.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-NUM PIC 9(3).
PROCEDURE DIVISION.
MAIN.
IF WS-NUM > 10 AND < 20.
STOP RUN.
"#;
        let ast = parse_program("abbrev.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let condition = ir.semantic.conditions.first().expect("condition");
        let Some(ConditionIr::And(_, right)) = &condition.tree else {
            panic!("expected AND condition");
        };
        let ConditionIr::Relation { left, op, .. } = right.as_ref() else {
            panic!("expected abbreviated relation");
        };
        assert_eq!(*op, RelOpIr::Less);
        assert!(matches!(
            left,
            ConditionOperandIr::Identifier(reference) if reference.raw == "WS-NUM"
        ));
    }

    #[test]
    fn procedure_cfg_uses_sentence_level_blocks_for_top_level_imperatives() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SENTCFG.
PROCEDURE DIVISION.
MAIN.
DISPLAY "A" DISPLAY "B".
STOP RUN.
"#;
        let ir = analyze_src("sentcfg.cbl", src);
        let blocks = &ir.procedure_cfg.blocks;
        assert_eq!(ir.paragraphs[0].sentences[0].statements.len(), 2);
        assert_eq!(ir.paragraphs[0].sentences[1].statements.len(), 1);
        assert_eq!(blocks.len(), 2, "{blocks:?}");
        assert_eq!(blocks[0].label, "MAIN");
        assert_eq!(blocks[0].sentence_index, 0);
        assert_eq!(blocks[1].label, "MAIN#2");
        assert_eq!(blocks[1].sentence_index, 1);
        assert_eq!(blocks[0].statements.len(), 2);
        assert!(matches!(
            blocks[0].transfer,
            ControlTransferIr::FallThrough(Some(ref target)) if target == "MAIN#2"
        ));
        assert!(matches!(blocks[1].transfer, ControlTransferIr::StopRun));
    }

    #[test]
    fn procedure_cfg_models_top_level_next_sentence_target() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. NEXTTOP.
PROCEDURE DIVISION.
MAIN.
NEXT SENTENCE.
DISPLAY "AFTER".
"#;
        let ir = analyze_src("next-top.cbl", src);
        let blocks = &ir.procedure_cfg.blocks;
        assert_eq!(blocks.len(), 2, "{blocks:?}");
        assert!(matches!(
            blocks[0].transfer,
            ControlTransferIr::NextSentence { target: Some(ref target) } if target == "MAIN#2"
        ));
        assert_eq!(ir.procedure_cfg.next_sentence_targets.len(), 1);
        assert_eq!(
            ir.procedure_cfg.next_sentence_targets[0].source_block,
            "MAIN"
        );
        assert_eq!(
            ir.procedure_cfg.next_sentence_targets[0].target.as_deref(),
            Some("MAIN#2")
        );
        assert_eq!(
            ir.procedure_cfg.next_sentence_targets[0].path,
            vec![StatementPathElementIr::Statement(0)]
        );
    }

    #[test]
    fn procedure_cfg_records_nested_next_sentence_without_unconditional_transfer() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. NEXTIF.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FLAG PIC X.
PROCEDURE DIVISION.
MAIN.
IF WS-FLAG = "Y" NEXT SENTENCE ELSE DISPLAY "N".
DISPLAY "AFTER".
"#;
        let ir = analyze_src("next-if.cbl", src);
        let blocks = &ir.procedure_cfg.blocks;
        assert_eq!(blocks.len(), 2, "{blocks:?}");
        assert!(matches!(
            blocks[0].transfer,
            ControlTransferIr::FallThrough(Some(ref target)) if target == "MAIN#2"
        ));
        assert_eq!(ir.procedure_cfg.next_sentence_targets.len(), 1);
        assert_eq!(
            ir.procedure_cfg.next_sentence_targets[0].path,
            vec![
                StatementPathElementIr::Statement(0),
                StatementPathElementIr::Branch(StatementBranchIr::Then),
                StatementPathElementIr::Statement(0)
            ]
        );
        assert_eq!(
            ir.procedure_cfg.next_sentence_targets[0].target.as_deref(),
            Some("MAIN#2")
        );
    }

    #[test]
    fn procedure_cfg_records_start_branch_next_sentence_target() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. NEXTSTART.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT INFILE ASSIGN TO "INFILE".
DATA DIVISION.
FILE SECTION.
FD INFILE.
01 IN-REC PIC X.
WORKING-STORAGE SECTION.
01 WS-ID PIC X.
PROCEDURE DIVISION.
MAIN.
START INFILE KEY IS EQUAL TO WS-ID
    INVALID KEY NEXT SENTENCE
END-START.
DISPLAY "AFTER".
"#;
        let ir = analyze_src("next-start.cbl", src);
        let targets = &ir.procedure_cfg.next_sentence_targets;
        assert_eq!(targets.len(), 1, "{targets:?}");
        assert_eq!(targets[0].source_block, "MAIN");
        assert_eq!(targets[0].target.as_deref(), Some("MAIN#2"));
        assert_eq!(
            targets[0].path,
            vec![
                StatementPathElementIr::Statement(0),
                StatementPathElementIr::Branch(StatementBranchIr::InvalidKey),
                StatementPathElementIr::Statement(0)
            ]
        );
    }

    #[test]
    fn procedure_cfg_records_evaluate_arm_next_sentence_target() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. NEXTEVAL.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FLAG PIC X.
PROCEDURE DIVISION.
MAIN.
EVALUATE WS-FLAG WHEN "Y" NEXT SENTENCE WHEN OTHER DISPLAY "N" END-EVALUATE.
DISPLAY "AFTER".
"#;
        let ir = analyze_src("next-eval.cbl", src);
        let targets = &ir.procedure_cfg.next_sentence_targets;
        assert_eq!(targets.len(), 1, "{targets:?}");
        assert_eq!(targets[0].source_block, "MAIN");
        assert_eq!(targets[0].target.as_deref(), Some("MAIN#2"));
        assert_eq!(
            targets[0].path,
            vec![
                StatementPathElementIr::Statement(0),
                StatementPathElementIr::Branch(StatementBranchIr::EvaluateArm(0)),
                StatementPathElementIr::Statement(0)
            ]
        );
    }

    #[test]
    fn procedure_cfg_records_compute_size_error_next_sentence_target() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. NEXTCOMP.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-NUM PIC 9(3).
PROCEDURE DIVISION.
MAIN.
COMPUTE WS-NUM = 1 ON SIZE ERROR NEXT SENTENCE END-COMPUTE.
DISPLAY "AFTER".
"#;
        let ir = analyze_src("next-compute.cbl", src);
        let targets = &ir.procedure_cfg.next_sentence_targets;
        assert_eq!(targets.len(), 1, "{targets:?}");
        assert_eq!(targets[0].source_block, "MAIN");
        assert_eq!(targets[0].target.as_deref(), Some("MAIN#2"));
        assert_eq!(
            targets[0].path,
            vec![
                StatementPathElementIr::Statement(0),
                StatementPathElementIr::Branch(StatementBranchIr::OnSizeError),
                StatementPathElementIr::Statement(0)
            ]
        );
    }

    #[test]
    fn procedure_cfg_models_final_next_sentence_without_target() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. NEXTEND.
PROCEDURE DIVISION.
MAIN.
NEXT SENTENCE.
"#;
        let ir = analyze_src("next-end.cbl", src);
        let blocks = &ir.procedure_cfg.blocks;
        assert_eq!(blocks.len(), 1, "{blocks:?}");
        assert!(matches!(
            blocks[0].transfer,
            ControlTransferIr::NextSentence { target: None }
        ));
        assert_eq!(ir.procedure_cfg.next_sentence_targets.len(), 1);
        assert_eq!(ir.procedure_cfg.next_sentence_targets[0].target, None);
    }

    #[test]
    fn condition_ir_resolves_condition_names() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CONDNAME.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FLAG PIC X.
   88 WS-FLAG-OK VALUE "Y".
PROCEDURE DIVISION.
MAIN.
IF WS-FLAG-OK.
STOP RUN.
"#;
        let ast = parse_program("cond-name.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let condition = ir.semantic.conditions.first().expect("condition");
        assert_eq!(condition.status, ConditionStatusIr::Parsed);
        assert!(matches!(
            condition.tree,
            Some(ConditionIr::ConditionName { .. })
        ));
        assert!(ir.semantic.references.iter().any(|reference| {
            reference.raw == "WS-FLAG-OK"
                && reference.category == Some(ValueCategoryIr::ConditionName)
                && reference.status == ReferenceResolutionStatusIr::Resolved
        }));
    }

    #[test]
    fn condition_ir_rejects_mixed_numeric_and_nonnumeric_relation() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. MIXED.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-NUM PIC 9(3).
01 WS-TEXT PIC X(3).
PROCEDURE DIVISION.
MAIN.
IF WS-NUM = WS-TEXT.
STOP RUN.
"#;
        let ast = parse_program("mixed.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        assert!(ir
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E_CONDITION_TYPE_MISMATCH"));
        assert_eq!(
            ir.semantic
                .conditions
                .first()
                .map(|condition| condition.status),
            Some(ConditionStatusIr::SemanticError)
        );
    }

    #[test]
    fn condition_ir_desugars_bare_right_operand_abbreviation() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. BAREABBR.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-A PIC 9(3).
01 WS-B PIC 9(3).
01 WS-C PIC 9(3).
PROCEDURE DIVISION.
MAIN.
IF WS-A = WS-B OR WS-C.
STOP RUN.
"#;
        let ast = parse_program("bare-abbrev.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let condition = ir.semantic.conditions.first().expect("condition");
        let Some(ConditionIr::Or(_, right)) = &condition.tree else {
            panic!("expected OR condition");
        };
        let ConditionIr::Relation { left, op, right } = right.as_ref() else {
            panic!("expected abbreviated right operand relation");
        };
        assert_eq!(*op, RelOpIr::Equal);
        assert!(matches!(
            left,
            ConditionOperandIr::Identifier(reference) if reference.raw == "WS-A"
        ));
        assert!(matches!(
            right,
            ConditionOperandIr::Identifier(reference) if reference.raw == "WS-C"
        ));
        assert_eq!(condition.status, ConditionStatusIr::Parsed);
    }

    #[test]
    fn condition_ir_parses_subject_not_relop() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. NOTREL.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-A PIC 9(3).
PROCEDURE DIVISION.
MAIN.
IF WS-A NOT = 2.
STOP RUN.
"#;
        let ast = parse_program("not-rel.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let condition = ir.semantic.conditions.first().expect("condition");
        let Some(ConditionIr::Relation { left, op, right }) = &condition.tree else {
            panic!("expected relation");
        };
        assert_eq!(*op, RelOpIr::NotEqual);
        assert!(matches!(
            left,
            ConditionOperandIr::Identifier(reference) if reference.raw == "WS-A"
        ));
        assert!(matches!(right, ConditionOperandIr::Number(value) if value == "2"));
        assert_eq!(condition.status, ConditionStatusIr::Parsed);
    }

    #[test]
    fn if_condition_stops_before_imperative_statement() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. IFBOUND.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-A PIC 9(3).
PROCEDURE DIVISION.
MAIN.
IF WS-A = 1 DISPLAY "OK".
STOP RUN.
"#;
        let ast = parse_program("if-bound.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let condition = ir.semantic.conditions.first().expect("condition");
        assert_eq!(condition.raw, "WS-A = 1");
        assert!(ir
            .diagnostics
            .iter()
            .all(|diagnostic| !diagnostic.message.contains("DISPLAY")));
    }

    #[test]
    fn if_condition_keeps_doubled_quote_literal_together() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. IFQUOTE.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TEXT PIC X(3).
PROCEDURE DIVISION.
MAIN.
IF WS-TEXT = "A""B" DISPLAY "OK".
STOP RUN.
"#;
        let ast = parse_program("if-quote.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let condition = ir.semantic.conditions.first().expect("condition");
        assert_eq!(condition.raw, "WS-TEXT = \"A\"\"B\"");
        assert_eq!(condition.status, ConditionStatusIr::Parsed);
        assert!(matches!(
            condition.tree,
            Some(ConditionIr::Relation {
                right: ConditionOperandIr::Literal(ref value),
                ..
            }) if value == "A\"\"B"
        ));
    }

    #[test]
    fn nested_if_binds_else_to_nearest_unterminated_if() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. NESTIF.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-A PIC X.
01 WS-B PIC X.
PROCEDURE DIVISION.
MAIN.
IF WS-A = "Y" IF WS-B = "Y" DISPLAY "B" ELSE DISPLAY "A" END-IF ELSE DISPLAY "N" END-IF.
STOP RUN.
"#;
        let ast = parse_program("nest-if.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let StatementIr::If {
            then_statements,
            else_statements,
            ..
        } = &ir.paragraphs[0].statements[0]
        else {
            panic!("expected outer IF");
        };
        assert_eq!(
            else_statements,
            &[StatementIr::Display(vec![OperandIr::Literal(
                "N".to_string()
            )])]
        );
        let StatementIr::If {
            then_statements: inner_then,
            else_statements: inner_else,
            ..
        } = &then_statements[0]
        else {
            panic!("expected inner IF");
        };
        assert_eq!(
            inner_then,
            &[StatementIr::Display(vec![OperandIr::Literal(
                "B".to_string()
            )])]
        );
        assert_eq!(
            inner_else,
            &[StatementIr::Display(vec![OperandIr::Literal(
                "A".to_string()
            )])]
        );
    }

    #[test]
    fn display_upon_console_keeps_destination_out_of_data_references() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. DISPUPON.
PROCEDURE DIVISION.
MAIN.
DISPLAY "HELLO" UPON CONSOLE.
STOP RUN.
"#;
        let ir = analyze_src("display-upon.cbl", src);

        let StatementIr::Display(values) = &ir.paragraphs[0].statements[0] else {
            panic!("expected DISPLAY");
        };
        assert_eq!(values, &[OperandIr::Literal("HELLO".to_string())]);
        assert!(ir
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "E_UNRESOLVED_DATA"));
    }

    #[test]
    fn display_literals_unescape_doubled_quote_delimiters() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. DISPLIT.
PROCEDURE DIVISION.
MAIN.
DISPLAY "A"".B".
DISPLAY 'CAN''T STOP'.
STOP RUN.
"#;
        let ir = analyze_src("display-literals.cbl", src);

        assert_eq!(
            ir.paragraphs[0].statements[0],
            StatementIr::Display(vec![OperandIr::Literal("A\".B".to_string())])
        );
        assert_eq!(
            ir.paragraphs[0].statements[1],
            StatementIr::Display(vec![OperandIr::Literal("CAN'T STOP".to_string())])
        );
    }

    #[test]
    fn display_no_advancing_fails_closed_without_unresolved_option_words() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. DISPNOADV.
PROCEDURE DIVISION.
MAIN.
DISPLAY "HELLO" WITH NO ADVANCING.
STOP RUN.
"#;
        let ir = analyze_src("display-no-advancing.cbl", src);

        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_DISPLAY_NO_ADVANCING"));
        assert!(ir
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "E_UNRESOLVED_DATA"));
    }

    #[test]
    fn add_to_giving_lowers_to_compute_without_bogus_target_reference() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. ADDGIVE.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-A PIC 9 VALUE 2.
01 WS-B PIC 9 VALUE 3.
01 WS-C PIC 9.
PROCEDURE DIVISION.
MAIN.
ADD WS-A TO WS-B GIVING WS-C.
DISPLAY WS-C.
STOP RUN.
"#;
        let ir = analyze_src("add-giving.cbl", src);

        let StatementIr::Compute {
            target, expression, ..
        } = &ir.paragraphs[0].statements[0]
        else {
            panic!("expected ADD GIVING to lower as COMPUTE");
        };
        assert_eq!(target.normalized, "WS_C");
        assert_eq!(expression, "WS-A + WS-B");
        assert!(ir
            .diagnostics
            .iter()
            .all(|diagnostic| !(diagnostic.code == "E_UNRESOLVED_DATA"
                && diagnostic.message.contains("GIVING"))));
        assert!(ir
            .diagnostics
            .iter()
            .all(|diagnostic| !(diagnostic.code == "E_UNRESOLVED_DATA"
                && diagnostic.message.contains("WS_B_GIVING_WS_C"))));
    }

    #[test]
    fn compute_size_error_branch_lowers_multiple_typed_statements() {
        let statement = lower_single_imperative(
            "COMPUTE WS-NUM = 1 / 0 ON SIZE ERROR DISPLAY \"A\" DISPLAY \"B\" END-COMPUTE",
        );
        let StatementIr::Compute {
            on_size_error_ops, ..
        } = statement
        else {
            panic!("expected COMPUTE");
        };
        assert_eq!(
            on_size_error_ops,
            vec![
                StatementIr::Display(vec![OperandIr::Literal("A".to_string())]),
                StatementIr::Display(vec![OperandIr::Literal("B".to_string())])
            ]
        );
    }

    #[test]
    fn compute_rounded_lowers_as_metadata_without_bogus_target_reference() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. COMPROUND.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 N PIC 9 VALUE 9.
PROCEDURE DIVISION.
MAIN.
COMPUTE N ROUNDED = N + 1.
STOP RUN.
"#;
        let ir = analyze_src("compute-rounded.cbl", src);

        let StatementIr::Compute {
            target, rounded, ..
        } = &ir.paragraphs[0].statements[0]
        else {
            panic!("expected COMPUTE");
        };
        assert_eq!(target.normalized, "N");
        assert!(*rounded);
        assert!(ir.diagnostics.iter().all(|diagnostic| {
            !(diagnostic.code == "E_UNRESOLVED_DATA" && diagnostic.message.contains("N_ROUNDED"))
        }));
    }

    #[test]
    fn add_rounded_lowers_as_compute_metadata_without_bogus_target_reference() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. ADDROUND.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 N PIC 9 VALUE 9.
PROCEDURE DIVISION.
MAIN.
ADD 1 TO N ROUNDED.
STOP RUN.
"#;
        let ir = analyze_src("add-rounded.cbl", src);

        let StatementIr::Compute {
            target,
            expression,
            rounded,
            ..
        } = &ir.paragraphs[0].statements[0]
        else {
            panic!("expected ADD ROUNDED to lower as COMPUTE");
        };
        assert_eq!(target.normalized, "N");
        assert_eq!(expression, "N + 1");
        assert!(*rounded);
        assert!(ir.diagnostics.iter().all(|diagnostic| {
            !(diagnostic.code == "E_UNRESOLVED_DATA" && diagnostic.message.contains("N_ROUNDED"))
        }));
    }

    #[test]
    fn add_rejects_unconsumed_source_tail_without_bogus_source_reference() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. ADDSRC.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-A PIC 9.
01 WS-N PIC 9.
PROCEDURE DIVISION.
MAIN.
ADD WS-A GARBAGE TO WS-N.
STOP RUN.
"#;
        let ir = analyze_src("add-source-tail.cbl", src);

        assert!(matches!(
            ir.paragraphs[0].statements[0],
            StatementIr::Unsupported { ref keyword, .. } if keyword == "ADD"
        ));
        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_STATEMENT"));
        assert!(ir.diagnostics.iter().all(|diagnostic| {
            !(diagnostic.code == "E_UNRESOLVED_DATA" && diagnostic.message.contains("WS_A_GARBAGE"))
        }));
    }

    #[test]
    fn add_giving_rounded_lowers_as_compute_metadata_without_bogus_target_reference() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. ADDGIVEROUND.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-A PIC 9 VALUE 2.
01 WS-B PIC 9 VALUE 3.
01 WS-C PIC 9.
PROCEDURE DIVISION.
MAIN.
ADD WS-A TO WS-B GIVING WS-C ROUNDED.
STOP RUN.
"#;
        let ir = analyze_src("add-giving-rounded.cbl", src);

        let StatementIr::Compute {
            target,
            expression,
            rounded,
            ..
        } = &ir.paragraphs[0].statements[0]
        else {
            panic!("expected ADD GIVING ROUNDED to lower as COMPUTE");
        };
        assert_eq!(target.normalized, "WS_C");
        assert_eq!(expression, "WS-A + WS-B");
        assert!(*rounded);
        assert!(ir.diagnostics.iter().all(|diagnostic| {
            !(diagnostic.code == "E_UNRESOLVED_DATA"
                && (diagnostic.message.contains("WS_C_ROUNDED")
                    || diagnostic.message.contains("GIVING")))
        }));
    }

    #[test]
    fn subtract_giving_lowers_to_compute_without_bogus_target_reference() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SUBGIVE.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-A PIC 9 VALUE 2.
01 WS-B PIC 9 VALUE 7.
01 WS-C PIC 9.
PROCEDURE DIVISION.
MAIN.
SUBTRACT WS-A FROM WS-B GIVING WS-C.
STOP RUN.
"#;
        let ir = analyze_src("subtract-giving.cbl", src);

        let StatementIr::Compute {
            target, expression, ..
        } = &ir.paragraphs[0].statements[0]
        else {
            panic!("expected SUBTRACT GIVING to lower as COMPUTE");
        };
        assert_eq!(target.normalized, "WS_C");
        assert_eq!(expression, "WS-B - WS-A");
        assert!(ir.diagnostics.iter().all(|diagnostic| {
            !(diagnostic.code == "E_UNRESOLVED_DATA"
                && (diagnostic.message.contains("GIVING")
                    || diagnostic.message.contains("WS_B_GIVING_WS_C")))
        }));
    }

    #[test]
    fn subtract_giving_rejects_unconsumed_receiver_tail_without_bogus_receiver_reference() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SUBRECV.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-A PIC 9.
01 WS-B PIC 9.
01 WS-C PIC 9.
PROCEDURE DIVISION.
MAIN.
SUBTRACT WS-A FROM WS-B GARBAGE GIVING WS-C.
STOP RUN.
"#;
        let ir = analyze_src("subtract-receiver-tail.cbl", src);

        assert!(matches!(
            ir.paragraphs[0].statements[0],
            StatementIr::Unsupported { ref keyword, .. } if keyword == "SUBTRACT"
        ));
        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_STATEMENT"));
        assert!(ir.diagnostics.iter().all(|diagnostic| {
            !(diagnostic.code == "E_UNRESOLVED_DATA" && diagnostic.message.contains("WS_B_GARBAGE"))
        }));
    }

    #[test]
    fn multiply_giving_rounded_lowers_as_compute_metadata_without_bogus_target_reference() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. MULGIVEROUND.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-A PIC 9 VALUE 2.
01 WS-B PIC 9 VALUE 3.
01 WS-C PIC 9.
PROCEDURE DIVISION.
MAIN.
MULTIPLY WS-A BY WS-B GIVING WS-C ROUNDED.
STOP RUN.
"#;
        let ir = analyze_src("multiply-giving-rounded.cbl", src);

        let StatementIr::Compute {
            target,
            expression,
            rounded,
            ..
        } = &ir.paragraphs[0].statements[0]
        else {
            panic!("expected MULTIPLY GIVING ROUNDED to lower as COMPUTE");
        };
        assert_eq!(target.normalized, "WS_C");
        assert_eq!(expression, "WS-A * WS-B");
        assert!(*rounded);
        assert!(ir.diagnostics.iter().all(|diagnostic| {
            !(diagnostic.code == "E_UNRESOLVED_DATA"
                && (diagnostic.message.contains("WS_C_ROUNDED")
                    || diagnostic.message.contains("GIVING")))
        }));
    }

    #[test]
    fn divide_giving_size_error_lowers_to_compute_branch_without_bogus_target_reference() {
        let statement = lower_single_imperative(
            "DIVIDE WS-D INTO WS-N GIVING WS-Q ON SIZE ERROR DISPLAY \"DIV\" END-DIVIDE",
        );

        let StatementIr::Compute {
            target,
            expression,
            on_size_error_ops,
            ..
        } = statement
        else {
            panic!("expected DIVIDE GIVING to lower as COMPUTE");
        };
        assert_eq!(target.normalized, "WS_Q");
        assert_eq!(expression, "WS-N / WS-D");
        assert_eq!(
            on_size_error_ops,
            vec![StatementIr::Display(vec![OperandIr::Literal(
                "DIV".to_string()
            )])]
        );
    }

    #[test]
    fn compute_target_preserves_subscript_without_fabricated_symbol() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. COMPSUB.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TABLE OCCURS 3 TIMES PIC 9.
PROCEDURE DIVISION.
MAIN.
COMPUTE WS-TABLE(2) = 5.
STOP RUN.
"#;
        let ir = analyze_src("compute-subscript.cbl", src);
        let StatementIr::Compute { target, .. } = &ir.paragraphs[0].statements[0] else {
            panic!("expected COMPUTE");
        };
        assert_eq!(target.normalized, "WS_TABLE");
        assert_eq!(target.subscripts, vec!["2"]);
        let reference = ir
            .semantic
            .references
            .iter()
            .find(|reference| {
                reference.raw == "WS-TABLE(2)" && reference.role == ReferenceRoleIr::ComputeTarget
            })
            .expect("expected COMPUTE target reference");
        assert_eq!(reference.status, ReferenceResolutionStatusIr::Resolved);
        assert!(ir.diagnostics.iter().all(|diagnostic| {
            !(diagnostic.code == "E_UNRESOLVED_DATA"
                && (diagnostic.message.contains("WS_TABLE2")
                    || diagnostic.message.contains("WS-TABLE2")))
        }));
        assert!(ir.diagnostics.iter().all(|diagnostic| {
            !matches!(
                diagnostic.code.as_str(),
                "E_CODEGEN_SUBSCRIPT" | "E_CODEGEN_OCCURS_REFERENCE"
            )
        }));
    }

    #[test]
    fn arithmetic_target_allows_fully_subscripted_occurs_element() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. ADDSUB.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TABLE OCCURS 3 TIMES PIC 9.
PROCEDURE DIVISION.
MAIN.
ADD 1 TO WS-TABLE(2).
STOP RUN.
"#;
        let ir = analyze_src("add-subscript.cbl", src);
        let reference = ir
            .semantic
            .references
            .iter()
            .find(|reference| {
                reference.raw == "WS-TABLE(2)"
                    && reference.role == ReferenceRoleIr::ArithmeticTarget
            })
            .expect("expected arithmetic target reference");
        assert_eq!(reference.status, ReferenceResolutionStatusIr::Resolved);
        assert_eq!(reference.target.as_deref(), Some("WS_TABLE"));
        assert!(ir.diagnostics.iter().all(|diagnostic| {
            !matches!(
                diagnostic.code.as_str(),
                "E_CODEGEN_SUBSCRIPT" | "E_CODEGEN_OCCURS_REFERENCE"
            )
        }));
    }

    #[test]
    fn compute_expression_allows_fully_subscripted_occurs_source() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. COMPEXPRSUB.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TABLE OCCURS 3 TIMES PIC 9 VALUE 0.
01 WS-OUT PIC 9 VALUE 0.
PROCEDURE DIVISION.
MAIN.
COMPUTE WS-OUT = WS-TABLE(2) + 1.
STOP RUN.
"#;
        let ir = analyze_src("compute-expression-subscript.cbl", src);
        let reference = ir
            .semantic
            .references
            .iter()
            .find(|reference| {
                reference.raw == "WS-TABLE(2)"
                    && reference.role == ReferenceRoleIr::ArithmeticSource
            })
            .expect("expected arithmetic source reference");
        assert_eq!(reference.status, ReferenceResolutionStatusIr::Resolved);
        assert_eq!(reference.target.as_deref(), Some("WS_TABLE"));
        assert!(ir.diagnostics.iter().all(|diagnostic| {
            !matches!(
                diagnostic.code.as_str(),
                "E_CODEGEN_SUBSCRIPT" | "E_CODEGEN_OCCURS_REFERENCE"
            )
        }));
    }

    #[test]
    fn compute_expression_source_inside_odo_remains_fail_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. COMPEXPRODO.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-COUNT PIC 9 VALUE 3.
01 WS-TABLE OCCURS 0 TO 3 TIMES DEPENDING ON WS-COUNT PIC 9 VALUE 0.
01 WS-OUT PIC 9 VALUE 0.
PROCEDURE DIVISION.
MAIN.
COMPUTE WS-OUT = WS-TABLE(2) + 1.
STOP RUN.
"#;
        let ir = analyze_src("compute-expression-odo.cbl", src);
        let reference = ir
            .semantic
            .references
            .iter()
            .find(|reference| {
                reference.raw == "WS-TABLE(2)"
                    && reference.role == ReferenceRoleIr::ArithmeticSource
            })
            .expect("expected arithmetic source reference");
        assert_eq!(
            reference.status,
            ReferenceResolutionStatusIr::UnsupportedDynamic
        );
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_CODEGEN_ODO_REFERENCE"
                && diagnostic.message.contains("OCCURS DEPENDING ON")
        }));
    }

    #[test]
    fn compute_target_inside_odo_remains_fail_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. COMPODO.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-COUNT PIC 9 VALUE 3.
01 WS-TABLE OCCURS 0 TO 3 TIMES DEPENDING ON WS-COUNT PIC 9 VALUE 0.
PROCEDURE DIVISION.
MAIN.
COMPUTE WS-TABLE(2) = 1.
STOP RUN.
"#;
        let ir = analyze_src("compute-odo.cbl", src);
        let reference = ir
            .semantic
            .references
            .iter()
            .find(|reference| {
                reference.raw == "WS-TABLE(2)" && reference.role == ReferenceRoleIr::ComputeTarget
            })
            .expect("expected COMPUTE target reference");
        assert_eq!(
            reference.status,
            ReferenceResolutionStatusIr::UnsupportedDynamic
        );
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_CODEGEN_ODO_REFERENCE"
                && diagnostic.message.contains("OCCURS DEPENDING ON")
        }));
    }

    #[test]
    fn return_at_end_branch_lowers_multiple_typed_statements() {
        let statement = lower_single_imperative(
            "RETURN SORT-FILE AT END DISPLAY \"A\" DISPLAY \"B\" END-RETURN",
        );
        let StatementIr::ReturnSortRecord(ret) = statement else {
            panic!("expected RETURN");
        };
        assert_eq!(
            ret.at_end_ops,
            vec![
                StatementIr::Display(vec![OperandIr::Literal("A".to_string())]),
                StatementIr::Display(vec![OperandIr::Literal("B".to_string())])
            ]
        );
    }

    #[test]
    fn string_overflow_branch_lowers_multiple_typed_statements() {
        let statement = lower_single_imperative(
            "STRING \"A\" DELIMITED BY SIZE INTO WS-TEXT ON OVERFLOW DISPLAY \"A\" DISPLAY \"B\" END-STRING",
        );
        let StatementIr::StringOp(string) = statement else {
            panic!("expected STRING");
        };
        assert_eq!(
            string.on_overflow_ops,
            vec![
                StatementIr::Display(vec![OperandIr::Literal("A".to_string())]),
                StatementIr::Display(vec![OperandIr::Literal("B".to_string())])
            ]
        );
    }

    #[test]
    fn read_at_end_branch_lowers_multiple_typed_statements() {
        let statement =
            lower_single_imperative("READ INFILE AT END DISPLAY \"A\" DISPLAY \"B\" END-READ");
        let StatementIr::ReadFile(read) = statement else {
            panic!("expected typed READ");
        };
        assert_eq!(read.file, "INFILE");
        assert_eq!(
            read.at_end_ops,
            vec![
                StatementIr::Display(vec![OperandIr::Literal("A".to_string())]),
                StatementIr::Display(vec![OperandIr::Literal("B".to_string())])
            ]
        );
    }

    #[test]
    fn read_on_exception_branch_lowers_multiple_typed_statements() {
        let statement = lower_single_imperative(
            "READ INFILE ON EXCEPTION DISPLAY \"A\" DISPLAY \"B\" END-READ",
        );
        let StatementIr::ReadFile(read) = statement else {
            panic!("expected typed READ");
        };
        assert_eq!(
            read.on_exception_ops,
            vec![
                StatementIr::Display(vec![OperandIr::Literal("A".to_string())]),
                StatementIr::Display(vec![OperandIr::Literal("B".to_string())])
            ]
        );
    }

    #[test]
    fn rewrite_invalid_key_branch_lowers_multiple_typed_statements() {
        let statement = lower_single_imperative(
            "REWRITE IN-REC INVALID KEY DISPLAY \"A\" DISPLAY \"B\" END-REWRITE",
        );
        let StatementIr::RewriteFile(rewrite) = statement else {
            panic!("expected typed REWRITE");
        };
        assert_eq!(rewrite.record.normalized, "IN_REC");
        assert_eq!(
            rewrite.invalid_key_ops,
            vec![
                StatementIr::Display(vec![OperandIr::Literal("A".to_string())]),
                StatementIr::Display(vec![OperandIr::Literal("B".to_string())])
            ]
        );
    }

    #[test]
    fn start_invalid_key_branch_lowers_nested_start_branches() {
        let statement = lower_single_imperative(
            "START OUTER-FILE KEY IS EQUAL TO OUTER-ID INVALID KEY START INNER-FILE KEY IS EQUAL TO INNER-ID INVALID KEY DISPLAY \"INNER-BAD\" NOT INVALID KEY DISPLAY \"INNER-OK\" END-START NOT INVALID KEY DISPLAY \"OUTER-OK\" END-START",
        );
        let StatementIr::StartFile(outer) = statement else {
            panic!("expected typed START");
        };
        assert_eq!(outer.file, "OUTER_FILE");
        assert_eq!(outer.invalid_key_ops.len(), 1, "{outer:?}");
        let StatementIr::StartFile(inner) = &outer.invalid_key_ops[0] else {
            panic!("expected nested START");
        };
        assert_eq!(inner.file, "INNER_FILE");
        assert_eq!(
            inner.invalid_key_ops,
            vec![StatementIr::Display(vec![OperandIr::Literal(
                "INNER-BAD".to_string()
            )])]
        );
        assert_eq!(
            inner.not_invalid_key_ops,
            vec![StatementIr::Display(vec![OperandIr::Literal(
                "INNER-OK".to_string()
            )])]
        );
        assert_eq!(
            outer.not_invalid_key_ops,
            vec![StatementIr::Display(vec![OperandIr::Literal(
                "OUTER-OK".to_string()
            )])]
        );
    }

    #[test]
    fn delete_not_invalid_key_branch_lowers_multiple_typed_statements() {
        let statement = lower_single_imperative(
            "DELETE INFILE NOT INVALID KEY DISPLAY \"A\" DISPLAY \"B\" END-DELETE",
        );
        let StatementIr::DeleteFile(delete) = statement else {
            panic!("expected typed DELETE");
        };
        assert_eq!(delete.file, "INFILE");
        assert_eq!(
            delete.not_invalid_key_ops,
            vec![
                StatementIr::Display(vec![OperandIr::Literal("A".to_string())]),
                StatementIr::Display(vec![OperandIr::Literal("B".to_string())])
            ]
        );
    }

    #[test]
    fn open_statement_lowers_to_typed_file_mode() {
        let statement = lower_single_imperative("OPEN I-O INFILE");
        let StatementIr::OpenFile(open) = statement else {
            panic!("expected typed OPEN");
        };
        assert_eq!(open.file, "INFILE");
        assert_eq!(open.mode, FileOpenModeIr::Io);
    }

    #[test]
    fn write_statement_lowers_record_and_advancing() {
        let statement = lower_single_imperative("WRITE OUT-REC BEFORE ADVANCING 2 LINES");
        let StatementIr::WriteFile(write) = statement else {
            panic!("expected typed WRITE");
        };
        assert_eq!(write.record.normalized, "OUT_REC");
        assert_eq!(write.advancing, WriteAdvancingIr::BeforeLines(2));
    }

    #[test]
    fn write_invalid_key_branches_lower_typed_but_remain_fail_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. WRITEBR.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT OUTFILE ASSIGN TO "OUTFILE"
        ORGANIZATION IS SEQUENTIAL
        ACCESS MODE IS SEQUENTIAL.
DATA DIVISION.
FILE SECTION.
FD OUTFILE.
01 OUT-REC PIC X.
PROCEDURE DIVISION.
MAIN.
WRITE OUT-REC INVALID KEY DISPLAY "BAD" NOT INVALID KEY DISPLAY "OK" END-WRITE.
STOP RUN.
"#;
        let ir = analyze_src("writebr.cbl", src);

        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_WRITE_BRANCH"));
        let StatementIr::WriteFile(write) = &ir.paragraphs[0].statements[0] else {
            panic!("expected typed WRITE");
        };
        assert_eq!(write.record.normalized, "OUT_REC");
        assert_eq!(
            write.branch_phrases,
            vec!["INVALID KEY".to_string(), "NOT INVALID KEY".to_string()]
        );
        assert_eq!(
            write.invalid_key_ops,
            vec![StatementIr::Display(vec![OperandIr::Literal(
                "BAD".to_string()
            )])]
        );
        assert_eq!(
            write.not_invalid_key_ops,
            vec![StatementIr::Display(vec![OperandIr::Literal(
                "OK".to_string()
            )])]
        );
        assert!(write.on_exception_ops.is_empty());
        assert!(write.not_on_exception_ops.is_empty());
    }

    #[test]
    fn write_exception_branches_lower_typed_but_remain_fail_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. WRITEEX.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT OUTFILE ASSIGN TO "OUTFILE"
        ORGANIZATION IS SEQUENTIAL
        ACCESS MODE IS SEQUENTIAL.
DATA DIVISION.
FILE SECTION.
FD OUTFILE.
01 OUT-REC PIC X.
PROCEDURE DIVISION.
MAIN.
WRITE OUT-REC ON EXCEPTION DISPLAY "BAD" NOT ON EXCEPTION DISPLAY "OK" END-WRITE.
STOP RUN.
"#;
        let ir = analyze_src("writeex.cbl", src);

        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_WRITE_BRANCH"));
        let StatementIr::WriteFile(write) = &ir.paragraphs[0].statements[0] else {
            panic!("expected typed WRITE");
        };
        assert_eq!(write.record.normalized, "OUT_REC");
        assert_eq!(
            write.branch_phrases,
            vec!["ON EXCEPTION".to_string(), "NOT ON EXCEPTION".to_string()]
        );
        assert_eq!(
            write.on_exception_ops,
            vec![StatementIr::Display(vec![OperandIr::Literal(
                "BAD".to_string()
            )])]
        );
        assert_eq!(
            write.not_on_exception_ops,
            vec![StatementIr::Display(vec![OperandIr::Literal(
                "OK".to_string()
            )])]
        );
        assert!(write.invalid_key_ops.is_empty());
        assert!(write.not_invalid_key_ops.is_empty());
    }

    #[test]
    fn close_statement_lowers_to_typed_file() {
        let statement = lower_single_imperative("CLOSE INFILE");
        let StatementIr::CloseFile(close) = statement else {
            panic!("expected typed CLOSE");
        };
        assert_eq!(close.file, "INFILE");
    }

    #[test]
    fn typed_file_validation_rejects_dynamic_assign() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. DYNASSIGN.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT INFILE ASSIGN TO WS-PATH
        ORGANIZATION IS SEQUENTIAL
        ACCESS MODE IS SEQUENTIAL.
DATA DIVISION.
FILE SECTION.
FD INFILE.
01 IN-REC PIC X.
WORKING-STORAGE SECTION.
01 WS-PATH PIC X(8).
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ast = parse_program("dynassign.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let data_index = DataReferenceIndex::new(&ir.data_items, &ir.storage.condition_names);
        let open = OpenFileIr {
            file: "INFILE".to_string(),
            mode: FileOpenModeIr::Input,
        };
        let err = validate_open_file(&open, &ir.files, &data_index).expect_err("dynamic assign");
        assert!(err.contains("dynamic ASSIGN"));
    }

    #[test]
    fn typed_file_validation_rejects_missing_read_into_target() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. READMISS.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT INFILE ASSIGN TO "INFILE"
        ORGANIZATION IS SEQUENTIAL
        ACCESS MODE IS SEQUENTIAL.
DATA DIVISION.
FILE SECTION.
FD INFILE.
01 IN-REC PIC X.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ast = parse_program("readmiss.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let data_index = DataReferenceIndex::new(&ir.data_items, &ir.storage.condition_names);
        let read = ReadFileIr {
            file: "INFILE".to_string(),
            into: Some(parse_data_ref("MISSING-TARGET")),
            at_end_ops: Vec::new(),
            not_at_end_ops: Vec::new(),
            on_exception_ops: Vec::new(),
        };
        let err = validate_read_file(&read, &ir.files, &data_index).expect_err("missing target");
        assert!(err.contains("READ INTO target MISSING_TARGET does not resolve"));
    }

    #[test]
    fn typed_file_validation_resolves_write_record_to_file() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. WRITEREC.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT OUTFILE ASSIGN TO "OUTFILE"
        ORGANIZATION IS SEQUENTIAL
        ACCESS MODE IS SEQUENTIAL.
DATA DIVISION.
FILE SECTION.
FD OUTFILE.
01 OUT-REC PIC X.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ast = parse_program("writerec.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let data_index = DataReferenceIndex::new(&ir.data_items, &ir.storage.condition_names);
        let write = WriteFileIr {
            record: parse_data_ref("OUT-REC"),
            advancing: WriteAdvancingIr::None,
            invalid_key_ops: Vec::new(),
            not_invalid_key_ops: Vec::new(),
            on_exception_ops: Vec::new(),
            not_on_exception_ops: Vec::new(),
            branch_phrases: Vec::new(),
        };
        validate_write_file(&write, &ir.files, &data_index)
            .expect("write record resolves to FD file");
    }

    #[test]
    fn condition_names_preserve_value_ranges() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CONDRANGE.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-STATUS PIC 99.
   88 WS-ERROR VALUE 10 THRU 20, 25.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ast = parse_program("cond-range.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let condition_name = ir.storage.condition_names.first().expect("condition name");
        assert_eq!(
            condition_name.value_set,
            vec![
                ConditionValueIr::Range {
                    start: "10".to_string(),
                    end: "20".to_string()
                },
                ConditionValueIr::Single("25".to_string())
            ]
        );
        let record_condition = ir
            .storage
            .record_plan
            .condition_names
            .first()
            .expect("record condition name");
        assert_eq!(record_condition.value_set.len(), 2);
    }

    #[test]
    fn condition_names_use_typed_values_clause_with_optional_are() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CONDARE.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-STATUS PIC 99.
   88 WS-ERROR VALUES ARE 10 THRU 20, 25.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ast = parse_program("cond-are.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let condition_name = ir.storage.condition_names.first().expect("condition name");
        assert_eq!(
            condition_name.value_set,
            vec![
                ConditionValueIr::Range {
                    start: "10".to_string(),
                    end: "20".to_string()
                },
                ConditionValueIr::Single("25".to_string())
            ]
        );
    }

    #[test]
    fn condition_name_alphanumeric_value_must_fit_parent_storage() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CONDLEN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FLAG PIC X.
   88 WS-LONG VALUE "YES".
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("condition-length.cbl", src);

        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_CONDITION_VALUE"
                && diagnostic.message.contains("WS_LONG")
                && diagnostic.message.contains("WS_FLAG")
                && diagnostic.message.contains("3 bytes")
                && diagnostic.message.contains("1 bytes")
        }));
        assert!(ir
            .storage
            .items
            .iter()
            .all(|item| item.qualified_name != "WS_FLAG.WS_LONG"));
    }

    #[test]
    fn condition_name_numeric_value_must_fit_parent_picture() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CONDNUM.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-CODE PIC 99.
   88 WS-BIG VALUE 100.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("condition-numeric-big.cbl", src);

        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_CONDITION_VALUE"
                && diagnostic.message.contains("WS_BIG")
                && diagnostic.message.contains("100")
                && diagnostic.message.contains("WS_CODE")
                && diagnostic.message.contains("picture 99")
        }));
    }

    #[test]
    fn condition_name_numeric_range_endpoints_must_fit_parent_picture() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CONDNUMRANGE.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-AMOUNT PIC 99V9.
   88 WS-TOO-PRECISE VALUE 10.02 THRU 12.34.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("condition-numeric-range.cbl", src);

        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_CONDITION_VALUE"
                && diagnostic.message.contains("WS_TOO_PRECISE")
                && diagnostic.message.contains("10.02")
                && diagnostic.message.contains("WS_AMOUNT")
                && diagnostic.message.contains("picture 99V9")
        }));
    }

    #[test]
    fn condition_name_unsigned_numeric_parent_rejects_negative_value() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CONDNEG.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-COUNT PIC 99.
   88 WS-NEG VALUE -1.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("condition-numeric-negative.cbl", src);

        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_CONDITION_VALUE"
                && diagnostic.message.contains("WS_NEG")
                && diagnostic.message.contains("-1")
                && diagnostic.message.contains("signed false")
        }));
    }

    #[test]
    fn condition_name_numeric_parent_rejects_non_numeric_value() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CONDNONNUM.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-CODE PIC 99.
   88 WS-BAD VALUE "AA".
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ir = analyze_src("condition-numeric-nonnumeric.cbl", src);

        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_CONDITION_VALUE"
                && diagnostic.message.contains("WS_BAD")
                && diagnostic.message.contains("AA")
                && diagnostic.message.contains("not a numeric literal")
        }));
    }

    #[test]
    fn evaluate_when_other_is_represented_as_any_patterns() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. EVALOTHER.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FIELD PIC X.
PROCEDURE DIVISION.
MAIN.
EVALUATE WS-FIELD
  WHEN OTHER DISPLAY "OTHER"
END-EVALUATE.
STOP RUN.
"#;
        let ast = parse_program("eval-other.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let evaluate = ir
            .semantic
            .evaluates
            .first()
            .and_then(|analysis| analysis.evaluate.as_ref())
            .expect("evaluate analysis");
        assert_eq!(evaluate.arms.len(), 1);
        assert_eq!(evaluate.arms[0].patterns, vec![EvaluatePatternIr::Any]);
        assert!(ir.diagnostics.iter().all(|diagnostic| {
            !(diagnostic.code == "E_UNRESOLVED_DATA" && diagnostic.message.contains("OTHER"))
        }));
    }

    #[test]
    fn evaluate_arm_lowers_multiple_typed_statements() {
        let statement = lower_single_imperative(
            "EVALUATE WS-FIELD WHEN \"Y\" DISPLAY \"A\" DISPLAY \"B\" END-EVALUATE",
        );
        let StatementIr::Evaluate(evaluate) = statement else {
            panic!("expected EVALUATE");
        };
        assert_eq!(evaluate.subjects.len(), 1);
        assert_eq!(evaluate.arms.len(), 1);
        assert_eq!(
            evaluate.arms[0].statements,
            vec![
                StatementIr::Display(vec![OperandIr::Literal("A".to_string())]),
                StatementIr::Display(vec![OperandIr::Literal("B".to_string())])
            ]
        );
    }

    #[test]
    fn serial_search_lowers_typed_at_end_and_when_statements() {
        let statement = lower_single_imperative(
            "SEARCH WS-ITEM VARYING WS-IDX AT END DISPLAY \"END\" WHEN WS-ITEM = \"B\" DISPLAY \"FOUND\" END-SEARCH",
        );
        let StatementIr::Search(search) = statement else {
            panic!("expected SEARCH");
        };
        assert_eq!(search.table, "WS_ITEM");
        assert_eq!(search.index.as_deref(), Some("WS_IDX"));
        assert_eq!(
            search.at_end,
            vec![StatementIr::Display(vec![OperandIr::Literal(
                "END".to_string()
            )])]
        );
        assert_eq!(search.whens.len(), 1);
        assert_eq!(
            search.whens[0].statements,
            vec![StatementIr::Display(vec![OperandIr::Literal(
                "FOUND".to_string()
            )])]
        );
    }

    #[test]
    fn function_length_accepts_argument_without_space_before_paren() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FUNCLEN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FIELD PIC X(3).
PROCEDURE DIVISION.
MAIN.
IF FUNCTION LENGTH(WS-FIELD) = 3 DISPLAY "LEN".
STOP RUN.
"#;
        let ast = parse_program("func-len.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let condition = ir.semantic.conditions.first().expect("condition");
        let Some(ConditionIr::Relation { left, .. }) = condition.tree.as_ref() else {
            panic!("expected relation condition");
        };
        assert!(matches!(
            left,
            ConditionOperandIr::Function(FunctionOperandIr::Length(arg))
                if matches!(arg.as_ref(), ConditionOperandIr::Identifier(reference) if reference.normalized == "WS_FIELD")
        ));
        assert!(ir.diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "E_UNSUPPORTED_FUNCTION_OPERAND"
                && !(diagnostic.code == "E_UNRESOLVED_DATA"
                    && diagnostic.message.contains("WS_FIELD"))
        }));
    }

    #[test]
    fn display_current_date_function_fails_closed_without_unresolved_data() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FUNCDISP.
PROCEDURE DIVISION.
MAIN.
DISPLAY FUNCTION CURRENT-DATE.
STOP RUN.
"#;
        let ast = parse_program("func-disp.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let StatementIr::Display(values) = &ir.paragraphs[0].statements[0] else {
            panic!("expected DISPLAY");
        };
        assert!(matches!(
            values.as_slice(),
            [OperandIr::Function(FunctionOperandIr::UserDefined { name, raw, .. })]
                if name == "CURRENT-DATE" && raw == "FUNCTION CURRENT-DATE"
        ));
        assert!(has_diagnostic(&ir, "E_UNSUPPORTED_FUNCTION_OPERAND"));
        assert!(ir
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "E_UNRESOLVED_DATA"));
    }

    #[test]
    fn display_function_length_of_resolves_argument_as_intrinsic() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FUNCLENOF.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TEXT PIC X(3).
PROCEDURE DIVISION.
MAIN.
DISPLAY FUNCTION LENGTH OF WS-TEXT.
STOP RUN.
"#;
        let ast = parse_program("func-len-of.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let StatementIr::Display(values) = &ir.paragraphs[0].statements[0] else {
            panic!("expected DISPLAY");
        };
        assert!(matches!(
            values.as_slice(),
            [OperandIr::Function(FunctionOperandIr::Length(arg))]
                if matches!(arg.as_ref(), ConditionOperandIr::Identifier(reference) if reference.normalized == "WS_TEXT")
        ));
        assert!(ir.diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "E_INVALID_FUNCTION_ARITY"
                && diagnostic.code != "E_UNSUPPORTED_FUNCTION_OPERAND"
                && !(diagnostic.code == "E_UNRESOLVED_DATA"
                    && diagnostic.message.contains("WS_TEXT"))
        }));
    }

    #[test]
    fn display_function_length_of_reference_modified_argument_stays_intrinsic() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FUNCLENREFMOD.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TEXT PIC X(5) VALUE "ABCDE".
PROCEDURE DIVISION.
MAIN.
DISPLAY FUNCTION LENGTH OF WS-TEXT(2:3).
STOP RUN.
"#;
        let ast = parse_program("func-len-refmod.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let StatementIr::Display(values) = &ir.paragraphs[0].statements[0] else {
            panic!("expected DISPLAY");
        };
        let [OperandIr::Function(FunctionOperandIr::Length(arg))] = values.as_slice() else {
            panic!("expected DISPLAY FUNCTION LENGTH, got {values:?}");
        };
        let ConditionOperandIr::Identifier(reference) = arg.as_ref() else {
            panic!("expected identifier argument, got {arg:?}");
        };
        assert_eq!(reference.normalized, "WS_TEXT");
        let modifier = reference
            .reference_modifier
            .as_ref()
            .expect("reference modifier");
        assert_eq!(modifier.start, "2");
        assert_eq!(modifier.length.as_deref(), Some("3"));
        assert!(ir.diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "E_INVALID_FUNCTION_ARITY"
                && diagnostic.code != "E_INVALID_FUNCTION_ARGUMENT"
                && diagnostic.code != "E_UNSUPPORTED_FUNCTION_OPERAND"
                && !(diagnostic.code == "E_UNRESOLVED_DATA"
                    && diagnostic.message.contains("WS_TEXT"))
        }));
    }

    #[test]
    fn display_function_length_of_qualified_argument_stays_single_intrinsic_operand() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FUNCLENOFQ.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-GROUP.
   05 WS-ITEM PIC X(3).
PROCEDURE DIVISION.
MAIN.
DISPLAY FUNCTION LENGTH OF WS-ITEM OF WS-GROUP.
STOP RUN.
"#;
        let ast = parse_program("func-len-of-qualified.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let StatementIr::Display(values) = &ir.paragraphs[0].statements[0] else {
            panic!("expected DISPLAY");
        };
        assert!(matches!(
            values.as_slice(),
            [OperandIr::Function(FunctionOperandIr::Length(arg))]
                if matches!(arg.as_ref(), ConditionOperandIr::Identifier(reference) if reference.normalized == "WS_GROUP.WS_ITEM")
        ));
        assert!(ir.diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "E_INVALID_FUNCTION_ARITY"
                && diagnostic.code != "E_UNSUPPORTED_FUNCTION_OPERAND"
                && !(diagnostic.code == "E_UNRESOLVED_DATA"
                    && (diagnostic.message.contains("WS_ITEM")
                        || diagnostic.message.contains("WS_GROUP")))
        }));
    }

    #[test]
    fn display_function_literal_parentheses_do_not_consume_following_statement() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FUNCLITPAR.
PROCEDURE DIVISION.
MAIN.
DISPLAY FUNCTION LENGTH("(") DISPLAY "NEXT".
STOP RUN.
"#;
        let ast = parse_program("func-literal-paren.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        assert_eq!(ir.paragraphs[0].statements.len(), 3);
        assert!(matches!(
            &ir.paragraphs[0].statements[0],
            StatementIr::Display(values)
                if matches!(
                    values.as_slice(),
                    [OperandIr::Function(FunctionOperandIr::Length(arg))]
                        if matches!(arg.as_ref(), ConditionOperandIr::Literal(value) if value == "(")
                )
        ));
        assert!(matches!(
            &ir.paragraphs[0].statements[1],
            StatementIr::Display(values)
                if matches!(values.as_slice(), [OperandIr::Literal(value)] if value == "NEXT")
        ));
        assert!(ir.diagnostics.iter().all(|diagnostic| {
            diagnostic.code != "E_INVALID_FUNCTION_ARITY"
                && diagnostic.code != "E_INVALID_FUNCTION_ARGUMENT"
                && diagnostic.code != "E_UNSUPPORTED_FUNCTION_OPERAND"
                && diagnostic.code != "E_UNSUPPORTED_DISPLAY_NO_ADVANCING"
        }));
    }

    #[test]
    fn display_intrinsic_function_without_required_arg_fails_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FUNCBAD.
PROCEDURE DIVISION.
MAIN.
DISPLAY FUNCTION LENGTH.
STOP RUN.
"#;
        let ast = parse_program("func-missing-arg.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let StatementIr::Display(values) = &ir.paragraphs[0].statements[0] else {
            panic!("expected DISPLAY");
        };
        assert!(matches!(
            values.as_slice(),
            [OperandIr::Function(FunctionOperandIr::UserDefined { name, args, .. })]
                if name == "LENGTH" && args.is_empty()
        ));
        assert!(has_diagnostic(&ir, "E_INVALID_FUNCTION_ARITY"));
        assert!(ir
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "E_UNRESOLVED_DATA"));
    }

    #[test]
    fn display_intrinsic_function_ambiguous_comma_args_fail_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FUNCBADARGS.
PROCEDURE DIVISION.
MAIN.
DISPLAY FUNCTION LENGTH("A", "B").
STOP RUN.
"#;
        let ast = parse_program("func-comma-args.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let StatementIr::Display(values) = &ir.paragraphs[0].statements[0] else {
            panic!("expected DISPLAY");
        };
        assert!(matches!(
            values.as_slice(),
            [OperandIr::Function(FunctionOperandIr::UserDefined { name, args, raw })]
                if name == "LENGTH" && args.len() == 2 && raw == "FUNCTION LENGTH(\"A\", \"B\")"
        ));
        assert!(has_diagnostic(&ir, "E_INVALID_FUNCTION_ARITY"));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_FUNCTION_ARITY"
                && diagnostic.message.contains("FUNCTION LENGTH(\"A\", \"B\")")
        }));
        assert!(ir
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "E_UNRESOLVED_DATA"));
    }

    #[test]
    fn if_intrinsic_function_without_required_arg_fails_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FUNCBADIF.
PROCEDURE DIVISION.
MAIN.
IF FUNCTION LENGTH = 0 DISPLAY "BAD".
STOP RUN.
"#;
        let ast = parse_program("func-missing-arg-if.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        assert!(has_diagnostic(&ir, "E_INVALID_FUNCTION_ARITY"));
        assert!(ir
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "E_UNRESOLVED_DATA"));
    }

    #[test]
    fn if_intrinsic_function_ambiguous_comma_args_fail_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FUNCBADIFARGS.
PROCEDURE DIVISION.
MAIN.
IF FUNCTION LENGTH("A", "B") = 1 DISPLAY "BAD".
STOP RUN.
"#;
        let ast = parse_program("func-comma-args-if.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let StatementIr::If { condition, .. } = &ir.paragraphs[0].statements[0] else {
            panic!("expected IF");
        };
        assert_eq!(condition, "FUNCTION LENGTH(\"A\", \"B\") = 1");
        assert!(has_diagnostic(&ir, "E_INVALID_FUNCTION_ARITY"));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_FUNCTION_ARITY"
                && diagnostic.message.contains("FUNCTION LENGTH(\"A\", \"B\")")
        }));
        assert!(ir
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "E_UNRESOLVED_DATA"));
    }

    #[test]
    fn if_intrinsic_function_unclosed_parenthesis_fails_closed_as_function() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FUNCBADIFPAREN.
PROCEDURE DIVISION.
MAIN.
IF FUNCTION LENGTH("A" = 1 DISPLAY "BAD".
STOP RUN.
"#;
        let ast = parse_program("func-unclosed-paren-if.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        assert!(has_diagnostic(&ir, "E_INVALID_FUNCTION_ARGUMENT"));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_FUNCTION_ARGUMENT"
                && diagnostic.message.contains("FUNCTION LENGTH(\"A\"")
        }));
        assert!(ir
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "E_UNRESOLVED_DATA"));
    }

    #[test]
    fn compute_function_ord_is_not_treated_as_data_reference() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FUNCORD.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-N PIC 999.
PROCEDURE DIVISION.
MAIN.
COMPUTE WS-N = FUNCTION ORD("A").
STOP RUN.
"#;
        let ast = parse_program("func-ord.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let StatementIr::Compute { expression, .. } = &ir.paragraphs[0].statements[0] else {
            panic!("expected COMPUTE");
        };
        assert_eq!(expression, "FUNCTION ORD(\"A\")");
        assert!(ir.diagnostics.iter().all(|diagnostic| {
            !(diagnostic.code == "E_UNRESOLVED_DATA"
                && (diagnostic.message.contains("FUNCTION") || diagnostic.message.contains("ORD")))
        }));
    }

    #[test]
    fn compute_intrinsic_function_ambiguous_comma_args_fail_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FUNCBADCARGS.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-N PIC 999.
PROCEDURE DIVISION.
MAIN.
COMPUTE WS-N = FUNCTION LENGTH("A", "B").
STOP RUN.
"#;
        let ast = parse_program("func-comma-args-compute.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let StatementIr::Compute { expression, .. } = &ir.paragraphs[0].statements[0] else {
            panic!("expected COMPUTE");
        };
        assert_eq!(expression, "FUNCTION LENGTH(\"A\", \"B\")");
        assert!(has_diagnostic(&ir, "E_INVALID_FUNCTION_ARITY"));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_FUNCTION_ARITY"
                && diagnostic.message.contains("FUNCTION LENGTH(\"A\", \"B\")")
        }));
        assert!(ir
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "E_UNRESOLVED_DATA"));
    }

    #[test]
    fn compute_intrinsic_function_unclosed_parenthesis_fails_closed_as_function() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FUNCBADCPAREN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-N PIC 999.
PROCEDURE DIVISION.
MAIN.
COMPUTE WS-N = FUNCTION LENGTH("A".
STOP RUN.
"#;
        let ast = parse_program("func-unclosed-paren-compute.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        assert!(has_diagnostic(&ir, "E_INVALID_FUNCTION_ARGUMENT"));
        assert!(ir.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_FUNCTION_ARGUMENT"
                && diagnostic.message.contains("FUNCTION LENGTH(\"A\"")
        }));
        assert!(ir
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "E_UNRESOLVED_DATA"));
    }

    #[test]
    fn compute_intrinsic_function_without_required_arg_fails_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FUNCBADC.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-N PIC 999.
PROCEDURE DIVISION.
MAIN.
COMPUTE WS-N = FUNCTION LENGTH.
STOP RUN.
"#;
        let ast = parse_program("func-missing-arg-compute.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        assert!(has_diagnostic(&ir, "E_INVALID_FUNCTION_ARITY"));
        assert!(ir
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "E_UNRESOLVED_DATA"));
    }

    #[test]
    fn compute_function_length_resolves_argument_without_fake_function_refs() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FUNCLENC.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-N PIC 999.
01 WS-TEXT PIC X(3).
PROCEDURE DIVISION.
MAIN.
COMPUTE WS-N = FUNCTION LENGTH(WS-TEXT).
STOP RUN.
"#;
        let ast = parse_program("func-len-compute.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        assert!(ir.diagnostics.iter().all(|diagnostic| {
            !(diagnostic.code == "E_UNRESOLVED_DATA"
                && (diagnostic.message.contains("FUNCTION")
                    || diagnostic.message.contains("LENGTH")
                    || diagnostic.message.contains("WS_TEXT")))
        }));
    }

    #[test]
    fn search_all_resolves_declared_occurs_key_descriptor() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SEARCHKEY.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TABLE.
   05 WS-ITEM OCCURS 3 TIMES ASCENDING KEY IS WS-KEY INDEXED BY WS-IDX.
      10 WS-KEY PIC 9.
      10 WS-TEXT PIC X.
PROCEDURE DIVISION.
MAIN.
SEARCH ALL WS-ITEM
    WHEN WS-KEY(WS-IDX) = 2 DISPLAY "FOUND"
END-SEARCH.
STOP RUN.
"#;
        let ast = parse_program("search-key.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let table = ir
            .data_items
            .iter()
            .find(|item| item.qualified_name == "WS_TABLE.WS_ITEM")
            .expect("table item");
        let occurs = table.occurs.as_ref().expect("occurs");
        assert_eq!(occurs.keys.len(), 1);
        assert_eq!(occurs.keys[0].direction, OccursKeyDirectionIr::Ascending);
        assert_eq!(occurs.keys[0].name, "WS_KEY");

        let StatementIr::SearchAll(search) = &ir.paragraphs[0].statements[0] else {
            panic!("expected SEARCH ALL statement");
        };
        let declared_key = search.declared_key.as_ref().expect("declared key");
        assert_eq!(declared_key.direction, OccursKeyDirectionIr::Ascending);
        assert_eq!(declared_key.name, "WS_KEY");
        assert_eq!(declared_key.qualified_name, "WS_TABLE.WS_ITEM.WS_KEY");
        assert!(declared_key.children.is_empty());
        assert!(ir
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "E_UNSUPPORTED_SEARCH_ALL"));
        assert!(ir
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "E_SEARCH_ALL_KEY_UNRESOLVED"));
    }

    #[test]
    fn search_all_declared_key_resolves_inside_file_io_branch() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SEARCHBRANCH.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT INFILE ASSIGN TO "INFILE".
DATA DIVISION.
FILE SECTION.
FD INFILE.
01 IN-REC PIC X.
WORKING-STORAGE SECTION.
01 WS-TABLE.
   05 WS-ITEM OCCURS 3 TIMES ASCENDING KEY IS WS-KEY INDEXED BY WS-IDX.
      10 WS-KEY PIC 9.
PROCEDURE DIVISION.
MAIN.
READ INFILE
    AT END
        SEARCH ALL WS-ITEM
            WHEN WS-KEY(WS-IDX) = 2 DISPLAY "FOUND"
        END-SEARCH
END-READ.
STOP RUN.
"#;
        let ir = analyze_src("search-all-branch.cbl", src);
        let StatementIr::ReadFile(read) = &ir.paragraphs[0].statements[0] else {
            panic!("expected READ");
        };
        let Some(StatementIr::SearchAll(search)) = read.at_end_ops.first() else {
            panic!("expected nested SEARCH ALL");
        };
        let declared_key = search.declared_key.as_ref().expect("declared key");
        assert_eq!(declared_key.qualified_name, "WS_TABLE.WS_ITEM.WS_KEY");
        assert!(ir
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "E_SEARCH_ALL_KEY_UNRESOLVED"));
    }

    #[test]
    fn search_all_requires_declared_occurs_key() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SEARCHNOKEY.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TABLE.
   05 WS-ITEM OCCURS 3 TIMES INDEXED BY WS-IDX PIC X.
PROCEDURE DIVISION.
MAIN.
SEARCH ALL WS-ITEM
    WHEN WS-ITEM(WS-IDX) = "A" DISPLAY "FOUND"
END-SEARCH.
STOP RUN.
"#;
        let ast = parse_program("search-no-key.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let StatementIr::SearchAll(search) = &ir.paragraphs[0].statements[0] else {
            panic!("expected SEARCH ALL statement");
        };
        assert!(search.declared_key.is_none());
        assert!(ir
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E_SEARCH_ALL_REQUIRES_KEY"));
    }

    #[test]
    fn occurs_indexed_by_list_ignores_comma_separators() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. OCCURSIDX.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TABLE.
   05 WS-ITEM OCCURS 4 TIMES INDEXED BY WS-IDX, WS-IDY PIC X.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#;
        let ast = parse_program("occurs-indexed.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        let item = ir
            .data_items
            .iter()
            .find(|item| item.qualified_name == "WS_TABLE.WS_ITEM")
            .expect("table item");
        let occurs = item.occurs.as_ref().expect("occurs");
        assert_eq!(occurs.indexed_by, vec!["WS_IDX", "WS_IDY"]);
    }

    #[test]
    fn search_all_without_declared_key_remains_blocked_even_when_condition_resolves() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SEARCHNOKEY2.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TABLE.
   05 WS-ITEM OCCURS 3 TIMES INDEXED BY WS-IDX.
      10 WS-KEY PIC X.
      10 WS-FLAG PIC X.
PROCEDURE DIVISION.
MAIN.
SEARCH ALL WS-ITEM
    WHEN WS-KEY(WS-IDX) = "A" MOVE "Y" TO WS-FLAG(WS-IDX)
END-SEARCH.
STOP RUN.
"#;
        let ir = analyze_src("search-no-key-resolved-condition.cbl", src);
        let StatementIr::SearchAll(search) = &ir.paragraphs[0].statements[0] else {
            panic!("expected SEARCH ALL statement");
        };
        assert!(search.declared_key.is_none());
        assert!(has_diagnostic(&ir, "E_SEARCH_ALL_REQUIRES_KEY"));
        assert!(ir
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code != "E_SEARCH_ALL_KEY_UNRESOLVED"));
    }

    #[test]
    fn search_all_blocks_non_equality_key_condition() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SEARCHBADCOND.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TABLE.
   05 WS-ITEM OCCURS 3 TIMES ASCENDING KEY IS WS-KEY INDEXED BY WS-IDX.
      10 WS-KEY PIC 9.
PROCEDURE DIVISION.
MAIN.
SEARCH ALL WS-ITEM
    WHEN WS-KEY(WS-IDX) > 2 DISPLAY "FOUND"
END-SEARCH.
STOP RUN.
"#;
        let ast = parse_program("search-bad-cond.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        assert!(ir
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E_UNSUPPORTED_SEARCH_ALL_CONDITION"));
    }

    #[test]
    fn serial_search_condition_type_mismatch_fails_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SEARCHMIXED.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TABLE.
   05 WS-ITEM OCCURS 3 TIMES INDEXED BY WS-IDX.
      10 WS-NUM PIC 9.
      10 WS-TEXT PIC X.
PROCEDURE DIVISION.
MAIN.
SEARCH WS-ITEM
    WHEN WS-NUM(WS-IDX) = WS-TEXT(WS-IDX) DISPLAY "FOUND"
END-SEARCH.
STOP RUN.
"#;
        let ir = analyze_src("search-condition-mixed.cbl", src);

        assert!(
            has_diagnostic(&ir, "E_CONDITION_TYPE_MISMATCH"),
            "{:#?}",
            ir.diagnostics
        );
    }

    #[test]
    fn perform_until_condition_type_mismatch_fails_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. PERFMIXED.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-NUM PIC 9.
01 WS-TEXT PIC X.
PROCEDURE DIVISION.
MAIN.
PERFORM BODY UNTIL WS-NUM = WS-TEXT.
STOP RUN.
BODY.
DISPLAY "BODY".
"#;
        let ir = analyze_src("perform-until-mixed.cbl", src);
        let StatementIr::Perform {
            until, until_tree, ..
        } = &ir.paragraphs[0].statements[0]
        else {
            panic!("expected PERFORM statement");
        };
        assert_eq!(until.as_deref(), Some("WS-NUM = WS-TEXT"));
        assert!(until_tree.is_some());

        assert!(
            has_diagnostic(&ir, "E_CONDITION_TYPE_MISMATCH"),
            "{:#?}",
            ir.diagnostics
        );
    }

    #[test]
    fn perform_times_count_reference_must_resolve() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. PERFTIMEMISS.
PROCEDURE DIVISION.
MAIN.
PERFORM BODY MISSING-COUNT TIMES.
STOP RUN.
BODY.
DISPLAY "BODY".
"#;
        let ir = analyze_src("perform-times-missing.cbl", src);
        let StatementIr::Perform { times, .. } = &ir.paragraphs[0].statements[0] else {
            panic!("expected PERFORM statement");
        };
        assert!(matches!(
            times,
            Some(OperandIr::Identifier(reference)) if reference.raw == "MISSING-COUNT"
        ));

        assert!(ir.semantic.references.iter().any(|reference| {
            reference.raw == "MISSING-COUNT"
                && reference.role == ReferenceRoleIr::ArithmeticSource
                && reference.status == ReferenceResolutionStatusIr::Missing
        }));
        assert!(has_diagnostic(&ir, "E_UNRESOLVED_DATA"));
    }

    #[test]
    fn perform_varying_operands_must_resolve() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. PERFVARMISS.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FLAG PIC X.
PROCEDURE DIVISION.
MAIN.
PERFORM BODY VARYING MISSING-IDX FROM 1 BY MISSING-STEP UNTIL WS-FLAG = "Y".
STOP RUN.
BODY.
DISPLAY "BODY".
"#;
        let ir = analyze_src("perform-varying-missing.cbl", src);
        let StatementIr::Perform {
            varying_ir,
            until_tree,
            ..
        } = &ir.paragraphs[0].statements[0]
        else {
            panic!("expected PERFORM statement");
        };
        assert!(varying_ir.is_some());
        assert!(until_tree.is_some());

        assert!(ir.semantic.references.iter().any(|reference| {
            reference.raw == "MISSING-IDX"
                && reference.role == ReferenceRoleIr::ArithmeticTarget
                && reference.status == ReferenceResolutionStatusIr::Missing
        }));
        assert!(ir.semantic.references.iter().any(|reference| {
            reference.raw == "MISSING-STEP"
                && reference.role == ReferenceRoleIr::ArithmeticSource
                && reference.status == ReferenceResolutionStatusIr::Missing
        }));
        assert!(has_diagnostic(&ir, "E_UNRESOLVED_DATA"));
    }

    #[test]
    fn perform_varying_after_does_not_fabricate_nested_by_reference() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. PERFVAFT.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 I PIC 9 VALUE 1.
01 J PIC 9 VALUE 1.
PROCEDURE DIVISION.
MAIN.
PERFORM BODY VARYING I FROM 1 BY 1 AFTER J FROM 1 BY 1 UNTIL I > 2.
STOP RUN.
BODY.
DISPLAY I J.
"#;
        let ir = analyze_src("perform-varying-after.cbl", src);
        let StatementIr::Perform { varying_ir, .. } = &ir.paragraphs[0].statements[0] else {
            panic!("expected PERFORM statement");
        };

        assert!(
            varying_ir.is_none(),
            "nested PERFORM VARYING AFTER is unsupported and must not fabricate a partial varying IR"
        );
        assert!(!ir.semantic.references.iter().any(|reference| {
            reference.raw == "1 AFTER J FROM 1 BY 1"
                && reference.status == ReferenceResolutionStatusIr::Missing
        }));
    }

    #[test]
    fn search_all_key_condition_type_mismatch_fails_closed() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SEARCHALLMIXED.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TABLE.
   05 WS-ITEM OCCURS 3 TIMES ASCENDING KEY IS WS-KEY INDEXED BY WS-IDX.
      10 WS-KEY PIC 9.
      10 WS-TEXT PIC X.
PROCEDURE DIVISION.
MAIN.
SEARCH ALL WS-ITEM
    WHEN WS-KEY(WS-IDX) = WS-TEXT(WS-IDX) DISPLAY "FOUND"
END-SEARCH.
STOP RUN.
"#;
        let ir = analyze_src("search-all-condition-mixed.cbl", src);

        assert!(
            has_diagnostic(&ir, "E_CONDITION_TYPE_MISMATCH"),
            "{:#?}",
            ir.diagnostics
        );
    }
}
