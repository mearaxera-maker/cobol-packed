use cobol_ir::{
    CobolDialect, DataItemIr, Diagnostic, FileIr, OccursIr, OperandIr, ParagraphIr, ProgramIr,
    Severity, StatementIr, UsageIr,
};
use cobol_syntax::{DataDeclAst, ProgramAst, StatementKindAst};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Ibm,
    GnuCobol,
    MicroFocus,
}

pub fn analyze(ast: ProgramAst, dialect: Dialect) -> ProgramIr {
    let mut diagnostics = ast.diagnostics;
    let mut seen_data = HashSet::new();
    let mut data_items = Vec::new();
    let mut parent_stack: Vec<(u8, String)> = Vec::new();

    for item in ast.data_items {
        if !seen_data.insert(item.name.clone()) {
            diagnostics.push(Diagnostic::error(
                "E_DUPLICATE_SYMBOL",
                format!("duplicate data item {}", item.name),
                item.span.clone(),
            ));
        }
        while parent_stack
            .last()
            .map(|(level, _)| *level >= item.level)
            .unwrap_or(false)
        {
            parent_stack.pop();
        }
        let parent = parent_stack.last().map(|(_, name)| name.clone());
        let ir = lower_data_item(&item, parent);
        if matches!(ir.usage, UsageIr::Unknown(_)) {
            diagnostics.push(Diagnostic::warning(
                "W_UNKNOWN_USAGE",
                format!(
                    "data item {} has unknown usage; generated runtime stores it as text",
                    item.name
                ),
                item.span.clone(),
            ));
        }
        if ir.picture.is_none() && !matches!(ir.usage, UsageIr::Group) && ir.level != 88 {
            diagnostics.push(Diagnostic::warning(
                "W_MISSING_PICTURE",
                format!("data item {} has no PIC clause", item.name),
                item.span.clone(),
            ));
        }
        parent_stack.push((item.level, item.name.clone()));
        data_items.push(ir);
    }

    let paragraphs = ast
        .paragraphs
        .into_iter()
        .map(|paragraph| ParagraphIr {
            rust_name: rust_ident(&paragraph.name),
            name: paragraph.name,
            span: paragraph.span,
            statements: paragraph
                .statements
                .into_iter()
                .map(|statement| {
                    let raw = statement.raw.clone();
                    let span = statement.span.clone();
                    let lowered = lower_statement(statement.kind, raw.clone());
                    if let StatementIr::Unsupported { keyword, .. } = &lowered {
                        diagnostics.push(Diagnostic::error(
                            "E_UNSUPPORTED_STATEMENT",
                            format!("unsupported or not-yet-lowered COBOL statement: {keyword}"),
                            span,
                        ));
                    }
                    lowered
                })
                .collect(),
        })
        .collect();

    ProgramIr {
        name: ast.name,
        dialect: match dialect {
            Dialect::Ibm => CobolDialect::Ibm,
            Dialect::GnuCobol => CobolDialect::GnuCobol,
            Dialect::MicroFocus => CobolDialect::MicroFocus,
        },
        data_items,
        paragraphs,
        files: Vec::<FileIr>::new(),
        diagnostics: dedupe_diagnostics(diagnostics),
    }
}

fn lower_data_item(item: &DataDeclAst, parent: Option<String>) -> DataItemIr {
    let upper = item.clauses.to_ascii_uppercase();
    let picture = extract_picture(&item.clauses);
    let usage = if picture.is_none() && !upper.contains("USAGE") && !upper.contains("PIC") {
        UsageIr::Group
    } else if upper.contains("COMP-3") || upper.contains("PACKED-DECIMAL") {
        UsageIr::PackedDecimal
    } else if upper.contains("COMP-5") {
        UsageIr::NativeBinary
    } else if upper.contains("COMP-1") {
        UsageIr::Float32
    } else if upper.contains("COMP-2") {
        UsageIr::Float64
    } else if upper.contains("COMP") || upper.contains("BINARY") || upper.contains("COMP-4") {
        UsageIr::Binary
    } else if picture
        .as_deref()
        .map(|pic| pic.to_ascii_uppercase().contains('X'))
        .unwrap_or(false)
    {
        UsageIr::Alphanumeric
    } else if picture.is_some() {
        UsageIr::Display
    } else {
        UsageIr::Unknown(item.clauses.clone())
    };

    DataItemIr {
        level: item.level,
        rust_name: rust_ident(&item.name),
        name: item.name.clone(),
        picture,
        usage,
        occurs: extract_occurs(&item.clauses),
        redefines: extract_after_keyword(&item.clauses, "REDEFINES").map(|value| value.to_string()),
        parent,
        span: item.span.clone(),
    }
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
            target,
        },
        StatementKindAst::Add { source, target } => StatementIr::Add {
            source: parse_operand(&source),
            target,
        },
        StatementKindAst::Subtract { source, target } => StatementIr::Subtract {
            source: parse_operand(&source),
            target,
        },
        StatementKindAst::Multiply { source, target } => StatementIr::Multiply {
            source: parse_operand(&source),
            target,
        },
        StatementKindAst::Divide { source, target } => StatementIr::Divide {
            source: parse_operand(&source),
            target,
        },
        StatementKindAst::Compute { target, expression } => {
            StatementIr::Compute { target, expression }
        }
        StatementKindAst::Perform { target, through } => StatementIr::Perform { target, through },
        StatementKindAst::GoTo(target) => StatementIr::GoTo(target),
        StatementKindAst::Open(raw) => StatementIr::Open(raw),
        StatementKindAst::Read(raw) => StatementIr::Read(raw),
        StatementKindAst::Write(raw) => StatementIr::Write(raw),
        StatementKindAst::Close(raw) => StatementIr::Close(raw),
        StatementKindAst::StopRun => StatementIr::StopRun,
        StatementKindAst::If(_) => StatementIr::Unsupported {
            keyword: "IF".to_string(),
            raw,
        },
        StatementKindAst::Evaluate(_) => StatementIr::Unsupported {
            keyword: "EVALUATE".to_string(),
            raw,
        },
        StatementKindAst::Unsupported(keyword) => StatementIr::Unsupported { keyword, raw },
    }
}

fn parse_operand(value: &str) -> OperandIr {
    let clean = value.trim().trim_end_matches('.');
    if (clean.starts_with('"') && clean.ends_with('"'))
        || (clean.starts_with('\'') && clean.ends_with('\''))
    {
        OperandIr::Literal(clean.trim_matches('"').trim_matches('\'').to_string())
    } else if is_numeric_literal(clean) {
        OperandIr::Number(clean.to_string())
    } else {
        OperandIr::Identifier(clean.replace('-', "_").to_ascii_uppercase())
    }
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
        .map(|value| value.replace('-', "_").to_ascii_uppercase());
    Some(OccursIr {
        min,
        max,
        depending_on,
    })
}

fn extract_after_keyword<'a>(clauses: &'a str, keyword: &str) -> Option<&'a str> {
    let parts: Vec<&str> = clauses.split_whitespace().collect();
    parts
        .iter()
        .position(|part| part.eq_ignore_ascii_case(keyword))
        .and_then(|idx| parts.get(idx + 1).copied())
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

    #[test]
    fn lowers_data_and_display() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 WS-NAME PIC X(10).\nPROCEDURE DIVISION.\nMAIN.\nDISPLAY WS-NAME.\nSTOP RUN.\n";
        let ast = parse_program("hello.cbl", src).expect("parse");
        let ir = analyze(ast, Dialect::Ibm);
        assert_eq!(ir.data_items.len(), 1);
        assert_eq!(ir.paragraphs.len(), 1);
        assert!(!ir.has_errors());
    }
}
