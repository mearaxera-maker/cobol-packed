use cobol_ir::{
    CallTargetIr, ClassTestIr, CobolDialect, ConditionIr, ConditionOperandIr, ControlFlowIr,
    DataRefIr, DeclarativeTriggerIr, Diagnostic, DialectProfileIr, EvaluateIr, EvaluatePatternIr,
    EvaluateSubjectIr, FigurativeConstantIr, FileIr, FileKindIr, FunctionOperandIr,
    OccursKeyDirectionIr, OperandIr, PerformVaryingIr, ProgramIr, ReferenceModifierIr, RelOpIr,
    SemanticModelIr, SetIndexOperationIr, Severity, SignTestIr, SortDirectionIr, SourceSpan,
    StatementIr, StorageAreaIr, StoragePlanIr, UsageIr, ValueCategoryIr,
};
use cobol_sema::{
    analyze_with_catalog, parse_data_ref as parse_sema_data_ref, Dialect as SemaDialect,
    ProgramCatalog,
};
use cobol_source::{preprocess_file, PreprocessedSource, SourceError};
pub use cobol_source::{Dialect, SourceFormat};
use cobol_syntax::{parse_programs, SyntaxError};
use rust_decimal::Decimal;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::str::FromStr;

#[derive(Debug, Clone)]
pub struct ConvertOptions {
    pub input: PathBuf,
    pub copybook_dirs: Vec<PathBuf>,
    pub out_dir: PathBuf,
    pub dialect: Dialect,
    pub source_format: SourceFormat,
}

#[derive(Debug, Clone, Serialize)]
pub struct GeneratedProject {
    pub out_dir: PathBuf,
    pub files: Vec<PathBuf>,
    pub report_path: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct MigrationReport {
    pub version: u8,
    pub status: String,
    pub source: String,
    pub dialect: String,
    pub source_format: String,
    pub includes: Vec<String>,
    pub generated_files: Vec<String>,
    pub diagnostics: Vec<Diagnostic>,
    pub diagnostic_sections: DiagnosticSections,
    pub dialect_profile: DialectProfileIr,
    pub storage: StoragePlanIr,
    pub semantic: SemanticModelIr,
    pub control_flow: ControlFlowIr,
    pub procedure_cfg: cobol_ir::ProcedureCfgIr,
    pub files: Vec<cobol_ir::FileIr>,
    pub indexes: Vec<cobol_ir::IndexItemIr>,
    pub odo: Vec<cobol_ir::OdoDescriptorIr>,
    pub program_units: Vec<cobol_ir::ProgramUnitIr>,
    pub stats: ReportStats,
}

#[derive(Debug, Serialize)]
pub struct DiagnosticSections {
    pub source: Vec<Diagnostic>,
    pub syntax: Vec<Diagnostic>,
    pub symbols: Vec<Diagnostic>,
    pub layout: Vec<Diagnostic>,
    pub references: Vec<Diagnostic>,
    pub conditions: Vec<Diagnostic>,
    pub evaluate: Vec<Diagnostic>,
    pub vm: Vec<Diagnostic>,
    pub procedure: Vec<Diagnostic>,
    pub cfg: Vec<Diagnostic>,
    pub indexes: Vec<Diagnostic>,
    pub search: Vec<Diagnostic>,
    pub odo: Vec<Diagnostic>,
    pub file_io: Vec<Diagnostic>,
    pub nested_programs: Vec<Diagnostic>,
    pub national_dbcs: Vec<Diagnostic>,
    pub oracle: Vec<Diagnostic>,
    pub codegen: Vec<Diagnostic>,
}

#[derive(Debug, Serialize)]
pub struct ReportStats {
    pub data_items: usize,
    pub storage_items: usize,
    pub storage_bytes: usize,
    pub paragraphs: usize,
    pub statements: usize,
    pub cfg_edges: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CallTempDescriptor {
    caller_program: String,
    name: String,
    byte_len: usize,
    category: ValueCategoryIr,
    usage: UsageIr,
    picture: Option<cobol_ir::PicIr>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConvertError {
    #[error("{0}")]
    Source(#[from] SourceError),
    #[error("{0}")]
    Syntax(#[from] SyntaxError),
    #[error("failed to write {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("migration blocked; report written to {report_path}")]
    MigrationBlocked { report_path: PathBuf },
    #[error("failed to serialize migration report: {0}")]
    Report(#[from] serde_json::Error),
}

pub fn convert(options: ConvertOptions) -> Result<GeneratedProject, ConvertError> {
    fs::create_dir_all(&options.out_dir).map_err(|source| ConvertError::Io {
        path: options.out_dir.clone(),
        source,
    })?;

    let preprocessed = preprocess_file(
        &options.input,
        &options.copybook_dirs,
        options.source_format,
    )?;
    let programs = parse_and_analyze_compilation(&preprocessed, options.dialect)?;
    let report_ir = report_program_ir(&programs);

    let report_path = options.out_dir.join("migration-report.json");
    if programs.iter().any(ProgramIr::has_errors) {
        cleanup_generated_artifacts(&options.out_dir)?;
        write_report(
            &report_path,
            &build_report("blocked", &options, &preprocessed, &report_ir, &[]),
        )?;
        return Err(ConvertError::MigrationBlocked { report_path });
    }

    let generated = if programs.len() == 1 {
        write_generated_project(&options.out_dir, &programs[0])?
    } else {
        write_generated_project_multi(&options.out_dir, &programs)?
    };
    write_report(
        &report_path,
        &build_report("generated", &options, &preprocessed, &report_ir, &generated),
    )?;
    Ok(GeneratedProject {
        out_dir: options.out_dir,
        files: generated,
        report_path,
    })
}

fn cleanup_generated_artifacts(out_dir: &Path) -> Result<(), ConvertError> {
    for dir in [out_dir.join("src"), out_dir.join("vendor")] {
        match fs::remove_dir_all(&dir) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(source) => return Err(ConvertError::Io { path: dir, source }),
        }
    }
    for file in [out_dir.join("Cargo.toml"), out_dir.join("Cargo.lock")] {
        match fs::remove_file(&file) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(source) => return Err(ConvertError::Io { path: file, source }),
        }
    }
    Ok(())
}

fn parse_and_analyze_compilation(
    preprocessed: &PreprocessedSource,
    dialect: Dialect,
) -> Result<Vec<ProgramIr>, ConvertError> {
    let source_name = preprocessed.primary_path.to_string_lossy();
    let asts = parse_programs(&source_name, &preprocessed.text)?;
    let dialect = match dialect {
        Dialect::Ibm => SemaDialect::Ibm,
        Dialect::GnuCobol => SemaDialect::GnuCobol,
        Dialect::MicroFocus => SemaDialect::MicroFocus,
    };
    let catalog = ProgramCatalog::from_asts(&asts);
    let mut programs = asts
        .into_iter()
        .map(|ast| analyze_with_catalog(ast, dialect, &catalog))
        .collect::<Vec<_>>();
    if let Some(first) = programs.first_mut() {
        first
            .diagnostics
            .extend(preflight_diagnostics(&preprocessed.text, &source_name));
        first.diagnostics = dedupe_diagnostics(first.diagnostics.clone());
    }
    validate_external_storage(&mut programs);
    if !programs.iter().any(ProgramIr::has_errors) {
        validate_codegen_invariants(&mut programs);
    }
    Ok(programs)
}

fn validate_external_storage(programs: &mut [ProgramIr]) {
    let mut seen = BTreeMap::<String, (String, usize, ValueCategoryIr, UsageIr)>::new();
    let mut pending = Vec::<(usize, Diagnostic)>::new();
    for (program_idx, program) in programs.iter().enumerate() {
        for item in program.storage.items.iter().filter(|item| {
            item.external && item.addressable && item.value_category != ValueCategoryIr::Group
        }) {
            let external_name = normalize_vm_ref(&item.qualified_name);
            let signature = (
                item.qualified_name.clone(),
                item.byte_len,
                item.value_category,
                item.usage.clone(),
            );
            if let Some((first_name, first_len, first_category, first_usage)) =
                seen.get(&external_name)
            {
                if *first_len != item.byte_len
                    || *first_category != item.value_category
                    || *first_usage != item.usage
                {
                    pending.push((
                        program_idx,
                        Diagnostic::error(
                            "E_EXTERNAL_TYPE_MISMATCH",
                            format!(
                                "EXTERNAL item {} does not match prior declaration {}: got {:?}/{:?}/{} bytes, expected {:?}/{:?}/{} bytes",
                                item.qualified_name,
                                first_name,
                                item.value_category,
                                item.usage,
                                item.byte_len,
                                first_category,
                                first_usage,
                                first_len
                            ),
                            item.span.clone(),
                        ),
                    ));
                }
            } else {
                seen.insert(external_name, signature);
            }
        }
    }
    for (program_idx, diagnostic) in pending {
        if let Some(program) = programs.get_mut(program_idx) {
            program.diagnostics.push(diagnostic);
            program.diagnostics = dedupe_diagnostics(program.diagnostics.clone());
        }
    }
}

fn validate_codegen_invariants(programs: &mut [ProgramIr]) {
    for program in programs {
        let diagnostics = collect_codegen_invariant_diagnostics(program);
        if diagnostics.is_empty() {
            continue;
        }
        program.diagnostics.extend(diagnostics);
        program.diagnostics = dedupe_diagnostics(program.diagnostics.clone());
    }
}

fn collect_codegen_invariant_diagnostics(program: &ProgramIr) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    collect_storage_codegen_invariants(program, &mut diagnostics);
    for paragraph in &program.paragraphs {
        collect_statement_codegen_invariants(
            program,
            &paragraph.statements,
            &paragraph.span,
            &mut diagnostics,
        );
    }
    for declarative in &program.declaratives {
        collect_statement_codegen_invariants(
            program,
            &declarative.statements,
            &declarative.span,
            &mut diagnostics,
        );
    }
    diagnostics
}

fn collect_storage_codegen_invariants(program: &ProgramIr, diagnostics: &mut Vec<Diagnostic>) {
    for item in &program.storage.items {
        if !item.addressable || item.value_category != ValueCategoryIr::PackedDecimal {
            continue;
        }
        if let Err(message) = packed_decimal_initial_bytes(item, item.byte_len) {
            diagnostics.push(Diagnostic::error(
                "E_CODEGEN_PACKED_DECIMAL_INITIAL_VALUE",
                format!(
                    "packed decimal item {} cannot be initialized safely: {message}",
                    item.qualified_name
                ),
                item.span.clone(),
            ));
        }
    }
}

fn collect_statement_codegen_invariants(
    program: &ProgramIr,
    statements: &[StatementIr],
    span: &SourceSpan,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for statement in statements {
        match statement {
            StatementIr::Unsupported { keyword, raw } => diagnostics.push(Diagnostic::error(
                "E_CODEGEN_UNSUPPORTED_STATEMENT",
                format!(
                    "unsupported COBOL statement {keyword} reached code generation invariant validation: {raw}"
                ),
                span.clone(),
            )),
            StatementIr::NextSentence => diagnostics.push(Diagnostic::error(
                "E_CODEGEN_NEXT_SENTENCE_UNLOWERED",
                "NEXT SENTENCE reached code generation without executable period-scope lowering",
                span.clone(),
            )),
            StatementIr::SearchAll(search) => {
                if search.declared_key.is_none() || search_all_target_operand(search).is_none() {
                    diagnostics.push(Diagnostic::error(
                        "E_CODEGEN_SEARCH_ALL_UNLOWERED",
                        format!(
                            "SEARCH ALL for table {} reached code generation without a fully lowered key equality",
                            search.table
                        ),
                        span.clone(),
                    ));
                }
                collect_statement_codegen_invariants(
                    program,
                    &search.at_end,
                    span,
                    diagnostics,
                );
                collect_statement_codegen_invariants(
                    program,
                    &search.statements,
                    span,
                    diagnostics,
                );
            }
            StatementIr::MoveCorresponding { source, target }
                if storage_item_for_ref(source, program).is_none()
                    || storage_item_for_ref(target, program).is_none() =>
            {
                diagnostics.push(Diagnostic::error(
                    "E_CODEGEN_MOVE_CORRESPONDING_UNLOWERED",
                    format!(
                        "MOVE CORRESPONDING {} TO {} reached code generation without resolved group metadata",
                        source.raw, target.raw
                    ),
                    span.clone(),
                ));
            }
            StatementIr::Compute {
                on_size_error_ops,
                not_on_size_error_ops,
                ..
            } => {
                collect_statement_codegen_invariants(
                    program,
                    on_size_error_ops,
                    span,
                    diagnostics,
                );
                collect_statement_codegen_invariants(
                    program,
                    not_on_size_error_ops,
                    span,
                    diagnostics,
                );
            }
            StatementIr::If {
                then_statements,
                else_statements,
                ..
            } => {
                collect_statement_codegen_invariants(
                    program,
                    then_statements,
                    span,
                    diagnostics,
                );
                collect_statement_codegen_invariants(
                    program,
                    else_statements,
                    span,
                    diagnostics,
                );
            }
            StatementIr::Evaluate(evaluate) => {
                for arm in &evaluate.arms {
                    collect_statement_codegen_invariants(
                        program,
                        &arm.statements,
                        span,
                        diagnostics,
                    );
                }
            }
            StatementIr::Search(search) => {
                collect_statement_codegen_invariants(
                    program,
                    &search.at_end,
                    span,
                    diagnostics,
                );
                for when in &search.whens {
                    collect_statement_codegen_invariants(
                        program,
                        &when.statements,
                        span,
                        diagnostics,
                    );
                }
            }
            StatementIr::ReturnSortRecord(ret) => {
                collect_statement_codegen_invariants(
                    program,
                    &ret.at_end_ops,
                    span,
                    diagnostics,
                );
                collect_statement_codegen_invariants(
                    program,
                    &ret.not_at_end_ops,
                    span,
                    diagnostics,
                );
            }
            StatementIr::ReadFile(read) => {
                collect_statement_codegen_invariants(program, &read.at_end_ops, span, diagnostics);
                collect_statement_codegen_invariants(
                    program,
                    &read.not_at_end_ops,
                    span,
                    diagnostics,
                );
                collect_statement_codegen_invariants(
                    program,
                    &read.on_exception_ops,
                    span,
                    diagnostics,
                );
            }
            StatementIr::RewriteFile(rewrite) => {
                collect_statement_codegen_invariants(
                    program,
                    &rewrite.invalid_key_ops,
                    span,
                    diagnostics,
                );
                collect_statement_codegen_invariants(
                    program,
                    &rewrite.not_invalid_key_ops,
                    span,
                    diagnostics,
                );
            }
            StatementIr::DeleteFile(delete) => {
                collect_statement_codegen_invariants(
                    program,
                    &delete.invalid_key_ops,
                    span,
                    diagnostics,
                );
                collect_statement_codegen_invariants(
                    program,
                    &delete.not_invalid_key_ops,
                    span,
                    diagnostics,
                );
            }
            StatementIr::StringOp(string) => {
                collect_statement_codegen_invariants(
                    program,
                    &string.on_overflow_ops,
                    span,
                    diagnostics,
                );
                collect_statement_codegen_invariants(
                    program,
                    &string.not_on_overflow_ops,
                    span,
                    diagnostics,
                );
            }
            StatementIr::UnstringOp(unstring) => {
                collect_statement_codegen_invariants(
                    program,
                    &unstring.on_overflow_ops,
                    span,
                    diagnostics,
                );
                collect_statement_codegen_invariants(
                    program,
                    &unstring.not_on_overflow_ops,
                    span,
                    diagnostics,
                );
            }
            _ => {}
        }
    }
}

fn report_program_ir(programs: &[ProgramIr]) -> ProgramIr {
    let mut report = programs
        .first()
        .cloned()
        .expect("at least one program is parsed");
    report.diagnostics = programs
        .iter()
        .flat_map(|program| program.diagnostics.clone())
        .collect();
    report.program_units = programs
        .iter()
        .map(|program| cobol_ir::ProgramUnitIr {
            name: program.name.clone(),
            parent: None,
            is_common: program.is_common,
            is_initial: program.is_initial,
            contained_programs: Vec::new(),
            global_items: Vec::new(),
            external_items: program
                .program_units
                .iter()
                .flat_map(|unit| unit.external_items.clone())
                .collect(),
        })
        .collect();
    report
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CobolArea {
    Before,
    Environment,
    Data,
    Procedure,
}

fn preflight_diagnostics(text: &str, source_name: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut area = CobolArea::Before;
    for (idx, raw_line) in text.lines().enumerate() {
        let line_no = idx + 1;
        let masked = mask_literals(raw_line);
        let words = cobol_words(&masked);
        if words.is_empty() {
            continue;
        }
        if has_phrase(&words, &["ENVIRONMENT", "DIVISION"]) {
            area = CobolArea::Environment;
        } else if has_phrase(&words, &["DATA", "DIVISION"]) {
            area = CobolArea::Data;
        } else if has_phrase(&words, &["PROCEDURE", "DIVISION"]) {
            area = CobolArea::Procedure;
        }

        let span = SourceSpan {
            file: source_name.to_string(),
            line: line_no,
            column: 1,
        };

        for phrase in [
            &["SORT", "SECTION"][..],
            &["LOCAL-STORAGE", "SECTION"],
            &["SCREEN", "SECTION"],
            &["REPORT", "SECTION"],
            &["COMMUNICATION", "SECTION"],
        ] {
            if has_phrase(&words, phrase) {
                diagnostics.push(Diagnostic::error(
                    "E_UNSUPPORTED_SECTION",
                    format!("unsupported COBOL section or block: {}", phrase.join(" ")),
                    span.clone(),
                ));
            }
        }

        if area == CobolArea::Environment {
            for word in ["SD"] {
                if has_word(&words, word) {
                    diagnostics.push(Diagnostic::error(
                        "E_UNSUPPORTED_ENVIRONMENT",
                        format!(
                            "Environment Division feature {word} is not lowered by the converter preview"
                        ),
                        span.clone(),
                    ));
                }
            }
            for word in [
                "SEGMENT-LIMIT",
                "DECIMAL-POINT",
                "CURRENCY",
                "CLASS",
                "CHANNEL",
                "MULTIPLE",
            ] {
                if has_word(&words, word) {
                    diagnostics.push(Diagnostic::error(
                        "E_UNSUPPORTED_ENVIRONMENT",
                        format!(
                            "Environment Division feature {word} requires platform/runtime emulation and is not lowered yet"
                        ),
                        span.clone(),
                    ));
                }
            }
            if has_word(&words, "RERUN") && !supported_rerun_shape(&words) {
                diagnostics.push(Diagnostic::error(
                    "E_UNSUPPORTED_ENVIRONMENT",
                    "RERUN is only lowered for `RERUN ON file EVERY n RECORDS OF file` checkpoint snapshots",
                    span.clone(),
                ));
            }
            for phrase in [
                &["LABEL", "RECORDS"][..],
                &["BLOCK", "CONTAINS"],
                &["RESERVE"],
            ] {
                if has_phrase(&words, phrase) {
                    diagnostics.push(Diagnostic::error(
                        "E_UNSUPPORTED_ENVIRONMENT",
                        format!(
                            "Environment/File metadata feature {} requires platform/runtime emulation and is not lowered yet",
                            phrase.join(" ")
                        ),
                        span.clone(),
                    ));
                }
            }
        }

        if area == CobolArea::Data {
            for word in [
                "SIGN",
                "JUSTIFIED",
                "BLANK",
                "GLOBAL",
                "POINTER",
                "PROCEDURE-POINTER",
            ] {
                if has_word(&words, word) {
                    diagnostics.push(Diagnostic::error(
                        "E_UNSUPPORTED_DATA_CLAUSE",
                        format!(
                            "Data Division clause {word} requires real layout/runtime semantics and is not lowered yet"
                        ),
                        span.clone(),
                    ));
                }
            }
            for phrase in [
                &["LABEL", "RECORDS"][..],
                &["BLOCK", "CONTAINS"],
                &["RESERVE"],
            ] {
                if has_phrase(&words, phrase) {
                    diagnostics.push(Diagnostic::error(
                        "E_UNSUPPORTED_ENVIRONMENT",
                        format!(
                            "File metadata feature {} requires platform/runtime emulation and is not lowered yet",
                            phrase.join(" ")
                        ),
                        span.clone(),
                    ));
                }
            }
        }

        if area == CobolArea::Procedure {
            if has_phrase(&words, &["NEXT", "SENTENCE"]) {
                diagnostics.push(Diagnostic::error(
                    "E_UNSUPPORTED_CONTROL_FLOW",
                    "NEXT SENTENCE has sentence-level CFG targets but executable period-scope lowering is not enabled yet",
                    span.clone(),
                ));
            }
            if has_word(&words, "PERFORM")
                && has_word(&words, "VARYING")
                && has_word(&words, "AFTER")
            {
                diagnostics.push(Diagnostic::error(
                    "E_UNSUPPORTED_CONTROL_FLOW",
                    "PERFORM VARYING AFTER requires nested loop control-flow modeling and is not lowered yet",
                    span.clone(),
                ));
            }
            if has_word(&words, "COMPUTE")
                && (has_word(&words, "ROUNDED")
                    || has_word(&words, "FUNCTION")
                    || raw_line.contains("**"))
            {
                diagnostics.push(Diagnostic::error(
                    "E_UNSUPPORTED_ARITHMETIC",
                    "COMPUTE with ROUNDED, exponentiation, or function operands is not lowered yet",
                    span.clone(),
                ));
            }
            if has_word(&words, "CALL") {
                for phrase in [
                    &["BY", "REFERENCE"][..],
                    &["BY", "CONTENT"],
                    &["BY", "VALUE"],
                ] {
                    if has_phrase(&words, phrase) {
                        diagnostics.push(Diagnostic::error(
                            "E_UNSUPPORTED_CALL_MODE",
                            format!(
                                "CALL {} requires explicit parameter passing mode semantics and is not lowered yet",
                                phrase.join(" ")
                            ),
                            span.clone(),
                        ));
                    }
                }
            }
            for word in [
                "ACCEPT",
                "CANCEL",
                "ENTRY",
                "ENTER",
                "EXEC",
                "GENERATE",
                "INITIALIZE",
                "INVOKE",
                "MERGE",
                "NEXT",
                "READY",
                "RESET",
                "START",
                "XML",
                "JSON",
            ] {
                if has_word(&words, word) {
                    if word == "NEXT" && has_phrase(&words, &["NEXT", "SENTENCE"]) {
                        continue;
                    }
                    if matches!(word, "READY" | "RESET") && supported_trace_shape(&words, word) {
                        continue;
                    }
                    diagnostics.push(Diagnostic::error(
                        "E_UNSUPPORTED_VERB",
                        format!("Procedure Division verb {word} is not lowered by the converter preview"),
                        span.clone(),
                    ));
                }
            }
        }
    }
    dedupe_diagnostics(diagnostics)
}

fn mask_literals(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    for item in cobol_text::literal_aware_char_indices(line) {
        if item.inside_literal {
            out.push(' ');
        } else {
            out.push(item.ch);
        }
    }
    out
}

fn cobol_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    for ch in line.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' {
            current.push(ch.to_ascii_uppercase());
        } else if !current.is_empty() {
            words.push(current.clone());
            current.clear();
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn has_word(words: &[String], needle: &str) -> bool {
    words.iter().any(|word| word == needle)
}

fn has_phrase(words: &[String], phrase: &[&str]) -> bool {
    if phrase.is_empty() || words.len() < phrase.len() {
        return false;
    }
    words.windows(phrase.len()).any(|window| {
        window
            .iter()
            .zip(phrase)
            .all(|(word, expected)| word == expected)
    })
}

fn supported_rerun_shape(words: &[String]) -> bool {
    let Some(rerun_idx) = words.iter().position(|word| word == "RERUN") else {
        return false;
    };
    if words.get(rerun_idx + 1).map(String::as_str) != Some("ON")
        || words.get(rerun_idx + 2).is_none()
    {
        return false;
    }
    let Some(every_idx) = words
        .iter()
        .enumerate()
        .skip(rerun_idx + 3)
        .find_map(|(idx, word)| (word == "EVERY").then_some(idx))
    else {
        return false;
    };
    words
        .get(every_idx + 1)
        .is_some_and(|word| word.parse::<usize>().is_ok_and(|value| value > 0))
        && words.get(every_idx + 2).map(String::as_str) == Some("RECORDS")
        && words.get(every_idx + 3).map(String::as_str) == Some("OF")
        && words.get(every_idx + 4).is_some()
}

fn supported_trace_shape(words: &[String], verb: &str) -> bool {
    words.len() == 2
        && words.first().map(String::as_str) == Some(verb)
        && words.get(1).map(String::as_str) == Some("TRACE")
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
    out.sort_by(compare_diagnostics);
    out
}

fn compare_diagnostics(left: &Diagnostic, right: &Diagnostic) -> std::cmp::Ordering {
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
}

fn write_generated_project(out_dir: &Path, ir: &ProgramIr) -> Result<Vec<PathBuf>, ConvertError> {
    let src_dir = out_dir.join("src");
    fs::create_dir_all(&src_dir).map_err(|source| ConvertError::Io {
        path: src_dir.clone(),
        source,
    })?;

    let runtime_files = write_vendored_runtime(out_dir)?;
    let record_files = write_vendored_record(out_dir)?;
    let dialect_files = write_vendored_dialect(out_dir)?;
    let platform_files = write_vendored_platform(out_dir)?;
    let vm_files = write_vendored_vm(out_dir)?;

    let files = vec![
        (out_dir.join("Cargo.toml"), emit_cargo_toml(ir)),
        (src_dir.join("main.rs"), emit_main_rs()),
        (src_dir.join("data.rs"), emit_data_rs(ir)),
        (src_dir.join("files.rs"), emit_files_rs()),
        (src_dir.join("program.rs"), emit_program_rs(ir)),
    ];

    let mut written = Vec::new();
    for (path, contents) in files {
        fs::write(&path, contents).map_err(|source| ConvertError::Io {
            path: path.clone(),
            source,
        })?;
        written.push(path);
    }
    written.extend(runtime_files);
    written.extend(record_files);
    written.extend(dialect_files);
    written.extend(platform_files);
    written.extend(vm_files);
    Ok(written)
}

fn write_generated_project_multi(
    out_dir: &Path,
    programs: &[ProgramIr],
) -> Result<Vec<PathBuf>, ConvertError> {
    let src_dir = out_dir.join("src");
    fs::create_dir_all(&src_dir).map_err(|source| ConvertError::Io {
        path: src_dir.clone(),
        source,
    })?;

    let runtime_files = write_vendored_runtime(out_dir)?;
    let record_files = write_vendored_record(out_dir)?;
    let dialect_files = write_vendored_dialect(out_dir)?;
    let platform_files = write_vendored_platform(out_dir)?;
    let vm_files = write_vendored_vm(out_dir)?;
    let entry = programs
        .first()
        .expect("multi-program generation requires at least one program");

    let files = vec![
        (out_dir.join("Cargo.toml"), emit_cargo_toml(entry)),
        (src_dir.join("main.rs"), emit_main_rs()),
        (src_dir.join("data.rs"), emit_data_rs_multi(programs)),
        (src_dir.join("files.rs"), emit_files_rs()),
        (src_dir.join("program.rs"), emit_program_rs_multi(programs)),
    ];

    let mut written = Vec::new();
    for (path, contents) in files {
        fs::write(&path, contents).map_err(|source| ConvertError::Io {
            path: path.clone(),
            source,
        })?;
        written.push(path);
    }
    written.extend(runtime_files);
    written.extend(record_files);
    written.extend(dialect_files);
    written.extend(platform_files);
    written.extend(vm_files);
    Ok(written)
}

fn emit_cargo_toml(ir: &ProgramIr) -> String {
    format!(
        "[package]\nname = \"{}-rust\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n\n[dependencies]\ncobol-runtime = {{ path = \"vendor/cobol-runtime\" }}\ncobol-record = {{ path = \"vendor/cobol-record\" }}\ncobol-dialect = {{ path = \"vendor/cobol-dialect\" }}\ncobol-platform = {{ path = \"vendor/cobol-platform\" }}\ncobol-vm = {{ path = \"vendor/cobol-vm\" }}\nrust_decimal = \"1.36\"\nserde_json = \"1.0\"\n\n",
        package_name(&ir.name)
    )
}

fn write_vendored_runtime(out_dir: &Path) -> Result<Vec<PathBuf>, ConvertError> {
    let runtime_dir = out_dir.join("vendor").join("cobol-runtime");
    let runtime_src = runtime_dir.join("src");
    fs::create_dir_all(&runtime_src).map_err(|source| ConvertError::Io {
        path: runtime_src.clone(),
        source,
    })?;
    let files = [
        (
            runtime_dir.join("Cargo.toml"),
            include_str!("../../cobol-runtime/Cargo.toml"),
        ),
        (
            runtime_src.join("lib.rs"),
            include_str!("../../cobol-runtime/src/lib.rs"),
        ),
    ];
    let mut written = Vec::new();
    for (path, contents) in files {
        fs::write(&path, contents).map_err(|source| ConvertError::Io {
            path: path.clone(),
            source,
        })?;
        written.push(path);
    }
    Ok(written)
}

fn write_vendored_record(out_dir: &Path) -> Result<Vec<PathBuf>, ConvertError> {
    let record_dir = out_dir.join("vendor").join("cobol-record");
    let record_src = record_dir.join("src");
    fs::create_dir_all(&record_src).map_err(|source| ConvertError::Io {
        path: record_src.clone(),
        source,
    })?;
    let files = [
        (
            record_dir.join("Cargo.toml"),
            include_str!("../../cobol-record/Cargo.toml"),
        ),
        (
            record_src.join("lib.rs"),
            include_str!("../../cobol-record/src/lib.rs"),
        ),
    ];
    let mut written = Vec::new();
    for (path, contents) in files {
        fs::write(&path, contents).map_err(|source| ConvertError::Io {
            path: path.clone(),
            source,
        })?;
        written.push(path);
    }
    Ok(written)
}

fn write_vendored_dialect(out_dir: &Path) -> Result<Vec<PathBuf>, ConvertError> {
    let dialect_dir = out_dir.join("vendor").join("cobol-dialect");
    let dialect_src = dialect_dir.join("src");
    fs::create_dir_all(&dialect_src).map_err(|source| ConvertError::Io {
        path: dialect_src.clone(),
        source,
    })?;
    let files = [
        (
            dialect_dir.join("Cargo.toml"),
            include_str!("../../cobol-dialect/Cargo.toml"),
        ),
        (
            dialect_src.join("lib.rs"),
            include_str!("../../cobol-dialect/src/lib.rs"),
        ),
    ];
    let mut written = Vec::new();
    for (path, contents) in files {
        fs::write(&path, contents).map_err(|source| ConvertError::Io {
            path: path.clone(),
            source,
        })?;
        written.push(path);
    }
    Ok(written)
}

fn write_vendored_platform(out_dir: &Path) -> Result<Vec<PathBuf>, ConvertError> {
    let platform_dir = out_dir.join("vendor").join("cobol-platform");
    let platform_src = platform_dir.join("src");
    fs::create_dir_all(&platform_src).map_err(|source| ConvertError::Io {
        path: platform_src.clone(),
        source,
    })?;
    let files = [
        (
            platform_dir.join("Cargo.toml"),
            include_str!("../../cobol-platform/Cargo.toml"),
        ),
        (
            platform_src.join("lib.rs"),
            include_str!("../../cobol-platform/src/lib.rs"),
        ),
    ];
    let mut written = Vec::new();
    for (path, contents) in files {
        fs::write(&path, contents).map_err(|source| ConvertError::Io {
            path: path.clone(),
            source,
        })?;
        written.push(path);
    }
    Ok(written)
}

fn write_vendored_vm(out_dir: &Path) -> Result<Vec<PathBuf>, ConvertError> {
    let vm_dir = out_dir.join("vendor").join("cobol-vm");
    let vm_src = vm_dir.join("src");
    fs::create_dir_all(&vm_src).map_err(|source| ConvertError::Io {
        path: vm_src.clone(),
        source,
    })?;
    let files = [
        (
            vm_dir.join("Cargo.toml"),
            include_str!("../../cobol-vm/Cargo.toml"),
        ),
        (
            vm_src.join("lib.rs"),
            include_str!("../../cobol-vm/src/lib.rs"),
        ),
    ];
    let mut written = Vec::new();
    for (path, contents) in files {
        fs::write(&path, contents).map_err(|source| ConvertError::Io {
            path: path.clone(),
            source,
        })?;
        written.push(path);
    }
    Ok(written)
}

fn emit_main_rs() -> String {
    r#"mod data;
mod files;
mod program;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut program = program::Program::default();
    let args = runtime_args(std::env::args().skip(1))?;
    match (args.runtime_config.as_deref(), args.file_map.as_deref()) {
        (Some(runtime_config), file_map) => {
            apply_runtime_config(&mut program, runtime_config)?;
            if let Some(file_map) = file_map {
                apply_file_map(&mut program, file_map)?;
            }
        }
        (None, Some(file_map)) => {
            apply_file_map(&mut program, file_map)?;
        }
        (None, None) => {
            let default_runtime = std::path::Path::new("cobol-runtime.json");
            let default_map = std::path::Path::new("cobol-file-map.json");
            if default_runtime.exists() {
                apply_runtime_config(&mut program, default_runtime)?;
            } else if default_map.exists() {
                apply_file_map(&mut program, default_map)?;
            }
        }
    }
    program.run()?;
    Ok(())
}

struct RuntimeArgs {
    runtime_config: Option<std::path::PathBuf>,
    file_map: Option<std::path::PathBuf>,
}

fn runtime_args<I>(mut args: I) -> Result<RuntimeArgs, Box<dyn std::error::Error>>
where
    I: Iterator<Item = String>,
{
    let mut runtime_config = None;
    let mut file_map = None;
    while let Some(arg) = args.next() {
        if arg == "--file-map" {
            let Some(path) = args.next() else {
                return Err("--file-map requires a path".into());
            };
            file_map = Some(std::path::PathBuf::from(path));
        } else if arg == "--runtime-config" {
            let Some(path) = args.next() else {
                return Err("--runtime-config requires a path".into());
            };
            runtime_config = Some(std::path::PathBuf::from(path));
        } else {
            return Err(format!("unknown generated program argument {arg}").into());
        }
    }
    Ok(RuntimeArgs {
        runtime_config,
        file_map,
    })
}

fn apply_runtime_config(
    program: &mut program::Program,
    path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = cobol_platform::PlatformConfig::from_json_file(path)?;
    program.apply_platform_config(&config)?;
    Ok(())
}

fn apply_file_map(
    program: &mut program::Program,
    path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(path)?;
    let map: std::collections::BTreeMap<String, String> = serde_json::from_str(&text)?;
    for (name, path) in map {
        program.map_file(&name, path);
    }
    Ok(())
}
"#
    .to_string()
}

fn emit_data_rs(ir: &ProgramIr) -> String {
    let mut text = String::from(
        "#![allow(dead_code, unused_imports, unused_mut)]\n\nuse cobol_runtime::{CobolByteStorage, CobolFieldKind, CobolValue};\n\npub fn initial_storage() -> CobolByteStorage {\n",
    );
    text.push_str("    let mut storage = CobolByteStorage::default();\n");
    for item in &ir.storage.items {
        text.push_str(&format!(
            "    storage.define_field(\"{}\", {}, {}, CobolFieldKind::{});\n",
            escape_rust(&item.name),
            item.offset,
            item.byte_len,
            runtime_field_kind(item)
        ));
        if item.qualified_name != item.name {
            text.push_str(&format!(
                "    storage.define_field(\"{}\", {}, {}, CobolFieldKind::{});\n",
                escape_rust(&item.qualified_name),
                item.offset,
                item.byte_len,
                runtime_field_kind(item)
            ));
        }
        text.push_str(&emit_initial_storage_bytes(ir, item));
        if !initial_storage_uses_byte_copy(ir, item) {
            if let Some(value) = &item.value {
                text.push_str(&format!(
                    "    let _ = storage.move_value(CobolValue::Text(\"{}\".to_string()), \"{}\");\n",
                    escape_rust(value),
                    escape_rust(&item.name)
                ));
            }
        }
    }
    text.push_str("    storage\n}\n");
    text.push_str("\n#[allow(dead_code)]\npub struct DataView<'a> {\n    bytes: &'a [u8],\n}\n\n");
    text.push_str("impl<'a> DataView<'a> {\n    pub fn new(bytes: &'a [u8]) -> Self {\n        Self { bytes }\n    }\n\n");
    text.push_str(
        "    fn field(&self, name: &str, offset: usize, byte_len: usize) -> Result<&'a [u8], String> {\n        let end = offset.checked_add(byte_len).ok_or_else(|| format!(\"field {name} offset overflow\"))?;\n        self.bytes.get(offset..end).ok_or_else(|| format!(\"field {name} range {offset}..{end} exceeds record length {}\", self.bytes.len()))\n    }\n\n",
    );
    let mut emitted_accessors = HashSet::new();
    for item in &ir.storage.items {
        if item.addressable && emitted_accessors.insert(field_accessor_name(item)) {
            text.push_str(&emit_data_accessor(item));
        }
    }
    text.push_str("}\n");
    text
}

fn emit_initial_storage_bytes(ir: &ProgramIr, item: &cobol_ir::StorageItemIr) -> String {
    if !initial_storage_uses_byte_copy(ir, item) {
        return String::new();
    }

    let mut text = String::new();
    if occurs_chain_for_item(item, ir).is_empty() {
        let bytes = initial_storage_bytes_for_item(ir, item, item.byte_len);
        text.push_str(&emit_initial_storage_byte_copy(item.offset, &bytes));
    } else if let Some(occurs_item) = occurs_chain_for_item(item, ir).first().copied() {
        let max = occurs_item
            .occurs
            .as_ref()
            .map(|occurs| occurs.max.max(1))
            .unwrap_or(1);
        let len = occurrence_cell_len(item, occurs_item);
        for occurrence in 1..=max {
            let bytes = initial_occurrence_storage_bytes(ir, item, len, max, occurrence);
            let start = occurrence_source_offset(item, occurs_item, occurrence);
            text.push_str(&emit_initial_storage_byte_copy(start, &bytes));
        }
    }
    text
}

fn initial_storage_bytes_for_item(
    ir: &ProgramIr,
    item: &cobol_ir::StorageItemIr,
    len: usize,
) -> Vec<u8> {
    planned_initial_bytes_for_item(ir, item)
        .filter(|bytes| bytes.len() == len)
        .map(<[u8]>::to_vec)
        .unwrap_or_else(|| initial_template_bytes_for_item(item, len))
}

fn initial_occurrence_storage_bytes(
    ir: &ProgramIr,
    item: &cobol_ir::StorageItemIr,
    len: usize,
    max: usize,
    occurrence: usize,
) -> Vec<u8> {
    if let Some(bytes) = planned_initial_bytes_for_item(ir, item) {
        let planned_offset = if bytes.len() >= len.saturating_mul(max) {
            occurrence.saturating_sub(1).saturating_mul(len)
        } else {
            0
        };
        let planned_end = planned_offset.saturating_add(len);
        if let Some(slice) = bytes.get(planned_offset..planned_end) {
            return slice.to_vec();
        }
    }
    initial_template_bytes_for_item(item, len)
}

fn planned_initial_bytes_for_item<'a>(
    ir: &'a ProgramIr,
    item: &cobol_ir::StorageItemIr,
) -> Option<&'a [u8]> {
    ir.storage
        .storage_cells
        .iter()
        .find(|cell| {
            cell.item_id.eq_ignore_ascii_case(&item.qualified_name)
                || cell.key.eq_ignore_ascii_case(&item.qualified_name)
        })
        .map(|cell| cell.initial_bytes.as_slice())
}

fn initial_storage_uses_byte_copy(ir: &ProgramIr, item: &cobol_ir::StorageItemIr) -> bool {
    if !item.addressable || item.value_category == ValueCategoryIr::Group {
        return false;
    }
    item.value_category == ValueCategoryIr::PackedDecimal
        || item.value_category == ValueCategoryIr::NumericDisplay
        || (item.value.is_some() && !occurs_chain_for_item(item, ir).is_empty())
}

fn emit_initial_storage_byte_copy(start: usize, bytes: &[u8]) -> String {
    let end = start.saturating_add(bytes.len());
    format!(
        "    if let Some(bytes) = storage.bytes_mut().get_mut({start}..{end}) {{\n        bytes.copy_from_slice(&[{}]);\n    }}\n",
        bytes
            .iter()
            .map(|byte| format!("{byte}u8"))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn emit_data_rs_multi(programs: &[ProgramIr]) -> String {
    let mut text = String::from(
        "#![allow(dead_code, unused_imports, unused_mut)]\n\nuse cobol_runtime::{CobolByteStorage, CobolFieldKind, CobolValue};\n\n",
    );
    for program in programs {
        text.push_str(&emit_initial_storage_function(
            program,
            &format!("initial_storage_{}", program_suffix(&program.name)),
        ));
        text.push('\n');
    }
    if let Some(entry) = programs.first() {
        text.push_str(&emit_initial_storage_function(entry, "initial_storage"));
    }
    text
}

fn emit_initial_storage_function(ir: &ProgramIr, function_name: &str) -> String {
    let mut text = format!("pub fn {function_name}() -> CobolByteStorage {{\n");
    text.push_str("    let mut storage = CobolByteStorage::default();\n");
    for item in &ir.storage.items {
        text.push_str(&format!(
            "    storage.define_field(\"{}\", {}, {}, CobolFieldKind::{});\n",
            escape_rust(&item.name),
            item.offset,
            item.byte_len,
            runtime_field_kind(item)
        ));
        if item.qualified_name != item.name {
            text.push_str(&format!(
                "    storage.define_field(\"{}\", {}, {}, CobolFieldKind::{});\n",
                escape_rust(&item.qualified_name),
                item.offset,
                item.byte_len,
                runtime_field_kind(item)
            ));
        }
        text.push_str(&emit_initial_storage_bytes(ir, item));
        if !initial_storage_uses_byte_copy(ir, item) {
            if let Some(value) = &item.value {
                text.push_str(&format!(
                    "    let _ = storage.move_value(CobolValue::Text(\"{}\".to_string()), \"{}\");\n",
                    escape_rust(value),
                    escape_rust(&item.name)
                ));
            }
        }
    }
    text.push_str("    storage\n}\n");
    text
}

fn emit_data_accessor(item: &cobol_ir::StorageItemIr) -> String {
    let fn_name = field_accessor_name(item);
    let name = escape_rust(&item.qualified_name);
    match item.usage {
        UsageIr::Alphanumeric | UsageIr::Display | UsageIr::National | UsageIr::Dbcs => format!(
            "    pub fn {fn_name}(&self) -> Result<String, String> {{\n        let bytes = self.field(\"{name}\", {}, {})?;\n        Ok(String::from_utf8_lossy(bytes).trim_end().to_string())\n    }}\n\n",
            item.offset, item.byte_len
        ),
        UsageIr::PackedDecimal => {
            if let Some(pic) = &item.picture {
                format!(
                    "    pub fn {fn_name}(&self) -> Result<rust_decimal::Decimal, String> {{\n        let bytes = self.field(\"{name}\", {}, {})?;\n        cobol_record::decode_packed_decimal(bytes, {}, {}, {}).map_err(|err| err.to_string())\n    }}\n\n",
                    item.offset,
                    item.byte_len,
                    pic.digits,
                    pic.scale,
                    pic.signed
                )
            } else {
                emit_bytes_accessor(&fn_name, &name, item.offset, item.byte_len)
            }
        }
        UsageIr::Binary | UsageIr::NativeBinary => format!(
            "    pub fn {fn_name}(&self) -> Result<cobol_record::DecodedValue, String> {{\n        let bytes = self.field(\"{name}\", {}, {})?;\n        cobol_record::decode_binary_integer(bytes, {}, cobol_record::Endian::Big).map_err(|err| err.to_string())\n    }}\n\n",
            item.offset,
            item.byte_len,
            item.picture.as_ref().map(|pic| pic.signed).unwrap_or(false)
        ),
        UsageIr::Float32 => format!(
            "    pub fn {fn_name}(&self) -> Result<f64, String> {{\n        let bytes = self.field(\"{name}\", {}, {})?;\n        cobol_record::decode_ibm_float32(bytes, cobol_record::Endian::Big).map_err(|err| err.to_string())\n    }}\n\n",
            item.offset, item.byte_len
        ),
        UsageIr::Float64 => format!(
            "    pub fn {fn_name}(&self) -> Result<f64, String> {{\n        let bytes = self.field(\"{name}\", {}, {})?;\n        cobol_record::decode_ibm_float64(bytes, cobol_record::Endian::Big).map_err(|err| err.to_string())\n    }}\n\n",
            item.offset, item.byte_len
        ),
        UsageIr::Group | UsageIr::Unknown(_) => emit_bytes_accessor(&fn_name, &name, item.offset, item.byte_len),
    }
}

fn emit_bytes_accessor(fn_name: &str, name: &str, offset: usize, byte_len: usize) -> String {
    format!(
        "    pub fn {fn_name}(&self) -> Result<Vec<u8>, String> {{\n        Ok(self.field(\"{name}\", {offset}, {byte_len})?.to_vec())\n    }}\n\n"
    )
}

fn field_accessor_name(item: &cobol_ir::StorageItemIr) -> String {
    rust_ident(&item.qualified_name.replace('.', "_"))
}

fn runtime_field_kind(item: &cobol_ir::StorageItemIr) -> &'static str {
    match item.usage {
        UsageIr::Group => "Group",
        UsageIr::Alphanumeric => "Alphanumeric",
        UsageIr::Display => match item.value_category {
            ValueCategoryIr::NumericDisplay => "NumericDisplay",
            ValueCategoryIr::NumericEdited => "NumericEdited",
            _ => "Display",
        },
        UsageIr::PackedDecimal => "PackedDecimal",
        UsageIr::Binary => "Binary",
        UsageIr::NativeBinary => "NativeBinary",
        UsageIr::Float32 => "Float32",
        UsageIr::Float64 => "Float64",
        UsageIr::National => "Alphanumeric",
        UsageIr::Dbcs => "Alphanumeric",
        UsageIr::Unknown(_) => "Unknown",
    }
}

fn emit_files_rs() -> String {
    "#![allow(dead_code)]\n\npub type FileSystem = cobol_runtime::UnboundFileSystem;\n".to_string()
}

fn emit_program_rs(ir: &ProgramIr) -> String {
    let mut text = format!(
        "#![allow(dead_code, unused_mut, unused_variables)]\n\npub struct Program {{\n    runtime: cobol_vm::VmRuntime,\n    display_cursor: usize,\n}}\n\nimpl Default for Program {{\n    fn default() -> Self {{\n        let initial = crate::data::initial_storage();\n{}    }}\n}}\n\nimpl Program {{\n    pub fn run(&mut self) -> Result<(), cobol_vm::VmError> {{\n        let procedure = Self::vm_procedure();\n        self.runtime.execute_procedure(&procedure)?;\n        for line in &self.runtime.display[self.display_cursor..] {{\n            println!(\"{{line}}\");\n        }}\n        self.display_cursor = self.runtime.display.len();\n        Ok(())\n    }}\n\n    pub fn map_file(&mut self, name: &str, path: impl Into<std::path::PathBuf>) {{\n        self.runtime.files.map_external_name(name, path);\n    }}\n\n    pub fn apply_platform_config(&mut self, config: &cobol_platform::PlatformConfig) -> Result<(), cobol_vm::VmError> {{\n        self.runtime.files.apply_platform_config(config)\n    }}\n\n    pub fn checkpoint_snapshot_bytes(&self) -> Vec<u8> {{\n        self.runtime.checkpoint_snapshot_bytes()\n    }}\n\n    pub fn restore_checkpoint_snapshot(&mut self, bytes: &[u8]) -> Result<(), cobol_vm::VmError> {{\n        self.runtime.restore_checkpoint_snapshot(bytes)\n    }}\n\n    pub fn restore_last_rerun_checkpoint(&mut self, file: &str) -> Result<bool, cobol_vm::VmError> {{\n        self.runtime.restore_last_rerun_checkpoint(file)\n    }}\n\n    #[allow(dead_code)]\n    fn eval_condition(&self, condition: cobol_vm::VmCondition) -> Result<bool, cobol_vm::VmError> {{\n        self.runtime.eval_condition(&condition)\n    }}\n\n    #[allow(dead_code)]\n    fn set_condition_name(&mut self, name: &str) -> Result<(), cobol_vm::VmError> {{\n        self.runtime.set_condition_name_at(name)\n    }}\n",
        emit_program_default_body(ir)
    );
    text.push_str(&emit_vm_methods(ir));
    text.push_str(&emit_vm_procedure_method(ir));
    text.push_str(&emit_vm_declarative_methods(ir, None));
    text.push_str("}\n");
    text
}

fn emit_program_rs_multi(programs: &[ProgramIr]) -> String {
    let entry = programs
        .first()
        .expect("multi-program generation requires at least one program");
    let entry_method = format!("vm_procedure_{}", program_suffix(&entry.name));
    let mut text = format!(
        "#![allow(dead_code, unused_mut, unused_variables)]\n\npub struct Program {{\n    runtime: cobol_vm::VmRuntime,\n    display_cursor: usize,\n}}\n\nimpl Default for Program {{\n    fn default() -> Self {{\n{}    }}\n}}\n\nimpl Program {{\n    pub fn run(&mut self) -> Result<(), cobol_vm::VmError> {{\n        let procedure = Self::{entry_method}();\n        self.runtime.execute_procedure_as(&procedure, \"{}\")?;\n        for line in &self.runtime.display[self.display_cursor..] {{\n            println!(\"{{line}}\");\n        }}\n        self.display_cursor = self.runtime.display.len();\n        Ok(())\n    }}\n\n    pub fn map_file(&mut self, name: &str, path: impl Into<std::path::PathBuf>) {{\n        self.runtime.files.map_external_name(name, path);\n    }}\n\n    pub fn apply_platform_config(&mut self, config: &cobol_platform::PlatformConfig) -> Result<(), cobol_vm::VmError> {{\n        self.runtime.files.apply_platform_config(config)\n    }}\n\n    pub fn checkpoint_snapshot_bytes(&self) -> Vec<u8> {{\n        self.runtime.checkpoint_snapshot_bytes()\n    }}\n\n    pub fn restore_checkpoint_snapshot(&mut self, bytes: &[u8]) -> Result<(), cobol_vm::VmError> {{\n        self.runtime.restore_checkpoint_snapshot(bytes)\n    }}\n\n    pub fn restore_last_rerun_checkpoint(&mut self, file: &str) -> Result<bool, cobol_vm::VmError> {{\n        self.runtime.restore_last_rerun_checkpoint(file)\n    }}\n\n    #[allow(dead_code)]\n    fn eval_condition(&self, condition: cobol_vm::VmCondition) -> Result<bool, cobol_vm::VmError> {{\n        self.runtime.eval_condition(&condition)\n    }}\n\n    #[allow(dead_code)]\n    fn set_condition_name(&mut self, name: &str) -> Result<(), cobol_vm::VmError> {{\n        self.runtime.set_condition_name_at(name)\n    }}\n",
        emit_program_default_body_multi(programs),
        escape_rust(&entry.name)
    );
    text.push_str(&emit_vm_methods_multi(programs));
    for program in programs {
        text.push_str(&emit_vm_procedure_method_named_with_programs(
            program,
            &format!("vm_procedure_{}", program_suffix(&program.name)),
            Some(programs),
        ));
        text.push_str(&emit_vm_declarative_methods(program, Some(programs)));
    }
    text.push_str("}\n");
    text
}

fn emit_program_default_body_multi(programs: &[ProgramIr]) -> String {
    let mut text = String::from("        let mut pool = cobol_vm::StoragePool::default();\n");
    for program in programs {
        let initial_name = format!("initial_storage_{}", program_suffix(&program.name));
        text.push_str(&format!(
            "        let initial_{} = crate::data::{initial_name}();\n",
            program_suffix(&program.name)
        ));
        text.push_str(&emit_pool_cell_initializers(
            program,
            &format!("initial_{}", program_suffix(&program.name)),
            true,
        ));
        text.push_str(&emit_same_record_area_cell_initializers(
            program,
            &format!("initial_{}", program_suffix(&program.name)),
        ));
    }
    for program in programs {
        let suffix = program_suffix(&program.name);
        text.push_str(&emit_initial_lifecycle_vectors(
            program,
            &format!("initial_{suffix}"),
            &format!("__initial_cells_{suffix}"),
            &format!("__initial_odo_{suffix}"),
            true,
        ));
        text.push_str(&emit_initial_file_lifecycle_vector(
            program,
            &format!("__initial_files_{suffix}"),
        ));
    }
    for temp in collect_call_temps(programs) {
        let initial = initial_temp_bytes(&temp)
            .into_iter()
            .map(|byte| format!("{byte}u8"))
            .collect::<Vec<_>>()
            .join(", ");
        text.push_str(&format!(
            "        let _ = pool.define_cell(cobol_vm::StorageKey::scalar(\"{}\", \"{}\"), vec![{}]);\n",
            escape_rust(&temp.caller_program),
            escape_rust(&temp.name),
            initial
        ));
    }

    text.push_str(
        "        let mut runtime = cobol_vm::VmRuntime::new(Self::vm_program(), pool);\n",
    );
    for program in programs {
        text.push_str(&emit_runtime_file_definitions(program));
    }
    for program in programs {
        text.push_str(&emit_runtime_storage_bindings(program, true));
    }
    for program in programs {
        text.push_str(&emit_runtime_file_status_bindings(program));
    }
    for program in programs {
        text.push_str(&emit_runtime_declarative_registrations(program));
    }
    for program in programs {
        text.push_str(&emit_runtime_rerun_registrations(program));
    }
    for temp in collect_call_temps(programs) {
        text.push_str(&format!(
            "        runtime.bind_storage_cell(\"{}\", cobol_vm::StorageKey::scalar(\"{}\", \"{}\"));\n",
            escape_rust(&temp.name),
            escape_rust(&temp.caller_program),
            escape_rust(&temp.name)
        ));
    }
    for program in programs {
        text.push_str(&emit_runtime_indexes_and_odo(program));
    }
    for program in programs {
        let linkage = emit_linkage_descriptors(program);
        let suffix = program_suffix(&program.name);
        text.push_str(&format!(
            "        runtime.registry.insert_with_lifecycle_descriptors(\"{}\", Self::vm_procedure_{}(), {linkage}, {}, __initial_cells_{suffix}, __initial_odo_{suffix}, __initial_files_{suffix});\n",
            escape_rust(&program.name),
            suffix,
            program.is_initial
        ));
    }
    text.push_str(
        "        Self {\n            runtime,\n            display_cursor: 0,\n        }\n",
    );
    text
}

fn emit_pool_cell_initializers(ir: &ProgramIr, initial_var: &str, skip_linkage: bool) -> String {
    let mut text = String::new();
    for item in &ir.storage.items {
        if skip_linkage && item.storage_area == StorageAreaIr::Linkage {
            continue;
        }
        if !item.addressable || item.value_category == ValueCategoryIr::Group {
            continue;
        }
        if occurs_chain_for_item(item, ir).is_empty() {
            let key_item = storage_cell_key_item(ir, item);
            if key_item.qualified_name != item.qualified_name {
                continue;
            }
            let end = item.offset.saturating_add(item.byte_len);
            let key = scalar_storage_key_expr(ir, item);
            text.push_str(&format!(
                "        if let Some(bytes) = {initial_var}.bytes().get({}..{}) {{\n            let _ = pool.define_cell({key}, bytes.to_vec());\n        }}\n",
                item.offset,
                end
            ));
        } else if let Some(occurs_item) = occurs_chain_for_item(item, ir).first().copied() {
            let key_item = storage_cell_key_item(ir, item);
            if key_item.qualified_name != item.qualified_name {
                continue;
            }
            let max = occurs_item
                .occurs
                .as_ref()
                .map(|occurs| occurs.max.max(1))
                .unwrap_or(1);
            let len = occurrence_cell_len(item, occurs_item);
            for occurrence in 1..=max {
                let start = occurrence_source_offset(item, occurs_item, occurrence);
                let end = start.saturating_add(len);
                let key = occurrence_storage_key_expr(ir, item, occurrence);
                text.push_str(&format!(
                    "        if let Some(bytes) = {initial_var}.bytes().get({start}..{end}) {{\n            let _ = pool.define_cell({key}, bytes.to_vec());\n        }}\n"
                ));
            }
        }
    }
    text
}

fn emit_same_record_area_cell_initializers(ir: &ProgramIr, initial_var: &str) -> String {
    let mut text = String::new();
    for (idx, area) in ir.same_record_areas.iter().enumerate() {
        let Some((record, len)) = same_record_area_representative(ir, area) else {
            continue;
        };
        let key = same_record_area_storage_key_expr(ir, idx);
        let start = record.offset;
        let end = start.saturating_add(record.byte_len);
        text.push_str(&format!(
            "        let mut __same_area_{idx} = vec![b' '; {len}usize];\n"
        ));
        text.push_str(&format!(
            "        if let Some(bytes) = {initial_var}.bytes().get({start}..{end}) {{\n            for (idx, byte) in bytes.iter().take(__same_area_{idx}.len()).enumerate() {{\n                __same_area_{idx}[idx] = *byte;\n            }}\n        }}\n"
        ));
        text.push_str(&format!(
            "        let _ = pool.define_cell({key}, __same_area_{idx});\n"
        ));
    }
    text
}

fn emit_initial_lifecycle_vectors(
    ir: &ProgramIr,
    initial_var: &str,
    cells_var: &str,
    odo_var: &str,
    skip_linkage: bool,
) -> String {
    let mut text = format!(
        "        let mut {cells_var}: Vec<(cobol_vm::StorageKey, Vec<u8>)> = Vec::new();\n"
    );
    for item in &ir.storage.items {
        if skip_linkage && item.storage_area == StorageAreaIr::Linkage {
            continue;
        }
        if item.external || !item.addressable || item.value_category == ValueCategoryIr::Group {
            continue;
        }
        if occurs_chain_for_item(item, ir).is_empty() {
            let key_item = storage_cell_key_item(ir, item);
            if key_item.qualified_name != item.qualified_name {
                continue;
            }
            let key = scalar_storage_key_expr(ir, item);
            let end = item.offset.saturating_add(item.byte_len);
            text.push_str(&format!(
                "        if let Some(bytes) = {initial_var}.bytes().get({}..{}) {{\n            {cells_var}.push(({key}, bytes.to_vec()));\n        }}\n",
                item.offset, end
            ));
        } else if let Some(occurs_item) = occurs_chain_for_item(item, ir).first().copied() {
            let key_item = storage_cell_key_item(ir, item);
            if key_item.qualified_name != item.qualified_name {
                continue;
            }
            let max = occurs_item
                .occurs
                .as_ref()
                .map(|occurs| occurs.max.max(1))
                .unwrap_or(1);
            let len = occurrence_cell_len(item, occurs_item);
            for occurrence in 1..=max {
                let start = occurrence_source_offset(item, occurs_item, occurrence);
                let end = start.saturating_add(len);
                let key = occurrence_storage_key_expr(ir, item, occurrence);
                text.push_str(&format!(
                    "        if let Some(bytes) = {initial_var}.bytes().get({start}..{end}) {{\n            {cells_var}.push(({key}, bytes.to_vec()));\n        }}\n"
                ));
            }
        }
    }

    text.push_str(&format!(
        "        let mut {odo_var}: Vec<cobol_vm::VmOdoInitialState> = Vec::new();\n"
    ));
    for odo in &ir.odo_descriptors {
        let table_item = storage_item_by_name(ir, &odo.table);
        if table_item.map(|item| item.external).unwrap_or(false) {
            continue;
        }
        let active = ir
            .storage
            .items
            .iter()
            .find(|item| item.name.eq_ignore_ascii_case(&odo.depending_on))
            .and_then(|item| item.value.as_ref())
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(odo.min);
        text.push_str(&format!(
            "        {odo_var}.push(cobol_vm::VmOdoInitialState {{ program: \"{}\".to_string(), table: \"{}\".to_string(), active: {} }});\n",
            escape_rust(&ir.name),
            escape_rust(&odo.table),
            active
        ));
    }
    text
}

fn emit_initial_file_lifecycle_vector(ir: &ProgramIr, files_var: &str) -> String {
    let mut text = format!("        let mut {files_var}: Vec<String> = Vec::new();\n");
    for file in &ir.files {
        if file.kind == FileKindIr::Sd {
            continue;
        }
        text.push_str(&format!(
            "        {files_var}.push(\"{}\".to_string());\n",
            escape_rust(&file.name)
        ));
    }
    text
}

fn emit_runtime_file_definitions(ir: &ProgramIr) -> String {
    let mut text = String::new();
    let tape_checkpoint_files = ir
        .rerun_clauses
        .iter()
        .map(|rerun| normalize_vm_ref(&rerun.checkpoint_file))
        .collect::<BTreeSet<_>>();
    for file in &ir.files {
        if file.kind == FileKindIr::Sd {
            continue;
        }
        let Some(assign) = &file.assign else {
            continue;
        };
        if tape_checkpoint_files.contains(&normalize_vm_ref(&file.name)) {
            text.push_str(&format!(
                "        runtime.files.define_tape_file(\"{}\", \"{}\");\n",
                escape_rust(&file.name),
                escape_rust(assign)
            ));
        } else if let Some(record_len) = file_record_len(ir, file) {
            text.push_str(&format!(
                "        runtime.files.define_os_sequential_file_with_record_len(\"{}\", \"{}\", {record_len});\n",
                escape_rust(&file.name),
                escape_rust(assign)
            ));
        } else {
            text.push_str(&format!(
                "        runtime.files.define_os_sequential_file(\"{}\", \"{}\");\n",
                escape_rust(&file.name),
                escape_rust(assign)
            ));
        }
        if let Some(linage) = file.linage {
            text.push_str(&format!(
                "        runtime.files.set_linage(\"{}\", {linage});\n",
                escape_rust(&file.name)
            ));
        }
    }
    text
}

fn file_record_len(ir: &ProgramIr, file: &FileIr) -> Option<usize> {
    let record_name = file.record_name.as_deref()?;
    Some(storage_item_by_name(ir, record_name)?.byte_len)
}

fn emit_runtime_storage_bindings(ir: &ProgramIr, skip_linkage: bool) -> String {
    let mut runtime_setup = String::new();
    for item in &ir.storage.items {
        if skip_linkage && item.storage_area == StorageAreaIr::Linkage {
            continue;
        }
        if !item.addressable {
            continue;
        }
        let aliases = storage_aliases(item);
        if let Some(binding) = same_record_area_binding(ir, item) {
            let key = same_record_area_storage_key_expr(ir, binding.area_index);
            for alias in aliases {
                runtime_setup.push_str(&format!(
                    "        runtime.bind_storage_slice(\"{}\", {key}.clone(), {}, {});\n",
                    escape_rust(&alias),
                    binding.offset,
                    binding.len
                ));
            }
        } else if item.value_category == ValueCategoryIr::Group {
            let children = group_storage_child_aliases(ir, item)
                .into_iter()
                .map(|child| format!("\"{}\".to_string()", escape_rust(&child)))
                .collect::<Vec<_>>()
                .join(", ");
            for alias in aliases {
                runtime_setup.push_str(&format!(
                    "        runtime.bind_group_storage(\"{}\", vec![{}]);\n",
                    escape_rust(&alias),
                    children
                ));
            }
            let scoped_children = program_scoped_group_storage_child_aliases(ir, item)
                .into_iter()
                .map(|child| format!("\"{}\".to_string()", escape_rust(&child)))
                .collect::<Vec<_>>()
                .join(", ");
            for alias in program_scoped_storage_aliases(ir, item) {
                runtime_setup.push_str(&format!(
                    "        runtime.bind_group_storage(\"{}\", vec![{}]);\n",
                    escape_rust(&alias),
                    scoped_children
                ));
            }
        } else if occurs_chain_for_item(item, ir).is_empty() {
            let key_item = storage_cell_key_item(ir, item);
            let key = scalar_storage_key_expr(ir, key_item);
            for alias in aliases {
                runtime_setup.push_str(&format!(
                    "        runtime.bind_storage_cell(\"{}\", {key});\n",
                    escape_rust(&alias)
                ));
            }
            for alias in program_scoped_storage_aliases(ir, item) {
                runtime_setup.push_str(&format!(
                    "        runtime.bind_storage_cell(\"{}\", {key});\n",
                    escape_rust(&alias)
                ));
            }
        } else {
            let key_item = storage_cell_key_item(ir, item);
            if let Some(occurs_item) = occurs_chain_for_item(item, ir).first().copied() {
                let max = occurs_item
                    .occurs
                    .as_ref()
                    .map(|occurs| occurs.max.max(1))
                    .unwrap_or(1);
                for occurrence in 1..=max {
                    let key = occurrence_storage_key_expr(ir, key_item, occurrence);
                    runtime_setup.push_str(&format!(
                        "        runtime.bind_storage_cell(\"{}\", {key});\n",
                        escape_rust(&synthetic_occurrence_alias(item, occurrence))
                    ));
                    runtime_setup.push_str(&format!(
                        "        runtime.bind_storage_cell(\"{}\", {key});\n",
                        escape_rust(&program_scoped_alias(
                            ir,
                            &synthetic_occurrence_alias(item, occurrence)
                        ))
                    ));
                }
            }
            let binding_program = storage_binding_program_expr(ir, key_item);
            for alias in aliases {
                runtime_setup.push_str(&format!(
                    "        runtime.bind_occurs_storage_cell(\"{}\", {}, \"{}\");\n",
                    escape_rust(&alias),
                    binding_program,
                    escape_rust(&key_item.qualified_name)
                ));
            }
            for alias in program_scoped_storage_aliases(ir, item) {
                runtime_setup.push_str(&format!(
                    "        runtime.bind_occurs_storage_cell(\"{}\", {}, \"{}\");\n",
                    escape_rust(&alias),
                    binding_program,
                    escape_rust(&key_item.qualified_name)
                ));
            }
        }
    }
    runtime_setup
}

fn emit_runtime_file_status_bindings(ir: &ProgramIr) -> String {
    let mut text = String::new();
    for file in &ir.files {
        if file.kind == FileKindIr::Sd {
            continue;
        }
        let Some(status) = &file.file_status else {
            continue;
        };
        text.push_str(&format!(
            "        runtime.bind_file_status(\"{}\", {});\n",
            escape_rust(&file.name),
            emit_vm_access_path_value(&DataRefIr::simple(status.clone()), ir)
        ));
    }
    text
}

fn emit_runtime_declarative_registrations(ir: &ProgramIr) -> String {
    let mut text = String::new();
    for declarative in &ir.declaratives {
        match &declarative.trigger {
            DeclarativeTriggerIr::FileError { file } => {
                text.push_str(&format!(
                    "        runtime.register_file_error_declarative(\"{}\", Self::{}());\n",
                    escape_rust(file),
                    declarative_method_name(ir, &declarative.name)
                ));
            }
            DeclarativeTriggerIr::Debugging { paragraph } => {
                text.push_str(&format!(
                    "        runtime.register_debugging_declarative(\"{}\", Self::{}());\n",
                    escape_rust(paragraph),
                    declarative_method_name(ir, &declarative.name)
                ));
            }
            DeclarativeTriggerIr::Unsupported { .. } | DeclarativeTriggerIr::Missing => {}
        }
    }
    text
}

fn emit_runtime_rerun_registrations(ir: &ProgramIr) -> String {
    let mut text = String::new();
    for rerun in &ir.rerun_clauses {
        text.push_str(&format!(
            "        runtime.register_rerun_checkpoint(\"{}\", \"{}\", {});\n",
            escape_rust(&rerun.checkpoint_file),
            escape_rust(&rerun.watched_file),
            rerun.every_records
        ));
    }
    text
}

fn emit_runtime_indexes_and_odo(ir: &ProgramIr) -> String {
    let mut text = String::new();
    for index in &ir.indexes {
        text.push_str(&format!(
            "        runtime.define_index(\"{}\", \"{}\", {}, {});\n",
            escape_rust(&index.name),
            escape_rust(&index.table),
            index.occurrence_min,
            index.occurrence_max
        ));
        text.push_str(&format!(
            "        runtime.define_index(\"{}\", \"{}\", {}, {});\n",
            escape_rust(&program_scoped_alias(ir, &index.name)),
            escape_rust(&program_scoped_alias(ir, &index.table)),
            index.occurrence_min,
            index.occurrence_max
        ));
    }
    for odo in &ir.odo_descriptors {
        let active = ir
            .storage
            .items
            .iter()
            .find(|item| item.name.eq_ignore_ascii_case(&odo.depending_on))
            .and_then(|item| item.value.as_ref())
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(odo.min);
        text.push_str(&format!(
            "        let _ = runtime.define_odo(\"{}\", \"{}\", {}, {}, {});\n",
            escape_rust(&odo.table),
            escape_rust(&odo.depending_on),
            odo.min,
            odo.max,
            active
        ));
        text.push_str(&format!(
            "        let _ = runtime.define_odo_for_program(\"{}\", \"{}\", \"{}\", {}, {}, {});\n",
            escape_rust(&ir.name),
            escape_rust(&odo.table),
            escape_rust(&odo.depending_on),
            odo.min,
            odo.max,
            active
        ));
        if let Some(depending_on) = storage_item_by_name(ir, &odo.depending_on) {
            let table_item = storage_item_by_name(ir, &odo.table).unwrap_or(depending_on);
            let program = storage_binding_program_expr(ir, table_item);
            let table = escape_rust(&odo.table);
            let depending_key = scalar_storage_key_expr(ir, depending_on);
            let templates = odo_template_entries(ir, odo)
                .into_iter()
                .map(|(field, bytes)| {
                    let bytes = bytes
                        .iter()
                        .map(|byte| format!("{byte}u8"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!(
                        "            __templates.insert(\"{}\".to_string(), vec![{}]);\n",
                        escape_rust(&field),
                        bytes
                    )
                })
                .collect::<String>();
            text.push_str(&format!(
                "        let odo_key = {};\n        let mut __templates = std::collections::BTreeMap::new();\n{}        let _ = runtime.storage_pool.define_odo_table_with_templates({}, \"{}\", odo_key, {}, {}, {}, {}, __templates);\n",
                depending_key,
                templates,
                program,
                table,
                odo.stride,
                odo.min,
                odo.max,
                active
            ));
        }
    }
    text
}

fn emit_linkage_descriptors(ir: &ProgramIr) -> String {
    let params = ir
        .linkage_signature
        .parameters
        .iter()
        .map(|param| {
            let children = storage_item_by_name(ir, &param.qualified_name)
                .filter(|item| item.value_category == ValueCategoryIr::Group)
                .map(|item| {
                    group_elementary_children(ir, item)
                        .into_iter()
                        .map(|child| {
                            let aliases = storage_aliases(child)
                                .into_iter()
                                .map(|alias| format!("\"{}\".to_string()", escape_rust(&alias)))
                                .collect::<Vec<_>>()
                                .join(", ");
                            format!("cobol_vm::VmLinkageChild {{ aliases: vec![{}] }}", aliases)
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            format!(
                "cobol_vm::VmLinkageParam {{ name: \"{}\".to_string(), children: vec![{}] }}",
                escape_rust(&param.name),
                children
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("vec![{params}]")
}

fn emit_vm_call_op(
    call: &cobol_ir::CallIr,
    ir: &ProgramIr,
    indent: usize,
    programs: Option<&[ProgramIr]>,
) -> String {
    let pad = " ".repeat(indent);
    let target = match &call.target {
        CallTargetIr::Literal(name) => format!(
            "cobol_vm::VmCallTarget::Literal(\"{}\".to_string())",
            escape_rust(name)
        ),
        CallTargetIr::Identifier(reference) => format!(
            "cobol_vm::VmCallTarget::Dynamic({})",
            emit_vm_access_path(reference, ir)
        ),
    };
    let mut pre_ops = String::new();
    let using = call
        .using
        .iter()
        .enumerate()
        .map(|(idx, reference)| {
            if let Some(temp) = programs
                .and_then(|programs| call_temp_for_argument(ir, call, idx, reference, programs))
            {
                let target = call_temp_access_path(&temp);
                pre_ops.push_str(&format!(
                    "{pad}cobol_vm::VmProcedureOp::Move {{ source: {}, target: {} }},\n",
                    emit_vm_access_path(reference, ir),
                    target
                ));
                target
            } else {
                emit_vm_access_path_value(reference, ir)
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{pre_ops}{pad}cobol_vm::VmProcedureOp::Call {{ target: {target}, using: vec![{using}] }},\n"
    )
}

fn collect_call_temps(programs: &[ProgramIr]) -> Vec<CallTempDescriptor> {
    let mut temps = Vec::new();
    let mut seen = HashSet::new();
    for program in programs {
        for paragraph in &program.paragraphs {
            collect_call_temps_from_statements(
                &paragraph.statements,
                program,
                programs,
                &mut temps,
                &mut seen,
            );
        }
    }
    temps
}

fn collect_call_temps_from_statements(
    statements: &[StatementIr],
    program: &ProgramIr,
    programs: &[ProgramIr],
    temps: &mut Vec<CallTempDescriptor>,
    seen: &mut HashSet<String>,
) {
    for statement in statements {
        match statement {
            StatementIr::Call(call) => {
                for (idx, reference) in call.using.iter().enumerate() {
                    if let Some(temp) =
                        call_temp_for_argument(program, call, idx, reference, programs)
                    {
                        if seen.insert(temp.name.clone()) {
                            temps.push(temp);
                        }
                    }
                }
            }
            StatementIr::If {
                then_statements,
                else_statements,
                ..
            } => {
                collect_call_temps_from_statements(then_statements, program, programs, temps, seen);
                collect_call_temps_from_statements(else_statements, program, programs, temps, seen);
            }
            StatementIr::Evaluate(evaluate) => {
                for arm in &evaluate.arms {
                    collect_call_temps_from_statements(
                        &arm.statements,
                        program,
                        programs,
                        temps,
                        seen,
                    );
                }
            }
            StatementIr::Search(search) => {
                collect_call_temps_from_statements(&search.at_end, program, programs, temps, seen);
                for when in &search.whens {
                    collect_call_temps_from_statements(
                        &when.statements,
                        program,
                        programs,
                        temps,
                        seen,
                    );
                }
            }
            StatementIr::SearchAll(search) => {
                collect_call_temps_from_statements(&search.at_end, program, programs, temps, seen);
                collect_call_temps_from_statements(
                    &search.statements,
                    program,
                    programs,
                    temps,
                    seen,
                );
            }
            _ => {}
        }
    }
}

fn call_temp_for_argument(
    caller: &ProgramIr,
    call: &cobol_ir::CallIr,
    arg_idx: usize,
    reference: &DataRefIr,
    programs: &[ProgramIr],
) -> Option<CallTempDescriptor> {
    let CallTargetIr::Literal(target_name) = &call.target else {
        return None;
    };
    let callee = programs
        .iter()
        .find(|program| normalize_vm_ref(&program.name) == normalize_vm_ref(target_name))?;
    let formal = callee.linkage_signature.parameters.get(arg_idx)?;
    let formal_item = storage_item_by_name(callee, &formal.qualified_name)?;
    let actual_item = storage_item_for_ref(reference, caller)?;
    if !call_using_needs_temp(actual_item.value_category, formal_item.value_category) {
        return None;
    }
    if !call_using_temp_supported(actual_item.value_category, formal_item.value_category) {
        return None;
    }
    Some(CallTempDescriptor {
        caller_program: caller.name.clone(),
        name: call_temp_name(caller, target_name, arg_idx, reference, formal_item),
        byte_len: formal_item.byte_len,
        category: formal_item.value_category,
        usage: formal_item.usage.clone(),
        picture: formal_item.picture.clone(),
    })
}

fn call_temp_name(
    caller: &ProgramIr,
    target_name: &str,
    arg_idx: usize,
    reference: &DataRefIr,
    formal: &cobol_ir::StorageItemIr,
) -> String {
    format!(
        "__CALL_TMP_{}_{}_{}_{}_{}",
        normalize_vm_ref(&caller.name),
        normalize_vm_ref(target_name),
        arg_idx,
        normalize_vm_ref(&reference.raw),
        normalize_vm_ref(&formal.qualified_name)
    )
}

fn call_using_needs_temp(actual: ValueCategoryIr, formal: ValueCategoryIr) -> bool {
    actual != formal
        && !matches!(
            (actual, formal),
            (ValueCategoryIr::Alphabetic, ValueCategoryIr::Alphanumeric)
                | (ValueCategoryIr::Alphanumeric, ValueCategoryIr::Alphabetic)
        )
}

fn call_using_temp_supported(actual: ValueCategoryIr, formal: ValueCategoryIr) -> bool {
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
    let formal_supported = matches!(
        formal,
        ValueCategoryIr::Alphanumeric
            | ValueCategoryIr::Alphabetic
            | ValueCategoryIr::NumericEdited
            | ValueCategoryIr::NumericDisplay
    );
    actual_scalar && formal_supported
}

fn call_temp_access_path(temp: &CallTempDescriptor) -> String {
    format!(
        "cobol_vm::VmAccessPath {{ target: \"{}\".to_string(), condition_name: None, subscripts: Vec::new(), reference_modifier: None, result_len: Some({}) }}",
        escape_rust(&temp.name),
        temp.byte_len
    )
}

fn initial_temp_bytes(temp: &CallTempDescriptor) -> Vec<u8> {
    match temp.category {
        ValueCategoryIr::NumericDisplay => vec![b'0'; temp.byte_len],
        _ => vec![b' '; temp.byte_len],
    }
}

fn emit_vm_temp_field(temp: &CallTempDescriptor) -> String {
    let picture = temp
        .picture
        .as_ref()
        .map(|picture| {
            format!(
                "Some(cobol_vm::VmPicture {{ signed: {}, digits: {}, scale: {}, char_len: {} }})",
                picture.signed, picture.digits, picture.scale, picture.char_len
            )
        })
        .unwrap_or_else(|| "None".to_string());
    format!(
        "                cobol_vm::VmField {{ name: \"{}\".to_string(), offset: 0, byte_len: {}, category: {}, usage: {}, picture: {} }},\n",
        escape_rust(&temp.name),
        temp.byte_len,
        vm_category(temp.category),
        vm_usage(&temp.usage),
        picture
    )
}

fn emit_program_default_body(ir: &ProgramIr) -> String {
    let mut text = String::from("        let mut pool = cobol_vm::StoragePool::default();\n");
    for item in &ir.storage.items {
        if !item.addressable || item.value_category == ValueCategoryIr::Group {
            continue;
        }
        if occurs_chain_for_item(item, ir).is_empty() {
            let key_item = storage_cell_key_item(ir, item);
            if key_item.qualified_name != item.qualified_name {
                continue;
            }
            let end = item.offset.saturating_add(item.byte_len);
            let key = scalar_storage_key_expr(ir, item);
            text.push_str(&format!(
                "        if let Some(bytes) = initial.bytes().get({}..{}) {{\n            let _ = pool.define_cell({key}, bytes.to_vec());\n        }}\n",
                item.offset,
                end
            ));
        } else if let Some(occurs_item) = occurs_chain_for_item(item, ir).first().copied() {
            let key_item = storage_cell_key_item(ir, item);
            if key_item.qualified_name != item.qualified_name {
                continue;
            }
            let max = occurs_item
                .occurs
                .as_ref()
                .map(|occurs| occurs.max.max(1))
                .unwrap_or(1);
            let len = occurrence_cell_len(item, occurs_item);
            for occurrence in 1..=max {
                let start = occurrence_source_offset(item, occurs_item, occurrence);
                let end = start.saturating_add(len);
                let key = occurrence_storage_key_expr(ir, item, occurrence);
                text.push_str(&format!(
                    "        if let Some(bytes) = initial.bytes().get({start}..{end}) {{\n            let _ = pool.define_cell({key}, bytes.to_vec());\n        }}\n"
                ));
            }
        }
    }
    text.push_str(&emit_same_record_area_cell_initializers(ir, "initial"));
    text.push_str(&emit_initial_lifecycle_vectors(
        ir,
        "initial",
        "__initial_cells",
        "__initial_odo",
        false,
    ));
    text.push_str(&emit_initial_file_lifecycle_vector(ir, "__initial_files"));
    let mut runtime_setup = String::from(
        "        let mut runtime = cobol_vm::VmRuntime::new(Self::vm_program(), pool);\n",
    );
    runtime_setup.push_str(&emit_runtime_file_definitions(ir));
    runtime_setup.push_str(&format!(
        "        runtime.registry.insert_with_lifecycle_descriptors(\"{}\", Self::vm_procedure(), Vec::new(), {}, __initial_cells, __initial_odo, __initial_files);\n",
        escape_rust(&ir.name),
        ir.is_initial
    ));
    for item in &ir.storage.items {
        if !item.addressable {
            continue;
        }
        let aliases = storage_aliases(item);
        if let Some(binding) = same_record_area_binding(ir, item) {
            let key = same_record_area_storage_key_expr(ir, binding.area_index);
            for alias in aliases {
                runtime_setup.push_str(&format!(
                    "        runtime.bind_storage_slice(\"{}\", {key}.clone(), {}, {});\n",
                    escape_rust(&alias),
                    binding.offset,
                    binding.len
                ));
            }
            for alias in program_scoped_storage_aliases(ir, item) {
                runtime_setup.push_str(&format!(
                    "        runtime.bind_storage_slice(\"{}\", {key}.clone(), {}, {});\n",
                    escape_rust(&alias),
                    binding.offset,
                    binding.len
                ));
            }
        } else if item.value_category == ValueCategoryIr::Group {
            let children = group_storage_child_aliases(ir, item)
                .into_iter()
                .map(|child| format!("\"{}\".to_string()", escape_rust(&child)))
                .collect::<Vec<_>>()
                .join(", ");
            for alias in aliases {
                runtime_setup.push_str(&format!(
                    "        runtime.bind_group_storage(\"{}\", vec![{}]);\n",
                    escape_rust(&alias),
                    children
                ));
            }
        } else if occurs_chain_for_item(item, ir).is_empty() {
            let key_item = storage_cell_key_item(ir, item);
            let key = scalar_storage_key_expr(ir, key_item);
            for alias in aliases {
                runtime_setup.push_str(&format!(
                    "        runtime.bind_storage_cell(\"{}\", {key});\n",
                    escape_rust(&alias)
                ));
            }
        } else {
            let key_item = storage_cell_key_item(ir, item);
            if let Some(occurs_item) = occurs_chain_for_item(item, ir).first().copied() {
                let max = occurs_item
                    .occurs
                    .as_ref()
                    .map(|occurs| occurs.max.max(1))
                    .unwrap_or(1);
                for occurrence in 1..=max {
                    let key = occurrence_storage_key_expr(ir, key_item, occurrence);
                    runtime_setup.push_str(&format!(
                        "        runtime.bind_storage_cell(\"{}\", {key});\n",
                        escape_rust(&synthetic_occurrence_alias(item, occurrence))
                    ));
                }
            }
            let binding_program = storage_binding_program_expr(ir, key_item);
            for alias in aliases {
                runtime_setup.push_str(&format!(
                    "        runtime.bind_occurs_storage_cell(\"{}\", {}, \"{}\");\n",
                    escape_rust(&alias),
                    binding_program,
                    escape_rust(&key_item.qualified_name)
                ));
            }
        }
    }
    runtime_setup.push_str(&emit_runtime_file_status_bindings(ir));
    runtime_setup.push_str(&emit_runtime_declarative_registrations(ir));
    runtime_setup.push_str(&emit_runtime_rerun_registrations(ir));
    text.push_str(&runtime_setup);
    for index in &ir.indexes {
        text.push_str(&format!(
            "        runtime.define_index(\"{}\", \"{}\", {}, {});\n",
            escape_rust(&index.name),
            escape_rust(&index.table),
            index.occurrence_min,
            index.occurrence_max
        ));
    }
    for odo in &ir.odo_descriptors {
        let active = ir
            .storage
            .items
            .iter()
            .find(|item| item.name.eq_ignore_ascii_case(&odo.depending_on))
            .and_then(|item| item.value.as_ref())
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(odo.min);
        text.push_str(&format!(
            "        let _ = runtime.define_odo(\"{}\", \"{}\", {}, {}, {});\n",
            escape_rust(&odo.table),
            escape_rust(&odo.depending_on),
            odo.min,
            odo.max,
            active
        ));
        if let Some(depending_on) = storage_item_by_name(ir, &odo.depending_on) {
            let table_item = storage_item_by_name(ir, &odo.table).unwrap_or(depending_on);
            let program = storage_binding_program_expr(ir, table_item);
            let table = escape_rust(&odo.table);
            let depending_key = scalar_storage_key_expr(ir, depending_on);
            let templates = odo_template_entries(ir, odo)
                .into_iter()
                .map(|(field, bytes)| {
                    let bytes = bytes
                        .iter()
                        .map(|byte| format!("{byte}u8"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!(
                        "            __templates.insert(\"{}\".to_string(), vec![{}]);\n",
                        escape_rust(&field),
                        bytes
                    )
                })
                .collect::<String>();
            text.push_str(&format!(
                "        let odo_key = {};\n        let mut __templates = std::collections::BTreeMap::new();\n{}        let _ = runtime.storage_pool.define_odo_table_with_templates({}, \"{}\", odo_key, {}, {}, {}, {}, __templates);\n",
                depending_key,
                templates,
                program,
                table,
                odo.stride,
                odo.min,
                odo.max,
                active
            ));
        }
    }
    text.push_str(
        "        Self {\n            runtime,\n            display_cursor: 0,\n        }\n",
    );
    text
}

fn storage_item_by_name<'a>(ir: &'a ProgramIr, name: &str) -> Option<&'a cobol_ir::StorageItemIr> {
    ir.storage.items.iter().find(|item| {
        item.name.eq_ignore_ascii_case(name)
            || item.qualified_name.eq_ignore_ascii_case(name)
            || normalize_vm_ref(&item.name) == normalize_vm_ref(name)
            || normalize_vm_ref(&item.qualified_name) == normalize_vm_ref(name)
    })
}

struct SameRecordAreaBinding {
    area_index: usize,
    offset: usize,
    len: usize,
}

fn same_record_area_binding(
    ir: &ProgramIr,
    item: &cobol_ir::StorageItemIr,
) -> Option<SameRecordAreaBinding> {
    for (area_index, area) in ir.same_record_areas.iter().enumerate() {
        for record in same_record_area_records(ir, area) {
            let is_record = item
                .qualified_name
                .eq_ignore_ascii_case(&record.qualified_name);
            let is_descendant = item
                .qualified_name
                .strip_prefix(&format!("{}.", record.qualified_name))
                .is_some();
            if is_record || is_descendant {
                return Some(SameRecordAreaBinding {
                    area_index,
                    offset: item.offset.saturating_sub(record.offset),
                    len: item.byte_len,
                });
            }
        }
    }
    None
}

fn same_record_area_records<'a>(
    ir: &'a ProgramIr,
    area: &cobol_ir::SameRecordAreaIr,
) -> Vec<&'a cobol_ir::StorageItemIr> {
    area.files
        .iter()
        .filter_map(|file_name| {
            ir.files
                .iter()
                .find(|file| file.name.eq_ignore_ascii_case(file_name))
                .and_then(|file| file.record_name.as_deref())
                .and_then(|record| storage_item_by_name(ir, record))
        })
        .collect()
}

fn same_record_area_representative<'a>(
    ir: &'a ProgramIr,
    area: &cobol_ir::SameRecordAreaIr,
) -> Option<(&'a cobol_ir::StorageItemIr, usize)> {
    let records = same_record_area_records(ir, area);
    let first = records.first().copied()?;
    let len = records
        .iter()
        .map(|record| record.byte_len)
        .max()
        .unwrap_or(first.byte_len);
    Some((first, len))
}

fn same_record_area_storage_key_expr(ir: &ProgramIr, area_index: usize) -> String {
    format!(
        "cobol_vm::StorageKey::scalar(\"{}\", \"__SAME_RECORD_AREA_{}\")",
        escape_rust(&ir.name),
        area_index
    )
}

fn storage_aliases(item: &cobol_ir::StorageItemIr) -> Vec<String> {
    let mut aliases = vec![
        item.name.clone(),
        item.qualified_name.clone(),
        normalize_vm_ref(&item.name),
        normalize_vm_ref(&item.qualified_name),
    ];
    aliases.sort();
    aliases.dedup();
    aliases
}

fn program_scoped_alias(ir: &ProgramIr, alias: &str) -> String {
    format!("{}.{}", normalize_vm_ref(&ir.name), normalize_vm_ref(alias))
}

fn program_scoped_storage_aliases(ir: &ProgramIr, item: &cobol_ir::StorageItemIr) -> Vec<String> {
    let mut aliases = storage_aliases(item)
        .into_iter()
        .map(|alias| program_scoped_alias(ir, &alias))
        .collect::<Vec<_>>();
    aliases.sort();
    aliases.dedup();
    aliases
}

fn storage_cell_key_item<'a>(
    ir: &'a ProgramIr,
    item: &'a cobol_ir::StorageItemIr,
) -> &'a cobol_ir::StorageItemIr {
    ir.storage
        .items
        .iter()
        .find(|candidate| {
            candidate.addressable
                && candidate.value_category != ValueCategoryIr::Group
                && candidate.offset == item.offset
                && candidate.byte_len == item.byte_len
                && candidate.qualified_name < item.qualified_name
        })
        .unwrap_or(item)
}

fn scalar_storage_key_expr(ir: &ProgramIr, item: &cobol_ir::StorageItemIr) -> String {
    if item.external {
        format!(
            "cobol_vm::StorageKey::external(\"{}\")",
            escape_rust(&item.qualified_name)
        )
    } else {
        format!(
            "cobol_vm::StorageKey::scalar(\"{}\", \"{}\")",
            escape_rust(&ir.name),
            escape_rust(&item.qualified_name)
        )
    }
}

fn occurrence_storage_key_expr(
    ir: &ProgramIr,
    item: &cobol_ir::StorageItemIr,
    occurrence: usize,
) -> String {
    if item.external {
        format!(
            "cobol_vm::StorageKey::external_occurrence(\"{}\", vec![{}usize])",
            escape_rust(&item.qualified_name),
            occurrence
        )
    } else {
        format!(
            "cobol_vm::StorageKey::occurrence(\"{}\", \"{}\", vec![{}usize])",
            escape_rust(&ir.name),
            escape_rust(&item.qualified_name),
            occurrence
        )
    }
}

fn storage_binding_program_expr(ir: &ProgramIr, item: &cobol_ir::StorageItemIr) -> String {
    if item.external {
        "cobol_vm::StorageKey::EXTERNAL_PROGRAM".to_string()
    } else {
        format!("\"{}\"", escape_rust(&ir.name))
    }
}

fn group_elementary_children<'a>(
    ir: &'a ProgramIr,
    group: &'a cobol_ir::StorageItemIr,
) -> Vec<&'a cobol_ir::StorageItemIr> {
    let prefix = format!("{}.", group.qualified_name);
    let mut children = ir
        .storage
        .items
        .iter()
        .filter(|item| {
            item.addressable
                && item.value_category != ValueCategoryIr::Group
                && item.qualified_name.starts_with(&prefix)
        })
        .collect::<Vec<_>>();
    children.sort_by_key(|item| item.offset);
    children
}

fn group_storage_child_aliases(ir: &ProgramIr, group: &cobol_ir::StorageItemIr) -> Vec<String> {
    if let Some(rename) = ir.storage.renames.iter().find(|rename| {
        rename
            .renaming_item
            .eq_ignore_ascii_case(&group.qualified_name)
    }) {
        return rename
            .targets
            .iter()
            .map(|target| normalize_vm_ref(target))
            .collect();
    }
    let mut aliases = Vec::new();
    for child in group_elementary_children(ir, group) {
        if let Some(occurs_item) = occurs_chain_for_item(child, ir).first().copied() {
            let max = occurs_item
                .occurs
                .as_ref()
                .map(|occurs| occurs.max.max(1))
                .unwrap_or(1);
            for occurrence in 1..=max {
                aliases.push(synthetic_occurrence_alias(child, occurrence));
            }
        } else {
            aliases.push(normalize_vm_ref(&child.qualified_name));
        }
    }
    aliases
}

fn program_scoped_group_storage_child_aliases(
    ir: &ProgramIr,
    group: &cobol_ir::StorageItemIr,
) -> Vec<String> {
    group_storage_child_aliases(ir, group)
        .into_iter()
        .map(|alias| program_scoped_alias(ir, &alias))
        .collect()
}

fn synthetic_occurrence_alias(item: &cobol_ir::StorageItemIr, occurrence: usize) -> String {
    format!(
        "__{}_OCC_{}",
        normalize_vm_ref(&item.qualified_name),
        occurrence
    )
}

fn occurrence_cell_len(
    item: &cobol_ir::StorageItemIr,
    occurs_item: &cobol_ir::StorageItemIr,
) -> usize {
    if item.qualified_name == occurs_item.qualified_name {
        occurs_stride(occurs_item)
    } else {
        item.byte_len
    }
}

fn occurrence_source_offset(
    item: &cobol_ir::StorageItemIr,
    occurs_item: &cobol_ir::StorageItemIr,
    occurrence: usize,
) -> usize {
    item.offset
        .saturating_add((occurrence.saturating_sub(1)).saturating_mul(occurs_stride(occurs_item)))
}

fn odo_template_entries(ir: &ProgramIr, odo: &cobol_ir::OdoDescriptorIr) -> Vec<(String, Vec<u8>)> {
    let Some(table) = storage_item_by_name(ir, &odo.table) else {
        return Vec::new();
    };
    let mut entries = if table.value_category == ValueCategoryIr::Group {
        group_elementary_children(ir, table)
    } else {
        vec![table]
    };
    entries.sort_by_key(|item| item.offset);
    entries
        .into_iter()
        .filter(|item| item.value_category != ValueCategoryIr::Group)
        .map(|item| {
            (
                item.qualified_name.clone(),
                initial_template_bytes_for_item(item, occurrence_cell_len(item, table)),
            )
        })
        .collect()
}

fn initial_template_bytes_for_item(item: &cobol_ir::StorageItemIr, len: usize) -> Vec<u8> {
    let mut bytes = match item.value_category {
        ValueCategoryIr::NumericDisplay => {
            render_display_numeric_template(item.value.as_deref(), len)
        }
        ValueCategoryIr::PackedDecimal => packed_decimal_template_or_zero(item, len),
        ValueCategoryIr::Alphanumeric
        | ValueCategoryIr::Alphabetic
        | ValueCategoryIr::Group
        | ValueCategoryIr::NumericEdited => {
            let mut bytes = vec![b' '; len];
            if let Some(value) = &item.value {
                for (idx, byte) in value.as_bytes().iter().take(len).enumerate() {
                    bytes[idx] = *byte;
                }
            }
            bytes
        }
        _ => vec![0u8; len],
    };
    bytes.resize(len, b' ');
    bytes.truncate(len);
    bytes
}

fn packed_decimal_template_or_zero(item: &cobol_ir::StorageItemIr, len: usize) -> Vec<u8> {
    packed_decimal_initial_bytes(item, len).unwrap_or_else(|_| {
        let Some(picture) = &item.picture else {
            return vec![0u8; len];
        };
        cobol_record::encode_packed_decimal(
            Decimal::ZERO,
            picture.digits,
            picture.scale as u32,
            picture.signed,
        )
        .map(|bytes| fit_template_bytes(bytes, len, 0u8))
        .unwrap_or_else(|_| vec![0u8; len])
    })
}

fn packed_decimal_initial_bytes(
    item: &cobol_ir::StorageItemIr,
    len: usize,
) -> Result<Vec<u8>, String> {
    let picture = item
        .picture
        .as_ref()
        .ok_or_else(|| "missing PIC metadata".to_string())?;
    let value = parse_initial_decimal(item.value.as_deref().unwrap_or("0"))?;
    let bytes = cobol_record::encode_packed_decimal(
        value,
        picture.digits,
        picture.scale as u32,
        picture.signed,
    )
    .map_err(|err| err.to_string())?;
    if bytes.len() != len {
        return Err(format!(
            "encoded length {} does not match storage length {len}",
            bytes.len()
        ));
    }
    Ok(bytes)
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

fn fit_template_bytes(mut bytes: Vec<u8>, len: usize, pad: u8) -> Vec<u8> {
    bytes.resize(len, pad);
    bytes.truncate(len);
    bytes
}

fn render_display_numeric_template(value: Option<&str>, len: usize) -> Vec<u8> {
    let mut text = value.unwrap_or("0").trim().to_string();
    if text.len() > len {
        text = text[text.len() - len..].to_string();
    }
    while text.len() < len {
        text.insert(0, '0');
    }
    text.into_bytes()
}

#[allow(dead_code)]
fn emit_display_ref_method(_ir: &ProgramIr) -> String {
    "    fn display_ref(&self, _name: &str) -> Result<String, RuntimeError> {\n        Ok(String::new())\n    }\n".to_string()
}

fn emit_vm_methods(ir: &ProgramIr) -> String {
    let mut text = format!(
        "\n    fn vm_program() -> cobol_vm::VmProgram {{\n        cobol_vm::VmProgram::with_declared_views(\n            {},\n            vec![\n",
        emit_dialect_profile_constructor(&ir.dialect_profile)
    );
    for item in &ir.storage.items {
        if !item.addressable {
            continue;
        }
        text.push_str(&emit_vm_field(item, &item.name));
        if item.name != item.qualified_name {
            text.push_str(&emit_vm_field(item, &item.qualified_name));
        }
    }
    text.push_str("            ],\n            vec![\n");
    for condition in &ir.storage.condition_names {
        let values = condition
            .value_set
            .iter()
            .map(|value| match value {
                cobol_ir::ConditionValueIr::Single(value) => format!(
                    "cobol_vm::VmConditionValue::Single(\"{}\".to_string())",
                    escape_rust(value)
                ),
                cobol_ir::ConditionValueIr::Range { start, end } => format!(
                    "cobol_vm::VmConditionValue::Range {{ start: \"{}\".to_string(), end: \"{}\".to_string() }}",
                    escape_rust(start),
                    escape_rust(end)
                ),
            })
            .collect::<Vec<_>>()
            .join(", ");
        text.push_str(&format!(
            "                cobol_vm::VmConditionName {{ name: \"{}\".to_string(), parent: \"{}\".to_string(), values: vec![{}] }},\n",
            escape_rust(&condition.name),
            escape_rust(&condition.parent),
            values
        ));
    }
    text.push_str("            ],\n            vec![\n");
    text.push_str(&emit_condition_declared_views(ir));
    text.push_str("            ],\n        )\n    }\n");
    text
}

fn emit_condition_declared_views(ir: &ProgramIr) -> String {
    ir.storage
        .condition_names
        .iter()
        .filter_map(|condition| emit_condition_declared_view(ir, condition))
        .collect()
}

fn emit_condition_declared_view(
    ir: &ProgramIr,
    condition: &cobol_ir::ConditionNameIr,
) -> Option<String> {
    let parent = storage_item_by_name(ir, &condition.parent)?;
    if parent.value_category != ValueCategoryIr::Group {
        return None;
    }
    let children = group_storage_child_aliases(ir, parent)
        .into_iter()
        .map(|child| format!("\"{}\".to_string()", escape_rust(&child)))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "                cobol_vm::VmDeclaredView {{ condition: \"{}\".to_string(), parent: \"{}\".to_string(), children: vec![{}] }},\n",
        escape_rust(&condition.name),
        escape_rust(&condition.parent),
        children
    ))
}

fn emit_vm_procedure_method(ir: &ProgramIr) -> String {
    emit_vm_procedure_method_named_with_programs(ir, "vm_procedure", None)
}

fn declarative_method_name(ir: &ProgramIr, name: &str) -> String {
    format!(
        "vm_declarative_{}_{}",
        program_suffix(&ir.name),
        rust_ident(name)
    )
}

fn emit_vm_declarative_methods(ir: &ProgramIr, programs: Option<&[ProgramIr]>) -> String {
    let mut text = String::new();
    for declarative in &ir.declaratives {
        if !matches!(
            declarative.trigger,
            DeclarativeTriggerIr::FileError { .. } | DeclarativeTriggerIr::Debugging { .. }
        ) {
            continue;
        }
        text.push_str(&format!(
            "\n    fn {}() -> Vec<cobol_vm::VmProcedureOp> {{\n        vec![\n{}        ]\n    }}\n",
            declarative_method_name(ir, &declarative.name),
            emit_vm_op_vec_with_programs(&declarative.statements, ir, 12, programs)
        ));
    }
    text
}

fn emit_vm_methods_multi(programs: &[ProgramIr]) -> String {
    let dialect_constructor = programs
        .first()
        .map(|program| emit_dialect_profile_constructor(&program.dialect_profile))
        .unwrap_or("cobol_dialect::DialectProfile::ibm_zos()");
    let mut text = format!(
        "\n    fn vm_program() -> cobol_vm::VmProgram {{\n        cobol_vm::VmProgram::with_declared_views(\n            {dialect_constructor},\n            vec![\n",
    );
    let mut emitted_fields = HashSet::new();
    for program in programs {
        for item in &program.storage.items {
            if !item.addressable {
                continue;
            }
            for name in storage_aliases(item) {
                let key = normalize_vm_ref(&name);
                if emitted_fields.insert(key) {
                    text.push_str(&emit_vm_field(item, &name));
                }
            }
            for name in program_scoped_storage_aliases(program, item) {
                let key = normalize_vm_ref(&name);
                if emitted_fields.insert(key) {
                    text.push_str(&emit_vm_field(item, &name));
                }
            }
        }
    }
    for temp in collect_call_temps(programs) {
        let key = normalize_vm_ref(&temp.name);
        if emitted_fields.insert(key) {
            text.push_str(&emit_vm_temp_field(&temp));
        }
    }
    text.push_str("            ],\n            vec![\n");
    let mut emitted_conditions = HashSet::new();
    let mut declared_views = String::new();
    for program in programs {
        for condition in &program.storage.condition_names {
            if !emitted_conditions.insert(normalize_vm_ref(&condition.name)) {
                continue;
            }
            let values = condition
                .value_set
                .iter()
                .map(|value| match value {
                    cobol_ir::ConditionValueIr::Single(value) => format!(
                        "cobol_vm::VmConditionValue::Single(\"{}\".to_string())",
                        escape_rust(value)
                    ),
                    cobol_ir::ConditionValueIr::Range { start, end } => format!(
                        "cobol_vm::VmConditionValue::Range {{ start: \"{}\".to_string(), end: \"{}\".to_string() }}",
                        escape_rust(start),
                        escape_rust(end)
                    ),
                })
                .collect::<Vec<_>>()
                .join(", ");
            text.push_str(&format!(
                "                cobol_vm::VmConditionName {{ name: \"{}\".to_string(), parent: \"{}\".to_string(), values: vec![{}] }},\n",
                escape_rust(&condition.name),
                escape_rust(&condition.parent),
                values
            ));
            if let Some(view) = emit_condition_declared_view(program, condition) {
                declared_views.push_str(&view);
            }
        }
    }
    text.push_str("            ],\n            vec![\n");
    text.push_str(&declared_views);
    text.push_str("            ],\n        )\n    }\n");
    text
}

fn emit_dialect_profile_constructor(profile: &DialectProfileIr) -> &'static str {
    match profile.dialect {
        CobolDialect::Ibm => "cobol_dialect::DialectProfile::ibm_zos()",
        CobolDialect::GnuCobol => "cobol_dialect::DialectProfile::gnucobol()",
        CobolDialect::MicroFocus => "cobol_dialect::DialectProfile::micro_focus()",
    }
}

fn emit_vm_procedure_method_named_with_programs(
    ir: &ProgramIr,
    method_name: &str,
    programs: Option<&[ProgramIr]>,
) -> String {
    let entry = ir
        .procedure_cfg
        .entry
        .as_ref()
        .map(|entry| escape_rust(entry))
        .unwrap_or_default();
    let mut text = format!(
        "\n    fn {method_name}() -> cobol_vm::VmProcedure {{\n        cobol_vm::VmProcedure {{\n            entry: \"{entry}\".to_string(),\n            blocks: vec![\n"
    );
    for (idx, paragraph) in ir.paragraphs.iter().enumerate() {
        text.push_str(&format!(
            "                cobol_vm::VmBasicBlock {{\n                    name: \"{}\".to_string(),\n                    ops: vec![\n",
            escape_rust(&paragraph.name)
        ));
        for statement in &paragraph.statements {
            if statement_is_terminal(statement) {
                break;
            }
            text.push_str(&emit_vm_procedure_op_with_programs(
                statement, ir, 24, programs,
            ));
        }
        text.push_str("                    ],\n");
        text.push_str(&format!(
            "                    transfer: {},\n                }},\n",
            emit_vm_transfer(paragraph, idx, ir)
        ));
    }
    text.push_str("            ],\n        }\n    }\n");
    text
}

fn emit_vm_transfer(paragraph: &cobol_ir::ParagraphIr, idx: usize, ir: &ProgramIr) -> String {
    for statement in &paragraph.statements {
        match statement {
            StatementIr::GoTo(target) => {
                if target == "." {
                    return format!(
                        "cobol_vm::VmControlTransfer::AlteredGoTo {{ slot: \"{}\".to_string() }}",
                        escape_rust(&paragraph.name)
                    );
                }
                return format!(
                    "cobol_vm::VmControlTransfer::GoTo(\"{}\".to_string())",
                    escape_rust(target)
                );
            }
            StatementIr::StopRun => return "cobol_vm::VmControlTransfer::StopRun".to_string(),
            _ => {}
        }
    }
    ir.paragraphs
        .get(idx + 1)
        .map(|next| {
            format!(
                "cobol_vm::VmControlTransfer::FallThrough(Some(\"{}\".to_string()))",
                escape_rust(&next.name)
            )
        })
        .unwrap_or_else(|| "cobol_vm::VmControlTransfer::FallThrough(None)".to_string())
}

fn emit_vm_procedure_op_with_programs(
    statement: &StatementIr,
    ir: &ProgramIr,
    indent: usize,
    programs: Option<&[ProgramIr]>,
) -> String {
    let pad = " ".repeat(indent);
    match statement {
        StatementIr::Display(values) => {
            let values = values
                .iter()
                .map(|value| emit_vm_expr_from_operand(value, ir))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{pad}cobol_vm::VmProcedureOp::Display(vec![{values}]),\n")
        }
        StatementIr::Move { source, target } => format!(
            "{pad}cobol_vm::VmProcedureOp::Move {{ source: {}, target: {} }},\n",
            emit_vm_expr_from_operand(source, ir),
            emit_vm_access_path_value(target, ir)
        ),
        StatementIr::Continue => format!("{pad}cobol_vm::VmProcedureOp::Noop,\n"),
        StatementIr::NextSentence => emit_vm_unsupported_trap(
            &pad,
            "NEXT SENTENCE reached VM emission without executable period-scope lowering",
        ),
        StatementIr::ReadyTrace => format!("{pad}cobol_vm::VmProcedureOp::TraceOn,\n"),
        StatementIr::ResetTrace => format!("{pad}cobol_vm::VmProcedureOp::TraceOff,\n"),
        StatementIr::MoveCorresponding { source, target } => {
            emit_vm_move_corresponding_ops(source, target, ir, &pad)
        }
        StatementIr::Add { source, target } => {
            emit_vm_arithmetic_op("Add", source, target, ir, &pad)
        }
        StatementIr::Subtract { source, target } => {
            emit_vm_arithmetic_op("Subtract", source, target, ir, &pad)
        }
        StatementIr::Multiply { source, target } => {
            emit_vm_arithmetic_op("Multiply", source, target, ir, &pad)
        }
        StatementIr::Divide { source, target } => {
            emit_vm_arithmetic_op("Divide", source, target, ir, &pad)
        }
        StatementIr::If {
            condition_tree,
            then_statements,
            else_statements,
            ..
        } => {
            let condition = condition_tree
                .as_ref()
                .map(|condition| emit_vm_condition(condition, ir))
                .unwrap_or_else(|| {
                    "cobol_vm::VmCondition::Relation { left: cobol_vm::VmExpr::Bool(true), op: cobol_vm::VmRelOp::Equal, right: cobol_vm::VmExpr::Bool(false) }".to_string()
                });
            let then_ops = emit_vm_op_vec_with_programs(then_statements, ir, indent + 12, programs);
            let else_ops = emit_vm_op_vec_with_programs(else_statements, ir, indent + 12, programs);
            format!(
                "{pad}cobol_vm::VmProcedureOp::If {{\n{pad}    condition: {condition},\n{pad}    then_ops: vec![\n{then_ops}{pad}    ],\n{pad}    else_ops: vec![\n{else_ops}{pad}    ],\n{pad}}},\n"
            )
        }
        StatementIr::Evaluate(evaluate) => {
            let evaluate_expr = emit_vm_evaluate(evaluate, ir);
            let mut branches = String::new();
            for arm in &evaluate.arms {
                branches.push_str(&format!("{pad}        vec![\n"));
                branches.push_str(&emit_vm_op_vec_with_programs(
                    &arm.statements,
                    ir,
                    indent + 12,
                    programs,
                ));
                branches.push_str(&format!("{pad}        ],\n"));
            }
            format!(
                "{pad}cobol_vm::VmProcedureOp::Evaluate {{\n{pad}    evaluate: {evaluate_expr},\n{pad}    branches: vec![\n{branches}{pad}    ],\n{pad}}},\n"
            )
        }
        StatementIr::Search(search) => emit_vm_search_op(search, ir, indent),
        StatementIr::SearchAll(search) => emit_vm_search_all_op(search, ir, indent),
        StatementIr::SetCondition { condition, value } => {
            if *value {
                format!(
                    "{pad}cobol_vm::VmProcedureOp::SetConditionName {{ name: \"{}\".to_string() }},\n",
                    escape_rust(&condition.normalized)
                )
            } else {
                format!("{pad}cobol_vm::VmProcedureOp::StopRun,\n")
            }
        }
        StatementIr::SetIndex { index, operation } => {
            let operation = emit_vm_set_index_operation(operation, ir);
            format!(
                "{pad}cobol_vm::VmProcedureOp::SetIndex {{ name: \"{}\".to_string(), operation: {operation} }},\n",
                escape_rust(index)
            )
        }
        StatementIr::Perform {
            target,
            through,
            varying_ir,
            until_tree,
            times,
            ..
        } => {
            if paragraph_index(ir, target).is_none() && storage_item_by_name(ir, target).is_some() {
                return format!(
                    "{pad}cobol_vm::VmProcedureOp::DynamicPerform {{ target: {} }},\n",
                    emit_vm_expr_from_operand(
                        &OperandIr::Identifier(cobol_ir::DataRefIr {
                            raw: target.clone(),
                            normalized: target.clone(),
                            parts: vec![target.clone()],
                            subscripts: vec![],
                            reference_modifier: None,
                        }),
                        ir
                    )
                );
            }
            let through = through
                .as_ref()
                .map(|target| format!("Some(\"{}\".to_string())", escape_rust(target)))
                .unwrap_or_else(|| "None".to_string());
            if varying_ir.is_some() || until_tree.is_some() {
                let varying = varying_ir
                    .as_ref()
                    .map(|varying| emit_vm_perform_varying(varying, ir))
                    .map(|value| format!("Some({value})"))
                    .unwrap_or_else(|| "None".to_string());
                let until = until_tree
                    .as_ref()
                    .map(|condition| format!("Some({})", emit_vm_condition(condition, ir)))
                    .unwrap_or_else(|| "None".to_string());
                return format!(
                    "{pad}cobol_vm::VmProcedureOp::PerformLoop {{ target: \"{}\".to_string(), through: {through}, varying: {varying}, until: {until} }},\n",
                    escape_rust(target)
                );
            }
            let times = times
                .as_ref()
                .map(|operand| format!("Some({})", emit_vm_expr_from_operand(operand, ir)))
                .unwrap_or_else(|| "None".to_string());
            format!(
                "{pad}cobol_vm::VmProcedureOp::Perform {{ target: \"{}\".to_string(), through: {through}, times: {times} }},\n",
                escape_rust(target)
            )
        }
        StatementIr::GoTo(target) => format!(
            "{pad}cobol_vm::VmProcedureOp::GoTo {{ target: \"{}\".to_string() }},\n",
            escape_rust(target)
        ),
        StatementIr::ComputedGoTo {
            targets,
            depending_on,
        } => {
            let targets = targets
                .iter()
                .map(|target| format!("\"{}\".to_string()", escape_rust(target)))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "{pad}cobol_vm::VmProcedureOp::ComputedGoTo {{ targets: vec![{targets}], depending_on: {} }},\n",
                emit_vm_expr_from_operand(depending_on, ir)
            )
        }
        StatementIr::Alter { paragraph, target } => format!(
            "{pad}cobol_vm::VmProcedureOp::Alter {{ paragraph: \"{}\".to_string(), target: \"{}\".to_string() }},\n",
            escape_rust(paragraph),
            escape_rust(target)
        ),
        StatementIr::Call(call) => emit_vm_call_op(call, ir, indent, programs),
        StatementIr::StopRun => format!("{pad}cobol_vm::VmProcedureOp::StopRun,\n"),
        StatementIr::Compute { .. } => emit_vm_compute_op(statement, ir, &pad),
        StatementIr::Unsupported { keyword, raw } => emit_vm_unsupported_trap(
            &pad,
            &format!("unsupported COBOL statement reached code generation: {keyword} {raw}"),
        ),
        StatementIr::OpenFile(open) => emit_vm_file_open_typed_op(open, &pad),
        StatementIr::ReadFile(read) => emit_vm_file_read_typed_op(read, ir, &pad),
        StatementIr::WriteFile(write) => emit_vm_file_write_typed_op(write, ir, &pad),
        StatementIr::RewriteFile(rewrite) => emit_vm_file_rewrite_typed_op(rewrite, ir, &pad),
        StatementIr::DeleteFile(delete) => emit_vm_file_delete_typed_op(delete, ir, &pad),
        StatementIr::CloseFile(close) => emit_vm_file_close_typed_op(close, &pad),
        StatementIr::SortProcedure(sort) => emit_vm_sort_procedure_op(sort, ir, &pad),
        StatementIr::ReleaseSortRecord(release) => {
            let source = release
                .from
                .as_ref()
                .map(|source| format!("Some({})", emit_vm_access_path_value(source, ir)))
                .unwrap_or_else(|| "None".to_string());
            format!(
                "{pad}cobol_vm::VmProcedureOp::ReleaseSortRecord {{ record: {}, source: {source} }},\n",
                emit_vm_access_path_value(&release.record, ir)
            )
        }
        StatementIr::ReturnSortRecord(ret) => emit_vm_return_sort_record_op(ret, ir, &pad),
        StatementIr::InspectLike(inspect) => emit_vm_inspect_like_op(inspect, ir, &pad),
        StatementIr::StringOp(string) => emit_vm_string_op(string, ir, &pad),
        StatementIr::UnstringOp(unstring) => emit_vm_unstring_op(unstring, ir, &pad),
    }
}

fn emit_vm_op_vec(statements: &[StatementIr], ir: &ProgramIr, indent: usize) -> String {
    emit_vm_op_vec_with_programs(statements, ir, indent, None)
}

fn emit_vm_op_vec_with_programs(
    statements: &[StatementIr],
    ir: &ProgramIr,
    indent: usize,
    programs: Option<&[ProgramIr]>,
) -> String {
    let mut text = String::new();
    for statement in statements {
        text.push_str(&emit_vm_procedure_op_with_programs(
            statement, ir, indent, programs,
        ));
        if statement_is_terminal(statement) {
            break;
        }
    }
    text
}

fn emit_vm_sort_procedure_op(
    sort: &cobol_ir::SortProcedureIr,
    ir: &ProgramIr,
    pad: &str,
) -> String {
    let record = sort_file_record_name(ir, &sort.file).unwrap_or_else(|| sort.file.clone());
    let input = sort
        .input_range
        .as_ref()
        .map(emit_vm_procedure_range)
        .map(|range| format!("Some({range})"))
        .unwrap_or_else(|| "None".to_string());
    let output = emit_vm_procedure_range(&sort.output_range);
    let key = emit_vm_sort_key_descriptor(sort, ir);
    format!(
        "{pad}cobol_vm::VmProcedureOp::SortProcedure {{\n{pad}    file: \"{}\".to_string(),\n{pad}    record: {},\n{pad}    key: {key},\n{pad}    input: {input},\n{pad}    output: {output},\n{pad}}},\n",
        escape_rust(&sort.file),
        emit_vm_access_path_value(&DataRefIr::simple(record), ir)
    )
}

fn emit_vm_return_sort_record_op(
    ret: &cobol_ir::ReturnSortRecordIr,
    ir: &ProgramIr,
    pad: &str,
) -> String {
    let record = sort_file_record_name(ir, &ret.file).unwrap_or_else(|| ret.file.clone());
    let target = ret
        .into
        .as_ref()
        .map(|target| format!("Some({})", emit_vm_access_path_value(target, ir)))
        .unwrap_or_else(|| "None".to_string());
    let at_end_ops = emit_vm_op_vec(&ret.at_end_ops, ir, pad.len() + 4);
    let not_at_end_ops = emit_vm_op_vec(&ret.not_at_end_ops, ir, pad.len() + 4);
    format!(
        "{pad}cobol_vm::VmProcedureOp::ReturnSortRecord {{ file: \"{}\".to_string(), record: {}, target: {target}, at_end_ops: vec![\n{}{pad}], not_at_end_ops: vec![\n{}{pad}] }},\n",
        escape_rust(&ret.file),
        emit_vm_access_path_value(&DataRefIr::simple(record), ir),
        at_end_ops,
        not_at_end_ops
    )
}

fn emit_vm_inspect_like_op(inspect: &cobol_ir::InspectLikeIr, ir: &ProgramIr, pad: &str) -> String {
    let tally = inspect
        .tally
        .as_ref()
        .map(|tally| {
            format!(
                "Some(cobol_vm::VmInspectTally {{ target: {}, pattern: \"{}\".to_string() }})",
                emit_vm_access_path_value(&tally.target, ir),
                escape_rust(&tally.pattern)
            )
        })
        .unwrap_or_else(|| "None".to_string());
    let replacing = inspect
        .replacing
        .as_ref()
        .map(|replacing| {
            format!(
                "Some(cobol_vm::VmInspectReplacing {{ pattern: \"{}\".to_string(), replacement: \"{}\".to_string() }})",
                escape_rust(&replacing.pattern),
                escape_rust(&replacing.replacement)
            )
        })
        .unwrap_or_else(|| "None".to_string());
    let converting = inspect
        .converting
        .as_ref()
        .map(|converting| {
            format!(
                "Some(cobol_vm::VmInspectConverting {{ from: \"{}\".to_string(), to: \"{}\".to_string() }})",
                escape_rust(&converting.from),
                escape_rust(&converting.to)
            )
        })
        .unwrap_or_else(|| "None".to_string());
    format!(
        "{pad}cobol_vm::VmProcedureOp::InspectLike {{ subject: {}, tally: {tally}, replacing: {replacing}, converting: {converting} }},\n",
        emit_vm_access_path_value(&inspect.subject, ir)
    )
}

fn emit_vm_string_op(string: &cobol_ir::StringOpIr, ir: &ProgramIr, pad: &str) -> String {
    let pieces = string
        .pieces
        .iter()
        .map(|piece| {
            format!(
                "cobol_vm::VmStringPiece {{ source: {}, delimiter: {} }}",
                emit_vm_expr_from_operand(&piece.source, ir),
                emit_vm_string_delimiter(&piece.delimiter)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let pointer = string
        .pointer
        .as_ref()
        .map(|pointer| format!("Some({})", emit_vm_access_path_value(pointer, ir)))
        .unwrap_or_else(|| "None".to_string());
    let on_overflow_ops = emit_vm_op_vec(&string.on_overflow_ops, ir, pad.len() + 4);
    let not_on_overflow_ops = emit_vm_op_vec(&string.not_on_overflow_ops, ir, pad.len() + 4);
    format!(
        "{pad}cobol_vm::VmProcedureOp::StringOp {{ pieces: vec![{pieces}], target: {}, pointer: {pointer}, on_overflow_ops: vec![\n{}{pad}], not_on_overflow_ops: vec![\n{}{pad}] }},\n",
        emit_vm_access_path_value(&string.target, ir),
        on_overflow_ops,
        not_on_overflow_ops
    )
}

fn emit_vm_unstring_op(unstring: &cobol_ir::UnstringOpIr, ir: &ProgramIr, pad: &str) -> String {
    let targets = unstring
        .targets
        .iter()
        .map(|target| {
            let count = target
                .count
                .as_ref()
                .map(|count| format!("Some({})", emit_vm_access_path_value(count, ir)))
                .unwrap_or_else(|| "None".to_string());
            format!(
                "cobol_vm::VmUnstringTarget {{ target: {}, count: {count} }}",
                emit_vm_access_path_value(&target.target, ir)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let pointer = unstring
        .pointer
        .as_ref()
        .map(|pointer| format!("Some({})", emit_vm_access_path_value(pointer, ir)))
        .unwrap_or_else(|| "None".to_string());
    let tallying = unstring
        .tallying
        .as_ref()
        .map(|tallying| format!("Some({})", emit_vm_access_path_value(tallying, ir)))
        .unwrap_or_else(|| "None".to_string());
    let on_overflow_ops = emit_vm_op_vec(&unstring.on_overflow_ops, ir, pad.len() + 4);
    let not_on_overflow_ops = emit_vm_op_vec(&unstring.not_on_overflow_ops, ir, pad.len() + 4);
    format!(
        "{pad}cobol_vm::VmProcedureOp::UnstringOp {{ source: {}, delimiter: {}, targets: vec![{targets}], pointer: {pointer}, tallying: {tallying}, on_overflow_ops: vec![\n{}{pad}], not_on_overflow_ops: vec![\n{}{pad}] }},\n",
        emit_vm_expr_from_operand(&unstring.source, ir),
        emit_vm_string_delimiter(&unstring.delimiter),
        on_overflow_ops,
        not_on_overflow_ops
    )
}

fn emit_vm_string_delimiter(delimiter: &cobol_ir::StringDelimiterIr) -> String {
    match delimiter {
        cobol_ir::StringDelimiterIr::Size => "cobol_vm::VmStringDelimiter::Size".to_string(),
        cobol_ir::StringDelimiterIr::Literal { value, all } => format!(
            "cobol_vm::VmStringDelimiter::Literal {{ value: \"{}\".to_string(), all: {all} }}",
            escape_rust(value),
        ),
    }
}

fn emit_vm_procedure_range(range: &cobol_ir::ProcedureRangeIr) -> String {
    let through = range
        .through
        .as_ref()
        .map(|through| format!("Some(\"{}\".to_string())", escape_rust(through)))
        .unwrap_or_else(|| "None".to_string());
    format!(
        "cobol_vm::VmProcedureRange {{ target: \"{}\".to_string(), through: {through} }}",
        escape_rust(&range.target)
    )
}

fn emit_vm_sort_key_descriptor(sort: &cobol_ir::SortProcedureIr, ir: &ProgramIr) -> String {
    let Some(key) = &sort.key else {
        return "None".to_string();
    };
    let Some(record_name) = sort_file_record_name(ir, &sort.file) else {
        return "None".to_string();
    };
    let Some(record_item) = storage_item_by_name(ir, &record_name) else {
        return "None".to_string();
    };
    let Some(key_item) = storage_item_by_name(ir, &key.name) else {
        return "None".to_string();
    };
    let offset = key_item.offset.saturating_sub(record_item.offset);
    let direction = match key.direction {
        SortDirectionIr::Ascending => "cobol_vm::VmSortDirection::Ascending",
        SortDirectionIr::Descending => "cobol_vm::VmSortDirection::Descending",
    };
    let encoding = match key_item.value_category {
        ValueCategoryIr::NumericDisplay => {
            let picture = key_item
                .picture
                .as_ref()
                .expect("sema requires numeric DISPLAY SORT keys to have picture metadata");
            format!(
                "cobol_vm::VmSortKeyEncoding::NumericDisplay {{ digits: {}, scale: {}, signed: {} }}",
                picture.digits, picture.scale, picture.signed
            )
        }
        ValueCategoryIr::PackedDecimal => {
            let picture = key_item
                .picture
                .as_ref()
                .expect("sema requires packed decimal SORT keys to have picture metadata");
            format!(
                "cobol_vm::VmSortKeyEncoding::PackedDecimal {{ digits: {}, scale: {}, signed: {} }}",
                picture.digits, picture.scale, picture.signed
            )
        }
        _ => "cobol_vm::VmSortKeyEncoding::Bytes".to_string(),
    };
    format!(
        "Some(cobol_vm::VmSortKeyDescriptor {{ offset: {offset}, byte_len: {}, direction: {direction}, encoding: {encoding} }})",
        key_item.byte_len
    )
}

fn sort_file_record_name(ir: &ProgramIr, file: &str) -> Option<String> {
    ir.files
        .iter()
        .find(|candidate| {
            candidate.kind == FileKindIr::Sd && candidate.name.eq_ignore_ascii_case(file)
        })
        .and_then(|file| file.record_name.clone())
}

fn emit_vm_perform_varying(varying: &PerformVaryingIr, ir: &ProgramIr) -> String {
    let target = if is_index_name(&varying.target.normalized, ir) {
        format!(
            "cobol_vm::VmVaryingTarget::Index(\"{}\".to_string())",
            escape_rust(&varying.target.normalized)
        )
    } else {
        format!(
            "cobol_vm::VmVaryingTarget::Access({})",
            emit_vm_access_path_value(&varying.target, ir)
        )
    };
    format!(
        "cobol_vm::VmPerformVarying {{ target: {target}, from: {}, by: {} }}",
        emit_vm_expr_from_operand(&varying.from, ir),
        emit_vm_expr_from_operand(&varying.by, ir)
    )
}

fn emit_vm_arithmetic_op(
    name: &str,
    source: &OperandIr,
    target: &DataRefIr,
    ir: &ProgramIr,
    pad: &str,
) -> String {
    format!(
        "{pad}cobol_vm::VmProcedureOp::{name} {{ source: {}, target: {} }},\n",
        emit_vm_expr_from_operand(source, ir),
        emit_vm_access_path_value(target, ir)
    )
}

fn emit_vm_compute_op(statement: &StatementIr, ir: &ProgramIr, pad: &str) -> String {
    let StatementIr::Compute {
        target,
        expression,
        on_size_error_ops,
        not_on_size_error_ops,
    } = statement
    else {
        unreachable!("emit_vm_compute_op called for non-COMPUTE statement");
    };
    let on_size_error_ops = emit_vm_op_vec(on_size_error_ops, ir, pad.len() + 4);
    let not_on_size_error_ops = emit_vm_op_vec(not_on_size_error_ops, ir, pad.len() + 4);
    format!(
        "{pad}cobol_vm::VmProcedureOp::Compute {{ target: {}, expr: {}, on_size_error_ops: vec![\n{}{pad}], not_on_size_error_ops: vec![\n{}{pad}] }},\n",
        emit_vm_access_path_value(target, ir),
        emit_vm_expr_from_text(expression, ir),
        on_size_error_ops,
        not_on_size_error_ops
    )
}

fn emit_vm_expr_from_operand(operand: &OperandIr, ir: &ProgramIr) -> String {
    match operand {
        OperandIr::Literal(value) => format!(
            "cobol_vm::VmExpr::Literal(\"{}\".to_string())",
            escape_rust(value)
        ),
        OperandIr::Number(value) => format!(
            "cobol_vm::VmExpr::Number(\"{}\".to_string())",
            escape_rust(value)
        ),
        OperandIr::Identifier(reference) => emit_vm_access_path(reference, ir),
        OperandIr::Function(function) => emit_vm_function(function, ir),
    }
}

fn emit_vm_set_index_operation(operation: &SetIndexOperationIr, ir: &ProgramIr) -> String {
    match operation {
        SetIndexOperationIr::To(expr) => format!(
            "cobol_vm::VmSetIndexOperation::To({})",
            emit_vm_subscript_expr(expr, ir)
        ),
        SetIndexOperationIr::UpBy(expr) => format!(
            "cobol_vm::VmSetIndexOperation::UpBy({})",
            emit_vm_subscript_expr(expr, ir)
        ),
        SetIndexOperationIr::DownBy(expr) => format!(
            "cobol_vm::VmSetIndexOperation::DownBy({})",
            emit_vm_subscript_expr(expr, ir)
        ),
    }
}

fn emit_vm_search_op(search: &cobol_ir::SearchIr, ir: &ProgramIr, indent: usize) -> String {
    let pad = " ".repeat(indent);
    let item = search_table_item(search, ir);
    let min = 1usize;
    let max = item
        .and_then(|item| item.occurs.as_ref())
        .map(|occurs| occurs.max.max(occurs.min).max(1))
        .unwrap_or(1);
    let index_name =
        search_index_name(search, ir).unwrap_or_else(|| "UNRESOLVED_INDEX".to_string());
    let mut whens = String::new();
    for when in &search.whens {
        let condition = emit_vm_condition(&when.condition, ir);
        let ops = emit_vm_op_vec(&when.statements, ir, indent + 12);
        whens.push_str(&format!(
            "{pad}        cobol_vm::VmSearchWhen {{ condition: {condition}, ops: vec![\n{ops}{pad}        ] }},\n"
        ));
    }
    let at_end_ops = emit_vm_op_vec(&search.at_end, ir, indent + 8);
    format!(
        "{pad}cobol_vm::VmProcedureOp::SearchSerial {{\n{pad}    table: \"{}\".to_string(),\n{pad}    index_name: \"{}\".to_string(),\n{pad}    min: {min},\n{pad}    max: {max},\n{pad}    whens: vec![\n{whens}{pad}    ],\n{pad}    at_end_ops: vec![\n{at_end_ops}{pad}    ],\n{pad}}},\n",
        escape_rust(&search.table),
        escape_rust(&index_name)
    )
}

fn emit_vm_search_all_op(search: &cobol_ir::SearchAllIr, ir: &ProgramIr, indent: usize) -> String {
    let pad = " ".repeat(indent);
    let item = search_all_table_item(search, ir);
    let min = 1usize;
    let max = item
        .and_then(|item| item.occurs.as_ref())
        .map(|occurs| occurs.max.max(occurs.min).max(1))
        .unwrap_or(1);
    let index_name =
        search_all_index_name(search, ir).unwrap_or_else(|| "UNRESOLVED_INDEX".to_string());
    let Some(declared_key) = search.declared_key.as_ref() else {
        return emit_vm_unsupported_trap(
            &pad,
            &format!(
                "SEARCH ALL for table {} reached code generation without declared key metadata",
                search.table
            ),
        );
    };
    let Some(target_operand) = search_all_target_operand(search) else {
        return emit_vm_unsupported_trap(
            &pad,
            &format!(
                "SEARCH ALL for table {} reached code generation without a lowered key equality",
                search.table
            ),
        );
    };
    let key = emit_search_all_declared_key_expr(declared_key, &index_name, ir);
    let target = emit_vm_expr(target_operand, ir);
    let direction = emit_vm_search_direction(declared_key.direction);
    let found_ops = emit_vm_op_vec(&search.statements, ir, indent + 8);
    let at_end_ops = emit_vm_op_vec(&search.at_end, ir, indent + 8);
    format!(
        "{pad}cobol_vm::VmProcedureOp::SearchAll {{\n{pad}    table: \"{}\".to_string(),\n{pad}    index_name: \"{}\".to_string(),\n{pad}    min: {min},\n{pad}    max: {max},\n{pad}    direction: {direction},\n{pad}    key: {key},\n{pad}    target: {target},\n{pad}    found_ops: vec![\n{found_ops}{pad}    ],\n{pad}    at_end_ops: vec![\n{at_end_ops}{pad}    ],\n{pad}}},\n",
        escape_rust(&search.table),
        escape_rust(&index_name)
    )
}

fn emit_search_all_declared_key_expr(
    declared_key: &cobol_ir::SearchAllKeyIr,
    index_name: &str,
    ir: &ProgramIr,
) -> String {
    let normalized = normalize_vm_ref(&declared_key.qualified_name);
    let reference = DataRefIr {
        raw: declared_key.qualified_name.clone(),
        normalized: normalized.clone(),
        parts: normalized
            .split('.')
            .filter(|part| !part.is_empty())
            .map(ToString::to_string)
            .collect(),
        subscripts: vec![index_name.to_string()],
        reference_modifier: None,
    };
    emit_vm_access_path(&reference, ir)
}

fn search_all_target_operand(search: &cobol_ir::SearchAllIr) -> Option<&ConditionOperandIr> {
    let declared_key = search.declared_key.as_ref()?;
    let ConditionIr::Relation { left, op, right } = &search.key_condition else {
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
    declared_key: &cobol_ir::SearchAllKeyIr,
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

fn emit_vm_search_direction(direction: OccursKeyDirectionIr) -> &'static str {
    match direction {
        OccursKeyDirectionIr::Ascending => "cobol_vm::VmSearchDirection::Ascending",
        OccursKeyDirectionIr::Descending => "cobol_vm::VmSearchDirection::Descending",
    }
}

fn search_table_item<'a>(
    search: &cobol_ir::SearchIr,
    ir: &'a ProgramIr,
) -> Option<&'a cobol_ir::StorageItemIr> {
    let key = normalize_vm_ref(&search.table);
    ir.storage.items.iter().find(|item| {
        normalize_vm_ref(&item.qualified_name) == key || normalize_vm_ref(&item.name) == key
    })
}

fn search_all_table_item<'a>(
    search: &cobol_ir::SearchAllIr,
    ir: &'a ProgramIr,
) -> Option<&'a cobol_ir::StorageItemIr> {
    let key = normalize_vm_ref(&search.table);
    ir.storage.items.iter().find(|item| {
        normalize_vm_ref(&item.qualified_name) == key || normalize_vm_ref(&item.name) == key
    })
}

fn search_index_name(search: &cobol_ir::SearchIr, ir: &ProgramIr) -> Option<String> {
    search.index.clone().or_else(|| {
        search_table_item(search, ir)
            .and_then(|item| item.occurs.as_ref())
            .and_then(|occurs| occurs.indexed_by.first())
            .cloned()
    })
}

fn search_all_index_name(search: &cobol_ir::SearchAllIr, ir: &ProgramIr) -> Option<String> {
    search.index.clone().or_else(|| {
        search_all_table_item(search, ir)
            .and_then(|item| item.occurs.as_ref())
            .and_then(|occurs| occurs.indexed_by.first())
            .cloned()
    })
}

fn emit_vm_subscript_expr(expr: &cobol_ir::SubscriptExprIr, ir: &ProgramIr) -> String {
    match expr {
        cobol_ir::SubscriptExprIr::Literal(value) => format!(
            "cobol_vm::VmExpr::Number(\"{}\".to_string())",
            escape_rust(value)
        ),
        cobol_ir::SubscriptExprIr::DataRef(reference) => {
            if is_index_name(&reference.normalized, ir) {
                format!(
                    "cobol_vm::VmExpr::Index(\"{}\".to_string())",
                    escape_rust(&reference.normalized)
                )
            } else {
                emit_vm_access_path(reference, ir)
            }
        }
        cobol_ir::SubscriptExprIr::Add(left, right) => format!(
            "cobol_vm::VmExpr::Add(Box::new({}), Box::new({}))",
            emit_vm_subscript_expr(left, ir),
            emit_vm_subscript_expr(right, ir)
        ),
        cobol_ir::SubscriptExprIr::Subtract(left, right) => format!(
            "cobol_vm::VmExpr::Subtract(Box::new({}), Box::new({}))",
            emit_vm_subscript_expr(left, ir),
            emit_vm_subscript_expr(right, ir)
        ),
        cobol_ir::SubscriptExprIr::Multiply(left, right) => format!(
            "cobol_vm::VmExpr::Multiply(Box::new({}), Box::new({}))",
            emit_vm_subscript_expr(left, ir),
            emit_vm_subscript_expr(right, ir)
        ),
        cobol_ir::SubscriptExprIr::Divide(left, right) => format!(
            "cobol_vm::VmExpr::Divide(Box::new({}), Box::new({}))",
            emit_vm_subscript_expr(left, ir),
            emit_vm_subscript_expr(right, ir)
        ),
    }
}

fn emit_vm_evaluate(evaluate: &EvaluateIr, ir: &ProgramIr) -> String {
    let subjects = evaluate
        .subjects
        .iter()
        .map(|subject| match subject {
            EvaluateSubjectIr::Operand(operand) => emit_vm_expr(operand, ir),
            EvaluateSubjectIr::Condition(condition) => {
                format!(
                    "cobol_vm::VmExpr::Condition(Box::new({}))",
                    emit_vm_condition(condition, ir)
                )
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let branches = evaluate
        .arms
        .iter()
        .map(|arm| {
            let patterns = arm
                .patterns
                .iter()
                .map(|pattern| emit_vm_pattern(pattern, ir))
                .collect::<Vec<_>>()
                .join(", ");
            format!("cobol_vm::VmBranch {{ patterns: vec![{patterns}] }}")
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("cobol_vm::VmEvaluate {{ subjects: vec![{subjects}], branches: vec![{branches}] }}")
}

fn emit_vm_access_path_value(reference: &DataRefIr, ir: &ProgramIr) -> String {
    let expr = emit_vm_access_path(reference, ir);
    expr.strip_prefix("cobol_vm::VmExpr::Access(")
        .and_then(|value| value.strip_suffix(')'))
        .unwrap_or(&expr)
        .to_string()
}

fn emit_vm_move_corresponding_ops(
    source: &DataRefIr,
    target: &DataRefIr,
    ir: &ProgramIr,
    pad: &str,
) -> String {
    let Some(source_group) = storage_item_for_ref(source, ir) else {
        return emit_vm_unsupported_trap(
            pad,
            &format!(
                "MOVE CORRESPONDING source {} reached code generation unresolved",
                source.raw
            ),
        );
    };
    let Some(target_group) = storage_item_for_ref(target, ir) else {
        return emit_vm_unsupported_trap(
            pad,
            &format!(
                "MOVE CORRESPONDING target {} reached code generation unresolved",
                target.raw
            ),
        );
    };
    let source_items = corresponding_storage_descendants(ir, source_group);
    let target_items = corresponding_storage_descendants(ir, target_group);
    let mut out = String::new();
    for (name, source_item) in source_items {
        let Some(target_item) = target_items.get(&name) else {
            continue;
        };
        let source_ref = DataRefIr::simple(source_item.qualified_name.clone());
        let target_ref = DataRefIr::simple(target_item.qualified_name.clone());
        out.push_str(&format!(
            "{pad}cobol_vm::VmProcedureOp::Move {{ source: {}, target: {} }},\n",
            emit_vm_access_path(&source_ref, ir),
            emit_vm_access_path_value(&target_ref, ir)
        ));
    }
    if out.is_empty() {
        out.push_str(&format!("{pad}cobol_vm::VmProcedureOp::Noop,\n"));
    }
    out
}

fn emit_vm_unsupported_trap(pad: &str, message: &str) -> String {
    format!(
        "{pad}cobol_vm::VmProcedureOp::UnsupportedTrap {{ message: \"{}\".to_string() }},\n",
        escape_rust(message)
    )
}

fn corresponding_storage_descendants<'a>(
    ir: &'a ProgramIr,
    group: &cobol_ir::StorageItemIr,
) -> BTreeMap<String, &'a cobol_ir::StorageItemIr> {
    let prefix = format!("{}.", group.qualified_name.to_ascii_uppercase());
    let mut items = BTreeMap::new();
    for item in &ir.storage.items {
        if item
            .qualified_name
            .to_ascii_uppercase()
            .starts_with(&prefix)
            && item.addressable
            && item.value_category != ValueCategoryIr::Group
        {
            items.entry(item.name.to_ascii_uppercase()).or_insert(item);
        }
    }
    items
}

fn emit_vm_file_open_typed_op(open: &cobol_ir::OpenFileIr, pad: &str) -> String {
    let mode = emit_vm_open_mode(open.mode);
    format!(
        "{pad}cobol_vm::VmProcedureOp::OpenFile {{ name: \"{}\".to_string(), mode: {mode} }},\n",
        escape_rust(&open.file)
    )
}

fn emit_vm_open_mode(mode: cobol_ir::FileOpenModeIr) -> &'static str {
    match mode {
        cobol_ir::FileOpenModeIr::Input => "cobol_vm::VmOpenMode::Input",
        cobol_ir::FileOpenModeIr::Output => "cobol_vm::VmOpenMode::Output",
        cobol_ir::FileOpenModeIr::Io => "cobol_vm::VmOpenMode::Io",
        cobol_ir::FileOpenModeIr::Extend => "cobol_vm::VmOpenMode::Extend",
    }
}

fn emit_vm_file_read_typed_op(read: &cobol_ir::ReadFileIr, ir: &ProgramIr, pad: &str) -> String {
    let name = read.file.clone();
    let record = ir
        .files
        .iter()
        .find(|file| file.name.eq_ignore_ascii_case(&name))
        .and_then(|file| file.record_name.as_ref())
        .cloned()
        .unwrap_or_else(|| name.clone());
    let at_end_ops = emit_vm_op_vec(&read.at_end_ops, ir, pad.len() + 4);
    let not_at_end_ops = emit_file_read_typed_not_at_end_ops(read, &record, ir, pad);
    let on_exception_ops = emit_vm_op_vec(&read.on_exception_ops, ir, pad.len() + 4);
    format!(
        "{pad}cobol_vm::VmProcedureOp::ReadFile {{ name: \"{}\".to_string(), target: {}, at_end_ops: vec![\n{}{pad}], not_at_end_ops: vec![\n{}{pad}], on_exception_ops: vec![\n{}{pad}] }},\n",
        escape_rust(&name),
        emit_vm_access_path_value(&DataRefIr::simple(record), ir),
        at_end_ops,
        not_at_end_ops,
        on_exception_ops
    )
}

fn emit_file_read_typed_not_at_end_ops(
    read: &cobol_ir::ReadFileIr,
    record: &str,
    ir: &ProgramIr,
    pad: &str,
) -> String {
    let inner_pad = format!("{pad}    ");
    let mut ops = String::new();
    if let Some(target) = &read.into {
        if let Some(resize) = read_into_odo_resize(ir, record, &target.normalized) {
            ops.push_str(&format!(
                "{inner_pad}cobol_vm::VmProcedureOp::Move {{ source: cobol_vm::VmExpr::Number(\"{}\".to_string()), target: {} }},\n",
                resize.active,
                emit_vm_access_path_value(&DataRefIr::simple(resize.depending_on), ir)
            ));
        }
        ops.push_str(&format!(
            "{inner_pad}cobol_vm::VmProcedureOp::Move {{ source: {}, target: {} }},\n",
            emit_vm_access_path(&DataRefIr::simple(record.to_string()), ir),
            emit_vm_access_path_value(target, ir)
        ));
    }
    ops.push_str(&emit_vm_op_vec(&read.not_at_end_ops, ir, pad.len() + 4));
    ops
}

struct ReadIntoOdoResize {
    depending_on: String,
    active: usize,
}

fn read_into_odo_resize(ir: &ProgramIr, record: &str, target: &str) -> Option<ReadIntoOdoResize> {
    let record_item = storage_item_by_name(ir, record)?;
    let target_item = storage_item_by_name(ir, target)?;
    let odo_item = dynamic_odo_items_in_subtree(ir, target_item)
        .into_iter()
        .next()?;
    let occurs = odo_item.occurs.as_ref()?;
    let depending_on = occurs.depending_on.clone()?;
    let element_len = occurs_stride(odo_item);
    if element_len == 0 || target_item.byte_len < odo_item.byte_len {
        return None;
    }
    let fixed_len = target_item.byte_len - odo_item.byte_len;
    let variable_len = record_item.byte_len.checked_sub(fixed_len)?;
    if variable_len % element_len != 0 {
        return None;
    }
    let active = variable_len / element_len;
    if active < occurs.min || active > occurs.max {
        return None;
    }
    Some(ReadIntoOdoResize {
        depending_on,
        active,
    })
}

fn dynamic_odo_items_in_subtree<'a>(
    ir: &'a ProgramIr,
    target: &cobol_ir::StorageItemIr,
) -> Vec<&'a cobol_ir::StorageItemIr> {
    let prefix = format!("{}.", target.qualified_name);
    ir.storage
        .items
        .iter()
        .filter(|item| {
            (item.qualified_name == target.qualified_name
                || item.qualified_name.starts_with(&prefix))
                && item
                    .occurs
                    .as_ref()
                    .and_then(|occurs| occurs.depending_on.as_ref())
                    .is_some()
        })
        .collect()
}

fn emit_vm_file_write_typed_op(write: &cobol_ir::WriteFileIr, ir: &ProgramIr, pad: &str) -> String {
    let record = write.record.normalized.clone();
    let name = file_name_for_record(ir, &record);
    let advancing = emit_vm_write_advancing_ir(&write.advancing);
    format!(
        "{pad}cobol_vm::VmProcedureOp::WriteFile {{ name: \"{}\".to_string(), source: {}, advancing: {advancing} }},\n",
        escape_rust(&name),
        emit_vm_access_path_value(&write.record, ir)
    )
}

fn emit_vm_write_advancing_ir(advancing: &cobol_ir::WriteAdvancingIr) -> String {
    match advancing {
        cobol_ir::WriteAdvancingIr::None => "cobol_vm::VmWriteAdvancing::None".to_string(),
        cobol_ir::WriteAdvancingIr::BeforeLines(lines) => {
            format!("cobol_vm::VmWriteAdvancing::BeforeLines({lines})")
        }
        cobol_ir::WriteAdvancingIr::AfterLines(lines) => {
            format!("cobol_vm::VmWriteAdvancing::AfterLines({lines})")
        }
        cobol_ir::WriteAdvancingIr::BeforePage => {
            "cobol_vm::VmWriteAdvancing::BeforePage".to_string()
        }
        cobol_ir::WriteAdvancingIr::AfterPage => {
            "cobol_vm::VmWriteAdvancing::AfterPage".to_string()
        }
    }
}

fn emit_vm_file_rewrite_typed_op(
    rewrite: &cobol_ir::RewriteFileIr,
    ir: &ProgramIr,
    pad: &str,
) -> String {
    let record = rewrite.record.normalized.clone();
    let name = file_name_for_record(ir, &record);
    let invalid_key_ops = emit_vm_op_vec(&rewrite.invalid_key_ops, ir, pad.len() + 4);
    let not_invalid_key_ops = emit_vm_op_vec(&rewrite.not_invalid_key_ops, ir, pad.len() + 4);
    format!(
        "{pad}cobol_vm::VmProcedureOp::RewriteFile {{ name: \"{}\".to_string(), source: {}, invalid_key_ops: vec![\n{}{pad}], not_invalid_key_ops: vec![\n{}{pad}] }},\n",
        escape_rust(&name),
        emit_vm_access_path_value(&rewrite.record, ir),
        invalid_key_ops,
        not_invalid_key_ops
    )
}

fn emit_vm_file_delete_typed_op(
    delete: &cobol_ir::DeleteFileIr,
    ir: &ProgramIr,
    pad: &str,
) -> String {
    let invalid_key_ops = emit_vm_op_vec(&delete.invalid_key_ops, ir, pad.len() + 4);
    let not_invalid_key_ops = emit_vm_op_vec(&delete.not_invalid_key_ops, ir, pad.len() + 4);
    format!(
        "{pad}cobol_vm::VmProcedureOp::DeleteFile {{ name: \"{}\".to_string(), invalid_key_ops: vec![\n{}{pad}], not_invalid_key_ops: vec![\n{}{pad}] }},\n",
        escape_rust(&delete.file),
        invalid_key_ops,
        not_invalid_key_ops
    )
}

fn file_name_for_record(ir: &ProgramIr, record: &str) -> String {
    ir.files
        .iter()
        .find(|file| {
            file.record_name
                .as_ref()
                .map(|name| name.eq_ignore_ascii_case(record))
                .unwrap_or(false)
        })
        .map(|file| file.name.clone())
        .unwrap_or_else(|| record.to_string())
}

fn emit_vm_file_close_typed_op(close: &cobol_ir::CloseFileIr, pad: &str) -> String {
    format!(
        "{pad}cobol_vm::VmProcedureOp::CloseFile {{ name: \"{}\".to_string() }},\n",
        escape_rust(&close.file)
    )
}

fn emit_vm_field(item: &cobol_ir::StorageItemIr, name: &str) -> String {
    let picture = item
        .picture
        .as_ref()
        .map(|picture| {
            format!(
                "Some(cobol_vm::VmPicture {{ signed: {}, digits: {}, scale: {}, char_len: {} }})",
                picture.signed, picture.digits, picture.scale, picture.char_len
            )
        })
        .unwrap_or_else(|| "None".to_string());
    format!(
        "                cobol_vm::VmField {{ name: \"{}\".to_string(), offset: {}, byte_len: {}, category: {}, usage: {}, picture: {} }},\n",
        escape_rust(name),
        item.offset,
        item.byte_len,
        vm_category(item.value_category),
        vm_usage(&item.usage),
        picture
    )
}

fn vm_category(category: ValueCategoryIr) -> &'static str {
    match category {
        ValueCategoryIr::Group => "cobol_vm::VmCategory::Group",
        ValueCategoryIr::Alphanumeric => "cobol_vm::VmCategory::Alphanumeric",
        ValueCategoryIr::Alphabetic => "cobol_vm::VmCategory::Alphabetic",
        ValueCategoryIr::National => "cobol_vm::VmCategory::National",
        ValueCategoryIr::Dbcs => "cobol_vm::VmCategory::Dbcs",
        ValueCategoryIr::NumericDisplay => "cobol_vm::VmCategory::NumericDisplay",
        ValueCategoryIr::NumericEdited => "cobol_vm::VmCategory::NumericEdited",
        ValueCategoryIr::PackedDecimal => "cobol_vm::VmCategory::PackedDecimal",
        ValueCategoryIr::Binary => "cobol_vm::VmCategory::Binary",
        ValueCategoryIr::NativeBinary => "cobol_vm::VmCategory::NativeBinary",
        ValueCategoryIr::Float => "cobol_vm::VmCategory::Float",
        ValueCategoryIr::ConditionName | ValueCategoryIr::Unsupported => {
            "cobol_vm::VmCategory::Unsupported"
        }
    }
}

fn vm_usage(usage: &UsageIr) -> &'static str {
    match usage {
        UsageIr::Display => "cobol_vm::VmUsage::Display",
        UsageIr::PackedDecimal => "cobol_vm::VmUsage::PackedDecimal",
        UsageIr::Binary => "cobol_vm::VmUsage::Binary",
        UsageIr::NativeBinary => "cobol_vm::VmUsage::NativeBinary",
        UsageIr::Float32 => "cobol_vm::VmUsage::Float32",
        UsageIr::Float64 => "cobol_vm::VmUsage::Float64",
        UsageIr::National => "cobol_vm::VmUsage::National",
        UsageIr::Dbcs => "cobol_vm::VmUsage::Dbcs",
        UsageIr::Alphanumeric => "cobol_vm::VmUsage::Alphanumeric",
        UsageIr::Group => "cobol_vm::VmUsage::Group",
        UsageIr::Unknown(_) => "cobol_vm::VmUsage::Unknown",
    }
}

#[allow(dead_code)]
fn emit_statement(statement: &StatementIr, ir: &ProgramIr) -> String {
    match statement {
        StatementIr::Display(values) => {
            let parts = values
                .iter()
                .map(emit_operand_display)
                .collect::<Vec<_>>()
                .join(", ");
            format!("        self.runtime.display_line(vec![{parts}].join(\"\"));\n")
        }
        StatementIr::Move { source, target } => format!(
            "        self.storage.move_value({}, \"{}\")?;\n",
            emit_operand_value(source),
            escape_rust(&target.normalized)
        ),
        StatementIr::Add { source, target } => format!(
            "        self.storage.add({}, \"{}\")?;\n",
            emit_operand_value(source),
            escape_rust(&target.normalized)
        ),
        StatementIr::Subtract { source, target } => format!(
            "        self.storage.subtract({}, \"{}\")?;\n",
            emit_operand_value(source),
            escape_rust(&target.normalized)
        ),
        StatementIr::Multiply { source, target } => format!(
            "        self.storage.multiply({}, \"{}\")?;\n",
            emit_operand_value(source),
            escape_rust(&target.normalized)
        ),
        StatementIr::Divide { source, target } => format!(
            "        self.storage.divide({}, \"{}\")?;\n",
            emit_operand_value(source),
            escape_rust(&target.normalized)
        ),
        StatementIr::Perform {
            target,
            through,
            times,
            ..
        } => {
            let variant = paragraph_index(ir, target)
                .and_then(|idx| ir.paragraphs.get(idx))
                .map(|paragraph| enum_variant(&paragraph.name))
                .unwrap_or_else(|| enum_variant(target));
            let end = through
                .as_ref()
                .and_then(|target| paragraph_index(ir, target))
                .and_then(|idx| ir.paragraphs.get(idx))
                .map(|paragraph| format!("Some(ParagraphId::{})", enum_variant(&paragraph.name)))
                .unwrap_or_else(|| "None".to_string());
            let times = times
                .as_ref()
                .map(|operand| format!("cobol_value_to_usize({})?", emit_operand_value(operand)))
                .unwrap_or_else(|| "1".to_string());
            format!(
                "        if let Some(flow) = self.perform_range(ParagraphId::{variant}, {end}, {times})? {{\n            return Ok(flow);\n        }}\n"
            )
        }
        StatementIr::GoTo(target) => {
            let idx = paragraph_index(ir, target).unwrap_or(usize::MAX);
            format!("        return Ok(ControlFlow::GoTo({idx}));\n")
        }
        StatementIr::StopRun => "        return Ok(ControlFlow::StopRun);\n".to_string(),
        StatementIr::Continue => String::new(),
        StatementIr::OpenFile(open) => format!(
            "        cobol_runtime::CobolFileSystem::open(&mut self.files, \"OPEN {} {}\")?;\n",
            emit_direct_open_mode(open.mode),
            escape_rust(&open.file)
        ),
        StatementIr::ReadFile(read) => format!(
            "        cobol_runtime::CobolFileSystem::read(&mut self.files, \"READ {}\")?;\n",
            escape_rust(&read.file)
        ),
        StatementIr::WriteFile(write) => format!(
            "        cobol_runtime::CobolFileSystem::write(&mut self.files, \"WRITE {}\")?;\n",
            escape_rust(&write.record.raw)
        ),
        StatementIr::RewriteFile(rewrite) => format!(
            "        cobol_runtime::CobolFileSystem::write(&mut self.files, \"REWRITE {}\")?;\n",
            escape_rust(&rewrite.record.raw)
        ),
        StatementIr::DeleteFile(delete) => format!(
            "        cobol_runtime::CobolFileSystem::write(&mut self.files, \"DELETE {}\")?;\n",
            escape_rust(&delete.file)
        ),
        StatementIr::CloseFile(close) => format!(
            "        cobol_runtime::CobolFileSystem::close(&mut self.files, \"CLOSE {}\")?;\n",
            escape_rust(&close.file)
        ),
        StatementIr::If {
            condition,
            condition_tree,
            then_statements,
            else_statements,
        } => emit_if_statement(
            condition,
            condition_tree.as_ref(),
            then_statements,
            else_statements,
            ir,
        ),
        StatementIr::Evaluate(evaluate) => emit_evaluate_statement(evaluate, ir),
        StatementIr::Search(_) | StatementIr::SearchAll(_) => {
            "        return Ok(ControlFlow::StopRun);\n".to_string()
        }
        StatementIr::SetCondition { condition, value } => {
            if *value {
                format!(
                    "        self.set_condition_name(\"{}\")?;\n",
                    escape_rust(&condition.normalized)
                )
            } else {
                "        return Ok(ControlFlow::StopRun);\n".to_string()
            }
        }
        StatementIr::SetIndex { .. } => "        return Ok(ControlFlow::StopRun);\n".to_string(),
        StatementIr::Call(_)
        | StatementIr::NextSentence
        | StatementIr::ReadyTrace
        | StatementIr::ResetTrace
        | StatementIr::MoveCorresponding { .. }
        | StatementIr::ComputedGoTo { .. }
        | StatementIr::Alter { .. }
        | StatementIr::SortProcedure(_)
        | StatementIr::ReleaseSortRecord(_)
        | StatementIr::ReturnSortRecord(_)
        | StatementIr::InspectLike(_)
        | StatementIr::StringOp(_)
        | StatementIr::UnstringOp(_)
        | StatementIr::Compute { .. }
        | StatementIr::Unsupported { .. } => {
            "        return Ok(ControlFlow::StopRun);\n".to_string()
        }
    }
}

fn emit_direct_open_mode(mode: cobol_ir::FileOpenModeIr) -> &'static str {
    match mode {
        cobol_ir::FileOpenModeIr::Input => "INPUT",
        cobol_ir::FileOpenModeIr::Output => "OUTPUT",
        cobol_ir::FileOpenModeIr::Io => "I-O",
        cobol_ir::FileOpenModeIr::Extend => "EXTEND",
    }
}

#[allow(dead_code)]
fn emit_if_statement(
    condition: &str,
    condition_tree: Option<&ConditionIr>,
    then_statements: &[StatementIr],
    else_statements: &[StatementIr],
    ir: &ProgramIr,
) -> String {
    let parsed = condition_tree
        .map(|condition| emit_vm_condition(condition, ir))
        .unwrap_or_else(|| {
            format!(
                "cobol_vm::VmCondition::Relation {{ left: {}, op: cobol_vm::VmRelOp::Equal, right: cobol_vm::VmExpr::Bool(false) }}",
                emit_vm_expr(&ConditionOperandIr::Bool(true), ir)
            )
        });
    let _ = condition;
    let mut text = format!("        if self.eval_condition({parsed})? {{\n");
    for statement in then_statements {
        text.push_str(&emit_statement(statement, ir));
    }
    if else_statements.is_empty() {
        text.push_str("        }\n");
    } else {
        text.push_str("        } else {\n");
        for statement in else_statements {
            text.push_str(&emit_statement(statement, ir));
        }
        text.push_str("        }\n");
    }
    text
}

#[allow(dead_code)]
fn emit_evaluate_statement(evaluate: &EvaluateIr, ir: &ProgramIr) -> String {
    let mut text = String::from("        {\n");
    text.push_str("            let __vm = self.vm_program();\n");
    text.push_str("            let __subjects = vec![\n");
    for subject in &evaluate.subjects {
        text.push_str(&format!(
            "                {},\n",
            emit_evaluate_subject_eval(subject, ir)
        ));
    }
    text.push_str("            ];\n");
    for (idx, arm) in evaluate.arms.iter().enumerate() {
        if idx == 0 {
            text.push_str("            if ");
        } else {
            text.push_str("            else if ");
        }
        text.push_str(&emit_evaluate_arm_match(idx, arm, ir));
        text.push_str(" {\n");
        for statement in &arm.statements {
            text.push_str(&emit_statement(statement, ir));
        }
        text.push_str("            }\n");
    }
    text.push_str("        }\n");
    text
}

#[allow(dead_code)]
fn emit_evaluate_subject_eval(subject: &EvaluateSubjectIr, ir: &ProgramIr) -> String {
    match subject {
        EvaluateSubjectIr::Operand(operand) => format!(
            "__vm.eval_operand(&[], &{}).map_err(|err| RuntimeError::Codec {{ message: err.to_string() }})?",
            emit_vm_expr(operand, ir)
        ),
        EvaluateSubjectIr::Condition(condition) => format!(
            "cobol_vm::VmEvaluatedValue {{ value: cobol_vm::VmValue::Bool(__vm.eval_condition(&[], &{}).map_err(|err| RuntimeError::Codec {{ message: err.to_string() }})?), category: cobol_vm::VmCategory::Unsupported, byte_len: 1 }}",
            emit_vm_condition(condition, ir)
        ),
    }
}

#[allow(dead_code)]
fn emit_evaluate_arm_match(_idx: usize, arm: &cobol_ir::EvaluateArmIr, ir: &ProgramIr) -> String {
    if arm.patterns.is_empty() {
        return "false".to_string();
    }
    arm.patterns
        .iter()
        .enumerate()
        .map(|(pattern_idx, pattern)| {
            format!(
                "__vm.match_evaluate_pattern(&[], &__subjects[{pattern_idx}], &{}).map_err(|err| RuntimeError::Codec {{ message: err.to_string() }})?",
                emit_vm_pattern(pattern, ir)
            )
        })
        .collect::<Vec<_>>()
        .join(" && ")
}

fn emit_vm_condition(condition: &ConditionIr, ir: &ProgramIr) -> String {
    match condition {
        ConditionIr::Relation { left, op, right } => format!(
            "cobol_vm::VmCondition::Relation {{ left: {}, op: {}, right: {} }}",
            emit_vm_expr(left, ir),
            emit_vm_rel_op(*op),
            emit_vm_expr(right, ir)
        ),
        ConditionIr::ClassTest {
            operand,
            class,
            negated,
        } => format!(
            "cobol_vm::VmCondition::ClassTest {{ operand: {}, class: {}, negated: {} }}",
            emit_vm_expr(operand, ir),
            emit_vm_class_test(*class),
            negated
        ),
        ConditionIr::SignTest {
            operand,
            sign,
            negated,
        } => format!(
            "cobol_vm::VmCondition::SignTest {{ operand: {}, sign: {}, negated: {} }}",
            emit_vm_expr(operand, ir),
            emit_vm_sign_test(*sign),
            negated
        ),
        ConditionIr::ConditionName { reference } => format!(
            "cobol_vm::VmCondition::ConditionName {{ reference: \"{}\".to_string() }}",
            escape_rust(&reference.normalized)
        ),
        ConditionIr::Not(inner) => {
            format!(
                "cobol_vm::VmCondition::Not(Box::new({}))",
                emit_vm_condition(inner, ir)
            )
        }
        ConditionIr::And(left, right) => format!(
            "cobol_vm::VmCondition::And(Box::new({}), Box::new({}))",
            emit_vm_condition(left, ir),
            emit_vm_condition(right, ir)
        ),
        ConditionIr::Or(left, right) => format!(
            "cobol_vm::VmCondition::Or(Box::new({}), Box::new({}))",
            emit_vm_condition(left, ir),
            emit_vm_condition(right, ir)
        ),
    }
}

fn emit_vm_expr(operand: &ConditionOperandIr, ir: &ProgramIr) -> String {
    match operand {
        ConditionOperandIr::Identifier(reference) => emit_vm_access_path(reference, ir),
        ConditionOperandIr::Literal(value) => format!(
            "cobol_vm::VmExpr::Literal(\"{}\".to_string())",
            escape_rust(value)
        ),
        ConditionOperandIr::Number(value) => format!(
            "cobol_vm::VmExpr::Number(\"{}\".to_string())",
            escape_rust(value)
        ),
        ConditionOperandIr::Figurative(value) => {
            format!(
                "cobol_vm::VmExpr::Figurative({})",
                emit_vm_figurative(*value)
            )
        }
        ConditionOperandIr::AllLiteral(value) => format!(
            "cobol_vm::VmExpr::AllLiteral(\"{}\".to_string())",
            escape_rust(value)
        ),
        ConditionOperandIr::Function(function) => emit_vm_function(function, ir),
        ConditionOperandIr::Bool(value) => format!("cobol_vm::VmExpr::Bool({value})"),
    }
}

fn emit_vm_access_path(reference: &DataRefIr, ir: &ProgramIr) -> String {
    let item = storage_item_for_ref(reference, ir);
    let subscripts = item
        .map(|item| emit_vm_subscripts(reference, item, ir))
        .unwrap_or_default();
    let result_len = item
        .and_then(|item| vm_result_len(reference, item, ir))
        .map(|len| format!("Some({len})"))
        .unwrap_or_else(|| "None".to_string());
    let reference_modifier = reference
        .reference_modifier
        .as_ref()
        .map(|modifier| emit_vm_reference_modifier(modifier, ir))
        .unwrap_or_else(|| "None".to_string());
    format!(
        "cobol_vm::VmExpr::Access(cobol_vm::VmAccessPath {{ target: \"{}\".to_string(), condition_name: None, subscripts: vec![{}], reference_modifier: {}, result_len: {} }})",
        escape_rust(&reference.normalized),
        subscripts,
        reference_modifier,
        result_len
    )
}

fn emit_vm_reference_modifier(modifier: &ReferenceModifierIr, ir: &ProgramIr) -> String {
    let start = emit_vm_expr_from_text(&modifier.start, ir);
    let length = modifier
        .length
        .as_ref()
        .map(|length| format!("Some(Box::new({}))", emit_vm_expr_from_text(length, ir)))
        .unwrap_or_else(|| "None".to_string());
    format!(
        "Some(cobol_vm::VmReferenceModifier {{ start: Box::new({}), length: {} }})",
        start, length
    )
}

fn emit_vm_function(function: &FunctionOperandIr, ir: &ProgramIr) -> String {
    match function {
        FunctionOperandIr::Length(arg) => format!(
            "cobol_vm::VmExpr::Function {{ function: cobol_vm::VmFunction::Length, args: vec![{}] }}",
            emit_vm_expr(arg, ir)
        ),
        FunctionOperandIr::Ord(arg) => format!(
            "cobol_vm::VmExpr::Function {{ function: cobol_vm::VmFunction::Ord, args: vec![{}] }}",
            emit_vm_expr(arg, ir)
        ),
        FunctionOperandIr::Numval(arg) => format!(
            "cobol_vm::VmExpr::Function {{ function: cobol_vm::VmFunction::Numval, args: vec![{}] }}",
            emit_vm_expr(arg, ir)
        ),
        FunctionOperandIr::UserDefined { args, .. } => {
            let args = args
                .iter()
                .map(|arg| emit_vm_expr(arg, ir))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "cobol_vm::VmExpr::Function {{ function: cobol_vm::VmFunction::UserDefined, args: vec![{}] }}",
                args
            )
        }
    }
}

fn emit_vm_expr_from_text(text: &str, ir: &ProgramIr) -> String {
    let clean = text.trim();
    let literal = clean.trim_end_matches('.');
    if (literal.starts_with('"') && literal.ends_with('"'))
        || (literal.starts_with('\'') && literal.ends_with('\''))
    {
        return format!(
            "cobol_vm::VmExpr::Literal(\"{}\".to_string())",
            escape_rust(literal.trim_matches('"').trim_matches('\''))
        );
    }
    if let Some((left, op, right)) = split_vm_expr_binary(clean, &['+', '-']) {
        let variant = if op == '+' { "Add" } else { "Subtract" };
        return format!(
            "cobol_vm::VmExpr::{variant}(Box::new({}), Box::new({}))",
            emit_vm_expr_from_text(left, ir),
            emit_vm_expr_from_text(right, ir)
        );
    }
    if let Some((left, op, right)) = split_vm_expr_binary(clean, &['*', '/']) {
        let variant = if op == '*' { "Multiply" } else { "Divide" };
        return format!(
            "cobol_vm::VmExpr::{variant}(Box::new({}), Box::new({}))",
            emit_vm_expr_from_text(left, ir),
            emit_vm_expr_from_text(right, ir)
        );
    }
    if is_vm_numeric_literal(clean) {
        return format!(
            "cobol_vm::VmExpr::Number(\"{}\".to_string())",
            escape_rust(clean)
        );
    }
    let reference = parse_sema_data_ref(clean);
    if reference.subscripts.is_empty()
        && reference.reference_modifier.is_none()
        && is_index_name(&reference.normalized, ir)
    {
        return format!(
            "cobol_vm::VmExpr::Index(\"{}\".to_string())",
            escape_rust(&reference.normalized)
        );
    }
    emit_vm_access_path(&reference, ir)
}

fn is_vm_numeric_literal(value: &str) -> bool {
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

fn split_vm_expr_binary<'a>(value: &'a str, ops: &[char]) -> Option<(&'a str, char, &'a str)> {
    let value = strip_outer_vm_parens(value).trim();
    let mut depth = 0usize;
    for (idx, ch) in value.char_indices().rev() {
        match ch {
            ')' => depth = depth.saturating_add(1),
            '(' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth != 0 || !ops.contains(&ch) || idx == 0 {
            continue;
        }
        if ch == '-' && !vm_operator_has_space_around(value, idx) {
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

fn vm_operator_has_space_around(value: &str, idx: usize) -> bool {
    let before = value[..idx].chars().next_back();
    let after = value[idx + 1..].chars().next();
    before.map(char::is_whitespace).unwrap_or(false)
        || after.map(char::is_whitespace).unwrap_or(false)
}

fn strip_outer_vm_parens(value: &str) -> &str {
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

fn emit_vm_subscripts(
    reference: &DataRefIr,
    item: &cobol_ir::StorageItemIr,
    ir: &ProgramIr,
) -> String {
    let chain = occurs_chain_for_item(item, ir);
    reference
        .subscripts
        .iter()
        .zip(chain)
        .map(|(expr, occurs_item)| {
            let max = occurs_item.occurs.as_ref().map(|occurs| occurs.max).unwrap_or(1);
            let min = 1usize;
            let stride = occurs_stride(occurs_item);
            let normalized_expr = normalize_vm_ref(expr);
            let index_name = if is_index_name(&normalized_expr, ir) {
                format!("Some(\"{}\".to_string())", escape_rust(&normalized_expr))
            } else {
                "None".to_string()
            };
            let expr = if is_index_name(&normalized_expr, ir) {
                "cobol_vm::VmExpr::Number(\"1\".to_string())".to_string()
            } else {
                emit_vm_expr_from_text(expr, ir)
            };
            let depending_on = occurs_item
                .occurs
                .as_ref()
                .and_then(|occurs| occurs.depending_on.as_ref())
                .map(|name| format!("Some(\"{}\".to_string())", escape_rust(name)))
                .unwrap_or_else(|| "None".to_string());
            format!(
                "cobol_vm::VmSubscript {{ expr: Box::new({}), stride: {}, min: {}, max: {}, depending_on: {}, index_name: {} }}",
                expr,
                stride,
                min,
                max,
                depending_on,
                index_name
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn storage_item_for_ref<'a>(
    reference: &DataRefIr,
    ir: &'a ProgramIr,
) -> Option<&'a cobol_ir::StorageItemIr> {
    ir.storage.items.iter().find(|item| {
        normalize_vm_ref(&item.qualified_name).eq_ignore_ascii_case(&reference.normalized)
            || normalize_vm_ref(&item.name).eq_ignore_ascii_case(&reference.normalized)
    })
}

fn occurs_chain_for_item<'a>(
    item: &'a cobol_ir::StorageItemIr,
    ir: &'a ProgramIr,
) -> Vec<&'a cobol_ir::StorageItemIr> {
    let mut chain = Vec::new();
    let mut ancestors = Vec::new();
    let mut parent = item.parent.as_deref();
    while let Some(parent_name) = parent {
        if let Some(parent_item) = ir
            .storage
            .items
            .iter()
            .find(|candidate| candidate.qualified_name == parent_name)
        {
            ancestors.push(parent_item);
            parent = parent_item.parent.as_deref();
        } else {
            break;
        }
    }
    ancestors.reverse();
    for ancestor in ancestors {
        if ancestor.occurs.is_some() {
            chain.push(ancestor);
        }
    }
    if item.occurs.is_some() {
        chain.push(item);
    }
    chain
}

fn occurs_stride(item: &cobol_ir::StorageItemIr) -> usize {
    item.occurs
        .as_ref()
        .map(|occurs| item.byte_len / occurs.max.max(1))
        .unwrap_or(item.byte_len)
}

fn vm_result_len(
    reference: &DataRefIr,
    item: &cobol_ir::StorageItemIr,
    ir: &ProgramIr,
) -> Option<usize> {
    if reference.subscripts.is_empty() {
        return None;
    }
    if item.occurs.is_some() {
        return Some(occurs_stride(item));
    }
    let _ = ir;
    Some(item.byte_len)
}

fn normalize_vm_ref(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .replace('-', "_")
        .to_ascii_uppercase()
}

fn is_index_name(value: &str, ir: &ProgramIr) -> bool {
    let key = normalize_vm_ref(value);
    ir.indexes
        .iter()
        .any(|index| normalize_vm_ref(&index.name) == key)
}

fn emit_vm_pattern(pattern: &EvaluatePatternIr, ir: &ProgramIr) -> String {
    match pattern {
        EvaluatePatternIr::Any => "cobol_vm::VmEvaluatePattern::Any".to_string(),
        EvaluatePatternIr::Operand(operand) => {
            format!(
                "cobol_vm::VmEvaluatePattern::Operand({})",
                emit_vm_expr(operand, ir)
            )
        }
        EvaluatePatternIr::Range { start, end } => format!(
            "cobol_vm::VmEvaluatePattern::Range {{ start: {}, end: {} }}",
            emit_vm_expr(start, ir),
            emit_vm_expr(end, ir)
        ),
        EvaluatePatternIr::Condition(condition) => format!(
            "cobol_vm::VmEvaluatePattern::Condition({})",
            emit_vm_condition(condition, ir)
        ),
    }
}

fn emit_vm_rel_op(op: RelOpIr) -> &'static str {
    match op {
        RelOpIr::Equal => "cobol_vm::VmRelOp::Equal",
        RelOpIr::NotEqual => "cobol_vm::VmRelOp::NotEqual",
        RelOpIr::Greater => "cobol_vm::VmRelOp::Greater",
        RelOpIr::GreaterOrEqual => "cobol_vm::VmRelOp::GreaterOrEqual",
        RelOpIr::Less => "cobol_vm::VmRelOp::Less",
        RelOpIr::LessOrEqual => "cobol_vm::VmRelOp::LessOrEqual",
    }
}

fn emit_vm_class_test(class: ClassTestIr) -> &'static str {
    match class {
        ClassTestIr::Numeric => "cobol_vm::VmClassTest::Numeric",
        ClassTestIr::Alphabetic => "cobol_vm::VmClassTest::Alphabetic",
        ClassTestIr::AlphabeticUpper => "cobol_vm::VmClassTest::AlphabeticUpper",
        ClassTestIr::AlphabeticLower => "cobol_vm::VmClassTest::AlphabeticLower",
    }
}

fn emit_vm_sign_test(sign: SignTestIr) -> &'static str {
    match sign {
        SignTestIr::Positive => "cobol_vm::VmSignTest::Positive",
        SignTestIr::Negative => "cobol_vm::VmSignTest::Negative",
        SignTestIr::Zero => "cobol_vm::VmSignTest::Zero",
    }
}

fn emit_vm_figurative(value: FigurativeConstantIr) -> &'static str {
    match value {
        FigurativeConstantIr::Zero => "cobol_vm::VmFigurative::Zero",
        FigurativeConstantIr::Space => "cobol_vm::VmFigurative::Space",
        FigurativeConstantIr::HighValue => "cobol_vm::VmFigurative::HighValue",
        FigurativeConstantIr::LowValue => "cobol_vm::VmFigurative::LowValue",
        FigurativeConstantIr::Quote => "cobol_vm::VmFigurative::Quote",
    }
}

#[allow(dead_code)]
fn emit_operand_display(operand: &OperandIr) -> String {
    match operand {
        OperandIr::Literal(value) => format!("\"{}\".to_string()", escape_rust(value)),
        OperandIr::Number(value) => format!("\"{}\".to_string()", escape_rust(value)),
        OperandIr::Identifier(reference) => {
            format!(
                "self.display_ref(\"{}\")?",
                escape_rust(&reference.normalized)
            )
        }
        OperandIr::Function(_) => {
            "unimplemented!(\"FUNCTION operand reached legacy display emitter\")".to_string()
        }
    }
}

#[allow(dead_code)]
fn emit_operand_value(operand: &OperandIr) -> String {
    match operand {
        OperandIr::Literal(value) => {
            format!(
                "cobol_runtime::CobolValue::Text(\"{}\".to_string())",
                escape_rust(value)
            )
        }
        OperandIr::Number(value) => {
            format!(
                "cobol_runtime::CobolValue::Text(\"{}\".to_string())",
                escape_rust(value)
            )
        }
        OperandIr::Identifier(reference) => {
            format!(
                "self.storage.get(\"{}\")?",
                escape_rust(&reference.normalized)
            )
        }
        OperandIr::Function(_) => {
            "unimplemented!(\"FUNCTION operand reached legacy value emitter\")".to_string()
        }
    }
}

#[allow(dead_code)]
fn statement_is_terminal(statement: &StatementIr) -> bool {
    matches!(statement, StatementIr::GoTo(_) | StatementIr::StopRun)
}

#[allow(dead_code)]
fn paragraph_index(ir: &ProgramIr, target: &str) -> Option<usize> {
    ir.paragraphs
        .iter()
        .position(|paragraph| paragraph.name.eq_ignore_ascii_case(target))
}

fn build_report(
    status: &str,
    options: &ConvertOptions,
    preprocessed: &PreprocessedSource,
    ir: &ProgramIr,
    generated: &[PathBuf],
) -> MigrationReport {
    MigrationReport {
        version: 1,
        status: status.to_string(),
        source: options.input.to_string_lossy().to_string(),
        dialect: format!("{:?}", options.dialect).to_ascii_lowercase(),
        source_format: format!("{:?}", preprocessed.format).to_ascii_lowercase(),
        includes: preprocessed
            .includes
            .iter()
            .map(|include| include.resolved_path.to_string_lossy().to_string())
            .collect(),
        generated_files: generated
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect(),
        diagnostics: ir.diagnostics.clone(),
        diagnostic_sections: diagnostic_sections(&ir.diagnostics),
        dialect_profile: ir.dialect_profile.clone(),
        storage: ir.storage.clone(),
        semantic: ir.semantic.clone(),
        control_flow: ir.control_flow.clone(),
        procedure_cfg: ir.procedure_cfg.clone(),
        files: ir.files.clone(),
        indexes: ir.indexes.clone(),
        odo: ir.odo_descriptors.clone(),
        program_units: ir.program_units.clone(),
        stats: ReportStats {
            data_items: ir.data_items.len(),
            storage_items: ir.storage.items.len(),
            storage_bytes: ir.storage.record_length,
            paragraphs: ir.paragraphs.len(),
            statements: ir
                .paragraphs
                .iter()
                .map(|paragraph| paragraph.statements.len())
                .sum(),
            cfg_edges: ir
                .control_flow
                .paragraphs
                .iter()
                .map(|paragraph| paragraph.edges.len())
                .sum(),
        },
    }
}

fn diagnostic_sections(diagnostics: &[Diagnostic]) -> DiagnosticSections {
    let mut sections = DiagnosticSections {
        source: Vec::new(),
        syntax: Vec::new(),
        symbols: Vec::new(),
        layout: Vec::new(),
        references: Vec::new(),
        conditions: Vec::new(),
        evaluate: Vec::new(),
        vm: Vec::new(),
        procedure: Vec::new(),
        cfg: Vec::new(),
        indexes: Vec::new(),
        search: Vec::new(),
        odo: Vec::new(),
        file_io: Vec::new(),
        nested_programs: Vec::new(),
        national_dbcs: Vec::new(),
        oracle: Vec::new(),
        codegen: Vec::new(),
    };
    for diagnostic in diagnostics {
        let target = if diagnostic.code.contains("COPY") || diagnostic.code.contains("SOURCE") {
            &mut sections.source
        } else if diagnostic.code.contains("SYNTAX") || diagnostic.code == "E_UNSUPPORTED_STATEMENT"
        {
            &mut sections.syntax
        } else if diagnostic.code.contains("DUPLICATE")
            || diagnostic.code.contains("SYMBOL")
            || diagnostic.code.contains("CONDITION")
        {
            &mut sections.symbols
        } else if diagnostic.code.contains("NATIONAL") || diagnostic.code.contains("DBCS") {
            &mut sections.national_dbcs
        } else if diagnostic.code.contains("LAYOUT")
            || diagnostic.code.contains("REDEFINES")
            || diagnostic.code.contains("OCCURS")
            || diagnostic.code.contains("LEVEL")
        {
            &mut sections.layout
        } else if diagnostic.code.contains("DATA")
            || diagnostic.code.contains("REFERENCE")
            || diagnostic.code.contains("SUBSCRIPT")
        {
            &mut sections.references
        } else if diagnostic.code.contains("CONDITION") {
            &mut sections.conditions
        } else if diagnostic.code.contains("EVALUATE") {
            &mut sections.evaluate
        } else if diagnostic.code.contains("SEARCH") {
            &mut sections.search
        } else if diagnostic.code.contains("INDEX") {
            &mut sections.indexes
        } else if diagnostic.code.contains("ODO") {
            &mut sections.odo
        } else if diagnostic.code.contains("FILE") {
            &mut sections.file_io
        } else if diagnostic.code.contains("NESTED") || diagnostic.code.contains("CALL") {
            &mut sections.nested_programs
        } else if diagnostic.code.contains("ORACLE") || diagnostic.code.contains("GNUCOBOL") {
            &mut sections.oracle
        } else if diagnostic.code.contains("CFG") {
            &mut sections.cfg
        } else if diagnostic.code.contains("VM") {
            &mut sections.vm
        } else if diagnostic.code.contains("PERFORM")
            || diagnostic.code.contains("VERB")
            || diagnostic.code.contains("SECTION")
            || diagnostic.code.contains("ENVIRONMENT")
        {
            &mut sections.procedure
        } else {
            &mut sections.codegen
        };
        target.push(diagnostic.clone());
    }
    sections
}

fn write_report(path: &Path, report: &MigrationReport) -> Result<(), ConvertError> {
    let text = serde_json::to_string_pretty(report)?;
    fs::write(path, text).map_err(|source| ConvertError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn package_name(name: &str) -> String {
    let mut out = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "converted-cobol".to_string()
    } else {
        out
    }
}

fn program_suffix(name: &str) -> String {
    rust_ident(&normalize_vm_ref(name))
}

#[allow(dead_code)]
fn enum_variant(name: &str) -> String {
    let mut out = String::new();
    let mut uppercase_next = true;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            if uppercase_next {
                out.push(ch.to_ascii_uppercase());
                uppercase_next = false;
            } else {
                out.push(ch.to_ascii_lowercase());
            }
        } else {
            uppercase_next = true;
        }
    }
    if out.is_empty() {
        "Paragraph".to_string()
    } else if out
        .chars()
        .next()
        .map(|ch| ch.is_ascii_digit())
        .unwrap_or(false)
    {
        format!("P{out}")
    } else {
        out
    }
}

fn rust_ident(name: &str) -> String {
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
    let mut out = out.trim_matches('_').to_string();
    if out.is_empty() {
        out = "field".to_string();
    }
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

fn escape_rust(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            ch if ch.is_control() => out.push_str(&format!("\\u{{{:X}}}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn preflight_fixture(procedure_lines: &str) -> String {
        format!(
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. T.\nPROCEDURE DIVISION.\nMAIN.\n{procedure_lines}"
        )
    }

    fn count_code(diagnostics: &[Diagnostic], code: &str) -> usize {
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == code)
            .count()
    }

    #[test]
    fn generated_hello_world_project_has_expected_files() {
        let dir = std::env::temp_dir().join(format!("cobol_codegen_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let input = dir.join("hello.cbl");
        let out = dir.join("out");
        fs::write(
            &input,
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\nPROCEDURE DIVISION.\nMAIN.\nDISPLAY \"HELLO\".\nSTOP RUN.\n",
        )
        .expect("write input");
        let result = convert(ConvertOptions {
            input,
            copybook_dirs: Vec::new(),
            out_dir: out.clone(),
            dialect: Dialect::Ibm,
            source_format: SourceFormat::Free,
        })
        .expect("conversion succeeds");
        assert!(result.files.iter().any(|path| path.ends_with("Cargo.toml")));
        assert!(out
            .join("vendor")
            .join("cobol-platform")
            .join("Cargo.toml")
            .is_file());
        let manifest = fs::read_to_string(out.join("Cargo.toml")).expect("manifest");
        assert!(manifest.contains("cobol-platform = { path = \"vendor/cobol-platform\" }"));
        let main_rs = fs::read_to_string(out.join("src").join("main.rs")).expect("main rs");
        assert!(main_rs.contains("--runtime-config"));
        assert!(main_rs.contains("cobol-runtime.json"));
        assert!(main_rs.contains("cobol-file-map.json"));
        let program_rs =
            fs::read_to_string(out.join("src").join("program.rs")).expect("program rs");
        assert!(program_rs.contains("pub fn apply_platform_config"));
        assert!(out.join("migration-report.json").is_file());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn packed_decimal_initial_template_encodes_value_bytes() {
        let preprocessed = PreprocessedSource {
            primary_path: PathBuf::from("packed-value.cbl"),
            text: "IDENTIFICATION DIVISION.\nPROGRAM-ID. PACKEDVALUE.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 WS-AMT PIC S9(5) COMP-3 VALUE 00123.\nPROCEDURE DIVISION.\nMAIN.\nSTOP RUN.\n".to_string(),
            format: SourceFormat::Free,
            includes: Vec::new(),
        };
        let programs = parse_and_analyze_compilation(&preprocessed, Dialect::Ibm).expect("analyze");
        assert!(!programs[0].has_errors(), "{:?}", programs[0].diagnostics);
        let item = programs[0]
            .storage
            .items
            .iter()
            .find(|item| item.qualified_name == "WS_AMT")
            .expect("WS-AMT item");

        assert_eq!(
            initial_template_bytes_for_item(item, item.byte_len),
            vec![0x00, 0x12, 0x3c]
        );
        let data_rs = emit_data_rs(&programs[0]);
        assert!(data_rs.contains("copy_from_slice(&[0u8, 18u8, 60u8])"));
        assert!(!data_rs.contains("move_value(CobolValue::Text(\"00123\""));
    }

    #[test]
    fn packed_decimal_initial_storage_encodes_default_zero_bytes() {
        let preprocessed = PreprocessedSource {
            primary_path: PathBuf::from("packed-zero.cbl"),
            text: "IDENTIFICATION DIVISION.\nPROGRAM-ID. PACKEDZERO.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 WS-AMT PIC S9(5) COMP-3.\nPROCEDURE DIVISION.\nMAIN.\nSTOP RUN.\n".to_string(),
            format: SourceFormat::Free,
            includes: Vec::new(),
        };
        let programs = parse_and_analyze_compilation(&preprocessed, Dialect::Ibm).expect("analyze");
        assert!(!programs[0].has_errors(), "{:?}", programs[0].diagnostics);
        let data_rs = emit_data_rs(&programs[0]);

        assert!(data_rs.contains("copy_from_slice(&[0u8, 0u8, 12u8])"));
    }

    #[test]
    fn numeric_display_initial_storage_uses_planned_value_bytes() {
        let preprocessed = PreprocessedSource {
            primary_path: PathBuf::from("numeric-value.cbl"),
            text: "IDENTIFICATION DIVISION.\nPROGRAM-ID. NUMVALUE.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 WS-COUNT PIC 9(3) VALUE 7.\nPROCEDURE DIVISION.\nMAIN.\nSTOP RUN.\n".to_string(),
            format: SourceFormat::Free,
            includes: Vec::new(),
        };
        let programs = parse_and_analyze_compilation(&preprocessed, Dialect::Ibm).expect("analyze");
        assert!(!programs[0].has_errors(), "{:?}", programs[0].diagnostics);
        let data_rs = emit_data_rs(&programs[0]);

        assert!(data_rs.contains("copy_from_slice(&[48u8, 48u8, 55u8])"));
        assert!(!data_rs.contains("move_value(CobolValue::Text(\"7\""));
    }

    #[test]
    fn numeric_display_occurs_initial_storage_repeats_each_occurrence() {
        let preprocessed = PreprocessedSource {
            primary_path: PathBuf::from("numeric-occurs-value.cbl"),
            text: "IDENTIFICATION DIVISION.\nPROGRAM-ID. NUMOCC.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 WS-TABLE OCCURS 3 TIMES PIC 9 VALUE 0.\nPROCEDURE DIVISION.\nMAIN.\nSTOP RUN.\n".to_string(),
            format: SourceFormat::Free,
            includes: Vec::new(),
        };
        let programs = parse_and_analyze_compilation(&preprocessed, Dialect::Ibm).expect("analyze");
        assert!(!programs[0].has_errors(), "{:?}", programs[0].diagnostics);
        let data_rs = emit_data_rs(&programs[0]);

        assert!(data_rs.contains("get_mut(0..1)"));
        assert!(data_rs.contains("get_mut(1..2)"));
        assert!(data_rs.contains("get_mut(2..3)"));
        assert_eq!(data_rs.matches("copy_from_slice(&[48u8])").count(), 3);
        assert!(!data_rs.contains("move_value(CobolValue::Text(\"0\""));
    }

    #[test]
    fn alphanumeric_occurs_initial_storage_uses_planned_cell_bytes() {
        let preprocessed = PreprocessedSource {
            primary_path: PathBuf::from("alpha-occurs-value.cbl"),
            text: "IDENTIFICATION DIVISION.\nPROGRAM-ID. ALPHAOCC.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 WS-TABLE.\n   05 WS-ITEM OCCURS 3 TIMES PIC X VALUE \"A\".\nPROCEDURE DIVISION.\nMAIN.\nSTOP RUN.\n".to_string(),
            format: SourceFormat::Free,
            includes: Vec::new(),
        };
        let programs = parse_and_analyze_compilation(&preprocessed, Dialect::Ibm).expect("analyze");
        assert!(!programs[0].has_errors(), "{:?}", programs[0].diagnostics);
        let cell = programs[0]
            .storage
            .storage_cells
            .iter()
            .find(|cell| cell.key == "WS_TABLE.WS_ITEM")
            .expect("WS-ITEM storage cell");
        assert_eq!(cell.initial_bytes, vec![65, 32, 32]);

        let data_rs = emit_data_rs(&programs[0]);

        assert!(data_rs.contains("get_mut(0..1)"));
        assert!(data_rs.contains("copy_from_slice(&[65u8])"));
        assert!(data_rs.contains("get_mut(1..2)"));
        assert!(data_rs.contains("get_mut(2..3)"));
        assert_eq!(data_rs.matches("copy_from_slice(&[32u8])").count(), 2);
    }

    #[test]
    fn compute_expression_subscripted_operand_emits_subscripted_access_path() {
        let preprocessed = PreprocessedSource {
            primary_path: PathBuf::from("compute-expression-subscript.cbl"),
            text: "IDENTIFICATION DIVISION.\nPROGRAM-ID. COMPEXPRSUB.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 WS-TABLE OCCURS 3 TIMES PIC 9 VALUE 0.\n01 WS-OUT PIC 9 VALUE 0.\nPROCEDURE DIVISION.\nMAIN.\nCOMPUTE WS-OUT = WS-TABLE(2) + 1.\nSTOP RUN.\n".to_string(),
            format: SourceFormat::Free,
            includes: Vec::new(),
        };
        let programs = parse_and_analyze_compilation(&preprocessed, Dialect::Ibm).expect("analyze");
        assert!(!programs[0].has_errors(), "{:?}", programs[0].diagnostics);

        let expr = emit_vm_expr_from_text("WS-TABLE(2) + 1", &programs[0]);

        assert!(expr.contains("target: \"WS_TABLE\".to_string()"));
        assert!(expr.contains("cobol_vm::VmSubscript"));
        assert!(expr.contains("cobol_vm::VmExpr::Number(\"2\".to_string())"));
        assert!(!expr.contains("WS_TABLE(2)"));
    }

    #[test]
    fn invalid_packed_decimal_initial_value_is_blocked_before_codegen() {
        let preprocessed = PreprocessedSource {
            primary_path: PathBuf::from("packed-invalid.cbl"),
            text: "IDENTIFICATION DIVISION.\nPROGRAM-ID. PACKEDBAD.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 WS-AMT PIC 9(2) COMP-3 VALUE -1.\nPROCEDURE DIVISION.\nMAIN.\nSTOP RUN.\n".to_string(),
            format: SourceFormat::Free,
            includes: Vec::new(),
        };
        let mut programs =
            parse_and_analyze_compilation(&preprocessed, Dialect::Ibm).expect("analyze");
        assert_eq!(
            count_code(&programs[0].diagnostics, "E_INVALID_PACKED_DECIMAL_VALUE"),
            1
        );

        programs[0].diagnostics.clear();
        validate_codegen_invariants(&mut programs);

        assert!(programs[0].has_errors());
        assert_eq!(
            count_code(
                &programs[0].diagnostics,
                "E_CODEGEN_PACKED_DECIMAL_INITIAL_VALUE"
            ),
            1
        );
    }

    #[test]
    fn blocked_conversion_cleans_stale_artifacts_and_writes_blocked_report() {
        let dir =
            std::env::temp_dir().join(format!("cobol_codegen_blocked_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let input = dir.join("blocked.cbl");
        let out = dir.join("out");
        fs::create_dir_all(out.join("src")).expect("src dir");
        fs::create_dir_all(out.join("vendor")).expect("vendor dir");
        fs::write(out.join("src").join("main.rs"), "stale").expect("stale src");
        fs::write(out.join("vendor").join("shim.rs"), "stale").expect("stale vendor");
        fs::write(out.join("Cargo.toml"), "stale").expect("stale manifest");
        fs::write(out.join("Cargo.lock"), "stale").expect("stale lockfile");
        fs::write(
            &input,
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. BLOCKED.\nPROCEDURE DIVISION.\nMAIN.\nACCEPT WS-FIELD.\nSTOP RUN.\n",
        )
        .expect("write input");

        let result = convert(ConvertOptions {
            input,
            copybook_dirs: Vec::new(),
            out_dir: out.clone(),
            dialect: Dialect::Ibm,
            source_format: SourceFormat::Free,
        });

        let report_path = match result {
            Err(ConvertError::MigrationBlocked { report_path }) => report_path,
            other => panic!("expected blocked migration, got {other:?}"),
        };
        assert_eq!(report_path, out.join("migration-report.json"));
        assert!(!out.join("src").exists());
        assert!(!out.join("vendor").exists());
        assert!(!out.join("Cargo.toml").exists());
        assert!(!out.join("Cargo.lock").exists());

        let report_text = fs::read_to_string(&report_path).expect("read report");
        let report: serde_json::Value =
            serde_json::from_str(&report_text).expect("blocked report json");
        assert_eq!(report["status"], "blocked");
        assert!(report["generated_files"]
            .as_array()
            .expect("generated files")
            .is_empty());
        assert!(report["diagnostics"]
            .as_array()
            .expect("diagnostics")
            .iter()
            .any(|diagnostic| diagnostic["code"] == "E_UNSUPPORTED_VERB"));
        assert!(report["diagnostic_sections"]["procedure"]
            .as_array()
            .expect("procedure diagnostics")
            .iter()
            .any(|diagnostic| diagnostic["code"] == "E_UNSUPPORTED_VERB"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn mask_literals_masks_doubled_quote_literals() {
        let source = "DISPLAY \"ACCEPT \"\"EXEC\"\" NEXT\" AFTER.";
        let masked = mask_literals(source);

        assert_eq!(masked.len(), source.len());
        assert!(masked.starts_with("DISPLAY "));
        assert!(masked.ends_with(" AFTER."));
        assert!(masked.contains("DISPLAY"));
        assert!(masked.contains("AFTER"));
        assert!(!masked.contains("ACCEPT"));
        assert!(!masked.contains("EXEC"));
        assert!(!masked.contains("NEXT"));
    }

    #[test]
    fn preflight_ignores_unsupported_verbs_inside_literals() {
        let source =
            preflight_fixture("DISPLAY \"ACCEPT \"\"EXEC\"\" NEXT SENTENCE\".\nSTOP RUN.\n");
        let diagnostics = preflight_diagnostics(&source, "fixture.cbl");

        assert_eq!(count_code(&diagnostics, "E_UNSUPPORTED_VERB"), 0);
        assert_eq!(count_code(&diagnostics, "E_UNSUPPORTED_CONTROL_FLOW"), 0);
    }

    #[test]
    fn preflight_blocks_unsupported_verbs_outside_literals() {
        let source = preflight_fixture("ACCEPT WS-FIELD.\nSTOP RUN.\n");
        let diagnostics = preflight_diagnostics(&source, "fixture.cbl");

        assert_eq!(count_code(&diagnostics, "E_UNSUPPORTED_VERB"), 1);
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("ACCEPT")));
    }

    #[test]
    fn preflight_blocks_unsupported_next_sentence_shape() {
        let source =
            preflight_fixture("IF WS-FLAG = \"Y\" NEXT SENTENCE DISPLAY \"BAD\".\nSTOP RUN.\n");
        let diagnostics = preflight_diagnostics(&source, "fixture.cbl");

        assert_eq!(count_code(&diagnostics, "E_UNSUPPORTED_CONTROL_FLOW"), 1);
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("NEXT SENTENCE")));
    }

    #[test]
    fn preflight_blocks_simple_next_sentence_without_duplicate_verb_blocker() {
        let source =
            preflight_fixture("IF WS-FLAG = \"Y\" NEXT SENTENCE ELSE DISPLAY \"OK\".\nSTOP RUN.\n");
        let diagnostics = preflight_diagnostics(&source, "fixture.cbl");

        assert_eq!(count_code(&diagnostics, "E_UNSUPPORTED_CONTROL_FLOW"), 1);
        assert!(!diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_UNSUPPORTED_VERB" && diagnostic.message.contains("NEXT")
        }));
    }

    #[test]
    fn preflight_blocks_perform_varying_after() {
        let source = preflight_fixture(
            "PERFORM BODY VARYING I FROM 1 BY 1 AFTER J FROM 1 BY 1 UNTIL I > 2.\nBODY.\nSTOP RUN.\n",
        );
        let diagnostics = preflight_diagnostics(&source, "fixture.cbl");

        assert_eq!(count_code(&diagnostics, "E_UNSUPPORTED_CONTROL_FLOW"), 1);
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("PERFORM VARYING AFTER")));
    }

    #[test]
    fn display_current_date_function_is_blocked_as_function_not_data() {
        let preprocessed = PreprocessedSource {
            primary_path: PathBuf::from("display-function.cbl"),
            text: "IDENTIFICATION DIVISION.\nPROGRAM-ID. FUNCDISP.\nPROCEDURE DIVISION.\nMAIN.\nDISPLAY FUNCTION CURRENT-DATE.\nSTOP RUN.\n".to_string(),
            format: SourceFormat::Free,
            includes: Vec::new(),
        };
        let programs = parse_and_analyze_compilation(&preprocessed, Dialect::Ibm).expect("analyze");
        assert_eq!(
            count_code(&programs[0].diagnostics, "E_UNSUPPORTED_FUNCTION_OPERAND"),
            1
        );
        assert_eq!(count_code(&programs[0].diagnostics, "E_UNRESOLVED_DATA"), 0);
        let StatementIr::Display(values) = &programs[0].paragraphs[0].statements[0] else {
            panic!("expected DISPLAY");
        };
        assert!(matches!(
            values.as_slice(),
            [OperandIr::Function(FunctionOperandIr::UserDefined { name, .. })]
                if name == "CURRENT-DATE"
        ));
    }

    #[test]
    fn display_intrinsic_function_missing_arg_is_blocked_before_codegen() {
        let preprocessed = PreprocessedSource {
            primary_path: PathBuf::from("display-function-missing-arg.cbl"),
            text: "IDENTIFICATION DIVISION.\nPROGRAM-ID. FUNCBAD.\nPROCEDURE DIVISION.\nMAIN.\nDISPLAY FUNCTION LENGTH.\nSTOP RUN.\n".to_string(),
            format: SourceFormat::Free,
            includes: Vec::new(),
        };
        let programs = parse_and_analyze_compilation(&preprocessed, Dialect::Ibm).expect("analyze");
        assert_eq!(
            count_code(&programs[0].diagnostics, "E_INVALID_FUNCTION_ARITY"),
            1
        );
        assert_eq!(count_code(&programs[0].diagnostics, "E_UNRESOLVED_DATA"), 0);
    }

    #[test]
    fn display_intrinsic_function_comma_args_are_blocked_before_codegen() {
        let preprocessed = PreprocessedSource {
            primary_path: PathBuf::from("display-function-comma-args.cbl"),
            text: "IDENTIFICATION DIVISION.\nPROGRAM-ID. FUNCBADARGS.\nPROCEDURE DIVISION.\nMAIN.\nDISPLAY FUNCTION LENGTH(\"A\", \"B\").\nSTOP RUN.\n".to_string(),
            format: SourceFormat::Free,
            includes: Vec::new(),
        };
        let programs = parse_and_analyze_compilation(&preprocessed, Dialect::Ibm).expect("analyze");
        assert_eq!(
            count_code(&programs[0].diagnostics, "E_INVALID_FUNCTION_ARITY"),
            1
        );
        assert!(programs[0].diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_FUNCTION_ARITY"
                && diagnostic.message.contains("FUNCTION LENGTH(\"A\", \"B\")")
        }));
        assert_eq!(count_code(&programs[0].diagnostics, "E_UNRESOLVED_DATA"), 0);
    }

    #[test]
    fn if_intrinsic_function_comma_args_preserve_raw_blocker() {
        let preprocessed = PreprocessedSource {
            primary_path: PathBuf::from("if-function-comma-args.cbl"),
            text: "IDENTIFICATION DIVISION.\nPROGRAM-ID. FUNCBADIFARGS.\nPROCEDURE DIVISION.\nMAIN.\nIF FUNCTION LENGTH(\"A\", \"B\") = 1 DISPLAY \"BAD\".\nSTOP RUN.\n".to_string(),
            format: SourceFormat::Free,
            includes: Vec::new(),
        };
        let programs = parse_and_analyze_compilation(&preprocessed, Dialect::Ibm).expect("analyze");
        assert_eq!(
            count_code(&programs[0].diagnostics, "E_INVALID_FUNCTION_ARITY"),
            1
        );
        assert!(programs[0].diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_FUNCTION_ARITY"
                && diagnostic.message.contains("FUNCTION LENGTH(\"A\", \"B\")")
        }));
        assert_eq!(count_code(&programs[0].diagnostics, "E_UNRESOLVED_DATA"), 0);
    }

    #[test]
    fn compute_intrinsic_function_comma_args_preserve_raw_blocker() {
        let preprocessed = PreprocessedSource {
            primary_path: PathBuf::from("compute-function-comma-args.cbl"),
            text: "IDENTIFICATION DIVISION.\nPROGRAM-ID. FUNCBADCARGS.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 WS-N PIC 999.\nPROCEDURE DIVISION.\nMAIN.\nCOMPUTE WS-N = FUNCTION LENGTH(\"A\", \"B\").\nSTOP RUN.\n".to_string(),
            format: SourceFormat::Free,
            includes: Vec::new(),
        };
        let programs = parse_and_analyze_compilation(&preprocessed, Dialect::Ibm).expect("analyze");
        assert_eq!(
            count_code(&programs[0].diagnostics, "E_INVALID_FUNCTION_ARITY"),
            1
        );
        assert!(programs[0].diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_FUNCTION_ARITY"
                && diagnostic.message.contains("FUNCTION LENGTH(\"A\", \"B\")")
        }));
        assert_eq!(
            count_code(&programs[0].diagnostics, "E_UNSUPPORTED_ARITHMETIC"),
            1
        );
        assert_eq!(count_code(&programs[0].diagnostics, "E_UNRESOLVED_DATA"), 0);
    }

    #[test]
    fn if_intrinsic_function_unclosed_parenthesis_is_blocked_as_function() {
        let preprocessed = PreprocessedSource {
            primary_path: PathBuf::from("if-function-unclosed-paren.cbl"),
            text: "IDENTIFICATION DIVISION.\nPROGRAM-ID. FUNCBADIFPAREN.\nPROCEDURE DIVISION.\nMAIN.\nIF FUNCTION LENGTH(\"A\" = 1 DISPLAY \"BAD\".\nSTOP RUN.\n".to_string(),
            format: SourceFormat::Free,
            includes: Vec::new(),
        };
        let programs = parse_and_analyze_compilation(&preprocessed, Dialect::Ibm).expect("analyze");
        assert_eq!(
            count_code(&programs[0].diagnostics, "E_INVALID_FUNCTION_ARGUMENT"),
            1
        );
        assert_eq!(count_code(&programs[0].diagnostics, "E_UNRESOLVED_DATA"), 0);
    }

    #[test]
    fn compute_function_ord_is_preflight_blocked_without_fake_data_reference() {
        let preprocessed = PreprocessedSource {
            primary_path: PathBuf::from("compute-function.cbl"),
            text: "IDENTIFICATION DIVISION.\nPROGRAM-ID. FUNCORD.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 WS-N PIC 999.\nPROCEDURE DIVISION.\nMAIN.\nCOMPUTE WS-N = FUNCTION ORD(\"A\").\nSTOP RUN.\n".to_string(),
            format: SourceFormat::Free,
            includes: Vec::new(),
        };
        let programs = parse_and_analyze_compilation(&preprocessed, Dialect::Ibm).expect("analyze");
        assert_eq!(
            count_code(&programs[0].diagnostics, "E_UNSUPPORTED_ARITHMETIC"),
            1
        );
        assert!(programs[0].diagnostics.iter().all(|diagnostic| {
            !(diagnostic.code == "E_UNRESOLVED_DATA"
                && (diagnostic.message.contains("FUNCTION") || diagnostic.message.contains("ORD")))
        }));
    }

    #[test]
    fn preflight_blocks_call_by_reference_content_and_value_modes() {
        for mode in ["REFERENCE", "CONTENT", "VALUE"] {
            let source =
                preflight_fixture(&format!("CALL \"SUB\" USING BY {mode} ARG.\nSTOP RUN.\n"));
            let diagnostics = preflight_diagnostics(&source, "fixture.cbl");

            assert_eq!(
                count_code(&diagnostics, "E_UNSUPPORTED_CALL_MODE"),
                1,
                "expected one call-mode blocker for BY {mode}"
            );
            assert!(diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains(&format!("BY {mode}"))));
        }
    }

    #[test]
    fn dedupe_diagnostics_removes_exact_duplicate_blockers() {
        let diagnostic = Diagnostic::error(
            "E_UNSUPPORTED_VERB",
            "Unsupported COBOL verb `ACCEPT` is not conversion-safe yet",
            SourceSpan {
                file: "fixture.cbl".to_string(),
                line: 5,
                column: 1,
            },
        );

        let deduped = dedupe_diagnostics(vec![diagnostic.clone(), diagnostic]);

        assert_eq!(deduped.len(), 1);
        assert_eq!(deduped[0].code, "E_UNSUPPORTED_VERB");
    }

    #[test]
    fn diagnostic_sections_route_blockers_to_expected_report_buckets() {
        let span = SourceSpan {
            file: "fixture.cbl".to_string(),
            line: 5,
            column: 1,
        };
        let diagnostics = vec![
            Diagnostic::error(
                "E_UNSUPPORTED_VERB",
                "Procedure Division verb ACCEPT is not lowered by the converter preview",
                span.clone(),
            ),
            Diagnostic::error(
                "E_UNSUPPORTED_CALL_MODE",
                "CALL operand mode BY REFERENCE is not conversion-safe yet",
                span.clone(),
            ),
            Diagnostic::error(
                "E_CODEGEN_UNSUPPORTED_STATEMENT",
                "unsupported COBOL statement EXEC reached code generation invariant validation",
                span.clone(),
            ),
            Diagnostic::error(
                "E_CODEGEN_SEARCH_ALL_UNLOWERED",
                "SEARCH ALL reached code generation without a fully lowered key equality",
                span,
            ),
        ];

        let sections = diagnostic_sections(&diagnostics);

        assert_eq!(sections.procedure.len(), 1);
        assert_eq!(sections.procedure[0].code, "E_UNSUPPORTED_VERB");
        assert_eq!(sections.nested_programs.len(), 1);
        assert_eq!(sections.nested_programs[0].code, "E_UNSUPPORTED_CALL_MODE");
        assert_eq!(sections.codegen.len(), 1);
        assert_eq!(sections.codegen[0].code, "E_CODEGEN_UNSUPPORTED_STATEMENT");
        assert_eq!(sections.search.len(), 1);
        assert_eq!(sections.search[0].code, "E_CODEGEN_SEARCH_ALL_UNLOWERED");
    }

    #[test]
    fn codegen_invariant_blocks_unsupported_statement_if_sema_missed_it() {
        let preprocessed = PreprocessedSource {
            primary_path: PathBuf::from("missed-unsupported.cbl"),
            text: "IDENTIFICATION DIVISION.\nPROGRAM-ID. MISSED.\nPROCEDURE DIVISION.\nMAIN.\nDISPLAY \"OK\".\nSTOP RUN.\n".to_string(),
            format: SourceFormat::Free,
            includes: Vec::new(),
        };
        let mut programs =
            parse_and_analyze_compilation(&preprocessed, Dialect::Ibm).expect("analyze");
        programs[0].diagnostics.clear();
        programs[0].paragraphs[0].statements.insert(
            0,
            StatementIr::Unsupported {
                keyword: "EXEC".to_string(),
                raw: "EXEC SQL SELECT 1 END-EXEC".to_string(),
            },
        );
        programs[0].paragraphs[0].statements.insert(
            1,
            StatementIr::Unsupported {
                keyword: "EXEC".to_string(),
                raw: "EXEC SQL SELECT 1 END-EXEC".to_string(),
            },
        );

        validate_codegen_invariants(&mut programs);

        assert!(programs[0].has_errors());
        assert_eq!(
            count_code(&programs[0].diagnostics, "E_CODEGEN_UNSUPPORTED_STATEMENT"),
            1
        );
    }

    #[test]
    fn codegen_invariant_blocks_next_sentence_if_sema_missed_it() {
        let preprocessed = PreprocessedSource {
            primary_path: PathBuf::from("missed-next-sentence.cbl"),
            text: "IDENTIFICATION DIVISION.\nPROGRAM-ID. MISSED.\nPROCEDURE DIVISION.\nMAIN.\nDISPLAY \"OK\".\nSTOP RUN.\n".to_string(),
            format: SourceFormat::Free,
            includes: Vec::new(),
        };
        let mut programs =
            parse_and_analyze_compilation(&preprocessed, Dialect::Ibm).expect("analyze");
        programs[0].diagnostics.clear();
        programs[0].paragraphs[0]
            .statements
            .insert(0, StatementIr::NextSentence);

        validate_codegen_invariants(&mut programs);

        assert!(programs[0].has_errors());
        assert_eq!(
            count_code(
                &programs[0].diagnostics,
                "E_CODEGEN_NEXT_SENTENCE_UNLOWERED"
            ),
            1
        );
    }

    #[test]
    fn codegen_source_has_no_legacy_raw_file_io_statement_paths() {
        let source = include_str!("lib.rs");
        for needle in [
            ["StatementIr::", "Read", "("].concat(),
            ["StatementIr::", "Rewrite", "("].concat(),
            ["StatementIr::", "Delete", "("].concat(),
            ["E_CODEGEN_", "LEGACY_RAW_FILE_IO"].concat(),
            ["legacy raw ", "READ"].concat(),
            ["legacy raw ", "REWRITE"].concat(),
            ["legacy raw ", "DELETE"].concat(),
        ] {
            assert!(
                !source.contains(&needle),
                "legacy raw file-I/O residue remains: {needle}"
            );
        }
    }

    #[test]
    fn codegen_invariant_blocks_unlowered_search_all_if_sema_missed_it() {
        let preprocessed = PreprocessedSource {
            primary_path: PathBuf::from("missed-search.cbl"),
            text: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. MISSEDSEARCH.
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
"#
            .to_string(),
            format: SourceFormat::Free,
            includes: Vec::new(),
        };
        let mut programs =
            parse_and_analyze_compilation(&preprocessed, Dialect::Ibm).expect("analyze");
        programs[0].diagnostics.clear();

        validate_codegen_invariants(&mut programs);

        assert!(programs[0].has_errors());
        assert_eq!(
            count_code(&programs[0].diagnostics, "E_CODEGEN_SEARCH_ALL_UNLOWERED"),
            1
        );
    }
}
