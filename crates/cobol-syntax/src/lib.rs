use cobol_ir::{Diagnostic, SourceSpan};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RawTokenKind {
    StringLiteral,
    QuotedLiteral,
    Number,
    Word,
    Period,
    Comma,
    LParen,
    RParen,
    Eq,
    Plus,
    Minus,
    Star,
    Slash,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: String,
    pub lexeme: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LosslessTree {
    pub tokens: Vec<Token>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramAst {
    pub name: String,
    pub data_items: Vec<DataDeclAst>,
    pub paragraphs: Vec<ParagraphAst>,
    pub diagnostics: Vec<Diagnostic>,
    pub cst: LosslessTree,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDeclAst {
    pub level: u8,
    pub name: String,
    pub clauses: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParagraphAst {
    pub name: String,
    pub statements: Vec<StatementAst>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementAst {
    pub kind: StatementKindAst,
    pub raw: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatementKindAst {
    Display(Vec<String>),
    Move {
        source: String,
        target: String,
    },
    Add {
        source: String,
        target: String,
    },
    Subtract {
        source: String,
        target: String,
    },
    Multiply {
        source: String,
        target: String,
    },
    Divide {
        source: String,
        target: String,
    },
    Compute {
        target: String,
        expression: String,
    },
    If(String),
    Evaluate(String),
    Perform {
        target: String,
        through: Option<String>,
    },
    GoTo(String),
    Open(String),
    Read(String),
    Write(String),
    Close(String),
    StopRun,
    Unsupported(String),
}

#[derive(Debug, thiserror::Error)]
pub enum SyntaxError {
    #[error("COBOL source contains no parseable program unit")]
    EmptyProgram,
}

pub fn lex(source_name: &str, text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = text.char_indices().peekable();
    let mut line = 1usize;
    let mut column = 1usize;
    while let Some((start, ch)) = chars.next() {
        if ch.is_whitespace() {
            if ch == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
            continue;
        }
        let start_line = line;
        let start_column = column;
        let (kind, end) = if ch == '"' || ch == '\'' {
            let quote = ch;
            let mut end = start + ch.len_utf8();
            let mut last = ch;
            for (idx, next) in chars.by_ref() {
                end = idx + next.len_utf8();
                last = next;
                if next == '\n' {
                    line += 1;
                    column = 1;
                } else {
                    column += 1;
                }
                if next == quote {
                    break;
                }
            }
            let kind = if quote == '"' {
                RawTokenKind::StringLiteral
            } else {
                RawTokenKind::QuotedLiteral
            };
            if last != '\n' {
                column += 1;
            }
            (kind, end)
        } else if ch.is_ascii_digit() {
            let mut end = start + ch.len_utf8();
            while let Some((idx, next)) = chars.peek().copied() {
                if !next.is_ascii_digit() {
                    break;
                }
                chars.next();
                end = idx + next.len_utf8();
                column += 1;
            }
            column += 1;
            (RawTokenKind::Number, end)
        } else if ch.is_ascii_alphabetic() {
            let mut end = start + ch.len_utf8();
            while let Some((idx, next)) = chars.peek().copied() {
                if !(next.is_ascii_alphanumeric() || next == '-' || next == '_') {
                    break;
                }
                chars.next();
                end = idx + next.len_utf8();
                column += 1;
            }
            column += 1;
            (RawTokenKind::Word, end)
        } else {
            column += 1;
            let kind = match ch {
                '.' => RawTokenKind::Period,
                ',' => RawTokenKind::Comma,
                '(' => RawTokenKind::LParen,
                ')' => RawTokenKind::RParen,
                '=' => RawTokenKind::Eq,
                '+' => RawTokenKind::Plus,
                '-' => RawTokenKind::Minus,
                '*' => RawTokenKind::Star,
                '/' => RawTokenKind::Slash,
                _ => RawTokenKind::Other,
            };
            (kind, start + ch.len_utf8())
        };
        tokens.push(Token {
            kind: format!("{kind:?}"),
            lexeme: text.get(start..end).unwrap_or("").to_string(),
            span: SourceSpan {
                file: source_name.to_string(),
                line: start_line,
                column: start_column,
            },
        });
    }
    tokens
}

pub fn parse_program(source_name: &str, text: &str) -> Result<ProgramAst, SyntaxError> {
    let tokens = lex(source_name, text);
    let cst = build_lossless_tree(&tokens);
    let mut diagnostics = Vec::new();
    let sentences = split_sentences(text, source_name);
    if sentences.is_empty() {
        return Err(SyntaxError::EmptyProgram);
    }

    let mut name = "COBOL_PROGRAM".to_string();
    let mut section = Section::Before;
    let mut data_items = Vec::new();
    let mut paragraphs = Vec::new();
    let mut current_paragraph: Option<ParagraphAst> = None;
    let mut expecting_program_id = false;

    for sentence in sentences {
        let upper = sentence.raw.to_ascii_uppercase();
        if expecting_program_id {
            name = sanitize_cobol_name(&sentence.raw);
            expecting_program_id = false;
            continue;
        }
        if upper.starts_with("IDENTIFICATION DIVISION") {
            section = Section::Identification;
            continue;
        }
        if upper.starts_with("ENVIRONMENT DIVISION") {
            section = Section::Environment;
            continue;
        }
        if upper.starts_with("DATA DIVISION") {
            section = Section::Data;
            continue;
        }
        if upper.starts_with("PROCEDURE DIVISION") {
            section = Section::Procedure;
            continue;
        }
        if upper == "PROGRAM-ID" {
            expecting_program_id = true;
            continue;
        }
        if upper.starts_with("PROGRAM-ID") {
            if let Some(program_name) = sentence.raw.split_whitespace().nth(1) {
                name = sanitize_cobol_name(program_name);
            }
            continue;
        }

        match section {
            Section::Data => {
                if let Some(item) = parse_data_decl(&sentence.raw, sentence.span.clone()) {
                    data_items.push(item);
                }
            }
            Section::Procedure => {
                if is_paragraph_label(&sentence.raw) {
                    if let Some(paragraph) = current_paragraph.take() {
                        paragraphs.push(paragraph);
                    }
                    current_paragraph = Some(ParagraphAst {
                        name: sanitize_cobol_name(&sentence.raw),
                        statements: Vec::new(),
                        span: sentence.span,
                    });
                } else {
                    let statement = parse_statement(&sentence.raw, sentence.span.clone());
                    if let StatementKindAst::Unsupported(keyword) = &statement.kind {
                        diagnostics.push(Diagnostic::error(
                            "E_UNSUPPORTED_STATEMENT",
                            format!("unsupported COBOL statement: {keyword}"),
                            sentence.span,
                        ));
                    }
                    if current_paragraph.is_none() {
                        current_paragraph = Some(ParagraphAst {
                            name: "MAIN".to_string(),
                            statements: Vec::new(),
                            span: SourceSpan {
                                file: source_name.to_string(),
                                line: 1,
                                column: 1,
                            },
                        });
                    }
                    if let Some(paragraph) = &mut current_paragraph {
                        paragraph.statements.push(statement);
                    }
                }
            }
            Section::Before | Section::Identification | Section::Environment => {}
        }
    }

    if let Some(paragraph) = current_paragraph {
        paragraphs.push(paragraph);
    }

    Ok(ProgramAst {
        name,
        data_items,
        paragraphs,
        diagnostics,
        cst,
    })
}

fn build_lossless_tree(tokens: &[Token]) -> LosslessTree {
    LosslessTree {
        tokens: tokens.to_vec(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Before,
    Identification,
    Environment,
    Data,
    Procedure,
}

#[derive(Debug, Clone)]
struct Sentence {
    raw: String,
    span: SourceSpan,
}

fn split_sentences(text: &str, source_name: &str) -> Vec<Sentence> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut line = 1usize;
    let mut column = 1usize;
    let mut start_line = 1usize;
    let mut start_column = 1usize;
    let mut seen_content = false;

    for (byte_idx, ch) in text.char_indices() {
        if !seen_content && !ch.is_whitespace() {
            start_line = line;
            start_column = column;
            seen_content = true;
        }
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '.' if !in_single && !in_double && is_sentence_period(text, byte_idx, &current) => {
                let raw = current.trim().to_string();
                if !raw.is_empty() {
                    sentences.push(Sentence {
                        raw,
                        span: SourceSpan {
                            file: source_name.to_string(),
                            line: start_line,
                            column: start_column,
                        },
                    });
                }
                current.clear();
                seen_content = false;
                column += 1;
                continue;
            }
            _ => {}
        }
        current.push(ch);
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    let raw = current.trim().to_string();
    if !raw.is_empty() {
        sentences.push(Sentence {
            raw,
            span: SourceSpan {
                file: source_name.to_string(),
                line: start_line,
                column: start_column,
            },
        });
    }
    sentences
}

fn parse_data_decl(raw: &str, span: SourceSpan) -> Option<DataDeclAst> {
    let mut parts = raw.split_whitespace();
    let level = parts.next()?.parse::<u8>().ok()?;
    let name = parts.next()?.trim_end_matches('.').to_string();
    Some(DataDeclAst {
        level,
        name: sanitize_cobol_name(&name),
        clauses: parts.collect::<Vec<_>>().join(" "),
        span,
    })
}

fn parse_statement(raw: &str, span: SourceSpan) -> StatementAst {
    let words = words(raw);
    let first = words
        .first()
        .map(|word| word.to_ascii_uppercase())
        .unwrap_or_default();
    let kind = match first.as_str() {
        "DISPLAY" => StatementKindAst::Display(words.into_iter().skip(1).collect()),
        "MOVE" => parse_binary_target_statement(&words, "TO")
            .map(|(source, target)| StatementKindAst::Move { source, target })
            .unwrap_or_else(|| StatementKindAst::Unsupported("MOVE".to_string())),
        "ADD" => parse_binary_target_statement(&words, "TO")
            .map(|(source, target)| StatementKindAst::Add { source, target })
            .unwrap_or_else(|| StatementKindAst::Unsupported("ADD".to_string())),
        "SUBTRACT" => parse_binary_target_statement(&words, "FROM")
            .map(|(source, target)| StatementKindAst::Subtract { source, target })
            .unwrap_or_else(|| StatementKindAst::Unsupported("SUBTRACT".to_string())),
        "MULTIPLY" => parse_binary_target_statement(&words, "BY")
            .map(|(source, target)| StatementKindAst::Multiply { source, target })
            .unwrap_or_else(|| StatementKindAst::Unsupported("MULTIPLY".to_string())),
        "DIVIDE" => parse_binary_target_statement(&words, "INTO")
            .map(|(source, target)| StatementKindAst::Divide { source, target })
            .unwrap_or_else(|| StatementKindAst::Unsupported("DIVIDE".to_string())),
        "COMPUTE" => parse_compute(&words)
            .map(|(target, expression)| StatementKindAst::Compute { target, expression })
            .unwrap_or_else(|| StatementKindAst::Unsupported("COMPUTE".to_string())),
        "IF" => StatementKindAst::If(raw.to_string()),
        "EVALUATE" => StatementKindAst::Evaluate(raw.to_string()),
        "PERFORM" => parse_perform(&words),
        "GO" => {
            if words.get(1).map(|w| w.eq_ignore_ascii_case("TO")) == Some(true) {
                words
                    .get(2)
                    .map(|target| StatementKindAst::GoTo(sanitize_cobol_name(target)))
                    .unwrap_or_else(|| StatementKindAst::Unsupported("GO".to_string()))
            } else {
                StatementKindAst::Unsupported("GO".to_string())
            }
        }
        "GOBACK" => StatementKindAst::StopRun,
        "STOP" => {
            if words.get(1).map(|word| word.eq_ignore_ascii_case("RUN")) == Some(true) {
                StatementKindAst::StopRun
            } else {
                StatementKindAst::Unsupported("STOP".to_string())
            }
        }
        "OPEN" => StatementKindAst::Open(raw.to_string()),
        "READ" => StatementKindAst::Read(raw.to_string()),
        "WRITE" => StatementKindAst::Write(raw.to_string()),
        "CLOSE" => StatementKindAst::Close(raw.to_string()),
        "EXEC" | "SORT" | "MERGE" | "ALTER" | "CALL" => StatementKindAst::Unsupported(first),
        _ => StatementKindAst::Unsupported(first),
    };
    StatementAst {
        kind,
        raw: raw.to_string(),
        span,
    }
}

fn parse_binary_target_statement(words: &[String], separator: &str) -> Option<(String, String)> {
    let sep = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case(separator))?;
    let source = words.get(1..sep)?.join(" ");
    let target = words.get(sep + 1)?.clone();
    Some((source, sanitize_cobol_name(&target)))
}

fn parse_compute(words: &[String]) -> Option<(String, String)> {
    if words.len() < 4 {
        return None;
    }
    let eq = words.iter().position(|word| word == "=")?;
    let target = sanitize_cobol_name(words.get(1)?);
    let expression = words.get(eq + 1..)?.join(" ");
    Some((target, expression))
}

fn parse_perform(words: &[String]) -> StatementKindAst {
    let Some(target) = words.get(1) else {
        return StatementKindAst::Unsupported("PERFORM".to_string());
    };
    let through = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("THRU") || word.eq_ignore_ascii_case("THROUGH"))
        .and_then(|idx| words.get(idx + 1))
        .map(|word| sanitize_cobol_name(word));
    StatementKindAst::Perform {
        target: sanitize_cobol_name(target),
        through,
    }
}

fn is_paragraph_label(raw: &str) -> bool {
    let words = words(raw);
    if words.len() != 1 {
        return false;
    }
    let upper = words[0].to_ascii_uppercase();
    !matches!(
        upper.as_str(),
        "EXIT" | "STOP" | "GOBACK" | "SECTION" | "DIVISION"
    )
}

fn words(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    for ch in raw.chars() {
        match ch {
            '\'' if !in_double => {
                current.push(ch);
                in_single = !in_single;
            }
            '"' if !in_single => {
                current.push(ch);
                in_double = !in_double;
            }
            ' ' | '\t' | '\n' | '\r' | ',' if !in_single && !in_double => {
                if !current.is_empty() {
                    out.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn is_sentence_period(text: &str, byte_idx: usize, current: &str) -> bool {
    let before = current.chars().last();
    let after = text[byte_idx + 1..].chars().next();
    if before.map(|ch| ch.is_ascii_digit()).unwrap_or(false)
        && after.map(|ch| ch.is_ascii_digit()).unwrap_or(false)
    {
        return false;
    }
    true
}

pub fn sanitize_cobol_name(name: &str) -> String {
    let mut clean = name
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim_end_matches('.')
        .replace('-', "_")
        .to_ascii_uppercase();
    clean.retain(|ch| ch.is_ascii_alphanumeric() || ch == '_');
    if clean.is_empty() {
        "ITEM".to_string()
    } else {
        clean
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hello_world() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\nPROCEDURE DIVISION.\nMAIN.\nDISPLAY \"HELLO\".\nSTOP RUN.\n";
        let ast = parse_program("hello.cbl", src).expect("program parses");
        assert_eq!(ast.name, "HELLO");
        assert_eq!(ast.paragraphs.len(), 1);
        assert_eq!(ast.paragraphs[0].statements.len(), 2);
    }

    #[test]
    fn catches_unsupported_exec() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. X.\nPROCEDURE DIVISION.\nEXEC SQL SELECT 1 END-EXEC.\n";
        let ast = parse_program("x.cbl", src).expect("program parses");
        assert!(ast
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E_UNSUPPORTED_STATEMENT"));
    }

    #[test]
    fn decimal_point_inside_numeric_literal_does_not_split_sentence() {
        let sentences = split_sentences("PROCEDURE DIVISION.\nMAIN.\nDISPLAY 12.34.\n", "x.cbl");
        assert!(sentences
            .iter()
            .any(|sentence| sentence.raw == "DISPLAY 12.34"));
    }

    #[test]
    fn stop_without_run_is_unsupported() {
        let statement = parse_statement("STOP \"PAUSE\"", SourceSpan::generated());
        assert!(matches!(statement.kind, StatementKindAst::Unsupported(_)));
    }
}
