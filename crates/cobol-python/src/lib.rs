#![allow(clippy::useless_conversion)]

use cobol_codegen_rust::{
    check, convert, CheckOptions, ConvertError, ConvertOptions, Dialect, SourceFormat,
};
use cobol_ir::Diagnostic;
use cobol_source::preprocess_file;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use tempfile::TempDir;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PythonConvertOptions {
    pub copybooks: Vec<(String, String)>,
    pub source_format: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PythonDiagnostic {
    pub code: String,
    pub severity: String,
    pub message: String,
    pub file: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub help: Option<String>,
}

pub type DiagnosticList = Vec<PythonDiagnostic>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceAnalysis {
    pub path: String,
    pub program_id: Option<String>,
    pub copybooks: Vec<String>,
    pub calls: Vec<CallDependency>,
    pub unsupported_features: Vec<UnsupportedFeatureAdvice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CallDependency {
    pub target: String,
    pub literal: bool,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnsupportedFeatureAdvice {
    pub feature: String,
    pub capability_id: String,
    pub status: String,
    pub paragraphs: Vec<String>,
    pub advice: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PythonGeneratedProject {
    pub out_dir: PathBuf,
    pub generated_files: Vec<String>,
    pub report_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BatchConversionSummary {
    pub total: usize,
    pub generated: usize,
    pub blocked: usize,
    pub failures: usize,
    pub projects: Vec<BatchProjectResult>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BatchProjectResult {
    pub input: String,
    pub out_dir: PathBuf,
    pub status: String,
    pub diagnostics: DiagnosticList,
}

#[derive(Debug, thiserror::Error)]
pub enum PythonBindingError {
    #[error("invalid source format {0}")]
    InvalidSourceFormat(String),
    #[error(
        "copybook name {0:?} must be relative and must not contain parent-directory components"
    )]
    InvalidCopybookName(String),
    #[error("failed to write {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{0}")]
    Source(#[from] cobol_source::SourceError),
}

pub fn preprocess_source_for_python(
    source: &str,
    copybooks: &[(String, String)],
    source_format: &str,
) -> Result<String, PythonBindingError> {
    let format = parse_source_format(source_format)?;
    let temp = TempDir::new().map_err(|source| PythonBindingError::Io {
        path: std::env::temp_dir(),
        source,
    })?;
    let input = temp.path().join("input.cbl");
    let copybook_dir = temp.path().join("copybooks");
    fs::create_dir_all(&copybook_dir).map_err(|source| PythonBindingError::Io {
        path: copybook_dir.clone(),
        source,
    })?;
    fs::write(&input, source).map_err(|source| PythonBindingError::Io {
        path: input.clone(),
        source,
    })?;
    write_copybooks(&copybook_dir, copybooks)?;

    let preprocessed = preprocess_file(&input, &[copybook_dir], format)?;
    Ok(preprocessed.text)
}

pub fn convert_cobol_source_for_python(
    source: &str,
    dialect: &str,
    options: PythonConvertOptions,
) -> Result<String, DiagnosticList> {
    let dialect = Dialect::parse(dialect).ok_or_else(|| {
        vec![PythonDiagnostic::new(
            "E_INVALID_DIALECT",
            "Error",
            format!("invalid dialect {dialect}"),
        )]
    })?;
    let source_format = options.source_format.as_deref().unwrap_or("auto");
    let source_format = parse_source_format(source_format).map_err(|err| {
        vec![PythonDiagnostic::new(
            "E_INVALID_SOURCE_FORMAT",
            "Error",
            err.to_string(),
        )]
    })?;

    let temp = TempDir::new().map_err(|err| {
        vec![PythonDiagnostic::new(
            "E_IO",
            "Error",
            format!("failed to create temporary conversion directory: {err}"),
        )]
    })?;
    let input = temp.path().join("input.cbl");
    let copybook_dir = temp.path().join("copybooks");
    let out_dir = temp.path().join("generated");
    fs::create_dir_all(&copybook_dir)
        .and_then(|_| fs::write(&input, source))
        .map_err(|err| vec![PythonDiagnostic::new("E_IO", "Error", err.to_string())])?;
    write_copybooks(&copybook_dir, &options.copybooks).map_err(|err| {
        vec![PythonDiagnostic::new(
            "E_COPYBOOK_MATERIALIZE",
            "Error",
            err.to_string(),
        )]
    })?;

    match convert(ConvertOptions {
        input,
        copybook_dirs: vec![copybook_dir],
        out_dir: out_dir.clone(),
        dialect,
        source_format,
    }) {
        Ok(_) => fs::read_to_string(out_dir.join("src").join("program.rs")).map_err(|err| {
            vec![PythonDiagnostic::new(
                "E_READ_GENERATED_PROGRAM",
                "Error",
                err.to_string(),
            )]
        }),
        Err(ConvertError::MigrationBlocked { report_path }) => {
            Err(diagnostics_from_report(&report_path))
        }
        Err(err) => Err(vec![PythonDiagnostic::new(
            "E_CONVERT",
            "Error",
            err.to_string(),
        )]),
    }
}

pub fn check_cobol_source_for_python(
    source: &str,
    dialect: &str,
    options: PythonConvertOptions,
) -> Result<DiagnosticList, DiagnosticList> {
    let dialect = parse_dialect_for_python(dialect)?;
    let source_format = parse_source_format_for_diagnostics(options.source_format.as_deref())?;

    let temp = TempDir::new().map_err(|err| {
        vec![PythonDiagnostic::new(
            "E_IO",
            "Error",
            format!("failed to create temporary check directory: {err}"),
        )]
    })?;
    let input = temp.path().join("input.cbl");
    let copybook_dir = temp.path().join("copybooks");
    fs::create_dir_all(&copybook_dir)
        .and_then(|_| fs::write(&input, source))
        .map_err(|err| vec![PythonDiagnostic::new("E_IO", "Error", err.to_string())])?;
    write_copybooks(&copybook_dir, &options.copybooks).map_err(|err| {
        vec![PythonDiagnostic::new(
            "E_COPYBOOK_MATERIALIZE",
            "Error",
            err.to_string(),
        )]
    })?;

    check(CheckOptions {
        input,
        copybook_dirs: vec![copybook_dir],
        dialect,
        source_format,
    })
    .map(|diagnostics| {
        diagnostics
            .iter()
            .map(PythonDiagnostic::from_ir)
            .collect::<Vec<_>>()
    })
    .map_err(|err| vec![PythonDiagnostic::new("E_CHECK", "Error", err.to_string())])
}

pub fn convert_cobol_project_for_python(
    source: &str,
    dialect: &str,
    options: PythonConvertOptions,
    out_dir: &Path,
) -> Result<PythonGeneratedProject, DiagnosticList> {
    let dialect = parse_dialect_for_python(dialect)?;
    let source_format = parse_source_format_for_diagnostics(options.source_format.as_deref())?;

    let temp = TempDir::new().map_err(|err| {
        vec![PythonDiagnostic::new(
            "E_IO",
            "Error",
            format!("failed to create temporary conversion directory: {err}"),
        )]
    })?;
    let input = temp.path().join("input.cbl");
    let copybook_dir = temp.path().join("copybooks");
    fs::create_dir_all(&copybook_dir)
        .and_then(|_| fs::write(&input, source))
        .map_err(|err| vec![PythonDiagnostic::new("E_IO", "Error", err.to_string())])?;
    write_copybooks(&copybook_dir, &options.copybooks).map_err(|err| {
        vec![PythonDiagnostic::new(
            "E_COPYBOOK_MATERIALIZE",
            "Error",
            err.to_string(),
        )]
    })?;

    match convert(ConvertOptions {
        input,
        copybook_dirs: vec![copybook_dir],
        out_dir: out_dir.to_path_buf(),
        dialect,
        source_format,
    }) {
        Ok(project) => Ok(PythonGeneratedProject {
            out_dir: project.out_dir,
            generated_files: project
                .files
                .iter()
                .map(|path| path.to_string_lossy().replace('\\', "/"))
                .collect(),
            report_path: project.report_path,
        }),
        Err(ConvertError::MigrationBlocked { report_path }) => {
            Err(diagnostics_from_report(&report_path))
        }
        Err(err) => Err(vec![PythonDiagnostic::new(
            "E_CONVERT",
            "Error",
            err.to_string(),
        )]),
    }
}

pub fn batch_convert_sources_for_python(
    sources: &[(String, String)],
    dialect: &str,
    options: PythonConvertOptions,
    out_dir: &Path,
) -> BatchConversionSummary {
    let mut projects = Vec::new();
    let mut seen_outputs = BTreeMap::new();
    for (input, source) in sources {
        let project_dir = out_dir.join(project_dir_name(input));
        if let Some(previous_input) = seen_outputs.insert(project_dir.clone(), input.clone()) {
            projects.push(BatchProjectResult {
                input: input.clone(),
                out_dir: project_dir,
                status: "failed".to_string(),
                diagnostics: vec![PythonDiagnostic::new(
                    "E_BATCH_OUTPUT_COLLISION",
                    "Error",
                    format!(
                        "batch output directory collides with {previous_input}; rename one source or choose distinct output roots"
                    ),
                )],
            });
            continue;
        }
        match convert_cobol_project_for_python(source, dialect, options.clone(), &project_dir) {
            Ok(_) => projects.push(BatchProjectResult {
                input: input.clone(),
                out_dir: project_dir,
                status: "generated".to_string(),
                diagnostics: Vec::new(),
            }),
            Err(diagnostics) => projects.push(BatchProjectResult {
                input: input.clone(),
                out_dir: project_dir,
                status: "blocked".to_string(),
                diagnostics,
            }),
        }
    }
    let generated = projects
        .iter()
        .filter(|project| project.status == "generated")
        .count();
    let blocked = projects
        .iter()
        .filter(|project| project.status == "blocked")
        .count();
    let failures = projects
        .iter()
        .filter(|project| project.status == "failed")
        .count();
    BatchConversionSummary {
        total: sources.len(),
        generated,
        blocked,
        failures,
        projects,
    }
}

pub fn analyze_source_for_python(path: &str, source: &str) -> SourceAnalysis {
    let program_id = extract_program_id(source);
    let fallback_name = Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(path)
        .to_string();
    let graph_name = program_id.clone().unwrap_or(fallback_name);
    let copybooks = extract_copybooks_from_source(source);
    let calls = extract_calls_from_source(source);
    let unsupported_features = extract_unsupported_feature_advice(path, source);

    SourceAnalysis {
        path: path.to_string(),
        program_id: Some(graph_name),
        copybooks,
        calls,
        unsupported_features,
    }
}

impl SourceAnalysis {
    pub fn to_dot(&self) -> String {
        let program = self.program_id.as_deref().unwrap_or(&self.path);
        let mut text = String::from("digraph cobol_dependencies {\n");
        text.push_str("  rankdir=LR;\n");
        text.push_str(&format!("  \"{}\";\n", dot_escape(program)));
        for copybook in &self.copybooks {
            text.push_str(&format!(
                "  \"{}\" -> \"copybook:{}\" [label=\"COPY\"];\n",
                dot_escape(program),
                dot_escape(copybook)
            ));
        }
        for call in &self.calls {
            let label = if call.literal { "CALL" } else { "CALL dynamic" };
            text.push_str(&format!(
                "  \"{}\" -> \"{}\" [label=\"{}\"];\n",
                dot_escape(program),
                dot_escape(&call.target),
                label
            ));
        }
        text.push_str("}\n");
        text
    }
}

fn parse_source_format(value: &str) -> Result<SourceFormat, PythonBindingError> {
    SourceFormat::parse(value)
        .ok_or_else(|| PythonBindingError::InvalidSourceFormat(value.to_string()))
}

fn parse_dialect_for_python(dialect: &str) -> Result<Dialect, DiagnosticList> {
    Dialect::parse(dialect).ok_or_else(|| {
        vec![PythonDiagnostic::new(
            "E_INVALID_DIALECT",
            "Error",
            format!("invalid dialect {dialect}"),
        )]
    })
}

fn convert_cobol_failure_payload(diagnostics: &[PythonDiagnostic]) -> serde_json::Result<Value> {
    let diagnostics_json = serde_json::to_string(diagnostics)?;
    Ok(serde_json::json!({
        "ok": false,
        "rust": null,
        "diagnostics": diagnostics,
        "diagnostics_json": diagnostics_json,
    }))
}

fn parse_source_format_for_diagnostics(
    source_format: Option<&str>,
) -> Result<SourceFormat, DiagnosticList> {
    let source_format = source_format.unwrap_or("auto");
    parse_source_format(source_format).map_err(|err| {
        vec![PythonDiagnostic::new(
            "E_INVALID_SOURCE_FORMAT",
            "Error",
            err.to_string(),
        )]
    })
}

fn write_copybooks(
    copybook_dir: &Path,
    copybooks: &[(String, String)],
) -> Result<(), PythonBindingError> {
    for (name, contents) in copybooks {
        let relative = validate_copybook_name(name)?;
        let path = copybook_dir.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| PythonBindingError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        fs::write(&path, contents).map_err(|source| PythonBindingError::Io {
            path: path.clone(),
            source,
        })?;
    }
    Ok(())
}

fn validate_copybook_name(name: &str) -> Result<PathBuf, PythonBindingError> {
    let path = Path::new(name);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(PythonBindingError::InvalidCopybookName(name.to_string()));
    }
    Ok(path.to_path_buf())
}

fn diagnostics_from_report(path: &Path) -> DiagnosticList {
    let Ok(text) = fs::read_to_string(path) else {
        return vec![PythonDiagnostic::new(
            "E_MIGRATION_BLOCKED",
            "Error",
            format!(
                "migration blocked; report was not readable at {}",
                path.display()
            ),
        )];
    };
    let Ok(report) = serde_json::from_str::<Value>(&text) else {
        return vec![PythonDiagnostic::new(
            "E_MIGRATION_BLOCKED",
            "Error",
            format!(
                "migration blocked; report was not valid JSON at {}",
                path.display()
            ),
        )];
    };
    let diagnostics = report
        .get("diagnostics")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(PythonDiagnostic::from_json)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if diagnostics.is_empty() {
        vec![PythonDiagnostic::new(
            "E_MIGRATION_BLOCKED",
            "Error",
            "migration blocked without structured diagnostics".to_string(),
        )]
    } else {
        diagnostics
    }
}

impl PythonDiagnostic {
    fn new(
        code: impl Into<String>,
        severity: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        let code = code.into();
        Self {
            help: python_diagnostic_default_help(&code).map(ToString::to_string),
            code,
            severity: severity.into(),
            message: message.into(),
            file: None,
            line: None,
            column: None,
        }
    }

    fn from_json(value: &Value) -> Self {
        let span = value.get("span").unwrap_or(&Value::Null);
        let code = value
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("E_UNKNOWN")
            .to_string();
        let help = value
            .get("help")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .or_else(|| python_diagnostic_default_help(&code).map(ToString::to_string));
        Self {
            code,
            severity: value
                .get("severity")
                .and_then(Value::as_str)
                .unwrap_or("Error")
                .to_string(),
            message: value
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("converter diagnostic")
                .to_string(),
            file: span
                .get("file")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            line: span
                .get("line")
                .and_then(Value::as_u64)
                .and_then(|line| usize::try_from(line).ok()),
            column: span
                .get("column")
                .and_then(Value::as_u64)
                .and_then(|column| usize::try_from(column).ok()),
            help,
        }
    }

    fn from_ir(diagnostic: &Diagnostic) -> Self {
        let help = diagnostic
            .help
            .clone()
            .or_else(|| python_diagnostic_default_help(&diagnostic.code).map(ToString::to_string));
        Self {
            code: diagnostic.code.clone(),
            severity: format!("{:?}", diagnostic.severity),
            message: diagnostic.message.clone(),
            file: Some(diagnostic.span.file.clone()),
            line: Some(diagnostic.span.line),
            column: Some(diagnostic.span.column),
            help,
        }
    }
}

fn python_diagnostic_default_help(code: &str) -> Option<&'static str> {
    match code {
        "E_INVALID_DIALECT" => Some(
            "Workaround: use a supported dialect alias such as ibm, ibm_zos, micro_focus, or gnucobol.",
        ),
        "E_INVALID_SOURCE_FORMAT" => Some(
            "Workaround: use source_format auto, fixed, or free, matching the COBOL source layout.",
        ),
        "E_IO" => Some(
            "Workaround: verify temporary directory, input path, output path, and filesystem permissions, then retry.",
        ),
        "E_COPYBOOK_MATERIALIZE" => Some(
            "Workaround: provide copybook names and contents that can be written into the temporary copybook directory.",
        ),
        "E_READ_GENERATED_PROGRAM" => Some(
            "Workaround: inspect the generated project directory and rerun conversion after resolving filesystem or generation errors.",
        ),
        "E_CONVERT" => Some(
            "Workaround: inspect the converter error and migration-report diagnostics, then simplify unsupported COBOL or adjust conversion options.",
        ),
        "E_CHECK" => Some(
            "Workaround: inspect the check error, source format, dialect, and copybook options before rerunning validation.",
        ),
        "E_MIGRATION_BLOCKED" => Some(
            "Workaround: inspect migration-report.json diagnostics and address each blocker before requesting generated Rust.",
        ),
        "E_UNKNOWN" => Some(
            "Workaround: inspect the raw diagnostic payload and rerun with migration-report JSON enabled to preserve structured details.",
        ),
        _ => None,
    }
}

fn extract_program_id(source: &str) -> Option<String> {
    for line in source.lines() {
        let words = words_outside_literals(line);
        for idx in 0..words.len().saturating_sub(1) {
            if words[idx].eq_ignore_ascii_case("PROGRAM-ID") {
                return Some(clean_cobol_token(&words[idx + 1]));
            }
        }
    }
    None
}

fn extract_copybooks_from_source(source: &str) -> Vec<String> {
    let mut copybooks = BTreeSet::new();
    for line in source.lines() {
        let words = words_outside_literals(line);
        for idx in 0..words.len().saturating_sub(1) {
            if words[idx].eq_ignore_ascii_case("COPY") {
                copybooks.insert(clean_cobol_token(&words[idx + 1]));
            }
        }
    }
    copybooks.into_iter().collect()
}

fn extract_calls_from_source(source: &str) -> Vec<CallDependency> {
    let mut calls = Vec::new();
    for (line_idx, line) in source.lines().enumerate() {
        let words = words_preserving_literals(line);
        for idx in 0..words.len().saturating_sub(1) {
            if words[idx].eq_ignore_ascii_case("CALL") {
                let raw = words[idx + 1].clone();
                calls.push(CallDependency {
                    target: clean_cobol_token(&raw),
                    literal: is_quoted(&raw),
                    line: line_idx + 1,
                });
            }
        }
    }
    calls
}

fn extract_unsupported_feature_advice(path: &str, source: &str) -> Vec<UnsupportedFeatureAdvice> {
    let mut findings = BTreeMap::<&'static str, BTreeSet<String>>::new();
    let mut current_paragraph = None;
    for line in source.lines() {
        let words = words_outside_literals(line);
        if words.len() == 1 && words[0].ends_with('.') && !reserved_paragraph_word(&words[0]) {
            current_paragraph = Some(clean_cobol_token(&words[0]));
        }
        let paragraph = current_paragraph
            .clone()
            .unwrap_or_else(|| "<unknown>".to_string());
        if words.iter().any(|word| word.eq_ignore_ascii_case("ALTER")) {
            findings
                .entry("procedure.alter")
                .or_default()
                .insert(paragraph.clone());
        }
        if has_word_sequence(&words, &["NEXT", "SENTENCE"]) {
            findings
                .entry("procedure.next-sentence")
                .or_default()
                .insert(paragraph.clone());
        }
        if has_word_sequence(&words, &["SEARCH", "ALL"]) {
            findings
                .entry("procedure.search-all")
                .or_default()
                .insert(paragraph);
        }
    }
    findings
        .into_iter()
        .filter_map(|(capability_id, paragraphs)| {
            capability_info(capability_id).map(|capability| {
                let paragraphs = paragraphs.into_iter().collect::<Vec<_>>();
                let paragraph_text = paragraphs.join(", ");
                let advice = capability_advice(path, &capability, &paragraph_text);
                UnsupportedFeatureAdvice {
                    feature: capability.name,
                    capability_id: capability.id,
                    status: capability.status,
                    paragraphs,
                    advice,
                }
            })
        })
        .collect()
}

#[derive(Debug, Clone)]
struct CapabilityInfo {
    id: String,
    name: String,
    status: String,
    notes: String,
}

fn capability_info(id: &str) -> Option<CapabilityInfo> {
    let matrix: Value =
        serde_json::from_str(include_str!("../../../migration-capability-matrix.json")).ok()?;
    matrix
        .get("features")?
        .as_array()?
        .iter()
        .find(|feature| feature.get("id").and_then(Value::as_str) == Some(id))
        .map(|feature| CapabilityInfo {
            id: id.to_string(),
            name: feature
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(id)
                .to_string(),
            status: feature
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            notes: feature
                .get("notes")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        })
}

fn capability_advice(path: &str, capability: &CapabilityInfo, paragraphs: &str) -> String {
    let action = match capability.id.as_str() {
        "procedure.alter" => "these must be refactored or compiled in ABYSS mode",
        "procedure.next-sentence" => {
            "rewrite period-scope control flow before generated Rust execution"
        }
        "procedure.search-all" => {
            "verify the SEARCH ALL shape is within the supported keyed equality subset"
        }
        _ => "review this feature before migration",
    };
    format!(
        "{path} uses {} in paragraphs {paragraphs} - {action}. Capability {} is {} in migration-capability-matrix.json: {}",
        capability.name, capability.id, capability.status, capability.notes
    )
}

fn has_word_sequence(words: &[String], sequence: &[&str]) -> bool {
    words.windows(sequence.len()).any(|window| {
        window
            .iter()
            .zip(sequence.iter())
            .all(|(word, expected)| clean_cobol_token(word).eq_ignore_ascii_case(expected))
    })
}

fn words_outside_literals(line: &str) -> Vec<String> {
    words_preserving_literals(line)
        .into_iter()
        .filter(|word| !is_quoted(word))
        .collect()
}

fn words_preserving_literals(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if let Some(active) = quote {
            current.push(ch);
            if ch == active {
                if chars.peek().is_some_and(|next| *next == ch) {
                    current.push(chars.next().expect("peeked char"));
                } else {
                    quote = None;
                }
            }
            continue;
        }
        if matches!(ch, '"' | '\'') {
            if !current.is_empty() {
                words.push(current.trim().to_string());
                current.clear();
            }
            current.push(ch);
            quote = Some(ch);
        } else if ch.is_whitespace() || ch == ',' {
            if !current.is_empty() {
                words.push(current.trim().to_string());
                current.clear();
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        words.push(current.trim().to_string());
    }
    words
}

fn clean_cobol_token(value: &str) -> String {
    value
        .trim()
        .trim_end_matches('.')
        .trim_matches('"')
        .trim_matches('\'')
        .to_string()
}

fn is_quoted(value: &str) -> bool {
    let trimmed = value.trim().trim_end_matches('.');
    (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
}

fn reserved_paragraph_word(value: &str) -> bool {
    matches!(
        clean_cobol_token(value).to_ascii_uppercase().as_str(),
        "IDENTIFICATION"
            | "ENVIRONMENT"
            | "DATA"
            | "PROCEDURE"
            | "DIVISION"
            | "SECTION"
            | "WORKING-STORAGE"
            | "LINKAGE"
            | "FILE"
    )
}

fn dot_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn project_dir_name(input: &str) -> String {
    let stem = Path::new(input)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(input);
    let mut out = stem
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
        "program".to_string()
    } else {
        out
    }
}

#[cfg(feature = "extension-module")]
mod python_api {
    use super::*;
    use pyo3::exceptions::{PyRuntimeError, PyValueError};
    use pyo3::prelude::*;
    use pyo3::types::{PyDict, PyList};

    #[pyfunction]
    #[pyo3(signature = (source, copybooks=None, source_format="auto"))]
    fn preprocess(
        source: &str,
        copybooks: Option<&Bound<'_, PyDict>>,
        source_format: &str,
    ) -> PyResult<String> {
        let copybooks = extract_copybooks(copybooks)?;
        preprocess_source_for_python(source, &copybooks, source_format)
            .map_err(|err| PyValueError::new_err(err.to_string()))
    }

    #[pyfunction]
    #[pyo3(signature = (source, dialect, options=None))]
    fn convert_cobol(
        py: Python<'_>,
        source: &str,
        dialect: &str,
        options: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyObject> {
        let options = extract_options(options)?;
        match convert_cobol_source_for_python(source, dialect, options) {
            Ok(rust) => {
                let result = PyDict::new_bound(py);
                result.set_item("ok", true)?;
                result.set_item("rust", rust)?;
                result.set_item("diagnostics", Vec::<String>::new())?;
                Ok(result.into())
            }
            Err(diagnostics) => {
                let payload = convert_cobol_failure_payload(&diagnostics)
                    .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
                let result = PyDict::new_bound(py);
                result.set_item("ok", false)?;
                result.set_item("rust", py.None())?;
                result.set_item("diagnostics", diagnostics_to_pylist(py, &diagnostics)?)?;
                let diagnostics_json = payload["diagnostics_json"].as_str().ok_or_else(|| {
                    PyRuntimeError::new_err("diagnostics_json missing from failure payload")
                })?;
                result.set_item("diagnostics_json", diagnostics_json)?;
                Ok(result.into())
            }
        }
    }

    #[pyfunction]
    #[pyo3(signature = (source, dialect, options=None))]
    fn check_cobol(
        py: Python<'_>,
        source: &str,
        dialect: &str,
        options: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyObject> {
        let options = extract_options(options)?;
        match check_cobol_source_for_python(source, dialect, options) {
            Ok(diagnostics) => {
                let diagnostics_json = serde_json::to_string(&diagnostics)
                    .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
                let result = PyDict::new_bound(py);
                result.set_item("ok", diagnostics.is_empty())?;
                result.set_item("diagnostics", diagnostics_to_pylist(py, &diagnostics)?)?;
                result.set_item("diagnostics_json", diagnostics_json)?;
                Ok(result.into())
            }
            Err(diagnostics) => {
                let diagnostics_json = serde_json::to_string(&diagnostics)
                    .map_err(|err| PyRuntimeError::new_err(err.to_string()))?;
                let result = PyDict::new_bound(py);
                result.set_item("ok", false)?;
                result.set_item("diagnostics", diagnostics_to_pylist(py, &diagnostics)?)?;
                result.set_item("diagnostics_json", diagnostics_json)?;
                Ok(result.into())
            }
        }
    }

    #[pyfunction]
    fn analyze_source(path: &str, source: &str) -> PyResult<String> {
        serde_json::to_string_pretty(&analyze_source_for_python(path, source))
            .map_err(|err| PyRuntimeError::new_err(err.to_string()))
    }

    #[pyfunction]
    fn dependency_graph_dot(path: &str, source: &str) -> PyResult<String> {
        Ok(analyze_source_for_python(path, source).to_dot())
    }

    #[pyfunction]
    #[pyo3(signature = (source, dialect, output_dir, options=None))]
    fn convert_project(
        source: &str,
        dialect: &str,
        output_dir: &str,
        options: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<String> {
        let options = extract_options(options)?;
        match convert_cobol_project_for_python(source, dialect, options, Path::new(output_dir)) {
            Ok(project) => serde_json::to_string_pretty(&project)
                .map_err(|err| PyRuntimeError::new_err(err.to_string())),
            Err(diagnostics) => serde_json::to_string_pretty(&serde_json::json!({
                "out_dir": output_dir,
                "generated_files": [],
                "report_path": null,
                "diagnostics": diagnostics,
            }))
            .map_err(|err| PyRuntimeError::new_err(err.to_string())),
        }
    }

    #[pyfunction]
    #[pyo3(signature = (sources, dialect, output_dir, options=None))]
    fn batch_convert_sources(
        sources: &Bound<'_, PyDict>,
        dialect: &str,
        output_dir: &str,
        options: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<String> {
        let sources = sources
            .iter()
            .map(|(key, value)| Ok((key.extract::<String>()?, value.extract::<String>()?)))
            .collect::<PyResult<Vec<_>>>()?;
        let options = extract_options(options)?;
        serde_json::to_string_pretty(&batch_convert_sources_for_python(
            &sources,
            dialect,
            options,
            Path::new(output_dir),
        ))
        .map_err(|err| PyRuntimeError::new_err(err.to_string()))
    }

    #[pymodule]
    fn _native(_py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
        module.add_function(wrap_pyfunction!(preprocess, module)?)?;
        module.add_function(wrap_pyfunction!(convert_cobol, module)?)?;
        module.add_function(wrap_pyfunction!(check_cobol, module)?)?;
        module.add_function(wrap_pyfunction!(analyze_source, module)?)?;
        module.add_function(wrap_pyfunction!(dependency_graph_dot, module)?)?;
        module.add_function(wrap_pyfunction!(convert_project, module)?)?;
        module.add_function(wrap_pyfunction!(batch_convert_sources, module)?)?;
        Ok(())
    }

    fn extract_options(options: Option<&Bound<'_, PyDict>>) -> PyResult<PythonConvertOptions> {
        let Some(options) = options else {
            return Ok(PythonConvertOptions::default());
        };
        let source_format = options
            .get_item("source_format")?
            .map(|value| value.extract::<String>())
            .transpose()?;
        let copybooks = options
            .get_item("copybooks")?
            .map(|value| value.downcast_into::<PyDict>())
            .transpose()?;
        Ok(PythonConvertOptions {
            copybooks: extract_copybooks(copybooks.as_ref())?,
            source_format,
        })
    }

    fn extract_copybooks(copybooks: Option<&Bound<'_, PyDict>>) -> PyResult<Vec<(String, String)>> {
        let Some(copybooks) = copybooks else {
            return Ok(Vec::new());
        };
        copybooks
            .iter()
            .map(|(key, value)| Ok((key.extract::<String>()?, value.extract::<String>()?)))
            .collect()
    }

    fn diagnostics_to_pylist<'py>(
        py: Python<'py>,
        diagnostics: &[PythonDiagnostic],
    ) -> PyResult<Bound<'py, PyList>> {
        let list = PyList::empty_bound(py);
        for diagnostic in diagnostics {
            let item = PyDict::new_bound(py);
            item.set_item("code", &diagnostic.code)?;
            item.set_item("severity", &diagnostic.severity)?;
            item.set_item("message", &diagnostic.message)?;
            item.set_item("file", diagnostic.file.as_deref())?;
            item.set_item("line", diagnostic.line)?;
            item.set_item("column", diagnostic.column)?;
            list.append(item)?;
        }
        Ok(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preprocess_expands_copybooks_from_memory() {
        let source = "IDENTIFICATION DIVISION.\nPROGRAM-ID. T.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\nCOPY FIELDS.\nPROCEDURE DIVISION.\nMAIN.\nSTOP RUN.\n";
        let copybooks = vec![(
            "FIELDS.cpy".to_string(),
            "01 WS-FLAG PIC X VALUE \"Y\".\n".to_string(),
        )];

        let preprocessed = preprocess_source_for_python(source, &copybooks, "free").unwrap();

        assert!(preprocessed.contains("01 WS-FLAG PIC X VALUE \"Y\"."));
        assert!(!preprocessed.contains("COPY FIELDS"));
    }

    #[test]
    fn convert_cobol_source_returns_generated_program_rs() {
        let source = "IDENTIFICATION DIVISION.\nPROGRAM-ID. PYHELLO.\nPROCEDURE DIVISION.\nMAIN.\nDISPLAY \"PY\".\nSTOP RUN.\n";

        let generated =
            convert_cobol_source_for_python(source, "ibm", PythonConvertOptions::default())
                .expect("convert source");

        assert!(generated.contains("pub struct Program"));
        assert!(generated.contains("VmProcedureOp::Display"));
        assert!(generated.contains("\"PY\""));
    }

    #[test]
    fn convert_cobol_source_accepts_documented_ibm_zos_dialect_alias() {
        let source = "IDENTIFICATION DIVISION.\nPROGRAM-ID. PYHELLO.\nPROCEDURE DIVISION.\nMAIN.\nDISPLAY \"PY\".\nSTOP RUN.\n";

        let generated =
            convert_cobol_source_for_python(source, "ibm_zos", PythonConvertOptions::default())
                .expect("convert source with ibm_zos alias");

        assert!(generated.contains("cobol_dialect::DialectName::IbmZos"));
        assert!(generated.contains("cobol_dialect::CollatingSequence::Ebcdic"));
    }

    #[test]
    fn check_cobol_source_validates_without_generating_rust() {
        let source = "IDENTIFICATION DIVISION.\nPROGRAM-ID. PYCHECK.\nPROCEDURE DIVISION.\nMAIN.\nDISPLAY \"PY\".\nSTOP RUN.\n";

        let diagnostics =
            check_cobol_source_for_python(source, "ibm_zos", PythonConvertOptions::default())
                .expect("check source");

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn check_cobol_source_returns_structured_diagnostics() {
        let diagnostics = check_cobol_source_for_python("", "ibm", PythonConvertOptions::default())
            .expect("check source");

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_SYNTAX_EMPTY_PROGRAM"
                && diagnostic.severity == "Error"
                && diagnostic.line == Some(1)
                && diagnostic.column == Some(1)
                && diagnostic
                    .help
                    .as_deref()
                    .is_some_and(|help| help.contains("Workaround"))
        }));
    }

    #[test]
    fn convert_cobol_failure_payload_includes_structured_diagnostics() {
        let diagnostic = PythonDiagnostic::new(
            "E_INVALID_DIALECT",
            "Error",
            "invalid dialect not_a_dialect",
        );
        let diagnostics = vec![diagnostic];

        let payload = convert_cobol_failure_payload(&diagnostics).expect("payload");

        assert_eq!(payload["ok"], false);
        assert!(payload["rust"].is_null());
        assert_eq!(payload["diagnostics"][0]["code"], "E_INVALID_DIALECT");
        assert!(payload["diagnostics"][0]["help"]
            .as_str()
            .is_some_and(|help| help.contains("supported dialect alias")));
        assert!(payload["diagnostics_json"]
            .as_str()
            .expect("diagnostics_json string")
            .contains("supported dialect alias"));
    }

    #[test]
    fn wrapper_diagnostics_include_default_help() {
        let diagnostics =
            convert_cobol_source_for_python("", "not_a_dialect", PythonConvertOptions::default())
                .expect_err("invalid dialect should fail");

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "E_INVALID_DIALECT"
                && diagnostic
                    .help
                    .as_deref()
                    .is_some_and(|help| help.contains("Workaround"))
        }));
    }

    #[test]
    fn analysis_extracts_copybooks_calls_and_dot_graph() {
        let source = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLMAIN.
DATA DIVISION.
WORKING-STORAGE SECTION.
COPY CUSTOMER.
01 WS-PROG PIC X(7) VALUE "SUBPROG".
PROCEDURE DIVISION.
MAIN.
    CALL "SUBPROG".
    CALL WS-PROG.
    STOP RUN.
"#;

        let analysis = analyze_source_for_python("src/CALLMAIN.cbl", source);

        assert_eq!(analysis.program_id.as_deref(), Some("CALLMAIN"));
        assert_eq!(analysis.copybooks, vec!["CUSTOMER"]);
        assert!(analysis
            .calls
            .iter()
            .any(|call| call.target == "SUBPROG" && call.literal));
        assert!(analysis
            .calls
            .iter()
            .any(|call| call.target == "WS-PROG" && !call.literal));
        assert!(analysis.to_dot().contains("\"CALLMAIN\" -> \"SUBPROG\""));
        assert!(analysis
            .to_dot()
            .contains("\"CALLMAIN\" -> \"copybook:CUSTOMER\""));
    }

    #[test]
    fn analysis_reports_alter_refactoring_advice_with_paragraphs() {
        let source = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. ALTERME.
PROCEDURE DIVISION.
P1.
    ALTER P2 TO PROCEED TO P3.
P2.
    GO TO .
P3.
    STOP RUN.
"#;

        let analysis = analyze_source_for_python("ALTERME.cbl", source);

        assert_eq!(analysis.unsupported_features.len(), 1);
        let feature = &analysis.unsupported_features[0];
        assert_eq!(feature.feature, "ALTER");
        assert_eq!(feature.paragraphs, vec!["P1"]);
        assert!(feature
            .advice
            .contains("ALTERME.cbl uses ALTER in paragraphs P1"));
        assert!(feature
            .advice
            .contains("refactored or compiled in ABYSS mode"));
    }

    #[test]
    fn analysis_reports_capability_matrix_advice_for_blocked_and_partial_features() {
        let source = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. ADVISEME.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-IDX PIC 9 VALUE 1.
PROCEDURE DIVISION.
P1.
    ALTER P2 TO PROCEED TO P3.
    IF WS-IDX = 1 NEXT SENTENCE.
P2.
    SEARCH ALL WS-TABLE AT END DISPLAY "MISS" WHEN WS-IDX = 1 DISPLAY "HIT".
P3.
    STOP RUN.
"#;

        let analysis = analyze_source_for_python("ADVISEME.cbl", source);
        let features = analysis
            .unsupported_features
            .iter()
            .map(|feature| {
                (
                    feature.feature.as_str(),
                    feature.capability_id.as_str(),
                    feature.status.as_str(),
                )
            })
            .collect::<Vec<_>>();

        assert!(features.contains(&("ALTER", "procedure.alter", "partial")));
        assert!(features.contains(&("NEXT SENTENCE", "procedure.next-sentence", "blocked")));
        assert!(features.contains(&("SEARCH ALL", "procedure.search-all", "partial")));
        assert!(analysis
            .unsupported_features
            .iter()
            .any(|feature| feature.advice.contains("migration-capability-matrix.json")));
    }

    #[test]
    fn convert_cobol_project_writes_complete_generated_project() {
        let dir = TempDir::new().expect("tempdir");
        let out_dir = dir.path().join("project");
        let source = "IDENTIFICATION DIVISION.\nPROGRAM-ID. PYPROJ.\nPROCEDURE DIVISION.\nMAIN.\nDISPLAY \"PROJECT\".\nSTOP RUN.\n";

        let project = convert_cobol_project_for_python(
            source,
            "ibm",
            PythonConvertOptions {
                source_format: Some("free".to_string()),
                ..PythonConvertOptions::default()
            },
            &out_dir,
        )
        .expect("convert project");

        assert_eq!(project.out_dir, out_dir);
        assert!(out_dir.join("Cargo.toml").exists());
        assert!(out_dir.join("src").join("program.rs").exists());
        assert!(project
            .generated_files
            .iter()
            .any(|path| path.ends_with("src/program.rs")));
    }

    #[test]
    fn convert_cobol_project_refuses_unowned_existing_project() {
        let dir = TempDir::new().expect("tempdir");
        let out_dir = dir.path().join("project");
        fs::create_dir_all(out_dir.join("src")).expect("src dir");
        fs::write(out_dir.join("src").join("main.rs"), "user main").expect("user main");
        fs::write(out_dir.join("Cargo.toml"), "user manifest").expect("user manifest");
        let source = "IDENTIFICATION DIVISION.\nPROGRAM-ID. PYPROJ.\nPROCEDURE DIVISION.\nMAIN.\nDISPLAY \"PROJECT\".\nSTOP RUN.\n";

        let diagnostics = convert_cobol_project_for_python(
            source,
            "ibm",
            PythonConvertOptions {
                source_format: Some("free".to_string()),
                ..PythonConvertOptions::default()
            },
            &out_dir,
        )
        .expect_err("unowned project should be protected");

        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E_CONVERT"
                && diagnostic.message.contains("missing converter sentinel")));
        assert_eq!(
            fs::read_to_string(out_dir.join("src").join("main.rs")).expect("user main"),
            "user main"
        );
        assert_eq!(
            fs::read_to_string(out_dir.join("Cargo.toml")).expect("user manifest"),
            "user manifest"
        );
        assert!(!out_dir.join(".cobol2rust-generated").exists());
        assert!(!out_dir.join("migration-report.json").exists());
    }

    #[test]
    fn batch_convert_sources_writes_project_per_input_and_summarizes_results() {
        let dir = TempDir::new().expect("tempdir");
        let out_dir = dir.path().join("batch");
        let sources = vec![
            (
                "one.cbl".to_string(),
                "IDENTIFICATION DIVISION.\nPROGRAM-ID. ONE.\nPROCEDURE DIVISION.\nMAIN.\nDISPLAY \"ONE\".\nSTOP RUN.\n".to_string(),
            ),
            (
                "two.cbl".to_string(),
                "IDENTIFICATION DIVISION.\nPROGRAM-ID. TWO.\nPROCEDURE DIVISION.\nMAIN.\nDISPLAY \"TWO\".\nSTOP RUN.\n".to_string(),
            ),
        ];

        let summary = batch_convert_sources_for_python(
            &sources,
            "ibm",
            PythonConvertOptions {
                source_format: Some("free".to_string()),
                ..PythonConvertOptions::default()
            },
            &out_dir,
        );

        assert_eq!(summary.total, 2);
        assert_eq!(summary.generated, 2);
        assert_eq!(summary.blocked, 0);
        assert!(out_dir.join("one").join("Cargo.toml").exists());
        assert!(out_dir.join("two").join("src").join("program.rs").exists());
    }

    #[test]
    fn batch_convert_sources_reports_normalized_output_collisions() {
        let dir = TempDir::new().expect("tempdir");
        let out_dir = dir.path().join("batch");
        let sources = vec![
            (
                "FOO-BAR.cbl".to_string(),
                "IDENTIFICATION DIVISION.\nPROGRAM-ID. FIRST.\nPROCEDURE DIVISION.\nMAIN.\nDISPLAY \"FIRST\".\nSTOP RUN.\n".to_string(),
            ),
            (
                "foo_bar.cbl".to_string(),
                "IDENTIFICATION DIVISION.\nPROGRAM-ID. SECOND.\nPROCEDURE DIVISION.\nMAIN.\nDISPLAY \"SECOND\".\nSTOP RUN.\n".to_string(),
            ),
        ];

        let summary = batch_convert_sources_for_python(
            &sources,
            "ibm",
            PythonConvertOptions {
                source_format: Some("free".to_string()),
                ..PythonConvertOptions::default()
            },
            &out_dir,
        );

        assert_eq!(summary.total, 2);
        assert_eq!(summary.generated, 1);
        assert_eq!(summary.failures, 1);
        assert_eq!(summary.projects[1].status, "failed");
        assert_eq!(
            summary.projects[1].diagnostics[0].code,
            "E_BATCH_OUTPUT_COLLISION"
        );
    }
}
