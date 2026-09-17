use cobol_ir::{DataItemIr, Diagnostic, OperandIr, ProgramIr, Severity, SourceSpan, StatementIr};
use cobol_sema::{analyze, Dialect as SemaDialect};
use cobol_source::{preprocess_file, PreprocessedSource, SourceError};
pub use cobol_source::{Dialect, SourceFormat};
use cobol_syntax::{parse_program, SyntaxError};
use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

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
    pub stats: ReportStats,
}

#[derive(Debug, Serialize)]
pub struct ReportStats {
    pub data_items: usize,
    pub paragraphs: usize,
    pub statements: usize,
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
    let mut ir = parse_and_analyze(&preprocessed, options.dialect)?;
    add_codegen_support_diagnostics(&mut ir);

    let report_path = options.out_dir.join("migration-report.json");
    if ir.has_errors() {
        write_report(
            &report_path,
            &build_report("blocked", &options, &preprocessed, &ir, &[]),
        )?;
        return Err(ConvertError::MigrationBlocked { report_path });
    }

    let generated = write_generated_project(&options.out_dir, &ir)?;
    write_report(
        &report_path,
        &build_report("generated", &options, &preprocessed, &ir, &generated),
    )?;
    Ok(GeneratedProject {
        out_dir: options.out_dir,
        files: generated,
        report_path,
    })
}

fn parse_and_analyze(
    preprocessed: &PreprocessedSource,
    dialect: Dialect,
) -> Result<ProgramIr, ConvertError> {
    let source_name = preprocessed.primary_path.to_string_lossy();
    let ast = parse_program(&source_name, &preprocessed.text)?;
    let dialect = match dialect {
        Dialect::Ibm => SemaDialect::Ibm,
        Dialect::GnuCobol => SemaDialect::GnuCobol,
        Dialect::MicroFocus => SemaDialect::MicroFocus,
    };
    let mut ir = analyze(ast, dialect);
    ir.diagnostics
        .extend(preflight_diagnostics(&preprocessed.text, &source_name));
    Ok(ir)
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
            &["FILE", "SECTION"][..],
            &["LOCAL-STORAGE", "SECTION"],
            &["SCREEN", "SECTION"],
            &["REPORT", "SECTION"],
            &["COMMUNICATION", "SECTION"],
            &["DECLARATIVES"],
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
            for word in ["SELECT", "FD", "SD", "FILE-CONTROL", "I-O-CONTROL"] {
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
        }

        if area == CobolArea::Data {
            for word in [
                "REDEFINES",
                "RENAMES",
                "OCCURS",
                "DEPENDING",
                "INDEXED",
                "USAGE",
                "COMP",
                "COMP-1",
                "COMP-2",
                "COMP-3",
                "COMP-4",
                "COMP-5",
                "BINARY",
                "PACKED-DECIMAL",
                "VALUE",
                "VALUES",
                "SIGN",
                "SYNCHRONIZED",
                "SYNC",
                "JUSTIFIED",
                "BLANK",
                "EXTERNAL",
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
        }

        if area == CobolArea::Procedure {
            if has_word(&words, "PERFORM")
                && (has_word(&words, "VARYING")
                    || has_word(&words, "UNTIL")
                    || has_word(&words, "TIMES"))
            {
                diagnostics.push(Diagnostic::error(
                    "E_UNSUPPORTED_PERFORM_FORM",
                    "complex PERFORM forms are not lowered by the converter preview",
                    span.clone(),
                ));
            }

            for word in [
                "ACCEPT",
                "ALTER",
                "CALL",
                "CANCEL",
                "DELETE",
                "ENTRY",
                "EVALUATE",
                "EXEC",
                "GENERATE",
                "INITIALIZE",
                "INSPECT",
                "INVOKE",
                "MERGE",
                "RELEASE",
                "RETURN",
                "REWRITE",
                "SEARCH",
                "SET",
                "SORT",
                "START",
                "STRING",
                "UNSTRING",
                "XML",
                "JSON",
            ] {
                if has_word(&words, word) {
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
    let mut in_single = false;
    let mut in_double = false;
    for ch in line.chars() {
        match ch {
            '\'' if !in_double => {
                in_single = !in_single;
                out.push(' ');
            }
            '"' if !in_single => {
                in_double = !in_double;
                out.push(' ');
            }
            _ if in_single || in_double => out.push(' '),
            _ => out.push(ch),
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

fn add_codegen_support_diagnostics(ir: &mut ProgramIr) {
    let mut extra = Vec::new();
    let data_names = ir
        .data_items
        .iter()
        .map(|item| item.name.to_ascii_uppercase())
        .collect::<HashSet<_>>();
    for paragraph in &ir.paragraphs {
        for statement in &paragraph.statements {
            for reference in data_references(statement) {
                if !data_names.contains(&reference.to_ascii_uppercase()) {
                    extra.push(Diagnostic::error(
                        "E_UNRESOLVED_DATA",
                        format!(
                            "data reference {reference} does not resolve to a Data Division item"
                        ),
                        paragraph.span.clone(),
                    ));
                }
            }
            match statement {
                StatementIr::Compute { .. } => extra.push(Diagnostic::error(
                    "E_CODEGEN_COMPUTE",
                    "COMPUTE parsing is recognized but not yet code-generated in the converter MVP",
                    paragraph.span.clone(),
                )),
                StatementIr::Open(_)
                | StatementIr::Read(_)
                | StatementIr::Write(_)
                | StatementIr::Close(_) => {
                    extra.push(Diagnostic::error(
                        "E_CODEGEN_FILE_IO",
                        "file IO is modeled in IR but requires explicit runtime binding before Rust emission",
                        paragraph.span.clone(),
                    ));
                }
                StatementIr::Perform { target, through } => {
                    if through.is_some() {
                        extra.push(Diagnostic::error(
                            "E_CODEGEN_PERFORM_THRU",
                            "PERFORM THRU is represented in IR but not yet code-generated",
                            paragraph.span.clone(),
                        ));
                    }
                    if paragraph_index(ir, target).is_none() {
                        extra.push(Diagnostic::error(
                            "E_UNRESOLVED_PARAGRAPH",
                            format!("PERFORM target {target} does not resolve to a paragraph"),
                            paragraph.span.clone(),
                        ));
                    }
                }
                StatementIr::GoTo(target) if paragraph_index(ir, target).is_none() => {
                    extra.push(Diagnostic::error(
                        "E_UNRESOLVED_PARAGRAPH",
                        format!("GO TO target {target} does not resolve to a paragraph"),
                        paragraph.span.clone(),
                    ));
                }
                _ => {}
            }
        }
    }
    ir.diagnostics.extend(extra);
    ir.diagnostics.sort_by(compare_diagnostics);
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
    Ok(written)
}

fn emit_cargo_toml(ir: &ProgramIr) -> String {
    format!(
        "[package]\nname = \"{}-rust\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n\n[dependencies]\ncobol-runtime = {{ path = \"vendor/cobol-runtime\" }}\n\n",
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

fn emit_main_rs() -> String {
    "mod data;\nmod files;\nmod program;\n\nfn main() -> Result<(), Box<dyn std::error::Error>> {\n    let mut program = program::Program::default();\n    program.run()?;\n    Ok(())\n}\n"
        .to_string()
}

fn emit_data_rs(ir: &ProgramIr) -> String {
    let has_items = ir.data_items.iter().any(|item| item.level != 88);
    let mut text = String::from(
        "use cobol_runtime::CobolStorage;\n\npub fn initial_storage() -> CobolStorage {\n",
    );
    if has_items {
        text.push_str("    let mut storage = CobolStorage::default();\n");
    } else {
        text.push_str("    let storage = CobolStorage::default();\n");
    }
    for item in &ir.data_items {
        if item.level == 88 {
            continue;
        }
        text.push_str(&format!(
            "    storage.define(\"{}\", cobol_runtime::CobolValue::Text(String::new()));\n",
            escape_rust(&item.name)
        ));
    }
    text.push_str("    storage\n}\n");
    text
}

fn emit_files_rs() -> String {
    "pub type FileSystem = cobol_runtime::UnboundFileSystem;\n".to_string()
}

fn emit_program_rs(ir: &ProgramIr) -> String {
    let mut text = String::from(
        "use cobol_runtime::{CobolRuntime, CobolStorage, ControlFlow, RuntimeError};\n\n#[allow(dead_code)]\npub struct Program {\n    storage: CobolStorage,\n    runtime: CobolRuntime,\n    files: crate::files::FileSystem,\n}\n\nimpl Default for Program {\n    fn default() -> Self {\n        Self {\n            storage: crate::data::initial_storage(),\n            runtime: CobolRuntime::default(),\n            files: crate::files::FileSystem::default(),\n        }\n    }\n}\n\n#[derive(Debug, Clone, Copy, PartialEq, Eq)]\nenum ParagraphId {\n",
    );
    for paragraph in &ir.paragraphs {
        text.push_str(&format!("    {},\n", enum_variant(&paragraph.name)));
    }
    text.push_str("}\n\nimpl Program {\n    pub fn run(&mut self) -> Result<(), RuntimeError> {\n");
    if let Some(first) = ir.paragraphs.first() {
        text.push_str(&format!(
            "        let mut pc = Some(ParagraphId::{});\n",
            enum_variant(&first.name)
        ));
    } else {
        text.push_str("        let mut pc = None;\n");
    }
    text.push_str(
        "        while let Some(current) = pc {\n            let action = self.dispatch(current)?;\n            pc = match action {\n                ControlFlow::Next => self.next_paragraph(current),\n                ControlFlow::GoTo(idx) => self.paragraph_by_index(idx),\n                ControlFlow::Perform(idx) => {\n                    if let Some(target) = self.paragraph_by_index(idx) {\n                        let _ = self.dispatch(target)?;\n                    }\n                    self.next_paragraph(current)\n                }\n                ControlFlow::StopRun => None,\n            };\n        }\n        Ok(())\n    }\n\n    fn dispatch(&mut self, paragraph: ParagraphId) -> Result<ControlFlow, RuntimeError> {\n        match paragraph {\n",
    );
    for paragraph in &ir.paragraphs {
        text.push_str(&format!(
            "            ParagraphId::{} => self.{}(),\n",
            enum_variant(&paragraph.name),
            paragraph.rust_name
        ));
    }
    text.push_str("        }\n    }\n\n    fn next_paragraph(&self, paragraph: ParagraphId) -> Option<ParagraphId> {\n        match paragraph {\n");
    for (idx, paragraph) in ir.paragraphs.iter().enumerate() {
        let next = ir.paragraphs.get(idx + 1);
        let target = next
            .map(|next| format!("Some(ParagraphId::{})", enum_variant(&next.name)))
            .unwrap_or_else(|| "None".to_string());
        text.push_str(&format!(
            "            ParagraphId::{} => {},\n",
            enum_variant(&paragraph.name),
            target
        ));
    }
    text.push_str("        }\n    }\n\n    fn paragraph_by_index(&self, index: usize) -> Option<ParagraphId> {\n        match index {\n");
    for (idx, paragraph) in ir.paragraphs.iter().enumerate() {
        text.push_str(&format!(
            "            {} => Some(ParagraphId::{}),\n",
            idx,
            enum_variant(&paragraph.name)
        ));
    }
    text.push_str("            _ => None,\n        }\n    }\n");
    for paragraph in &ir.paragraphs {
        text.push_str(&format!(
            "\n    fn {}(&mut self) -> Result<ControlFlow, RuntimeError> {{\n",
            paragraph.rust_name
        ));
        let mut terminal = false;
        for statement in &paragraph.statements {
            text.push_str(&emit_statement(statement, ir));
            if statement_is_terminal(statement) {
                terminal = true;
                break;
            }
        }
        if !terminal {
            text.push_str("        Ok(ControlFlow::Next)\n");
        }
        text.push_str("    }\n");
    }
    text.push_str("}\n");
    text
}

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
            escape_rust(target)
        ),
        StatementIr::Add { source, target } => format!(
            "        self.storage.add({}, \"{}\")?;\n",
            emit_operand_value(source),
            escape_rust(target)
        ),
        StatementIr::Subtract { source, target } => format!(
            "        self.storage.subtract({}, \"{}\")?;\n",
            emit_operand_value(source),
            escape_rust(target)
        ),
        StatementIr::Multiply { source, target } => format!(
            "        self.storage.multiply({}, \"{}\")?;\n",
            emit_operand_value(source),
            escape_rust(target)
        ),
        StatementIr::Divide { source, target } => format!(
            "        self.storage.divide({}, \"{}\")?;\n",
            emit_operand_value(source),
            escape_rust(target)
        ),
        StatementIr::Perform { target, .. } => {
            let variant = paragraph_index(ir, target)
                .and_then(|idx| ir.paragraphs.get(idx))
                .map(|paragraph| enum_variant(&paragraph.name))
                .unwrap_or_else(|| enum_variant(target));
            format!("        let _ = self.dispatch(ParagraphId::{variant})?;\n")
        }
        StatementIr::GoTo(target) => {
            let idx = paragraph_index(ir, target).unwrap_or(usize::MAX);
            format!("        return Ok(ControlFlow::GoTo({idx}));\n")
        }
        StatementIr::StopRun => "        return Ok(ControlFlow::StopRun);\n".to_string(),
        StatementIr::Open(raw) => format!(
            "        cobol_runtime::CobolFileSystem::open(&mut self.files, \"{}\")?;\n",
            escape_rust(raw)
        ),
        StatementIr::Read(raw) => format!(
            "        cobol_runtime::CobolFileSystem::read(&mut self.files, \"{}\")?;\n",
            escape_rust(raw)
        ),
        StatementIr::Write(raw) => format!(
            "        cobol_runtime::CobolFileSystem::write(&mut self.files, \"{}\")?;\n",
            escape_rust(raw)
        ),
        StatementIr::Close(raw) => format!(
            "        cobol_runtime::CobolFileSystem::close(&mut self.files, \"{}\")?;\n",
            escape_rust(raw)
        ),
        StatementIr::Compute { .. }
        | StatementIr::If { .. }
        | StatementIr::Evaluate { .. }
        | StatementIr::Unsupported { .. } => {
            "        return Ok(ControlFlow::StopRun);\n".to_string()
        }
    }
}

fn emit_operand_display(operand: &OperandIr) -> String {
    match operand {
        OperandIr::Literal(value) => format!("\"{}\".to_string()", escape_rust(value)),
        OperandIr::Number(value) => format!("\"{}\".to_string()", escape_rust(value)),
        OperandIr::Identifier(name) => {
            format!(
                "self.storage.get(\"{}\")?.display_string()",
                escape_rust(name)
            )
        }
    }
}

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
        OperandIr::Identifier(name) => {
            format!("self.storage.get(\"{}\")?.clone()", escape_rust(name))
        }
    }
}

fn statement_is_terminal(statement: &StatementIr) -> bool {
    matches!(statement, StatementIr::GoTo(_) | StatementIr::StopRun)
}

fn data_references(statement: &StatementIr) -> Vec<String> {
    let mut references = Vec::new();
    match statement {
        StatementIr::Display(values) => {
            references.extend(values.iter().filter_map(operand_identifier));
        }
        StatementIr::Move { source, target }
        | StatementIr::Add { source, target }
        | StatementIr::Subtract { source, target }
        | StatementIr::Multiply { source, target }
        | StatementIr::Divide { source, target } => {
            if let Some(source) = operand_identifier(source) {
                references.push(source);
            }
            references.push(target.clone());
        }
        StatementIr::Compute { target, .. } => references.push(target.clone()),
        StatementIr::If { .. }
        | StatementIr::Evaluate { .. }
        | StatementIr::Perform { .. }
        | StatementIr::GoTo(_)
        | StatementIr::Open(_)
        | StatementIr::Read(_)
        | StatementIr::Write(_)
        | StatementIr::Close(_)
        | StatementIr::StopRun
        | StatementIr::Unsupported { .. } => {}
    }
    references
}

fn operand_identifier(operand: &OperandIr) -> Option<String> {
    match operand {
        OperandIr::Identifier(name) => Some(name.clone()),
        OperandIr::Literal(_) | OperandIr::Number(_) => None,
    }
}

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
        stats: ReportStats {
            data_items: ir.data_items.len(),
            paragraphs: ir.paragraphs.len(),
            statements: ir
                .paragraphs
                .iter()
                .map(|paragraph| paragraph.statements.len())
                .sum(),
        },
    }
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

fn escape_rust(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

#[allow(dead_code)]
fn _data_item_names(items: &[DataItemIr]) -> Vec<&str> {
    items.iter().map(|item| item.name.as_str()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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
        assert!(out.join("migration-report.json").is_file());
        let _ = fs::remove_dir_all(&dir);
    }
}
