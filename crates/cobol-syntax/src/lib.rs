use cobol_ir::{Diagnostic, SourceSpan};
use cobol_text::SpannedWord;

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
    pub is_common: bool,
    pub is_initial: bool,
    pub data_items: Vec<DataDeclAst>,
    pub files: Vec<FileAst>,
    pub same_record_areas: Vec<SameRecordAreaAst>,
    pub rerun_clauses: Vec<RerunClauseAst>,
    pub paragraphs: Vec<ParagraphAst>,
    pub declaratives: Vec<DeclarativeAst>,
    pub diagnostics: Vec<Diagnostic>,
    pub cst: LosslessTree,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SameRecordAreaAst {
    pub files: Vec<String>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RerunClauseAst {
    pub checkpoint_file: String,
    pub every_records: usize,
    pub watched_file: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileAst {
    pub name: String,
    pub kind: FileKindAst,
    pub assign: Option<String>,
    pub assign_is_literal: bool,
    pub organization: Option<String>,
    pub access_mode: Option<String>,
    pub file_status: Option<String>,
    pub record_name: Option<String>,
    pub linage: Option<usize>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKindAst {
    Fd,
    Sd,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDeclAst {
    pub level: u8,
    pub name: String,
    pub clauses: String,
    pub clause_ast: Vec<DataClauseAst>,
    pub storage_area: StorageAreaAst,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageAreaAst {
    WorkingStorage,
    LocalStorage,
    Linkage,
    FileSection,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataClauseAst {
    Picture(String),
    Usage(String),
    Occurs {
        min: usize,
        max: usize,
        depending_on: Option<String>,
        indexed_by: Vec<String>,
        keys: Vec<DataOccursKeyAst>,
    },
    Redefines(String),
    Renames {
        first: String,
        last: Option<String>,
    },
    Value(String),
    Values(Vec<DataValueAst>),
    Sync,
    External,
    Global,
    Sign {
        position: Option<DataSignPositionAst>,
        separate: bool,
    },
    Justified {
        right: bool,
    },
    BlankWhenZero,
    Based {
        pointer: Option<String>,
    },
    AnyLength,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataValueAst {
    Single(String),
    Range { start: String, end: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataSignPositionAst {
    Leading,
    Trailing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataOccursKeyAst {
    pub direction: DataOccursKeyDirectionAst,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataOccursKeyDirectionAst {
    Ascending,
    Descending,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParagraphAst {
    pub name: String,
    pub statements: Vec<StatementAst>,
    pub sentences: Vec<ProcedureSentenceAst>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcedureSentenceAst {
    pub statements: Vec<StatementAst>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarativeAst {
    pub name: String,
    pub trigger: DeclarativeTriggerAst,
    pub statements: Vec<StatementAst>,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclarativeTriggerAst {
    FileError(String),
    Debugging(String),
    Unsupported(String),
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementAst {
    pub kind: StatementKindAst,
    pub raw: String,
    pub span: SourceSpan,
}

pub type ImperativeListAst = Vec<StatementAst>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatementKindAst {
    Display(Vec<String>),
    Move {
        source: String,
        target: String,
    },
    MoveCorresponding {
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
    Compute(ComputeAst),
    NextSentence,
    If {
        condition: String,
        then_statements: ImperativeListAst,
        else_statements: ImperativeListAst,
    },
    Evaluate(EvaluateAst),
    Search(SearchAst),
    SetCondition {
        condition: String,
        value: bool,
    },
    SetIndex {
        index: String,
        operation: SetIndexAst,
    },
    Perform {
        target: String,
        through: Option<String>,
        varying: Option<String>,
        until: Option<String>,
        times: Option<String>,
        test_position: Option<PerformTestPositionAst>,
    },
    GoTo(String),
    ComputedGoTo {
        targets: Vec<String>,
        depending_on: String,
    },
    Alter {
        paragraph: String,
        target: String,
    },
    Call {
        target: String,
        using: Vec<String>,
    },
    Open(OpenFileAst),
    Read(ReadFileAst),
    Write(WriteFileAst),
    Rewrite(RewriteFileAst),
    Delete(DeleteFileAst),
    Close(CloseFileAst),
    Sort(SortProcedureAst),
    Release(ReleaseSortRecordAst),
    Return(ReturnSortRecordAst),
    Inspect(InspectLikeAst),
    Examine(InspectLikeAst),
    String(StringOpAst),
    Unstring(UnstringOpAst),
    ReadyTrace,
    ResetTrace,
    Continue,
    StopRun,
    Unsupported(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetIndexAst {
    To(String),
    UpBy(String),
    DownBy(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerformTestPositionAst {
    Before,
    After,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComputeAst {
    pub target: String,
    pub expression: String,
    pub on_size_error: ImperativeListAst,
    pub not_on_size_error: ImperativeListAst,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluateAst {
    pub raw: String,
    pub subjects: Vec<String>,
    pub arms: Vec<EvaluateArmAst>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluateArmAst {
    pub raw: String,
    pub patterns: Vec<String>,
    pub statements: ImperativeListAst,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchAst {
    pub raw: String,
    pub all: bool,
    pub table: String,
    pub index: Option<String>,
    pub at_end: ImperativeListAst,
    pub whens: Vec<SearchWhenAst>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchWhenAst {
    pub condition: String,
    pub statements: ImperativeListAst,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOpenModeAst {
    Input,
    Output,
    Io,
    Extend,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenFileAst {
    pub file: String,
    pub mode: FileOpenModeAst,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadFileAst {
    pub file: String,
    pub into: Option<String>,
    pub at_end: ImperativeListAst,
    pub not_at_end: ImperativeListAst,
    pub on_exception: ImperativeListAst,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteAdvancingAst {
    None,
    BeforeLines(usize),
    AfterLines(usize),
    BeforePage,
    AfterPage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteFileAst {
    pub record: String,
    pub advancing: WriteAdvancingAst,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewriteFileAst {
    pub record: String,
    pub invalid_key: ImperativeListAst,
    pub not_invalid_key: ImperativeListAst,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteFileAst {
    pub file: String,
    pub invalid_key: ImperativeListAst,
    pub not_invalid_key: ImperativeListAst,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseFileAst {
    pub file: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcedureRangeAst {
    pub target: String,
    pub through: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirectionAst {
    Ascending,
    Descending,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortKeyAst {
    pub direction: SortDirectionAst,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortProcedureAst {
    pub file: String,
    pub key: Option<SortKeyAst>,
    pub input_range: Option<ProcedureRangeAst>,
    pub output_range: ProcedureRangeAst,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseSortRecordAst {
    pub record: String,
    pub from: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnSortRecordAst {
    pub file: String,
    pub into: Option<String>,
    pub at_end: ImperativeListAst,
    pub not_at_end: ImperativeListAst,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectLikeAst {
    pub subject: String,
    pub tally: Option<InspectTallyAst>,
    pub replacing: Option<InspectReplacingAst>,
    pub converting: Option<InspectConvertingAst>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectTallyAst {
    pub target: String,
    pub pattern: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectReplacingAst {
    pub pattern: String,
    pub replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectConvertingAst {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringOpAst {
    pub pieces: Vec<StringPieceAst>,
    pub target: String,
    pub pointer: Option<String>,
    pub on_overflow: ImperativeListAst,
    pub not_on_overflow: ImperativeListAst,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringPieceAst {
    pub source: String,
    pub delimiter: StringDelimiterAst,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StringDelimiterAst {
    Size,
    Literal { value: String, all: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnstringOpAst {
    pub source: String,
    pub delimiter: StringDelimiterAst,
    pub targets: Vec<UnstringTargetAst>,
    pub pointer: Option<String>,
    pub tallying: Option<String>,
    pub on_overflow: ImperativeListAst,
    pub not_on_overflow: ImperativeListAst,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnstringTargetAst {
    pub target: String,
    pub count: Option<String>,
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
            let end = cobol_text::quoted_literal_end(text, start).unwrap_or(start + ch.len_utf8());
            while chars.peek().map(|(idx, _)| *idx < end).unwrap_or(false) {
                chars.next();
            }
            advance_position(&text[start..end], &mut line, &mut column);
            let kind = if ch == '"' {
                RawTokenKind::StringLiteral
            } else {
                RawTokenKind::QuotedLiteral
            };
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

fn advance_position(text: &str, line: &mut usize, column: &mut usize) {
    for ch in text.chars() {
        if ch == '\n' {
            *line += 1;
            *column = 1;
        } else {
            *column += 1;
        }
    }
}

pub fn parse_program(source_name: &str, text: &str) -> Result<ProgramAst, SyntaxError> {
    parse_program_from_sentences(source_name, split_sentences(text, source_name))
}

pub fn parse_programs(source_name: &str, text: &str) -> Result<Vec<ProgramAst>, SyntaxError> {
    let sentences = split_sentences(text, source_name);
    if sentences.is_empty() {
        return Err(SyntaxError::EmptyProgram);
    }

    let mut chunks: Vec<Vec<Sentence>> = Vec::new();
    let mut current = Vec::new();
    for sentence in sentences {
        let upper = sentence.raw.to_ascii_uppercase();
        if upper.starts_with("IDENTIFICATION DIVISION") && !current.is_empty() {
            chunks.push(current);
            current = Vec::new();
        }
        current.push(sentence);
    }
    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
        .into_iter()
        .map(|chunk| parse_program_from_sentences(source_name, chunk))
        .collect()
}

fn parse_program_from_sentences(
    source_name: &str,
    sentences: Vec<Sentence>,
) -> Result<ProgramAst, SyntaxError> {
    let text = sentences
        .iter()
        .map(|sentence| format!("{}.\n", sentence.raw))
        .collect::<String>();
    let tokens = lex(source_name, &text);
    let cst = build_lossless_tree(&tokens);
    let mut diagnostics = Vec::new();
    if sentences.is_empty() {
        return Err(SyntaxError::EmptyProgram);
    }

    let mut name = "COBOL_PROGRAM".to_string();
    let mut section = Section::Before;
    let mut is_common = false;
    let mut is_initial = false;
    let mut data_items = Vec::new();
    let mut paragraphs = Vec::new();
    let mut declaratives = Vec::new();
    let mut files = Vec::new();
    let mut same_record_areas = Vec::new();
    let mut rerun_clauses = Vec::new();
    let mut current_paragraph: Option<ParagraphAst> = None;
    let mut current_declarative: Option<DeclarativeAst> = None;
    let mut in_declaratives = false;
    let mut expecting_program_id = false;
    let mut seen_program_id = false;
    let mut data_area = StorageAreaAst::WorkingStorage;
    let mut current_file: Option<(String, FileKindAst)> = None;

    for sentence in sentences {
        let upper = sentence.raw.to_ascii_uppercase();
        if expecting_program_id {
            let Some(parsed) = parse_program_id_clause(&sentence.raw) else {
                return Err(SyntaxError::EmptyProgram);
            };
            name = parsed.0;
            is_common = parsed.1;
            is_initial = parsed.2;
            expecting_program_id = false;
            seen_program_id = true;
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
            let Some(parsed) = parse_program_id_clause(&sentence.raw) else {
                return Err(SyntaxError::EmptyProgram);
            };
            name = parsed.0;
            is_common = parsed.1;
            is_initial = parsed.2;
            seen_program_id = true;
            continue;
        }
        if upper.starts_with("END PROGRAM") {
            continue;
        }

        match section {
            Section::Data => {
                if upper.starts_with("WORKING-STORAGE SECTION") {
                    data_area = StorageAreaAst::WorkingStorage;
                    continue;
                }
                if upper.starts_with("LOCAL-STORAGE SECTION") {
                    data_area = StorageAreaAst::LocalStorage;
                    continue;
                }
                if upper.starts_with("LINKAGE SECTION") {
                    data_area = StorageAreaAst::Linkage;
                    continue;
                }
                if upper.starts_with("FILE SECTION") {
                    data_area = StorageAreaAst::FileSection;
                    continue;
                }
                if let Some(item) = parse_data_decl(&sentence.raw, sentence.span.clone(), data_area)
                {
                    data_items.push(item);
                }
            }
            Section::Procedure => {
                if upper == "DECLARATIVES" {
                    if let Some(paragraph) = current_paragraph.take() {
                        paragraphs.push(paragraph);
                    }
                    in_declaratives = true;
                    continue;
                }
                if in_declaratives && upper == "END DECLARATIVES" {
                    if let Some(declarative) = current_declarative.take() {
                        declaratives.push(declarative);
                    }
                    in_declaratives = false;
                    continue;
                }
                if in_declaratives {
                    if is_section_label(&sentence.raw) {
                        if let Some(declarative) = current_declarative.take() {
                            declaratives.push(declarative);
                        }
                        current_declarative = Some(DeclarativeAst {
                            name: sanitize_cobol_name(
                                words(&sentence.raw)
                                    .first()
                                    .map(String::as_str)
                                    .unwrap_or("DECL"),
                            ),
                            trigger: DeclarativeTriggerAst::Missing,
                            statements: Vec::new(),
                            span: sentence.span,
                        });
                        continue;
                    }
                    if upper.starts_with("USE ") {
                        let trigger = parse_declarative_trigger(&sentence.raw);
                        if current_declarative.is_none() {
                            current_declarative = Some(DeclarativeAst {
                                name: "DECLARATIVE".to_string(),
                                trigger: DeclarativeTriggerAst::Missing,
                                statements: Vec::new(),
                                span: sentence.span.clone(),
                            });
                        }
                        if let Some(declarative) = &mut current_declarative {
                            declarative.trigger = trigger;
                        }
                        continue;
                    }
                    let procedure_sentence =
                        parse_procedure_sentence(&sentence.raw, sentence.span.clone());
                    report_unsupported_statements(&procedure_sentence.statements, &mut diagnostics);
                    if current_declarative.is_none() {
                        current_declarative = Some(DeclarativeAst {
                            name: "DECLARATIVE".to_string(),
                            trigger: DeclarativeTriggerAst::Missing,
                            statements: Vec::new(),
                            span: sentence.span.clone(),
                        });
                    }
                    if let Some(declarative) = &mut current_declarative {
                        declarative.statements.extend(procedure_sentence.statements);
                    }
                    continue;
                }
                if is_paragraph_label(&sentence.raw) {
                    if let Some(paragraph) = current_paragraph.take() {
                        paragraphs.push(paragraph);
                    }
                    current_paragraph = Some(ParagraphAst {
                        name: sanitize_cobol_name(&sentence.raw),
                        statements: Vec::new(),
                        sentences: Vec::new(),
                        span: sentence.span.clone(),
                    });
                } else {
                    let procedure_sentence =
                        parse_procedure_sentence(&sentence.raw, sentence.span.clone());
                    report_unsupported_statements(&procedure_sentence.statements, &mut diagnostics);
                    if current_paragraph.is_none() {
                        current_paragraph = Some(ParagraphAst {
                            name: "MAIN".to_string(),
                            statements: Vec::new(),
                            sentences: Vec::new(),
                            span: SourceSpan {
                                file: source_name.to_string(),
                                line: 1,
                                column: 1,
                            },
                        });
                    }
                    if let Some(paragraph) = &mut current_paragraph {
                        push_procedure_sentence(paragraph, procedure_sentence);
                    }
                }
            }
            Section::Environment => {
                if let Some(same_record_area) =
                    parse_same_record_area(&sentence.raw, sentence.span.clone())
                {
                    same_record_areas.push(same_record_area);
                }
                if let Some(rerun_clause) = parse_rerun_clause(&sentence.raw, sentence.span.clone())
                {
                    rerun_clauses.push(rerun_clause);
                }
                if let Some(file) = parse_select(&sentence.raw, sentence.span.clone()) {
                    files.push(file);
                }
            }
            Section::Before | Section::Identification => {}
        }

        if matches!(section, Section::Data)
            && (upper.starts_with("FD ") || upper.starts_with("SD "))
        {
            let kind = if upper.starts_with("SD ") {
                FileKindAst::Sd
            } else {
                FileKindAst::Fd
            };
            current_file = sentence
                .raw
                .split_whitespace()
                .nth(1)
                .map(|name| (sanitize_cobol_name(name), kind));
            if kind == FileKindAst::Sd
                && !files.iter().any(|file| {
                    file.name
                        .eq_ignore_ascii_case(&current_file.as_ref().unwrap().0)
                })
            {
                if let Some((name, _)) = &current_file {
                    files.push(FileAst {
                        name: name.clone(),
                        kind,
                        assign: None,
                        assign_is_literal: false,
                        organization: None,
                        access_mode: None,
                        file_status: None,
                        record_name: None,
                        linage: parse_file_linage(&sentence.raw),
                        span: sentence.span.clone(),
                    });
                }
            }
            if let Some((file_name, _)) = &current_file {
                if let Some(file) = files
                    .iter_mut()
                    .find(|file| file.name.eq_ignore_ascii_case(file_name))
                {
                    file.linage = parse_file_linage(&sentence.raw).or(file.linage);
                }
            }
        }
        if matches!(section, Section::Data)
            && data_area == StorageAreaAst::FileSection
            && !upper.starts_with("FD ")
            && !upper.starts_with("SD ")
        {
            if let Some(item) = data_items.last() {
                if item.level == 1 {
                    if let Some((file_name, file_kind)) = current_file.take() {
                        if let Some(file) = files
                            .iter_mut()
                            .find(|file| file.name.eq_ignore_ascii_case(&file_name))
                        {
                            file.kind = file_kind;
                            file.record_name = Some(item.name.clone());
                        } else {
                            files.push(FileAst {
                                name: file_name,
                                kind: file_kind,
                                assign: None,
                                assign_is_literal: false,
                                organization: None,
                                access_mode: None,
                                file_status: None,
                                record_name: Some(item.name.clone()),
                                linage: None,
                                span: item.span.clone(),
                            });
                        }
                    }
                }
            }
        }
    }

    if let Some(paragraph) = current_paragraph {
        paragraphs.push(paragraph);
    }
    if let Some(declarative) = current_declarative {
        declaratives.push(declarative);
    }
    if !seen_program_id || expecting_program_id {
        return Err(SyntaxError::EmptyProgram);
    }

    Ok(ProgramAst {
        name,
        is_common,
        is_initial,
        data_items,
        files,
        same_record_areas,
        rerun_clauses,
        paragraphs,
        declaratives,
        diagnostics,
        cst,
    })
}

fn parse_program_id_clause(raw: &str) -> Option<(String, bool, bool)> {
    let words = raw
        .split_whitespace()
        .map(|word| word.trim_end_matches('.'))
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    let start = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("PROGRAM-ID"))
        .map(|idx| idx + 1)
        .unwrap_or(0);
    if words
        .iter()
        .skip(start)
        .any(|word| word.eq_ignore_ascii_case("DIVISION") || word.eq_ignore_ascii_case("SECTION"))
    {
        return None;
    }
    let name = words
        .iter()
        .skip(start)
        .find(|word| {
            !word.eq_ignore_ascii_case("IS")
                && !word.eq_ignore_ascii_case("COMMON")
                && !word.eq_ignore_ascii_case("INITIAL")
                && !word.eq_ignore_ascii_case("PROGRAM")
        })
        .copied()?;
    let is_common = words.iter().any(|word| word.eq_ignore_ascii_case("COMMON"));
    let is_initial = words
        .iter()
        .any(|word| word.eq_ignore_ascii_case("INITIAL"));
    Some((sanitize_cobol_name(name), is_common, is_initial))
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
    let mut line = 1usize;
    let mut column = 1usize;
    let mut start_line = 1usize;
    let mut start_column = 1usize;
    let mut seen_content = false;

    for item in cobol_text::literal_aware_char_indices(text) {
        let byte_idx = item.byte_idx;
        let ch = item.ch;
        if !seen_content && !ch.is_whitespace() {
            start_line = line;
            start_column = column;
            seen_content = true;
        }
        if ch == '.' && !item.inside_literal && is_sentence_period(text, byte_idx, &current) {
            let raw = sentence_raw_before_period(text, byte_idx, &current);
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

fn sentence_raw_before_period(text: &str, byte_idx: usize, current: &str) -> String {
    let trimmed = current.trim();
    if trimmed.eq_ignore_ascii_case("GO TO")
        && text[..byte_idx]
            .chars()
            .next_back()
            .is_some_and(|ch| ch.is_whitespace())
    {
        "GO TO .".to_string()
    } else {
        trimmed.to_string()
    }
}

fn parse_data_decl(
    raw: &str,
    span: SourceSpan,
    storage_area: StorageAreaAst,
) -> Option<DataDeclAst> {
    let mut parts = raw.split_whitespace();
    let level = parts.next()?.parse::<u8>().ok()?;
    let name = parts.next()?.trim_end_matches('.').to_string();
    let clauses = parts.collect::<Vec<_>>().join(" ");
    let clause_ast = parse_data_clauses(&clauses);
    Some(DataDeclAst {
        level,
        name: sanitize_cobol_name(&name),
        clauses,
        clause_ast,
        storage_area,
        span,
    })
}

fn parse_data_clauses(clauses: &str) -> Vec<DataClauseAst> {
    let parts = words(clauses);
    let mut out = Vec::new();
    let mut idx = 0usize;
    while idx < parts.len() {
        match parts[idx]
            .trim_end_matches('.')
            .to_ascii_uppercase()
            .as_str()
        {
            "PIC" | "PICTURE" => {
                let value_idx = if parts
                    .get(idx + 1)
                    .map(|word| word.eq_ignore_ascii_case("IS"))
                    .unwrap_or(false)
                {
                    idx + 2
                } else {
                    idx + 1
                };
                if let Some(value) = parts.get(value_idx) {
                    out.push(DataClauseAst::Picture(
                        value.trim_end_matches('.').to_string(),
                    ));
                    idx = value_idx + 1;
                } else {
                    out.push(DataClauseAst::Other(parts[idx].to_string()));
                    idx += 1;
                }
            }
            "USAGE" => {
                if let Some(value) = parts.get(idx + 2).filter(|_| {
                    parts
                        .get(idx + 1)
                        .map(|word| word.eq_ignore_ascii_case("IS"))
                        .unwrap_or(false)
                }) {
                    out.push(DataClauseAst::Usage(
                        value.trim_end_matches('.').to_ascii_uppercase(),
                    ));
                    idx += 3;
                } else if let Some(value) = parts.get(idx + 1) {
                    out.push(DataClauseAst::Usage(
                        value.trim_end_matches('.').to_ascii_uppercase(),
                    ));
                    idx += 2;
                } else {
                    out.push(DataClauseAst::Other(parts[idx].to_string()));
                    idx += 1;
                }
            }
            "COMP" | "COMP-1" | "COMP-2" | "COMP-3" | "COMP-4" | "COMP-5" | "BINARY"
            | "PACKED-DECIMAL" | "NATIONAL" | "DISPLAY-1" | "DBCS" | "KANJI" | "POINTER"
            | "PROCEDURE-POINTER" => {
                out.push(DataClauseAst::Usage(
                    parts[idx].trim_end_matches('.').to_ascii_uppercase(),
                ));
                idx += 1;
            }
            "OCCURS" => {
                let (clause, next) = parse_occurs_clause(&parts, idx);
                out.push(clause);
                idx = next;
            }
            "REDEFINES" => {
                if let Some(value) = parts.get(idx + 1) {
                    out.push(DataClauseAst::Redefines(sanitize_cobol_name(value)));
                    idx += 2;
                } else {
                    out.push(DataClauseAst::Other(parts[idx].to_string()));
                    idx += 1;
                }
            }
            "RENAMES" => {
                if let Some(first) = parts.get(idx + 1) {
                    let mut last = None;
                    let mut next = idx + 2;
                    if parts
                        .get(next)
                        .map(|word| {
                            word.eq_ignore_ascii_case("THRU")
                                || word.eq_ignore_ascii_case("THROUGH")
                        })
                        .unwrap_or(false)
                    {
                        last = parts.get(next + 1).map(|value| sanitize_cobol_name(value));
                        next += 2;
                    }
                    out.push(DataClauseAst::Renames {
                        first: sanitize_cobol_name(first),
                        last,
                    });
                    idx = next;
                } else {
                    out.push(DataClauseAst::Other(parts[idx].to_string()));
                    idx += 1;
                }
            }
            "VALUE" | "VALUES" => {
                let (clause, next) = parse_data_value_clause(&parts, idx);
                out.push(clause);
                idx = next;
            }
            "SYNC" | "SYNCHRONIZED" => {
                out.push(DataClauseAst::Sync);
                idx += 1;
            }
            "EXTERNAL" => {
                out.push(DataClauseAst::External);
                idx += 1;
            }
            "GLOBAL" => {
                out.push(DataClauseAst::Global);
                idx += 1;
            }
            "SIGN" => {
                let (clause, next) = parse_sign_clause(&parts, idx);
                out.push(clause);
                idx = next;
            }
            "JUST" | "JUSTIFIED" => {
                let right = parts
                    .get(idx + 1)
                    .map(|word| data_clause_word_eq(word, "RIGHT"))
                    .unwrap_or(false);
                out.push(DataClauseAst::Justified { right });
                idx += if right { 2 } else { 1 };
            }
            "BLANK" => {
                let has_when = parts
                    .get(idx + 1)
                    .map(|word| data_clause_word_eq(word, "WHEN"))
                    .unwrap_or(false);
                let zero_idx = if has_when { idx + 2 } else { idx + 1 };
                if parts
                    .get(zero_idx)
                    .map(|word| data_clause_word_eq(word, "ZERO"))
                    .unwrap_or(false)
                {
                    out.push(DataClauseAst::BlankWhenZero);
                    idx = zero_idx + 1;
                } else {
                    out.push(DataClauseAst::Other(parts[idx].to_string()));
                    idx += 1;
                }
            }
            "BASED" => {
                let pointer = if parts
                    .get(idx + 1)
                    .map(|word| data_clause_word_eq(word, "ON"))
                    .unwrap_or(false)
                {
                    parts.get(idx + 2).map(|value| sanitize_cobol_name(value))
                } else {
                    None
                };
                out.push(DataClauseAst::Based { pointer });
                idx += if parts
                    .get(idx + 1)
                    .map(|word| data_clause_word_eq(word, "ON"))
                    .unwrap_or(false)
                    && parts.get(idx + 2).is_some()
                {
                    3
                } else {
                    1
                };
            }
            "ANY-LENGTH" => {
                out.push(DataClauseAst::AnyLength);
                idx += 1;
            }
            "ANY" => {
                if parts
                    .get(idx + 1)
                    .map(|word| data_clause_word_eq(word, "LENGTH"))
                    .unwrap_or(false)
                {
                    out.push(DataClauseAst::AnyLength);
                    idx += 2;
                } else {
                    out.push(DataClauseAst::Other(parts[idx].to_string()));
                    idx += 1;
                }
            }
            _ => {
                out.push(DataClauseAst::Other(parts[idx].to_string()));
                idx += 1;
            }
        }
    }
    out
}

fn parse_data_value_clause(parts: &[String], idx: usize) -> (DataClauseAst, usize) {
    let mut cursor = idx + 1;
    if parts
        .get(cursor)
        .map(|word| word.eq_ignore_ascii_case("IS") || word.eq_ignore_ascii_case("ARE"))
        .unwrap_or(false)
    {
        cursor += 1;
    }

    let mut values = Vec::new();
    while cursor < parts.len() {
        if !values.is_empty() && is_data_clause_starter(&parts[cursor]) {
            break;
        }
        if parts[cursor] == "," {
            cursor += 1;
            continue;
        }

        let start = clean_data_value_literal(&parts[cursor]);
        if start.is_empty() {
            cursor += 1;
            continue;
        }

        if parts
            .get(cursor + 1)
            .map(|word| word.eq_ignore_ascii_case("THRU") || word.eq_ignore_ascii_case("THROUGH"))
            .unwrap_or(false)
        {
            if let Some(end) = parts.get(cursor + 2) {
                values.push(DataValueAst::Range {
                    start,
                    end: clean_data_value_literal(end),
                });
                cursor += 3;
                continue;
            }
        }

        values.push(DataValueAst::Single(start));
        cursor += 1;
    }

    match values.as_slice() {
        [DataValueAst::Single(value)] => (DataClauseAst::Value(value.clone()), cursor),
        [] => (DataClauseAst::Other(parts[idx].to_string()), idx + 1),
        _ => (DataClauseAst::Values(values), cursor),
    }
}

fn parse_sign_clause(parts: &[String], idx: usize) -> (DataClauseAst, usize) {
    let mut cursor = idx + 1;
    if parts
        .get(cursor)
        .map(|word| data_clause_word_eq(word, "IS"))
        .unwrap_or(false)
    {
        cursor += 1;
    }

    let position = if parts
        .get(cursor)
        .map(|word| data_clause_word_eq(word, "LEADING"))
        .unwrap_or(false)
    {
        cursor += 1;
        Some(DataSignPositionAst::Leading)
    } else if parts
        .get(cursor)
        .map(|word| data_clause_word_eq(word, "TRAILING"))
        .unwrap_or(false)
    {
        cursor += 1;
        Some(DataSignPositionAst::Trailing)
    } else {
        None
    };

    let separate = if parts
        .get(cursor)
        .map(|word| data_clause_word_eq(word, "SEPARATE"))
        .unwrap_or(false)
    {
        cursor += 1;
        if parts
            .get(cursor)
            .map(|word| data_clause_word_eq(word, "CHARACTER"))
            .unwrap_or(false)
        {
            cursor += 1;
        }
        true
    } else {
        false
    };

    (DataClauseAst::Sign { position, separate }, cursor)
}

fn data_clause_word_eq(word: &str, expected: &str) -> bool {
    word.trim_end_matches('.')
        .trim_end_matches(',')
        .eq_ignore_ascii_case(expected)
}

fn parse_occurs_clause(parts: &[String], idx: usize) -> (DataClauseAst, usize) {
    let min = parts
        .get(idx + 1)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1);
    let mut max = min;
    let mut cursor = idx + 2;
    if parts
        .get(cursor)
        .map(|word| word.eq_ignore_ascii_case("TO"))
        .unwrap_or(false)
    {
        max = parts
            .get(cursor + 1)
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(min);
        cursor += 2;
    }

    let end = data_clause_end(parts, cursor);
    let depending_on = find_occurs_depending_on(parts, cursor, end);
    let indexed_by = find_occurs_indexed_by(parts, cursor, end);
    let keys = find_occurs_keys(parts, cursor, end);
    (
        DataClauseAst::Occurs {
            min,
            max,
            depending_on,
            indexed_by,
            keys,
        },
        end,
    )
}

fn data_clause_end(parts: &[String], start: usize) -> usize {
    let mut cursor = start;
    while cursor < parts.len() {
        if is_independent_data_clause_starter(&parts[cursor]) {
            break;
        }
        cursor += 1;
    }
    cursor
}

fn find_occurs_depending_on(parts: &[String], start: usize, end: usize) -> Option<String> {
    parts
        .iter()
        .enumerate()
        .take(end)
        .skip(start)
        .find_map(|(dep_idx, word)| {
            if word.eq_ignore_ascii_case("DEPENDING")
                && parts
                    .get(dep_idx + 1)
                    .map(|value| value.eq_ignore_ascii_case("ON"))
                    .unwrap_or(false)
            {
                parts
                    .get(dep_idx + 2)
                    .map(|value| sanitize_cobol_name(value))
            } else {
                None
            }
        })
}

fn find_occurs_indexed_by(parts: &[String], start: usize, end: usize) -> Vec<String> {
    let Some(indexed_idx) = parts
        .iter()
        .enumerate()
        .take(end)
        .skip(start)
        .find_map(|(idx, word)| word.eq_ignore_ascii_case("INDEXED").then_some(idx))
    else {
        return Vec::new();
    };
    if !parts
        .get(indexed_idx + 1)
        .map(|word| word.eq_ignore_ascii_case("BY"))
        .unwrap_or(false)
    {
        return Vec::new();
    }
    let mut indexes = Vec::new();
    let mut cursor = indexed_idx + 2;
    while cursor < end {
        let upper = parts[cursor]
            .trim_end_matches('.')
            .trim_end_matches(',')
            .to_ascii_uppercase();
        if matches!(upper.as_str(), "ASCENDING" | "DESCENDING" | "DEPENDING") {
            break;
        }
        if upper != "," && !matches!(upper.as_str(), "BY" | "ON" | "KEY" | "IS" | "ARE") {
            indexes.push(sanitize_cobol_name(
                parts[cursor].trim_end_matches(',').trim_end_matches('.'),
            ));
        }
        cursor += 1;
    }
    indexes
}

fn find_occurs_keys(parts: &[String], start: usize, end: usize) -> Vec<DataOccursKeyAst> {
    let mut keys = Vec::new();
    let mut cursor = start;
    while cursor < end {
        let direction = match parts[cursor].to_ascii_uppercase().as_str() {
            "ASCENDING" => Some(DataOccursKeyDirectionAst::Ascending),
            "DESCENDING" => Some(DataOccursKeyDirectionAst::Descending),
            _ => None,
        };
        let Some(direction) = direction else {
            cursor += 1;
            continue;
        };
        cursor += 1;
        if parts
            .get(cursor)
            .map(|word| word.eq_ignore_ascii_case("KEY"))
            .unwrap_or(false)
        {
            cursor += 1;
        }
        if parts
            .get(cursor)
            .map(|word| word.eq_ignore_ascii_case("IS") || word.eq_ignore_ascii_case("ARE"))
            .unwrap_or(false)
        {
            cursor += 1;
        }
        while cursor < end {
            let upper = parts[cursor]
                .trim_end_matches('.')
                .trim_end_matches(',')
                .to_ascii_uppercase();
            if matches!(
                upper.as_str(),
                "ASCENDING" | "DESCENDING" | "INDEXED" | "DEPENDING"
            ) {
                break;
            }
            if upper != ","
                && !matches!(upper.as_str(), "KEY" | "IS" | "ARE" | "BY" | "ON" | "TIMES")
            {
                keys.push(DataOccursKeyAst {
                    direction,
                    name: sanitize_cobol_name(
                        parts[cursor].trim_end_matches(',').trim_end_matches('.'),
                    ),
                });
            }
            cursor += 1;
        }
    }
    keys
}

fn is_data_clause_starter(word: &str) -> bool {
    matches!(
        word.trim_end_matches('.').to_ascii_uppercase().as_str(),
        "PIC"
            | "PICTURE"
            | "USAGE"
            | "COMP"
            | "COMP-1"
            | "COMP-2"
            | "COMP-3"
            | "COMP-4"
            | "COMP-5"
            | "BINARY"
            | "PACKED-DECIMAL"
            | "POINTER"
            | "PROCEDURE-POINTER"
            | "OCCURS"
            | "REDEFINES"
            | "RENAMES"
            | "VALUE"
            | "VALUES"
            | "SYNC"
            | "SYNCHRONIZED"
            | "EXTERNAL"
            | "GLOBAL"
            | "SIGN"
            | "JUST"
            | "JUSTIFIED"
            | "BLANK"
            | "BASED"
            | "ANY"
            | "ANY-LENGTH"
    )
}

fn is_independent_data_clause_starter(word: &str) -> bool {
    matches!(
        word.trim_end_matches('.').to_ascii_uppercase().as_str(),
        "PIC"
            | "PICTURE"
            | "USAGE"
            | "COMP"
            | "COMP-1"
            | "COMP-2"
            | "COMP-3"
            | "COMP-4"
            | "COMP-5"
            | "BINARY"
            | "PACKED-DECIMAL"
            | "POINTER"
            | "PROCEDURE-POINTER"
            | "REDEFINES"
            | "RENAMES"
            | "VALUE"
            | "VALUES"
            | "SYNC"
            | "SYNCHRONIZED"
            | "EXTERNAL"
            | "GLOBAL"
            | "SIGN"
            | "JUST"
            | "JUSTIFIED"
            | "BLANK"
            | "BASED"
            | "ANY"
            | "ANY-LENGTH"
    )
}

fn clean_data_value_literal(value: &str) -> String {
    value
        .trim_end_matches('.')
        .trim_end_matches(',')
        .trim_matches('"')
        .trim_matches('\'')
        .to_string()
}

fn word_texts(words: &[SpannedWord]) -> Vec<String> {
    words.iter().map(|word| word.text.clone()).collect()
}

fn raw_from_words(words: &[SpannedWord], raw_source: &str) -> String {
    let Some(first) = words.first() else {
        return String::new();
    };
    let last = words.last().expect("nonempty words");
    raw_source[first.start..last.end].to_string()
}

fn parse_statement(raw: &str, span: SourceSpan) -> StatementAst {
    let raw = raw.trim();
    let words = cobol_text::split_cobol_words_spanned(raw);
    parse_statement_from_words(&words, raw, span)
}

fn parse_procedure_sentence(raw: &str, span: SourceSpan) -> ProcedureSentenceAst {
    let raw = raw.trim();
    let tokens = cobol_text::split_cobol_words_spanned(raw);
    let statements = if tokens.is_empty() {
        Vec::new()
    } else {
        let (statements, next) = parse_imperative_tokens(&tokens, raw, 0, &[], span.clone());
        if next >= tokens.len() && !statements.is_empty() {
            statements
        } else {
            vec![parse_statement(raw, span.clone())]
        }
    };
    ProcedureSentenceAst { statements, span }
}

fn push_procedure_sentence(paragraph: &mut ParagraphAst, sentence: ProcedureSentenceAst) {
    if sentence.statements.is_empty() {
        return;
    }
    paragraph
        .statements
        .extend(sentence.statements.iter().cloned());
    paragraph.sentences.push(sentence);
}

fn report_unsupported_statements(statements: &[StatementAst], diagnostics: &mut Vec<Diagnostic>) {
    for statement in statements {
        if let StatementKindAst::Unsupported(keyword) = &statement.kind {
            diagnostics.push(Diagnostic::error(
                "E_UNSUPPORTED_STATEMENT",
                format!("unsupported COBOL statement: {keyword}"),
                statement.span.clone(),
            ));
        }
    }
}

fn parse_statement_from_words(
    spanned_words: &[SpannedWord],
    sentence_raw: &str,
    span: SourceSpan,
) -> StatementAst {
    let words = word_texts(spanned_words);
    let raw = raw_from_words(spanned_words, sentence_raw);
    let first = words
        .first()
        .map(|word| word.to_ascii_uppercase())
        .unwrap_or_default();
    let kind = match first.as_str() {
        "DISPLAY" => parse_display_statement(spanned_words, sentence_raw, &words),
        "MOVE" => parse_move_statement(&words)
            .unwrap_or_else(|| StatementKindAst::Unsupported("MOVE".to_string())),
        "ADD" => parse_add_statement(&words),
        "SUBTRACT" => parse_binary_target_statement(&words, "FROM")
            .map(|(source, target)| StatementKindAst::Subtract { source, target })
            .unwrap_or_else(|| StatementKindAst::Unsupported("SUBTRACT".to_string())),
        "MULTIPLY" => parse_binary_target_statement(&words, "BY")
            .map(|(source, target)| StatementKindAst::Multiply { source, target })
            .unwrap_or_else(|| StatementKindAst::Unsupported("MULTIPLY".to_string())),
        "DIVIDE" => parse_binary_target_statement(&words, "INTO")
            .map(|(source, target)| StatementKindAst::Divide { source, target })
            .unwrap_or_else(|| StatementKindAst::Unsupported("DIVIDE".to_string())),
        "COMPUTE" => parse_compute_ast(spanned_words, &words, sentence_raw, span.clone())
            .map(StatementKindAst::Compute)
            .unwrap_or_else(|| StatementKindAst::Unsupported("COMPUTE".to_string())),
        "NEXT"
            if words
                .get(1)
                .map(|word| word.eq_ignore_ascii_case("SENTENCE"))
                == Some(true)
                && words.len() == 2 =>
        {
            StatementKindAst::NextSentence
        }
        "IF" => parse_if_statement(spanned_words, &words, sentence_raw, span.clone())
            .unwrap_or_else(|| StatementKindAst::Unsupported("IF".to_string())),
        "EVALUATE" => parse_evaluate_ast(spanned_words, &words, sentence_raw, span.clone())
            .map(StatementKindAst::Evaluate)
            .unwrap_or_else(|| StatementKindAst::Unsupported("EVALUATE".to_string())),
        "SEARCH" => parse_search_ast(spanned_words, &words, sentence_raw, span.clone())
            .map(StatementKindAst::Search)
            .unwrap_or_else(|| StatementKindAst::Unsupported("SEARCH".to_string())),
        "SET" => {
            parse_set(&words).unwrap_or_else(|| StatementKindAst::Unsupported("SET".to_string()))
        }
        "PERFORM" => parse_perform(&words),
        "GO" => {
            if words.get(1).map(|w| w.eq_ignore_ascii_case("TO")) == Some(true) {
                parse_go_to(&words)
            } else {
                StatementKindAst::Unsupported("GO".to_string())
            }
        }
        "GOBACK" if words.len() == 1 => StatementKindAst::StopRun,
        "GOBACK" => StatementKindAst::Unsupported("GOBACK".to_string()),
        "STOP" => {
            if words.len() == 2
                && words.get(1).map(|word| word.eq_ignore_ascii_case("RUN")) == Some(true)
            {
                StatementKindAst::StopRun
            } else {
                StatementKindAst::Unsupported("STOP".to_string())
            }
        }
        "CALL" => parse_call(&words),
        "OPEN" => parse_open_file_ast(&words)
            .map(StatementKindAst::Open)
            .unwrap_or_else(|| StatementKindAst::Unsupported("OPEN".to_string())),
        "READ" => parse_read_file_ast(spanned_words, &words, sentence_raw, span.clone())
            .map(StatementKindAst::Read)
            .unwrap_or_else(|| StatementKindAst::Unsupported("READ".to_string())),
        "WRITE" => parse_write_file_ast(&words)
            .map(StatementKindAst::Write)
            .unwrap_or_else(|| StatementKindAst::Unsupported("WRITE".to_string())),
        "REWRITE" => parse_rewrite_file_ast(spanned_words, &words, sentence_raw, span.clone())
            .map(StatementKindAst::Rewrite)
            .unwrap_or_else(|| StatementKindAst::Unsupported("REWRITE".to_string())),
        "DELETE" => parse_delete_file_ast(spanned_words, &words, sentence_raw, span.clone())
            .map(StatementKindAst::Delete)
            .unwrap_or_else(|| StatementKindAst::Unsupported("DELETE".to_string())),
        "CLOSE" => parse_close_file_ast(&words)
            .map(StatementKindAst::Close)
            .unwrap_or_else(|| StatementKindAst::Unsupported("CLOSE".to_string())),
        "SORT" => parse_sort_procedure_ast(&words)
            .map(StatementKindAst::Sort)
            .unwrap_or_else(|| StatementKindAst::Unsupported("SORT".to_string())),
        "RELEASE" => parse_release_sort_record_ast(&words)
            .map(StatementKindAst::Release)
            .unwrap_or_else(|| StatementKindAst::Unsupported("RELEASE".to_string())),
        "RETURN" => parse_return_sort_record_ast(spanned_words, &words, sentence_raw, span.clone())
            .map(StatementKindAst::Return)
            .unwrap_or_else(|| StatementKindAst::Unsupported("RETURN".to_string())),
        "INSPECT" => parse_inspect_ast(&words)
            .map(StatementKindAst::Inspect)
            .unwrap_or_else(|| StatementKindAst::Unsupported("INSPECT".to_string())),
        "EXAMINE" => parse_examine_ast(&words)
            .map(StatementKindAst::Examine)
            .unwrap_or_else(|| StatementKindAst::Unsupported("EXAMINE".to_string())),
        "STRING" => parse_string_ast(spanned_words, &words, sentence_raw, span.clone())
            .map(StatementKindAst::String)
            .unwrap_or_else(|| StatementKindAst::Unsupported("STRING".to_string())),
        "UNSTRING" => parse_unstring_ast(spanned_words, &words, sentence_raw, span.clone())
            .map(StatementKindAst::Unstring)
            .unwrap_or_else(|| StatementKindAst::Unsupported("UNSTRING".to_string())),
        "READY" => {
            if words.len() == 2
                && words.get(1).map(|word| word.eq_ignore_ascii_case("TRACE")) == Some(true)
            {
                StatementKindAst::ReadyTrace
            } else {
                StatementKindAst::Unsupported("READY".to_string())
            }
        }
        "RESET" => {
            if words.len() == 2
                && words.get(1).map(|word| word.eq_ignore_ascii_case("TRACE")) == Some(true)
            {
                StatementKindAst::ResetTrace
            } else {
                StatementKindAst::Unsupported("RESET".to_string())
            }
        }
        "CONTINUE" if words.len() == 1 => StatementKindAst::Continue,
        "ALTER" => parse_alter(&words),
        "EXEC" | "MERGE" => StatementKindAst::Unsupported(first),
        _ => StatementKindAst::Unsupported(first),
    };
    StatementAst { kind, raw, span }
}

pub fn parse_imperative_list(raw: &str, span: SourceSpan) -> ImperativeListAst {
    let raw = raw.trim().trim_end_matches('.');
    let tokens = cobol_text::split_cobol_words_spanned(raw);
    let (statements, _) = parse_imperative_tokens(&tokens, raw, 0, &[], span);
    statements
}

fn parse_go_to(words: &[String]) -> StatementKindAst {
    let depending_idx = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("DEPENDING"));
    if let Some(dep_idx) = depending_idx {
        if words
            .get(dep_idx + 1)
            .map(|word| word.eq_ignore_ascii_case("ON"))
            != Some(true)
        {
            return StatementKindAst::Unsupported("GO".to_string());
        }
        let targets = words
            .get(2..dep_idx)
            .unwrap_or_default()
            .iter()
            .map(|target| sanitize_cobol_name(target))
            .collect::<Vec<_>>();
        let Some(depending_on) = words.get(dep_idx + 2) else {
            return StatementKindAst::Unsupported("GO".to_string());
        };
        if targets.is_empty() || dep_idx + 3 != words.len() {
            return StatementKindAst::Unsupported("GO".to_string());
        }
        return StatementKindAst::ComputedGoTo {
            targets,
            depending_on: depending_on.clone(),
        };
    }
    if words.len() != 3 {
        return StatementKindAst::Unsupported("GO".to_string());
    }
    words
        .get(2)
        .map(|target| {
            if target == "." {
                StatementKindAst::GoTo(".".to_string())
            } else {
                StatementKindAst::GoTo(sanitize_cobol_name(target))
            }
        })
        .unwrap_or_else(|| StatementKindAst::Unsupported("GO".to_string()))
}

fn parse_alter(words: &[String]) -> StatementKindAst {
    let Some(paragraph) = words.get(1) else {
        return StatementKindAst::Unsupported("ALTER".to_string());
    };
    let target = match words.len() {
        4 if words[2].eq_ignore_ascii_case("TO") => words.get(3),
        6 if words[2].eq_ignore_ascii_case("TO")
            && words[3].eq_ignore_ascii_case("PROCEED")
            && words[4].eq_ignore_ascii_case("TO") =>
        {
            words.get(5)
        }
        _ => None,
    };
    let Some(target) = target else {
        return StatementKindAst::Unsupported("ALTER".to_string());
    };
    StatementKindAst::Alter {
        paragraph: sanitize_cobol_name(paragraph),
        target: sanitize_cobol_name(target),
    }
}

fn branch_statements_ast(
    tokens: &[SpannedWord],
    raw_source: &str,
    span: SourceSpan,
) -> ImperativeListAst {
    if tokens.is_empty() {
        Vec::new()
    } else {
        parse_imperative_tokens(tokens, raw_source, 0, &[], span).0
    }
}

fn parse_open_file_ast(tokens: &[String]) -> Option<OpenFileAst> {
    if tokens.len() < 2 || !tokens[0].eq_ignore_ascii_case("OPEN") {
        return None;
    }
    let (mode, file_idx) = match tokens.get(1).map(|token| token.to_ascii_uppercase()) {
        Some(token) if token == "INPUT" => (FileOpenModeAst::Input, 2),
        Some(token) if token == "OUTPUT" => (FileOpenModeAst::Output, 2),
        Some(token) if token == "I-O" || token == "IO" => (FileOpenModeAst::Io, 2),
        Some(token) if token == "EXTEND" => (FileOpenModeAst::Extend, 2),
        Some(_) => (FileOpenModeAst::Input, 1),
        None => return None,
    };
    if tokens.len() != file_idx + 1 {
        return None;
    }
    Some(OpenFileAst {
        file: sanitize_cobol_name(tokens.get(file_idx)?),
        mode,
    })
}

#[derive(Debug, Clone, Copy)]
enum ReadBranchAst {
    AtEnd,
    NotAtEnd,
    OnException,
}

fn parse_read_file_ast(
    tokens: &[SpannedWord],
    words: &[String],
    raw_source: &str,
    span: SourceSpan,
) -> Option<ReadFileAst> {
    if words.len() < 2 || !words[0].eq_ignore_ascii_case("READ") {
        return None;
    }
    if find_not_on_exception_idx_ast(words).is_some() {
        return None;
    }
    let first_branch = first_read_branch_idx_ast(words).unwrap_or(words.len());
    let into = words
        .iter()
        .position(|token| token.eq_ignore_ascii_case("INTO"))
        .filter(|idx| *idx < first_branch)
        .and_then(|idx| tokens.get(idx + 1))
        .map(|word| word.text.clone());
    Some(ReadFileAst {
        file: sanitize_cobol_name(words.get(1)?),
        into,
        at_end: read_branch_ast(
            tokens,
            words,
            raw_source,
            ReadBranchAst::AtEnd,
            span.clone(),
        ),
        not_at_end: read_branch_ast(
            tokens,
            words,
            raw_source,
            ReadBranchAst::NotAtEnd,
            span.clone(),
        ),
        on_exception: read_branch_ast(tokens, words, raw_source, ReadBranchAst::OnException, span),
    })
}

fn first_read_branch_idx_ast(tokens: &[String]) -> Option<usize> {
    [
        find_read_branch_idx_ast(tokens, ReadBranchAst::AtEnd),
        find_read_branch_idx_ast(tokens, ReadBranchAst::NotAtEnd),
        find_read_branch_idx_ast(tokens, ReadBranchAst::OnException),
    ]
    .into_iter()
    .flatten()
    .min()
}

fn find_read_branch_idx_ast(tokens: &[String], branch: ReadBranchAst) -> Option<usize> {
    match branch {
        ReadBranchAst::AtEnd => find_top_level_idx(tokens, 1, |idx, token| {
            token.eq_ignore_ascii_case("AT")
                && tokens
                    .get(idx + 1)
                    .is_some_and(|token| token.eq_ignore_ascii_case("END"))
                && idx
                    .checked_sub(1)
                    .and_then(|prev| tokens.get(prev))
                    .map(|token| !token.eq_ignore_ascii_case("NOT"))
                    .unwrap_or(true)
        }),
        ReadBranchAst::NotAtEnd => find_top_level_idx(tokens, 1, |idx, token| {
            token.eq_ignore_ascii_case("NOT")
                && tokens
                    .get(idx + 1)
                    .is_some_and(|token| token.eq_ignore_ascii_case("AT"))
                && tokens
                    .get(idx + 2)
                    .is_some_and(|token| token.eq_ignore_ascii_case("END"))
        }),
        ReadBranchAst::OnException => find_top_level_idx(tokens, 1, |idx, token| {
            token.eq_ignore_ascii_case("ON")
                && tokens
                    .get(idx + 1)
                    .is_some_and(|token| token.eq_ignore_ascii_case("EXCEPTION"))
                && idx
                    .checked_sub(1)
                    .and_then(|prev| tokens.get(prev))
                    .map(|token| !token.eq_ignore_ascii_case("NOT"))
                    .unwrap_or(true)
        }),
    }
}

fn find_not_on_exception_idx_ast(tokens: &[String]) -> Option<usize> {
    find_top_level_idx(tokens, 1, |idx, token| {
        token.eq_ignore_ascii_case("NOT")
            && tokens
                .get(idx + 1)
                .is_some_and(|token| token.eq_ignore_ascii_case("ON"))
            && tokens
                .get(idx + 2)
                .is_some_and(|token| token.eq_ignore_ascii_case("EXCEPTION"))
    })
}

fn read_branch_ast(
    tokens: &[SpannedWord],
    words: &[String],
    raw_source: &str,
    branch: ReadBranchAst,
    span: SourceSpan,
) -> ImperativeListAst {
    let Some(start) = find_read_branch_idx_ast(words, branch) else {
        return Vec::new();
    };
    let marker_len = match branch {
        ReadBranchAst::AtEnd | ReadBranchAst::OnException => 2,
        ReadBranchAst::NotAtEnd => 3,
    };
    let body_start = start + marker_len;
    let terminator = matching_explicit_scope_end_idx(words, 0).unwrap_or(words.len());
    let mut end = terminator;
    for candidate in [
        find_read_branch_idx_ast(words, ReadBranchAst::AtEnd),
        find_read_branch_idx_ast(words, ReadBranchAst::NotAtEnd),
        find_read_branch_idx_ast(words, ReadBranchAst::OnException),
        find_not_on_exception_idx_ast(words),
    ]
    .into_iter()
    .flatten()
    {
        if candidate > start {
            end = end.min(candidate);
        }
    }
    branch_statements_ast(
        tokens.get(body_start..end).unwrap_or_default(),
        raw_source,
        span,
    )
}

fn parse_write_file_ast(tokens: &[String]) -> Option<WriteFileAst> {
    if tokens.len() < 2 || !tokens[0].eq_ignore_ascii_case("WRITE") {
        return None;
    }
    Some(WriteFileAst {
        record: tokens.get(1)?.clone(),
        advancing: parse_write_advancing_ast(tokens),
    })
}

fn parse_write_advancing_ast(tokens: &[String]) -> WriteAdvancingAst {
    let Some(direction_idx) = tokens.iter().position(|token| {
        token.eq_ignore_ascii_case("AFTER") || token.eq_ignore_ascii_case("BEFORE")
    }) else {
        return WriteAdvancingAst::None;
    };
    if tokens
        .get(direction_idx + 1)
        .map(|token| token.eq_ignore_ascii_case("ADVANCING"))
        != Some(true)
    {
        return WriteAdvancingAst::None;
    }
    let before = tokens[direction_idx].eq_ignore_ascii_case("BEFORE");
    let operand = tokens
        .get(direction_idx + 2)
        .map(|token| token.trim_end_matches('.').to_ascii_uppercase())
        .unwrap_or_default();
    let is_page = matches!(operand.as_str(), "PAGE" | "TOP-OF-PAGE" | "TOP_OF_PAGE");
    if is_page && before {
        WriteAdvancingAst::BeforePage
    } else if is_page {
        WriteAdvancingAst::AfterPage
    } else if let Ok(lines) = operand.parse::<usize>() {
        if before {
            WriteAdvancingAst::BeforeLines(lines)
        } else {
            WriteAdvancingAst::AfterLines(lines)
        }
    } else {
        WriteAdvancingAst::None
    }
}

fn parse_rewrite_file_ast(
    tokens: &[SpannedWord],
    words: &[String],
    raw_source: &str,
    span: SourceSpan,
) -> Option<RewriteFileAst> {
    if words.len() < 2 || !words[0].eq_ignore_ascii_case("REWRITE") {
        return None;
    }
    Some(RewriteFileAst {
        record: words.get(1)?.clone(),
        invalid_key: invalid_key_branch_ast(
            tokens,
            words,
            raw_source,
            true,
            "END-REWRITE",
            span.clone(),
        ),
        not_invalid_key: invalid_key_branch_ast(
            tokens,
            words,
            raw_source,
            false,
            "END-REWRITE",
            span,
        ),
    })
}

fn parse_delete_file_ast(
    tokens: &[SpannedWord],
    words: &[String],
    raw_source: &str,
    span: SourceSpan,
) -> Option<DeleteFileAst> {
    if words.len() < 2 || !words[0].eq_ignore_ascii_case("DELETE") {
        return None;
    }
    Some(DeleteFileAst {
        file: sanitize_cobol_name(words.get(1)?),
        invalid_key: invalid_key_branch_ast(
            tokens,
            words,
            raw_source,
            true,
            "END-DELETE",
            span.clone(),
        ),
        not_invalid_key: invalid_key_branch_ast(
            tokens,
            words,
            raw_source,
            false,
            "END-DELETE",
            span,
        ),
    })
}

fn parse_close_file_ast(tokens: &[String]) -> Option<CloseFileAst> {
    if tokens.len() != 2 || !tokens[0].eq_ignore_ascii_case("CLOSE") {
        return None;
    }
    Some(CloseFileAst {
        file: sanitize_cobol_name(tokens.get(1)?),
    })
}

fn find_invalid_key_branch_idx_ast(tokens: &[String], invalid_key: bool) -> Option<usize> {
    if invalid_key {
        (0..tokens.len().saturating_sub(1)).find(|idx| {
            tokens[*idx].eq_ignore_ascii_case("INVALID")
                && tokens[*idx + 1].eq_ignore_ascii_case("KEY")
                && !idx
                    .checked_sub(1)
                    .and_then(|prev| tokens.get(prev))
                    .map(|token| token.eq_ignore_ascii_case("NOT"))
                    .unwrap_or(false)
        })
    } else {
        tokens.windows(3).position(|window| {
            window[0].eq_ignore_ascii_case("NOT")
                && window[1].eq_ignore_ascii_case("INVALID")
                && window[2].eq_ignore_ascii_case("KEY")
        })
    }
}

fn invalid_key_branch_ast(
    tokens: &[SpannedWord],
    words: &[String],
    raw_source: &str,
    invalid_key: bool,
    terminator: &str,
    span: SourceSpan,
) -> ImperativeListAst {
    let Some(start) = find_invalid_key_branch_idx_ast(words, invalid_key) else {
        return Vec::new();
    };
    let marker_len = if invalid_key { 2 } else { 3 };
    let body_start = start + marker_len;
    let other = find_invalid_key_branch_idx_ast(words, !invalid_key).unwrap_or(words.len());
    let terminator = words
        .iter()
        .position(|token| token.eq_ignore_ascii_case(terminator))
        .unwrap_or(words.len());
    let mut end = words.len();
    for candidate in [other, terminator] {
        if candidate > start {
            end = end.min(candidate);
        }
    }
    branch_statements_ast(
        tokens.get(body_start..end).unwrap_or_default(),
        raw_source,
        span,
    )
}

fn parse_sort_procedure_ast(tokens: &[String]) -> Option<SortProcedureAst> {
    if tokens.len() < 2 || !tokens[0].eq_ignore_ascii_case("SORT") {
        return None;
    }
    if tokens
        .iter()
        .any(|token| token.eq_ignore_ascii_case("USING") || token.eq_ignore_ascii_case("GIVING"))
    {
        return None;
    }
    let key_count = tokens
        .iter()
        .filter(|token| token.eq_ignore_ascii_case("KEY"))
        .count();
    if key_count > 1 {
        return None;
    }
    Some(SortProcedureAst {
        file: sanitize_cobol_name(tokens.get(1)?),
        key: parse_sort_key_ast(tokens),
        input_range: parse_sort_procedure_range_ast(tokens, "INPUT"),
        output_range: parse_sort_procedure_range_ast(tokens, "OUTPUT")?,
    })
}

fn parse_sort_key_ast(tokens: &[String]) -> Option<SortKeyAst> {
    for idx in 0..tokens.len().saturating_sub(2) {
        let direction = if tokens[idx].eq_ignore_ascii_case("ASCENDING") {
            Some(SortDirectionAst::Ascending)
        } else if tokens[idx].eq_ignore_ascii_case("DESCENDING") {
            Some(SortDirectionAst::Descending)
        } else {
            None
        };
        if let Some(direction) = direction {
            if tokens
                .get(idx + 1)
                .map(|token| token.eq_ignore_ascii_case("KEY"))
                == Some(true)
            {
                return tokens.get(idx + 2).map(|name| SortKeyAst {
                    direction,
                    name: sanitize_cobol_name(name),
                });
            }
        }
    }
    None
}

fn parse_sort_procedure_range_ast(tokens: &[String], phrase: &str) -> Option<ProcedureRangeAst> {
    let idx = tokens
        .iter()
        .position(|token| token.eq_ignore_ascii_case(phrase))?;
    if tokens
        .get(idx + 1)
        .map(|token| token.eq_ignore_ascii_case("PROCEDURE"))
        != Some(true)
    {
        return None;
    }
    let mut target_idx = idx + 2;
    if tokens
        .get(target_idx)
        .map(|token| token.eq_ignore_ascii_case("IS"))
        == Some(true)
    {
        target_idx += 1;
    }
    let target = sanitize_cobol_name(tokens.get(target_idx)?);
    let through = if tokens
        .get(target_idx + 1)
        .map(|token| token.eq_ignore_ascii_case("THRU") || token.eq_ignore_ascii_case("THROUGH"))
        == Some(true)
    {
        tokens
            .get(target_idx + 2)
            .map(|token| sanitize_cobol_name(token))
    } else {
        None
    };
    Some(ProcedureRangeAst { target, through })
}

fn parse_release_sort_record_ast(tokens: &[String]) -> Option<ReleaseSortRecordAst> {
    if tokens.len() < 2 || !tokens[0].eq_ignore_ascii_case("RELEASE") {
        return None;
    }
    let from = tokens
        .iter()
        .position(|token| token.eq_ignore_ascii_case("FROM"));
    match from {
        Some(idx) if idx == 2 && idx + 1 < tokens.len() => {}
        Some(_) => return None,
        None if tokens.len() > 2 => return None,
        None => {}
    }
    Some(ReleaseSortRecordAst {
        record: tokens.get(1)?.clone(),
        from: from.and_then(|idx| {
            let source = tokens.get(idx + 1..)?.join(" ");
            (!source.is_empty()).then_some(source)
        }),
    })
}

fn parse_return_sort_record_ast(
    tokens: &[SpannedWord],
    words: &[String],
    raw_source: &str,
    span: SourceSpan,
) -> Option<ReturnSortRecordAst> {
    if words.len() < 2 || !words[0].eq_ignore_ascii_case("RETURN") {
        return None;
    }
    let branch_start = first_return_branch_idx_ast(words).unwrap_or(words.len());
    let into = words
        .iter()
        .position(|token| token.eq_ignore_ascii_case("INTO"))
        .filter(|idx| *idx < branch_start)
        .and_then(|idx| words.get(idx + 1))
        .cloned();
    Some(ReturnSortRecordAst {
        file: sanitize_cobol_name(words.get(1)?),
        into,
        at_end: return_branch_ast(tokens, words, raw_source, true, span.clone()),
        not_at_end: return_branch_ast(tokens, words, raw_source, false, span),
    })
}

fn first_return_branch_idx_ast(tokens: &[String]) -> Option<usize> {
    let at_end = find_return_branch_idx_ast(tokens, true);
    let not_at_end = find_return_branch_idx_ast(tokens, false);
    match (at_end, not_at_end) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(idx), None) | (None, Some(idx)) => Some(idx),
        (None, None) => None,
    }
}

fn find_return_branch_idx_ast(tokens: &[String], at_end: bool) -> Option<usize> {
    if at_end {
        find_top_level_idx(tokens, 1, |idx, token| {
            token.eq_ignore_ascii_case("AT")
                && tokens
                    .get(idx + 1)
                    .is_some_and(|token| token.eq_ignore_ascii_case("END"))
                && idx
                    .checked_sub(1)
                    .and_then(|prev| tokens.get(prev))
                    .map(|token| !token.eq_ignore_ascii_case("NOT"))
                    .unwrap_or(true)
        })
    } else {
        find_top_level_idx(tokens, 1, |idx, token| {
            token.eq_ignore_ascii_case("NOT")
                && tokens
                    .get(idx + 1)
                    .is_some_and(|token| token.eq_ignore_ascii_case("AT"))
                && tokens
                    .get(idx + 2)
                    .is_some_and(|token| token.eq_ignore_ascii_case("END"))
        })
    }
}

fn return_branch_ast(
    tokens: &[SpannedWord],
    words: &[String],
    raw_source: &str,
    at_end: bool,
    span: SourceSpan,
) -> ImperativeListAst {
    let Some(start) = find_return_branch_idx_ast(words, at_end) else {
        return Vec::new();
    };
    let marker_len = if at_end { 2 } else { 3 };
    let body_start = start + marker_len;
    let other = find_return_branch_idx_ast(words, !at_end).unwrap_or(words.len());
    let terminator = matching_explicit_scope_end_idx(words, 0).unwrap_or(words.len());
    let mut end = terminator;
    if other > start {
        end = end.min(other);
    }
    branch_statements_ast(
        tokens.get(body_start..end).unwrap_or_default(),
        raw_source,
        span,
    )
}

fn parse_inspect_ast(tokens: &[String]) -> Option<InspectLikeAst> {
    if tokens.len() < 3 || !tokens[0].eq_ignore_ascii_case("INSPECT") {
        return None;
    }
    if inspect_has_unsupported_phrase_ast(tokens) {
        return None;
    }
    let tallying = tokens
        .iter()
        .position(|token| token.eq_ignore_ascii_case("TALLYING"));
    let replacing = tokens
        .iter()
        .position(|token| token.eq_ignore_ascii_case("REPLACING"));
    let converting = tokens
        .iter()
        .position(|token| token.eq_ignore_ascii_case("CONVERTING"));
    if [tallying, replacing, converting]
        .into_iter()
        .flatten()
        .count()
        != 1
    {
        return None;
    }
    let subject = tokens.get(1)?.clone();
    if let Some(idx) = tallying {
        if tokens.len() != idx + 5
            || tokens
                .get(idx + 2)
                .map(|token| token.eq_ignore_ascii_case("FOR"))
                != Some(true)
            || tokens
                .get(idx + 3)
                .map(|token| token.eq_ignore_ascii_case("ALL"))
                != Some(true)
            || !is_inspect_literal_ast(tokens.get(idx + 4)?)
        {
            return None;
        }
        return Some(InspectLikeAst {
            subject,
            tally: Some(InspectTallyAst {
                target: tokens.get(idx + 1)?.clone(),
                pattern: tokens.get(idx + 4)?.clone(),
            }),
            replacing: None,
            converting: None,
        });
    }
    if let Some(idx) = replacing {
        if tokens.len() != idx + 5
            || tokens
                .get(idx + 1)
                .map(|token| token.eq_ignore_ascii_case("ALL"))
                != Some(true)
            || tokens
                .get(idx + 3)
                .map(|token| token.eq_ignore_ascii_case("BY"))
                != Some(true)
            || !is_inspect_literal_ast(tokens.get(idx + 2)?)
            || !is_inspect_literal_ast(tokens.get(idx + 4)?)
        {
            return None;
        }
        return Some(InspectLikeAst {
            subject,
            tally: None,
            replacing: Some(InspectReplacingAst {
                pattern: tokens.get(idx + 2)?.clone(),
                replacement: tokens.get(idx + 4)?.clone(),
            }),
            converting: None,
        });
    }
    if let Some(idx) = converting {
        if tokens.len() != idx + 4
            || tokens
                .get(idx + 2)
                .map(|token| token.eq_ignore_ascii_case("TO"))
                != Some(true)
            || !is_inspect_literal_ast(tokens.get(idx + 1)?)
            || !is_inspect_literal_ast(tokens.get(idx + 3)?)
        {
            return None;
        }
        return Some(InspectLikeAst {
            subject,
            tally: None,
            replacing: None,
            converting: Some(InspectConvertingAst {
                from: tokens.get(idx + 1)?.clone(),
                to: tokens.get(idx + 3)?.clone(),
            }),
        });
    }
    None
}

fn parse_examine_ast(tokens: &[String]) -> Option<InspectLikeAst> {
    if tokens.len() < 3 || !tokens[0].eq_ignore_ascii_case("EXAMINE") {
        return None;
    }
    if inspect_has_unsupported_phrase_ast(tokens)
        || tokens
            .iter()
            .any(|token| token.eq_ignore_ascii_case("CONVERTING"))
    {
        return None;
    }
    let subject = tokens.get(1)?.clone();
    match tokens.len() {
        5 if tokens[2].eq_ignore_ascii_case("TALLYING")
            && tokens[3].eq_ignore_ascii_case("ALL")
            && is_inspect_literal_ast(tokens.get(4)?) =>
        {
            Some(InspectLikeAst {
                subject,
                tally: Some(InspectTallyAst {
                    target: "TALLY".to_string(),
                    pattern: tokens.get(4)?.clone(),
                }),
                replacing: None,
                converting: None,
            })
        }
        7 if tokens[2].eq_ignore_ascii_case("REPLACING")
            && tokens[3].eq_ignore_ascii_case("ALL")
            && tokens[5].eq_ignore_ascii_case("BY")
            && is_inspect_literal_ast(tokens.get(4)?)
            && is_inspect_literal_ast(tokens.get(6)?) =>
        {
            Some(InspectLikeAst {
                subject,
                tally: None,
                replacing: Some(InspectReplacingAst {
                    pattern: tokens.get(4)?.clone(),
                    replacement: tokens.get(6)?.clone(),
                }),
                converting: None,
            })
        }
        8 if tokens[2].eq_ignore_ascii_case("TALLYING")
            && tokens[3].eq_ignore_ascii_case("ALL")
            && tokens[5].eq_ignore_ascii_case("REPLACING")
            && tokens[6].eq_ignore_ascii_case("BY")
            && is_inspect_literal_ast(tokens.get(4)?)
            && is_inspect_literal_ast(tokens.get(7)?) =>
        {
            Some(InspectLikeAst {
                subject,
                tally: Some(InspectTallyAst {
                    target: "TALLY".to_string(),
                    pattern: tokens.get(4)?.clone(),
                }),
                replacing: Some(InspectReplacingAst {
                    pattern: tokens.get(4)?.clone(),
                    replacement: tokens.get(7)?.clone(),
                }),
                converting: None,
            })
        }
        _ => None,
    }
}

fn inspect_has_unsupported_phrase_ast(tokens: &[String]) -> bool {
    tokens.iter().any(|token| {
        matches!(
            token.to_ascii_uppercase().as_str(),
            "BEFORE" | "AFTER" | "LEADING" | "FIRST"
        )
    })
}

fn is_inspect_literal_ast(token: &str) -> bool {
    let clean = token.trim().trim_end_matches('.');
    (clean.starts_with('"') && clean.ends_with('"'))
        || (clean.starts_with('\'') && clean.ends_with('\''))
        || matches!(
            clean.to_ascii_uppercase().as_str(),
            "SPACE"
                | "SPACES"
                | "ZERO"
                | "ZEROES"
                | "ZEROS"
                | "QUOTE"
                | "QUOTES"
                | "HIGH-VALUE"
                | "HIGH-VALUES"
                | "LOW-VALUE"
                | "LOW-VALUES"
        )
}

fn parse_string_ast(
    tokens: &[SpannedWord],
    words: &[String],
    raw_source: &str,
    span: SourceSpan,
) -> Option<StringOpAst> {
    if words.len() < 6 || !words[0].eq_ignore_ascii_case("STRING") {
        return None;
    }
    if string_has_unsupported_phrase_ast(words) {
        return None;
    }
    let into_idx = words
        .iter()
        .position(|token| token.eq_ignore_ascii_case("INTO"))?;
    if into_idx < 4 || into_idx + 1 >= words.len() {
        return None;
    }
    let options_start = first_string_option_idx_ast(words, into_idx + 2).unwrap_or(words.len());
    let mut pieces = Vec::new();
    let mut idx = 1usize;
    while idx < into_idx {
        let source = words.get(idx)?.clone();
        if words
            .get(idx + 1)
            .map(|token| token.eq_ignore_ascii_case("DELIMITED"))
            != Some(true)
            || words
                .get(idx + 2)
                .map(|token| token.eq_ignore_ascii_case("BY"))
                != Some(true)
        {
            return None;
        }
        pieces.push(StringPieceAst {
            source,
            delimiter: parse_string_delimiter_ast(words.get(idx + 3)?)?,
        });
        idx += 4;
    }
    if pieces.is_empty() {
        return None;
    }
    Some(StringOpAst {
        pieces,
        target: words.get(into_idx + 1)?.clone(),
        pointer: find_with_pointer_ast(words, options_start, words.len())
            .and_then(|idx| words.get(idx + 2).cloned()),
        on_overflow: overflow_branch_ast(
            tokens,
            words,
            raw_source,
            true,
            "END-STRING",
            span.clone(),
        ),
        not_on_overflow: overflow_branch_ast(tokens, words, raw_source, false, "END-STRING", span),
    })
}

fn parse_unstring_ast(
    tokens: &[SpannedWord],
    words: &[String],
    raw_source: &str,
    span: SourceSpan,
) -> Option<UnstringOpAst> {
    if words.len() < 7 || !words[0].eq_ignore_ascii_case("UNSTRING") {
        return None;
    }
    if string_has_unsupported_phrase_ast(words) {
        return None;
    }
    if words
        .get(2)
        .map(|token| token.eq_ignore_ascii_case("DELIMITED"))
        != Some(true)
        || words.get(3).map(|token| token.eq_ignore_ascii_case("BY")) != Some(true)
    {
        return None;
    }
    let (delimiter, after_delimiter) = parse_string_delimiter_at_ast(words, 4)?;
    let into_idx = words
        .iter()
        .position(|token| token.eq_ignore_ascii_case("INTO"))?;
    if into_idx != after_delimiter || into_idx + 1 >= tokens.len() {
        return None;
    }
    let targets_end = first_unstring_option_idx_ast(words, into_idx + 1).unwrap_or(words.len());
    let targets = parse_unstring_targets_ast(words.get(into_idx + 1..targets_end)?)?;
    if targets.is_empty() {
        return None;
    }
    Some(UnstringOpAst {
        source: words.get(1)?.clone(),
        delimiter,
        targets,
        pointer: find_with_pointer_ast(words, targets_end, words.len())
            .and_then(|idx| words.get(idx + 2).cloned()),
        tallying: find_tallying_in_ast(words, targets_end, words.len())
            .and_then(|idx| words.get(idx + 2).cloned()),
        on_overflow: overflow_branch_ast(
            tokens,
            words,
            raw_source,
            true,
            "END-UNSTRING",
            span.clone(),
        ),
        not_on_overflow: overflow_branch_ast(
            tokens,
            words,
            raw_source,
            false,
            "END-UNSTRING",
            span,
        ),
    })
}

fn string_has_unsupported_phrase_ast(tokens: &[String]) -> bool {
    tokens.iter().any(|token| {
        matches!(
            token.to_ascii_uppercase().as_str(),
            "DELIMITER" | "DELIMITERS" | "OR" | "INITIAL"
        )
    })
}

fn parse_string_delimiter_ast(token: &str) -> Option<StringDelimiterAst> {
    if token.eq_ignore_ascii_case("SIZE") {
        Some(StringDelimiterAst::Size)
    } else if is_inspect_literal_ast(token) {
        Some(StringDelimiterAst::Literal {
            value: token.to_string(),
            all: false,
        })
    } else {
        None
    }
}

fn parse_string_delimiter_at_ast(
    tokens: &[String],
    idx: usize,
) -> Option<(StringDelimiterAst, usize)> {
    if tokens
        .get(idx)
        .map(|token| token.eq_ignore_ascii_case("ALL"))
        == Some(true)
    {
        if !is_inspect_literal_ast(tokens.get(idx + 1)?) {
            return None;
        }
        Some((
            StringDelimiterAst::Literal {
                value: tokens.get(idx + 1)?.clone(),
                all: true,
            },
            idx + 2,
        ))
    } else {
        parse_string_delimiter_ast(tokens.get(idx)?).map(|delimiter| (delimiter, idx + 1))
    }
}

fn first_string_option_idx_ast(tokens: &[String], start: usize) -> Option<usize> {
    (start..tokens.len()).find(|idx| {
        tokens[*idx].eq_ignore_ascii_case("WITH")
            || tokens[*idx].eq_ignore_ascii_case("ON")
            || (tokens[*idx].eq_ignore_ascii_case("NOT")
                && tokens
                    .get(*idx + 1)
                    .map(|token| token.eq_ignore_ascii_case("ON"))
                    .unwrap_or(false))
            || tokens[*idx].eq_ignore_ascii_case("END-STRING")
    })
}

fn first_unstring_option_idx_ast(tokens: &[String], start: usize) -> Option<usize> {
    (start..tokens.len()).find(|idx| {
        tokens[*idx].eq_ignore_ascii_case("WITH")
            || tokens[*idx].eq_ignore_ascii_case("TALLYING")
            || tokens[*idx].eq_ignore_ascii_case("ON")
            || (tokens[*idx].eq_ignore_ascii_case("NOT")
                && tokens
                    .get(*idx + 1)
                    .map(|token| token.eq_ignore_ascii_case("ON"))
                    .unwrap_or(false))
            || tokens[*idx].eq_ignore_ascii_case("END-UNSTRING")
    })
}

fn find_with_pointer_ast(tokens: &[String], start: usize, end: usize) -> Option<usize> {
    (start..end.saturating_sub(2)).find(|idx| {
        tokens[*idx].eq_ignore_ascii_case("WITH")
            && tokens[*idx + 1].eq_ignore_ascii_case("POINTER")
    })
}

fn find_tallying_in_ast(tokens: &[String], start: usize, end: usize) -> Option<usize> {
    (start..end.saturating_sub(2)).find(|idx| {
        tokens[*idx].eq_ignore_ascii_case("TALLYING") && tokens[*idx + 1].eq_ignore_ascii_case("IN")
    })
}

fn parse_unstring_targets_ast(tokens: &[String]) -> Option<Vec<UnstringTargetAst>> {
    let mut out = Vec::new();
    let mut idx = 0usize;
    while idx < tokens.len() {
        let target = tokens.get(idx)?.clone();
        idx += 1;
        let count = if tokens
            .get(idx)
            .map(|token| token.eq_ignore_ascii_case("COUNT"))
            == Some(true)
        {
            if tokens
                .get(idx + 1)
                .map(|token| token.eq_ignore_ascii_case("IN"))
                != Some(true)
            {
                return None;
            }
            let count = tokens.get(idx + 2)?.clone();
            idx += 3;
            Some(count)
        } else {
            None
        };
        out.push(UnstringTargetAst { target, count });
    }
    Some(out)
}

fn find_overflow_branch_idx_ast(tokens: &[String], overflow: bool) -> Option<usize> {
    if overflow {
        find_top_level_idx(tokens, 1, |idx, token| {
            token.eq_ignore_ascii_case("ON")
                && tokens
                    .get(idx + 1)
                    .is_some_and(|token| token.eq_ignore_ascii_case("OVERFLOW"))
                && idx
                    .checked_sub(1)
                    .and_then(|prev| tokens.get(prev))
                    .map(|token| !token.eq_ignore_ascii_case("NOT"))
                    .unwrap_or(true)
        })
    } else {
        find_top_level_idx(tokens, 1, |idx, token| {
            token.eq_ignore_ascii_case("NOT")
                && tokens
                    .get(idx + 1)
                    .is_some_and(|token| token.eq_ignore_ascii_case("ON"))
                && tokens
                    .get(idx + 2)
                    .is_some_and(|token| token.eq_ignore_ascii_case("OVERFLOW"))
        })
    }
}

fn overflow_branch_ast(
    tokens: &[SpannedWord],
    words: &[String],
    raw_source: &str,
    overflow: bool,
    terminator: &str,
    span: SourceSpan,
) -> ImperativeListAst {
    let Some(start) = find_overflow_branch_idx_ast(words, overflow) else {
        return Vec::new();
    };
    let marker_len = if overflow { 2 } else { 3 };
    let body_start = start + marker_len;
    let other = find_overflow_branch_idx_ast(words, !overflow).unwrap_or(words.len());
    let terminator = matching_explicit_scope_end_idx(words, 0)
        .filter(|idx| words[*idx].eq_ignore_ascii_case(terminator))
        .unwrap_or(words.len());
    let mut end = words.len();
    for candidate in [other, terminator] {
        if candidate > start {
            end = end.min(candidate);
        }
    }
    branch_statements_ast(
        tokens.get(body_start..end).unwrap_or_default(),
        raw_source,
        span,
    )
}

fn parse_select(raw: &str, span: SourceSpan) -> Option<FileAst> {
    let words = words(raw);
    if words
        .first()
        .map(|word| word.eq_ignore_ascii_case("SELECT"))
        != Some(true)
    {
        return None;
    }
    let name_index = if words
        .get(1)
        .map(|word| word.eq_ignore_ascii_case("OPTIONAL"))
        == Some(true)
    {
        2
    } else {
        1
    };
    let name = words
        .get(name_index)
        .map(|word| sanitize_cobol_name(word))?;
    let assign_token = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("ASSIGN"))
        .and_then(|idx| {
            let start = if words
                .get(idx + 1)
                .map(|word| word.eq_ignore_ascii_case("TO"))
                == Some(true)
            {
                idx + 2
            } else {
                idx + 1
            };
            words.get(start).cloned()
        });
    let assign_is_literal = assign_token
        .as_ref()
        .map(|value| {
            (value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\''))
        })
        .unwrap_or(false);
    let assign = assign_token.map(|value| value.trim_matches('"').trim_matches('\'').to_string());
    let organization = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("ORGANIZATION"))
        .and_then(|idx| select_value_after_optional(&words, idx + 1, &["IS"]))
        .map(|value| {
            if value.eq_ignore_ascii_case("LINE") {
                "LINE SEQUENTIAL".to_string()
            } else {
                value.to_ascii_uppercase()
            }
        });
    let access_mode = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("ACCESS"))
        .and_then(|idx| select_value_after_optional(&words, idx + 1, &["MODE", "IS"]))
        .map(|value| value.to_ascii_uppercase());
    let file_status = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("STATUS"))
        .and_then(|idx| select_value_after_optional(&words, idx + 1, &["IS"]))
        .map(sanitize_cobol_name);
    Some(FileAst {
        name,
        kind: FileKindAst::Fd,
        assign,
        assign_is_literal,
        organization,
        access_mode,
        file_status,
        record_name: None,
        linage: None,
        span,
    })
}

fn parse_file_linage(raw: &str) -> Option<usize> {
    let words = words(raw);
    let idx = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("LINAGE"))?;
    let value_idx = if words
        .get(idx + 1)
        .map(|word| word.eq_ignore_ascii_case("IS"))
        == Some(true)
    {
        idx + 2
    } else {
        idx + 1
    };
    words
        .get(value_idx)
        .and_then(|value| value.trim_end_matches('.').parse::<usize>().ok())
}

fn parse_same_record_area(raw: &str, span: SourceSpan) -> Option<SameRecordAreaAst> {
    let words = words(raw);
    let same_idx = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("SAME"))?;
    if words
        .get(same_idx + 1)
        .map(|word| word.eq_ignore_ascii_case("RECORD"))
        != Some(true)
        || words
            .get(same_idx + 2)
            .map(|word| word.eq_ignore_ascii_case("AREA"))
            != Some(true)
    {
        return None;
    }
    let for_idx = words
        .iter()
        .enumerate()
        .skip(same_idx + 3)
        .find_map(|(idx, word)| word.eq_ignore_ascii_case("FOR").then_some(idx))?;
    let files = words
        .iter()
        .skip(for_idx + 1)
        .filter(|word| !word.eq_ignore_ascii_case("AND"))
        .map(|word| sanitize_cobol_name(word.trim_end_matches(',')))
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    if files.len() < 2 {
        return None;
    }
    Some(SameRecordAreaAst { files, span })
}

fn parse_rerun_clause(raw: &str, span: SourceSpan) -> Option<RerunClauseAst> {
    let words = words(raw);
    let rerun_idx = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("RERUN"))?;
    if words
        .get(rerun_idx + 1)
        .map(|word| word.eq_ignore_ascii_case("ON"))
        != Some(true)
    {
        return None;
    }
    let checkpoint_file = words
        .get(rerun_idx + 2)
        .map(|word| sanitize_cobol_name(word))?;
    let every_idx = words
        .iter()
        .enumerate()
        .skip(rerun_idx + 3)
        .find_map(|(idx, word)| word.eq_ignore_ascii_case("EVERY").then_some(idx))?;
    let every_records = words
        .get(every_idx + 1)
        .and_then(|word| word.parse::<usize>().ok())?;
    if words
        .get(every_idx + 2)
        .map(|word| word.eq_ignore_ascii_case("RECORDS"))
        != Some(true)
        || words
            .get(every_idx + 3)
            .map(|word| word.eq_ignore_ascii_case("OF"))
            != Some(true)
    {
        return None;
    }
    let watched_file = words
        .get(every_idx + 4)
        .map(|word| sanitize_cobol_name(word))?;
    if every_records == 0 {
        return None;
    }
    Some(RerunClauseAst {
        checkpoint_file,
        every_records,
        watched_file,
        span,
    })
}

fn select_value_after_optional<'a>(
    words: &'a [String],
    mut idx: usize,
    optional: &[&str],
) -> Option<&'a str> {
    while words.get(idx).is_some_and(|word| {
        optional
            .iter()
            .any(|candidate| word.eq_ignore_ascii_case(candidate))
    }) {
        idx += 1;
    }
    words.get(idx).map(String::as_str)
}

fn parse_binary_target_statement(words: &[String], separator: &str) -> Option<(String, String)> {
    let sep = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case(separator))?;
    let source = words.get(1..sep)?.join(" ");
    let target = words.get(sep + 1..)?.join(" ");
    Some((source, target))
}

fn parse_add_statement(words: &[String]) -> StatementKindAst {
    if words.iter().any(|word| word.eq_ignore_ascii_case("GIVING")) {
        return parse_add_giving_statement(words)
            .unwrap_or_else(|| StatementKindAst::Unsupported("ADD GIVING".to_string()));
    }
    parse_binary_target_statement(words, "TO")
        .map(|(source, target)| StatementKindAst::Add { source, target })
        .unwrap_or_else(|| StatementKindAst::Unsupported("ADD".to_string()))
}

fn parse_add_giving_statement(words: &[String]) -> Option<StatementKindAst> {
    let to_idx = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("TO"))?;
    let giving_idx = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("GIVING"))?;
    if to_idx <= 1 || giving_idx <= to_idx + 1 || giving_idx + 1 >= words.len() {
        return None;
    }
    if words
        .iter()
        .skip(giving_idx + 1)
        .any(|word| word.eq_ignore_ascii_case("ROUNDED"))
        || words
            .iter()
            .skip(giving_idx + 1)
            .any(|word| word.eq_ignore_ascii_case("ON") || word.eq_ignore_ascii_case("NOT"))
    {
        return None;
    }

    let source = words.get(1..to_idx)?.join(" ");
    let addend = words.get(to_idx + 1..giving_idx)?.join(" ");
    let target = words.get(giving_idx + 1..)?.join(" ");
    Some(StatementKindAst::Compute(ComputeAst {
        target,
        expression: format!("{source} + {addend}"),
        on_size_error: Vec::new(),
        not_on_size_error: Vec::new(),
    }))
}

fn parse_move_statement(words: &[String]) -> Option<StatementKindAst> {
    if words.get(1).is_some_and(|word| {
        word.eq_ignore_ascii_case("CORRESPONDING") || word.eq_ignore_ascii_case("CORR")
    }) {
        let sep = words
            .iter()
            .position(|word| word.eq_ignore_ascii_case("TO"))?;
        if sep <= 2 || sep + 1 >= words.len() {
            return None;
        }
        return Some(StatementKindAst::MoveCorresponding {
            source: words.get(2..sep)?.join(" "),
            target: words.get(sep + 1..)?.join(" "),
        });
    }
    parse_binary_target_statement(words, "TO")
        .map(|(source, target)| StatementKindAst::Move { source, target })
}

fn parse_compute_ast(
    tokens: &[SpannedWord],
    words: &[String],
    raw_source: &str,
    span: SourceSpan,
) -> Option<ComputeAst> {
    if words.len() < 4 {
        return None;
    }
    let eq = words.iter().position(|word| word == "=")?;
    if eq != 2 {
        return None;
    }
    let target = raw_from_words(tokens.get(1..eq)?, raw_source)
        .trim()
        .to_string();
    let expression_start = eq + 1;
    let expression_end =
        first_compute_option_idx_ast(words, expression_start).unwrap_or_else(|| {
            words
                .iter()
                .position(|word| word.eq_ignore_ascii_case("END-COMPUTE"))
                .unwrap_or(words.len())
        });
    let expression = raw_from_words(tokens.get(expression_start..expression_end)?, raw_source)
        .trim()
        .to_string();
    Some(ComputeAst {
        target,
        expression,
        on_size_error: compute_size_error_branch_ast(tokens, words, raw_source, true, span.clone()),
        not_on_size_error: compute_size_error_branch_ast(tokens, words, raw_source, false, span),
    })
}

fn first_compute_option_idx_ast(tokens: &[String], start: usize) -> Option<usize> {
    [
        find_size_error_branch_idx_ast(tokens, true, start),
        find_size_error_branch_idx_ast(tokens, false, start),
        matching_explicit_scope_end_idx(tokens, 0).filter(|idx| *idx >= start),
    ]
    .into_iter()
    .flatten()
    .min()
}

fn find_size_error_branch_idx_ast(
    tokens: &[String],
    size_error: bool,
    start: usize,
) -> Option<usize> {
    if size_error {
        find_top_level_idx(tokens, start, |idx, token| {
            token.eq_ignore_ascii_case("ON")
                && tokens
                    .get(idx + 1)
                    .is_some_and(|token| token.eq_ignore_ascii_case("SIZE"))
                && tokens
                    .get(idx + 2)
                    .is_some_and(|token| token.eq_ignore_ascii_case("ERROR"))
                && idx
                    .checked_sub(1)
                    .and_then(|prev| tokens.get(prev))
                    .map(|token| !token.eq_ignore_ascii_case("NOT"))
                    .unwrap_or(true)
        })
    } else {
        find_top_level_idx(tokens, start, |idx, token| {
            token.eq_ignore_ascii_case("NOT")
                && tokens
                    .get(idx + 1)
                    .is_some_and(|token| token.eq_ignore_ascii_case("ON"))
                && tokens
                    .get(idx + 2)
                    .is_some_and(|token| token.eq_ignore_ascii_case("SIZE"))
                && tokens
                    .get(idx + 3)
                    .is_some_and(|token| token.eq_ignore_ascii_case("ERROR"))
        })
    }
}

fn compute_size_error_branch_ast(
    tokens: &[SpannedWord],
    words: &[String],
    raw_source: &str,
    size_error: bool,
    span: SourceSpan,
) -> ImperativeListAst {
    let Some(start) = find_size_error_branch_idx_ast(words, size_error, 1) else {
        return Vec::new();
    };
    let marker_len = if size_error { 3 } else { 4 };
    let body_start = start + marker_len;
    let other =
        find_size_error_branch_idx_ast(words, !size_error, body_start).unwrap_or(words.len());
    let terminator = matching_explicit_scope_end_idx(words, 0).unwrap_or(words.len());
    let end = other.min(terminator);
    branch_statements_ast(
        tokens.get(body_start..end).unwrap_or_default(),
        raw_source,
        span,
    )
}

fn parse_evaluate_ast(
    tokens: &[SpannedWord],
    words: &[String],
    raw_source: &str,
    span: SourceSpan,
) -> Option<EvaluateAst> {
    if words
        .first()
        .map(|token| token.eq_ignore_ascii_case("EVALUATE"))
        != Some(true)
    {
        return None;
    }
    let terminator = matching_explicit_scope_end_idx(words, 0).unwrap_or(words.len());
    let body_words = words.get(1..terminator)?;
    let body_tokens = tokens.get(1..terminator)?;
    let first_when =
        find_top_level_token_idx(body_words, 0, |token| token.eq_ignore_ascii_case("WHEN"))?;
    let subjects = split_also_groups_ast(&body_words[..first_when])
        .into_iter()
        .map(|group| group.join(" "))
        .filter(|subject| !subject.trim().is_empty())
        .collect::<Vec<_>>();
    let subject_count = subjects.len().max(1);
    let mut arms = Vec::new();
    let mut idx = first_when;
    while idx < body_words.len() {
        if !body_words[idx].eq_ignore_ascii_case("WHEN") {
            idx += 1;
            continue;
        }
        let segment_start = idx + 1;
        let segment_end = find_top_level_token_idx(body_words, segment_start, |token| {
            token.eq_ignore_ascii_case("WHEN")
        })
        .unwrap_or(body_words.len());
        let segment_tokens = &body_tokens[segment_start..segment_end];
        let segment_words = &body_words[segment_start..segment_end];
        if let Some(arm) = parse_evaluate_arm_ast(
            segment_tokens,
            segment_words,
            raw_source,
            subject_count,
            span.clone(),
        ) {
            arms.push(arm);
        } else {
            return None;
        }
        idx = segment_end;
    }
    Some(EvaluateAst {
        raw: raw_from_words(tokens, raw_source),
        subjects,
        arms,
    })
}

fn parse_evaluate_arm_ast(
    tokens: &[SpannedWord],
    words: &[String],
    raw_source: &str,
    subject_count: usize,
    span: SourceSpan,
) -> Option<EvaluateArmAst> {
    if words.is_empty() {
        return None;
    }
    let action_idx = words
        .iter()
        .position(|token| is_statement_start(token))
        .unwrap_or(words.len());
    let pattern_tokens = &words[..action_idx];
    let body_tokens = &tokens[action_idx..];
    let patterns = if pattern_tokens.len() == 1 && pattern_tokens[0].eq_ignore_ascii_case("OTHER") {
        vec!["OTHER".to_string(); subject_count]
    } else if subject_count <= 1 {
        vec![pattern_tokens.join(" ")]
    } else {
        split_also_groups_ast(pattern_tokens)
            .into_iter()
            .map(|group| group.join(" "))
            .collect()
    };
    Some(EvaluateArmAst {
        raw: raw_from_words(tokens, raw_source),
        patterns,
        statements: branch_statements_ast(body_tokens, raw_source, span),
    })
}

fn parse_search_ast(
    tokens: &[SpannedWord],
    words: &[String],
    raw_source: &str,
    span: SourceSpan,
) -> Option<SearchAst> {
    if words
        .first()
        .map(|token| token.eq_ignore_ascii_case("SEARCH"))
        != Some(true)
    {
        return None;
    }
    let terminator = matching_explicit_scope_end_idx(words, 0).unwrap_or(words.len());
    let body_tokens = tokens.get(1..terminator)?;
    let body_words = words.get(1..terminator)?;
    let all = body_words
        .first()
        .map(|token| token.eq_ignore_ascii_case("ALL"))
        .unwrap_or(false);
    let table_idx = usize::from(all);
    let table = sanitize_cobol_name(body_words.get(table_idx)?);
    let mut cursor = table_idx + 1;
    let mut index = None;
    if body_words
        .get(cursor)
        .map(|token| token.eq_ignore_ascii_case("VARYING"))
        == Some(true)
    {
        index = body_words
            .get(cursor + 1)
            .map(|token| sanitize_cobol_name(token));
        cursor += 2;
    }
    let first_when = find_top_level_token_idx(body_words, cursor, |token| {
        token.eq_ignore_ascii_case("WHEN")
    })?;
    let at_end = if body_words
        .get(cursor)
        .map(|token| token.eq_ignore_ascii_case("AT"))
        == Some(true)
        && body_words
            .get(cursor + 1)
            .map(|token| token.eq_ignore_ascii_case("END"))
            == Some(true)
    {
        branch_statements_ast(
            body_tokens.get(cursor + 2..first_when).unwrap_or_default(),
            raw_source,
            span.clone(),
        )
    } else {
        Vec::new()
    };
    let mut whens = Vec::new();
    let mut pos = first_when;
    while pos < body_words.len() {
        if !body_words[pos].eq_ignore_ascii_case("WHEN") {
            pos += 1;
            continue;
        }
        let segment_start = pos + 1;
        let segment_end = find_top_level_token_idx(body_words, segment_start, |token| {
            token.eq_ignore_ascii_case("WHEN")
        })
        .unwrap_or(body_words.len());
        let segment_words = &body_words[segment_start..segment_end];
        let segment_tokens = &body_tokens[segment_start..segment_end];
        let action_idx = segment_words
            .iter()
            .enumerate()
            .find_map(|(idx, token)| (idx > 0 && is_statement_start(token)).then_some(idx))?;
        whens.push(SearchWhenAst {
            condition: segment_words[..action_idx].join(" "),
            statements: branch_statements_ast(
                &segment_tokens[action_idx..],
                raw_source,
                span.clone(),
            ),
        });
        pos = segment_end;
    }
    Some(SearchAst {
        raw: raw_from_words(tokens, raw_source),
        all,
        table,
        index,
        at_end,
        whens,
    })
}

fn split_also_groups_ast(tokens: &[String]) -> Vec<Vec<String>> {
    let mut groups = Vec::new();
    let mut current = Vec::new();
    for token in tokens {
        if token.eq_ignore_ascii_case("ALSO") {
            groups.push(current);
            current = Vec::new();
        } else {
            current.push(token.clone());
        }
    }
    groups.push(current);
    groups
}

fn parse_if_statement(
    tokens: &[SpannedWord],
    words: &[String],
    raw_source: &str,
    span: SourceSpan,
) -> Option<StatementKindAst> {
    if words.first().map(|token| token.eq_ignore_ascii_case("IF")) != Some(true) {
        return None;
    }
    let (kind, next) = parse_if_tokens(tokens, words, raw_source, 0, span)?;
    (next >= words.len()).then_some(kind)
}

fn parse_if_tokens(
    tokens: &[SpannedWord],
    words: &[String],
    raw_source: &str,
    start: usize,
    span: SourceSpan,
) -> Option<(StatementKindAst, usize)> {
    if words
        .get(start)
        .map(|token| token.eq_ignore_ascii_case("IF"))
        != Some(true)
    {
        return None;
    }
    let Some(action_idx) = find_if_action_idx(words, start + 1) else {
        let end_idx = words
            .iter()
            .enumerate()
            .skip(start + 1)
            .find_map(|(idx, token)| token.eq_ignore_ascii_case("END-IF").then_some(idx))
            .unwrap_or(words.len());
        let mut next = end_idx;
        if words
            .get(next)
            .map(|token| token.eq_ignore_ascii_case("END-IF"))
            == Some(true)
        {
            next += 1;
        }
        return Some((
            StatementKindAst::If {
                condition: raw_from_words(tokens.get(start + 1..end_idx)?, raw_source)
                    .trim()
                    .to_string(),
                then_statements: Vec::new(),
                else_statements: Vec::new(),
            },
            next,
        ));
    };
    let condition = raw_from_words(tokens.get(start + 1..action_idx)?, raw_source)
        .trim()
        .to_string();
    let mut body_start = action_idx;
    if words
        .get(body_start)
        .map(|token| token.eq_ignore_ascii_case("THEN"))
        == Some(true)
    {
        body_start += 1;
    }
    let (then_statements, mut next) = parse_imperative_tokens(
        tokens,
        raw_source,
        body_start,
        &["ELSE", "END-IF"],
        span.clone(),
    );
    let else_statements = if words
        .get(next)
        .map(|token| token.eq_ignore_ascii_case("ELSE"))
        == Some(true)
    {
        let (else_statements, after_else) =
            parse_imperative_tokens(tokens, raw_source, next + 1, &["END-IF"], span.clone());
        next = after_else;
        else_statements
    } else {
        Vec::new()
    };
    if words
        .get(next)
        .map(|token| token.eq_ignore_ascii_case("END-IF"))
        == Some(true)
    {
        next += 1;
    }
    Some((
        StatementKindAst::If {
            condition,
            then_statements,
            else_statements,
        },
        next,
    ))
}

fn parse_imperative_tokens(
    tokens: &[SpannedWord],
    raw_source: &str,
    mut idx: usize,
    terminators: &[&str],
    span: SourceSpan,
) -> (ImperativeListAst, usize) {
    let words = word_texts(tokens);
    let mut statements = Vec::new();
    while idx < tokens.len() {
        if terminators
            .iter()
            .any(|terminator| words[idx].eq_ignore_ascii_case(terminator))
        {
            break;
        }
        let Some(parsed) =
            parse_statement_at(tokens, &words, raw_source, idx, terminators, span.clone())
        else {
            break;
        };
        statements.push(parsed.statement);
        idx = parsed.next_idx;
    }
    (statements, idx)
}

struct ParsedStatementAst {
    statement: StatementAst,
    next_idx: usize,
}

fn explicit_scope_statement_end(tokens: &[String], start: usize) -> Option<usize> {
    matching_explicit_scope_end_idx(tokens, start).map(|idx| idx + 1)
}

fn explicit_scope_terminator(token: &str) -> Option<&'static str> {
    match token.to_ascii_uppercase().as_str() {
        "COMPUTE" => "END-COMPUTE",
        "EVALUATE" => "END-EVALUATE",
        "SEARCH" => "END-SEARCH",
        "READ" => "END-READ",
        "REWRITE" => "END-REWRITE",
        "DELETE" => "END-DELETE",
        "RETURN" => "END-RETURN",
        "STRING" => "END-STRING",
        "UNSTRING" => "END-UNSTRING",
        _ => return None,
    }
    .into()
}

fn matching_explicit_scope_end_idx(tokens: &[String], start: usize) -> Option<usize> {
    let mut stack = vec![explicit_scope_terminator(tokens.get(start)?)?];
    for (idx, token) in tokens.iter().enumerate().skip(start + 1) {
        if stack
            .last()
            .is_some_and(|terminator| token.eq_ignore_ascii_case(terminator))
        {
            stack.pop();
            if stack.is_empty() {
                return Some(idx);
            }
            continue;
        }
        if let Some(terminator) = explicit_scope_terminator(token) {
            stack.push(terminator);
        }
    }
    None
}

fn find_top_level_token_idx(
    tokens: &[String],
    start: usize,
    predicate: impl Fn(&str) -> bool,
) -> Option<usize> {
    find_top_level_idx(tokens, start, |_, token| predicate(token))
}

fn find_top_level_idx(
    tokens: &[String],
    start: usize,
    predicate: impl Fn(usize, &str) -> bool,
) -> Option<usize> {
    let mut stack = Vec::new();
    for (idx, token) in tokens.iter().enumerate().skip(start) {
        if stack
            .last()
            .is_some_and(|terminator: &&str| token.eq_ignore_ascii_case(terminator))
        {
            stack.pop();
            continue;
        }
        if stack.is_empty() && predicate(idx, token) {
            return Some(idx);
        }
        if let Some(terminator) = explicit_scope_terminator(token) {
            stack.push(terminator);
        }
    }
    None
}

fn parse_statement_at(
    tokens: &[SpannedWord],
    words: &[String],
    raw_source: &str,
    idx: usize,
    terminators: &[&str],
    span: SourceSpan,
) -> Option<ParsedStatementAst> {
    if idx >= tokens.len()
        || terminators
            .iter()
            .any(|terminator| words[idx].eq_ignore_ascii_case(terminator))
    {
        return None;
    }

    if words[idx].eq_ignore_ascii_case("IF") {
        if let Some((kind, next_idx)) =
            parse_if_tokens(tokens, words, raw_source, idx, span.clone())
        {
            let raw = raw_from_words(tokens.get(idx..next_idx)?, raw_source);
            return Some(ParsedStatementAst {
                statement: StatementAst {
                    kind,
                    raw,
                    span: span.clone(),
                },
                next_idx,
            });
        }
    }

    let next_idx = explicit_scope_statement_end(words, idx)
        .or_else(|| period_scoped_branch_statement_end(words, idx, terminators))
        .unwrap_or_else(|| find_next_statement_boundary(words, idx + 1, terminators));
    if next_idx <= idx {
        return None;
    }
    let statement = parse_statement_from_words(tokens.get(idx..next_idx)?, raw_source, span);
    debug_assert_eq!(
        statement.raw,
        raw_from_words(tokens.get(idx..next_idx)?, raw_source)
    );
    Some(ParsedStatementAst {
        statement,
        next_idx,
    })
}

fn period_scoped_branch_statement_end(
    tokens: &[String],
    start: usize,
    terminators: &[&str],
) -> Option<usize> {
    let branch_idx = first_period_scoped_branch_phrase_idx(tokens, start)?;
    let next_statement = find_next_statement_boundary(tokens, start + 1, terminators);
    if branch_idx >= next_statement {
        return None;
    }
    Some(first_outer_terminator_idx(tokens, start + 1, terminators).unwrap_or(tokens.len()))
}

fn first_period_scoped_branch_phrase_idx(tokens: &[String], start: usize) -> Option<usize> {
    let verb = tokens.get(start)?.to_ascii_uppercase();
    let rest = tokens.get(start..)?;
    let local_idx = match verb.as_str() {
        "READ" => first_read_branch_idx_ast(rest),
        "REWRITE" | "DELETE" => {
            let invalid = find_invalid_key_branch_idx_ast(rest, true);
            let not_invalid = find_invalid_key_branch_idx_ast(rest, false);
            [invalid, not_invalid].into_iter().flatten().min()
        }
        "RETURN" => first_return_branch_idx_ast(rest),
        "STRING" | "UNSTRING" => {
            let overflow = find_overflow_branch_idx_ast(rest, true);
            let not_overflow = find_overflow_branch_idx_ast(rest, false);
            [overflow, not_overflow].into_iter().flatten().min()
        }
        "COMPUTE" => {
            let size_error = find_size_error_branch_idx_ast(rest, true, 0);
            let not_size_error = find_size_error_branch_idx_ast(rest, false, 0);
            [size_error, not_size_error].into_iter().flatten().min()
        }
        _ => None,
    }?;
    Some(start + local_idx)
}

fn first_outer_terminator_idx(
    tokens: &[String],
    start: usize,
    terminators: &[&str],
) -> Option<usize> {
    tokens
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(idx, token)| {
            terminators
                .iter()
                .any(|terminator| token.eq_ignore_ascii_case(terminator))
                .then_some(idx)
        })
}

fn find_if_action_idx(tokens: &[String], start: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (idx, token) in tokens.iter().enumerate().skip(start) {
        if token == "(" {
            depth += 1;
        } else if token == ")" {
            depth = depth.saturating_sub(1);
        }
        if idx > start && depth == 0 && is_if_action_boundary(token) {
            return Some(idx);
        }
    }
    None
}

fn find_next_statement_boundary(tokens: &[String], start: usize, terminators: &[&str]) -> usize {
    for (idx, token) in tokens.iter().enumerate().skip(start) {
        if terminators
            .iter()
            .any(|terminator| token.eq_ignore_ascii_case(terminator))
            || is_statement_start(token)
        {
            return idx;
        }
    }
    tokens.len()
}

fn is_if_action_boundary(token: &str) -> bool {
    token.eq_ignore_ascii_case("THEN") || is_statement_start(token) || is_if_scope_token(token)
}

fn is_if_scope_token(token: &str) -> bool {
    token.eq_ignore_ascii_case("ELSE") || token.eq_ignore_ascii_case("END-IF")
}

fn is_statement_start(token: &str) -> bool {
    matches!(
        token.to_ascii_uppercase().as_str(),
        "DISPLAY"
            | "MOVE"
            | "ADD"
            | "SUBTRACT"
            | "MULTIPLY"
            | "DIVIDE"
            | "COMPUTE"
            | "IF"
            | "EVALUATE"
            | "SEARCH"
            | "SET"
            | "PERFORM"
            | "GO"
            | "GOBACK"
            | "STOP"
            | "CALL"
            | "OPEN"
            | "READ"
            | "WRITE"
            | "REWRITE"
            | "DELETE"
            | "CLOSE"
            | "SORT"
            | "RELEASE"
            | "RETURN"
            | "INSPECT"
            | "EXAMINE"
            | "STRING"
            | "UNSTRING"
            | "READY"
            | "RESET"
            | "CONTINUE"
            | "ALTER"
            | "EXEC"
            | "MERGE"
            | "NEXT"
    )
}

fn parse_set(words: &[String]) -> Option<StatementKindAst> {
    if words.len() >= 5
        && words.get(2).map(|word| word.eq_ignore_ascii_case("UP")) == Some(true)
        && words.get(3).map(|word| word.eq_ignore_ascii_case("BY")) == Some(true)
    {
        return Some(StatementKindAst::SetIndex {
            index: sanitize_cobol_name(words.get(1)?),
            operation: SetIndexAst::UpBy(words.get(4..)?.join(" ")),
        });
    }
    if words.len() >= 5
        && words.get(2).map(|word| word.eq_ignore_ascii_case("DOWN")) == Some(true)
        && words.get(3).map(|word| word.eq_ignore_ascii_case("BY")) == Some(true)
    {
        return Some(StatementKindAst::SetIndex {
            index: sanitize_cobol_name(words.get(1)?),
            operation: SetIndexAst::DownBy(words.get(4..)?.join(" ")),
        });
    }

    let to_idx = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("TO"))?;
    if to_idx <= 1 {
        return None;
    }
    let condition = words.get(1..to_idx)?.join(" ");
    let value = words.get(to_idx + 1)?;
    if value.eq_ignore_ascii_case("TRUE") {
        Some(StatementKindAst::SetCondition {
            condition,
            value: true,
        })
    } else if value.eq_ignore_ascii_case("FALSE") {
        Some(StatementKindAst::SetCondition {
            condition,
            value: false,
        })
    } else {
        Some(StatementKindAst::SetIndex {
            index: sanitize_cobol_name(&condition),
            operation: SetIndexAst::To(words.get(to_idx + 1..)?.join(" ")),
        })
    }
}

fn parse_call(words: &[String]) -> StatementKindAst {
    let Some(target) = words.get(1) else {
        return StatementKindAst::Unsupported("CALL".to_string());
    };
    let using = match words.len() {
        2 => Vec::new(),
        len if len > 3
            && words
                .get(2)
                .is_some_and(|word| word.eq_ignore_ascii_case("USING")) =>
        {
            group_qualified_operands(&words[3..])
        }
        _ => return StatementKindAst::Unsupported("CALL".to_string()),
    };
    StatementKindAst::Call {
        target: target.clone(),
        using,
    }
}

fn group_qualified_operands(words: &[String]) -> Vec<String> {
    let mut operands = Vec::new();
    let mut idx = 0usize;
    while idx < words.len() {
        let (operand, consumed) = qualified_operand_at(words, idx);
        idx += consumed;
        operands.push(operand);
    }
    operands
}

fn qualified_operand_at(words: &[String], start: usize) -> (String, usize) {
    if words[start].eq_ignore_ascii_case("BY")
        && words
            .get(start + 1)
            .is_some_and(|word| is_call_using_mode(word))
        && start + 2 < words.len()
    {
        let (argument, argument_consumed) = qualified_operand_at(words, start + 2);
        return (
            format!("{} {} {}", words[start], words[start + 1], argument),
            argument_consumed + 2,
        );
    }
    let mut operand = words[start].clone();
    let mut consumed = 1usize;
    while start + consumed + 1 < words.len()
        && (words[start + consumed].eq_ignore_ascii_case("OF")
            || words[start + consumed].eq_ignore_ascii_case("IN"))
    {
        operand.push(' ');
        operand.push_str(&words[start + consumed]);
        operand.push(' ');
        operand.push_str(&words[start + consumed + 1]);
        consumed += 2;
    }
    (operand, consumed)
}

fn is_call_using_mode(word: &str) -> bool {
    word.eq_ignore_ascii_case("REFERENCE")
        || word.eq_ignore_ascii_case("CONTENT")
        || word.eq_ignore_ascii_case("VALUE")
}

fn parse_perform(words: &[String]) -> StatementKindAst {
    let Some(target) = words.get(1) else {
        return StatementKindAst::Unsupported("PERFORM".to_string());
    };
    let upper_target = target.to_ascii_uppercase();
    if matches!(
        upper_target.as_str(),
        "VARYING" | "UNTIL" | "WITH" | "FOREVER"
    ) {
        return StatementKindAst::Unsupported(format!("PERFORM {upper_target}"));
    }
    if words
        .iter()
        .any(|word| word.eq_ignore_ascii_case("END-PERFORM"))
    {
        return StatementKindAst::Unsupported("INLINE PERFORM".to_string());
    }
    let through = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("THRU") || word.eq_ignore_ascii_case("THROUGH"))
        .and_then(|idx| words.get(idx + 1))
        .map(|word| sanitize_cobol_name(word));
    let times = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("TIMES"))
        .and_then(|idx| idx.checked_sub(1))
        .and_then(|idx| words.get(idx))
        .cloned();
    let until = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("UNTIL"))
        .map(|idx| {
            words[idx + 1..]
                .iter()
                .take_while(|word| !is_perform_until_trailer(word))
                .cloned()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|value| !value.is_empty());
    let varying = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("VARYING"))
        .map(|idx| {
            words[idx + 1..]
                .iter()
                .take_while(|word| !is_perform_varying_trailer(word))
                .cloned()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|value| !value.is_empty());
    let test_position = parse_perform_test_position(words);
    StatementKindAst::Perform {
        target: sanitize_cobol_name(target),
        through,
        varying,
        until,
        times,
        test_position,
    }
}

fn is_perform_until_trailer(word: &str) -> bool {
    word.eq_ignore_ascii_case("VARYING")
        || word.eq_ignore_ascii_case("TIMES")
        || word.eq_ignore_ascii_case("WITH")
}

fn is_perform_varying_trailer(word: &str) -> bool {
    word.eq_ignore_ascii_case("UNTIL")
        || word.eq_ignore_ascii_case("TIMES")
        || word.eq_ignore_ascii_case("WITH")
}

fn parse_perform_test_position(words: &[String]) -> Option<PerformTestPositionAst> {
    words.windows(3).find_map(|window| {
        if window[0].eq_ignore_ascii_case("WITH") && window[1].eq_ignore_ascii_case("TEST") {
            if window[2].eq_ignore_ascii_case("BEFORE") {
                Some(PerformTestPositionAst::Before)
            } else if window[2].eq_ignore_ascii_case("AFTER") {
                Some(PerformTestPositionAst::After)
            } else {
                None
            }
        } else {
            None
        }
    })
}

fn is_paragraph_label(raw: &str) -> bool {
    let words = words(raw);
    if words.len() != 1 {
        return false;
    }
    let upper = words[0].to_ascii_uppercase();
    !matches!(
        upper.as_str(),
        "ACCEPT"
            | "ADD"
            | "ALTER"
            | "CALL"
            | "CLOSE"
            | "COMPUTE"
            | "CONTINUE"
            | "DELETE"
            | "DISPLAY"
            | "DIVIDE"
            | "DIVISION"
            | "EVALUATE"
            | "EXAMINE"
            | "EXEC"
            | "EXIT"
            | "GO"
            | "GOBACK"
            | "IF"
            | "INSPECT"
            | "MERGE"
            | "MOVE"
            | "MULTIPLY"
            | "NEXT"
            | "OPEN"
            | "PERFORM"
            | "READ"
            | "READY"
            | "RELEASE"
            | "RESET"
            | "RETURN"
            | "REWRITE"
            | "SEARCH"
            | "SECTION"
            | "SET"
            | "SORT"
            | "STOP"
            | "STRING"
            | "SUBTRACT"
            | "UNSTRING"
            | "USE"
            | "WRITE"
    )
}

fn is_section_label(raw: &str) -> bool {
    let words = words(raw);
    words.len() == 2 && words[1].eq_ignore_ascii_case("SECTION")
}

fn parse_declarative_trigger(raw: &str) -> DeclarativeTriggerAst {
    let words = words(raw);
    if words.len() >= 5
        && words[0].eq_ignore_ascii_case("USE")
        && words[1].eq_ignore_ascii_case("AFTER")
        && words[2].eq_ignore_ascii_case("ERROR")
        && words[3].eq_ignore_ascii_case("ON")
    {
        let target = sanitize_cobol_name(&words[4]);
        if ["INPUT", "OUTPUT", "I-O", "IO"]
            .iter()
            .any(|word| word.eq_ignore_ascii_case(&target))
        {
            DeclarativeTriggerAst::Unsupported(raw.to_string())
        } else {
            DeclarativeTriggerAst::FileError(target)
        }
    } else if words.len() >= 5
        && words[0].eq_ignore_ascii_case("USE")
        && words[1].eq_ignore_ascii_case("FOR")
        && words[2].eq_ignore_ascii_case("DEBUGGING")
        && words[3].eq_ignore_ascii_case("ON")
    {
        let target = sanitize_cobol_name(&words[4]);
        if target.eq_ignore_ascii_case("ALL") {
            DeclarativeTriggerAst::Unsupported(raw.to_string())
        } else {
            DeclarativeTriggerAst::Debugging(target)
        }
    } else {
        DeclarativeTriggerAst::Unsupported(raw.to_string())
    }
}

fn words(raw: &str) -> Vec<String> {
    cobol_text::split_cobol_words(raw)
}

fn parse_display_statement(
    spanned_words: &[SpannedWord],
    sentence_raw: &str,
    words: &[String],
) -> StatementKindAst {
    let mut out = Vec::new();
    let mut idx = 1usize;
    while idx < words.len() {
        if words[idx].eq_ignore_ascii_case("UPON") {
            return parse_display_option_tail(words, idx, out);
        }
        if words[idx].eq_ignore_ascii_case("WITH") {
            return unsupported_display_with_clause(words, idx);
        }
        if words[idx].eq_ignore_ascii_case("FUNCTION") {
            let Some((operand, next_idx)) =
                display_function_operand(spanned_words, sentence_raw, words, idx)
            else {
                return StatementKindAst::Unsupported("DISPLAY FUNCTION".to_string());
            };
            out.push(operand);
            idx = next_idx;
            continue;
        }
        let (operand, consumed) = qualified_operand_at(words, idx);
        idx += consumed;
        out.push(operand);
    }
    StatementKindAst::Display(out)
}

fn display_function_operand(
    spanned_words: &[SpannedWord],
    sentence_raw: &str,
    words: &[String],
    idx: usize,
) -> Option<(String, usize)> {
    let name_idx = idx.checked_add(1)?;
    let function_name = words.get(name_idx)?;
    let mut next_idx = name_idx + 1;
    let mut depth = paren_delta(function_name);

    if depth == 0 && words.get(next_idx).is_some_and(|word| word == "(") {
        depth += 1;
        next_idx += 1;
    }

    while depth > 0 {
        let word = words.get(next_idx)?;
        depth += paren_delta(word);
        next_idx += 1;
    }

    if depth == 0
        && words
            .get(next_idx)
            .is_some_and(|word| word.eq_ignore_ascii_case("OF"))
        && words.get(next_idx + 1).is_some()
    {
        next_idx += 2;
        while next_idx + 1 < words.len()
            && (words[next_idx].eq_ignore_ascii_case("OF")
                || words[next_idx].eq_ignore_ascii_case("IN"))
        {
            next_idx += 2;
        }
    }

    Some((
        raw_from_words(spanned_words.get(idx..next_idx)?, sentence_raw)
            .trim()
            .to_string(),
        next_idx,
    ))
}

fn paren_delta(word: &str) -> isize {
    cobol_text::literal_aware_char_indices(word).fold(0isize, |depth, item| {
        if item.inside_literal {
            return depth;
        }
        match item.ch {
            '(' => depth + 1,
            ')' => depth - 1,
            _ => depth,
        }
    })
}

fn parse_display_option_tail(
    words: &[String],
    mut idx: usize,
    operands: Vec<String>,
) -> StatementKindAst {
    if !words[idx].eq_ignore_ascii_case("UPON") {
        return StatementKindAst::Unsupported("DISPLAY".to_string());
    }
    idx += 1;
    let Some(destination) = words.get(idx) else {
        return StatementKindAst::Unsupported("DISPLAY UPON".to_string());
    };
    if !destination.eq_ignore_ascii_case("CONSOLE") {
        return StatementKindAst::Unsupported(format!("DISPLAY UPON {destination}"));
    }
    idx += 1;
    if idx >= words.len() {
        return StatementKindAst::Display(operands);
    }
    if words[idx].eq_ignore_ascii_case("WITH") {
        return unsupported_display_with_clause(words, idx);
    }
    StatementKindAst::Unsupported("DISPLAY".to_string())
}

fn unsupported_display_with_clause(words: &[String], idx: usize) -> StatementKindAst {
    if idx + 2 < words.len()
        && words[idx].eq_ignore_ascii_case("WITH")
        && words[idx + 1].eq_ignore_ascii_case("NO")
        && words[idx + 2].eq_ignore_ascii_case("ADVANCING")
    {
        StatementKindAst::Unsupported("DISPLAY WITH NO ADVANCING".to_string())
    } else {
        StatementKindAst::Unsupported("DISPLAY WITH".to_string())
    }
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
    fn rejects_text_without_program_id() {
        assert!(matches!(
            parse_program("bad.cbl", "THIS IS NOT A PROGRAM."),
            Err(SyntaxError::EmptyProgram)
        ));
    }

    #[test]
    fn rejects_program_id_without_name() {
        assert!(matches!(
            parse_program(
                "bad.cbl",
                "IDENTIFICATION DIVISION.\nPROGRAM-ID.\nPROCEDURE DIVISION.\nSTOP RUN.\n"
            ),
            Err(SyntaxError::EmptyProgram)
        ));
    }

    #[test]
    fn select_optional_uses_following_file_name() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. X.\nENVIRONMENT DIVISION.\nINPUT-OUTPUT SECTION.\nFILE-CONTROL.\nSELECT OPTIONAL INFILE ASSIGN TO \"in.dat\".\n";
        let ast = parse_program("x.cbl", src).expect("program parses");
        assert_eq!(ast.files.len(), 1);
        assert_eq!(ast.files[0].name, "INFILE");
        assert_eq!(ast.files[0].assign.as_deref(), Some("in.dat"));
    }

    #[test]
    fn one_word_continue_is_statement_not_paragraph_label() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. X.\nPROCEDURE DIVISION.\nMAIN.\nCONTINUE.\nSTOP RUN.\n";
        let ast = parse_program("x.cbl", src).expect("program parses");
        assert_eq!(ast.paragraphs.len(), 1);
        assert_eq!(ast.paragraphs[0].name, "MAIN");
        assert_eq!(ast.paragraphs[0].statements.len(), 2);
        assert!(matches!(
            ast.paragraphs[0].statements[0].kind,
            StatementKindAst::Continue
        ));
        assert!(ast.diagnostics.is_empty(), "{:?}", ast.diagnostics);
    }

    #[test]
    fn ready_and_reset_trace_are_case_insensitive() {
        let ready = parse_statement("ready trace", SourceSpan::generated());
        assert!(matches!(ready.kind, StatementKindAst::ReadyTrace));
        let reset = parse_statement("reset trace", SourceSpan::generated());
        assert!(matches!(reset.kind, StatementKindAst::ResetTrace));
    }

    #[test]
    fn go_to_without_target_is_unsupported() {
        let statement = parse_statement("GO TO", SourceSpan::generated());
        assert!(matches!(statement.kind, StatementKindAst::Unsupported(_)));
    }

    #[test]
    fn go_to_with_extra_trailing_words_is_unsupported() {
        let statement = parse_statement("GO TO TARGET EXTRA", SourceSpan::generated());
        assert!(matches!(
            statement.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "GO"
        ));
    }

    #[test]
    fn stop_run_with_extra_trailing_words_is_unsupported() {
        let statement = parse_statement("STOP RUN EXTRA", SourceSpan::generated());
        assert!(matches!(
            statement.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "STOP"
        ));
    }

    #[test]
    fn computed_go_to_with_extra_trailing_words_is_unsupported() {
        let statement = parse_statement(
            "GO TO TARGET-A TARGET-B DEPENDING ON WS-IDX EXTRA",
            SourceSpan::generated(),
        );
        assert!(matches!(
            statement.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "GO"
        ));
    }

    #[test]
    fn call_without_using_rejects_extra_trailing_words() {
        let statement = parse_statement("CALL \"SUB\" EXTRA", SourceSpan::generated());
        assert!(matches!(
            statement.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "CALL"
        ));
    }

    #[test]
    fn call_using_keeps_qualified_arguments_together() {
        let statement = parse_statement(
            "CALL \"SUB\" USING CUSTOMER-NAME OF CUSTOMER-REC ACCOUNT-ID IN ACCOUNT-REC",
            SourceSpan::generated(),
        );
        let StatementKindAst::Call { target, using } = statement.kind else {
            panic!("expected CALL AST");
        };
        assert_eq!(target, "\"SUB\"");
        assert_eq!(
            using,
            vec![
                "CUSTOMER-NAME OF CUSTOMER-REC".to_string(),
                "ACCOUNT-ID IN ACCOUNT-REC".to_string()
            ]
        );
    }

    #[test]
    fn call_using_keeps_by_mode_with_argument() {
        let statement = parse_statement(
            "CALL \"SUB\" USING BY REFERENCE CUSTOMER-NAME OF CUSTOMER-REC BY CONTENT ACCOUNT-ID",
            SourceSpan::generated(),
        );
        let StatementKindAst::Call { using, .. } = statement.kind else {
            panic!("expected CALL AST");
        };
        assert_eq!(
            using,
            vec![
                "BY REFERENCE CUSTOMER-NAME OF CUSTOMER-REC".to_string(),
                "BY CONTENT ACCOUNT-ID".to_string()
            ]
        );
    }

    #[test]
    fn perform_until_does_not_include_with_test_phrase_in_condition() {
        let statement = parse_statement(
            "PERFORM CHECK-ROW UNTIL WS-FLAG = \"Y\" WITH TEST AFTER",
            SourceSpan::generated(),
        );
        let StatementKindAst::Perform {
            target,
            until,
            test_position,
            ..
        } = statement.kind
        else {
            panic!("expected PERFORM AST");
        };
        assert_eq!(target, "CHECK_ROW");
        assert_eq!(until.as_deref(), Some("WS-FLAG = \"Y\""));
        assert_eq!(test_position, Some(PerformTestPositionAst::After));
    }

    #[test]
    fn perform_with_test_before_is_preserved_in_ast() {
        let statement = parse_statement(
            "PERFORM CHECK-ROW WITH TEST BEFORE UNTIL WS-FLAG = \"Y\"",
            SourceSpan::generated(),
        );
        let StatementKindAst::Perform {
            until,
            test_position,
            ..
        } = statement.kind
        else {
            panic!("expected PERFORM AST");
        };
        assert_eq!(until.as_deref(), Some("WS-FLAG = \"Y\""));
        assert_eq!(test_position, Some(PerformTestPositionAst::Before));
    }

    #[test]
    fn release_without_from_rejects_extra_trailing_words() {
        let statement = parse_statement("RELEASE SORT-REC EXTRA", SourceSpan::generated());
        assert!(matches!(
            statement.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "RELEASE"
        ));
    }

    #[test]
    fn alter_rejects_extra_trailing_words_after_target() {
        let statement = parse_statement(
            "ALTER HANDLE-EXIT TO PROCEED TO ADD-EXIT EXTRA",
            SourceSpan::generated(),
        );
        assert!(matches!(
            statement.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "ALTER"
        ));
    }

    #[test]
    fn trace_statements_reject_extra_trailing_words() {
        let ready = parse_statement("READY TRACE EXTRA", SourceSpan::generated());
        assert!(matches!(
            ready.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "READY"
        ));

        let reset = parse_statement("RESET TRACE EXTRA", SourceSpan::generated());
        assert!(matches!(
            reset.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "RESET"
        ));
    }

    #[test]
    fn go_to_period_terminator_is_not_dot_target() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. X.\nPROCEDURE DIVISION.\nMAIN.\nGO TO.\n";
        let ast = parse_program("x.cbl", src).expect("program parses");
        assert_eq!(ast.paragraphs[0].statements.len(), 1);
        assert!(matches!(
            ast.paragraphs[0].statements[0].kind,
            StatementKindAst::Unsupported(_)
        ));
    }

    #[test]
    fn go_to_dot_placeholder_is_preserved_when_separated_from_period() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. X.\nPROCEDURE DIVISION.\nMAIN.\nGO TO .\n";
        assert!(src.contains("GO TO ."));
        let ast = parse_program("x.cbl", src).expect("program parses");
        assert_eq!(split_sentences(src, "x.cbl").last().unwrap().raw, "GO TO .");
        assert_eq!(ast.paragraphs[0].statements.len(), 1);
        assert!(
            matches!(
                ast.paragraphs[0].statements[0].kind,
                StatementKindAst::GoTo(ref target) if target == "."
            ),
            "{:?}",
            ast.paragraphs[0].statements[0].kind
        );
    }

    #[test]
    fn decimal_point_inside_numeric_literal_does_not_split_sentence() {
        let sentences = split_sentences("PROCEDURE DIVISION.\nMAIN.\nDISPLAY 12.34.\n", "x.cbl");
        assert!(sentences
            .iter()
            .any(|sentence| sentence.raw == "DISPLAY 12.34"));
    }

    #[test]
    fn lex_treats_doubled_quote_literal_as_one_token() {
        let tokens = lex("x.cbl", "DISPLAY \"A\"\"B\".");
        let literals = tokens
            .iter()
            .filter(|token| token.kind == "StringLiteral")
            .collect::<Vec<_>>();
        assert_eq!(literals.len(), 1, "{tokens:?}");
        assert_eq!(literals[0].lexeme, "\"A\"\"B\"");
    }

    #[test]
    fn doubled_quote_period_does_not_split_sentence() {
        let sentences = split_sentences(
            "PROCEDURE DIVISION.\nMAIN.\nDISPLAY \"A\"\".B\".\nDISPLAY \"DONE\".\n",
            "x.cbl",
        );
        assert!(sentences
            .iter()
            .any(|sentence| sentence.raw == "DISPLAY \"A\"\".B\""));
        assert!(sentences
            .iter()
            .any(|sentence| sentence.raw == "DISPLAY \"DONE\""));
    }

    #[test]
    fn period_inside_literal_does_not_split_sentence() {
        let sentences = split_sentences(
            "PROCEDURE DIVISION.\nMAIN.\nDISPLAY \"A.B\".\nDISPLAY \"DONE\".\n",
            "x.cbl",
        );
        assert!(sentences
            .iter()
            .any(|sentence| sentence.raw == "DISPLAY \"A.B\""));
        assert!(sentences
            .iter()
            .any(|sentence| sentence.raw == "DISPLAY \"DONE\""));
    }

    #[test]
    fn stop_without_run_is_unsupported() {
        let statement = parse_statement("STOP \"PAUSE\"", SourceSpan::generated());
        assert!(matches!(statement.kind, StatementKindAst::Unsupported(_)));
    }

    #[test]
    fn display_keeps_qualified_operands_together() {
        let statement = parse_statement("DISPLAY WS-ITEM(1) OF WS-TABLE", SourceSpan::generated());
        assert!(matches!(
            statement.kind,
            StatementKindAst::Display(values) if values == vec!["WS-ITEM(1) OF WS-TABLE"]
        ));
    }

    #[test]
    fn display_keeps_in_qualified_operands_together() {
        let statement = parse_statement("DISPLAY WS-ITEM IN WS-GROUP", SourceSpan::generated());
        assert!(matches!(
            statement.kind,
            StatementKindAst::Display(values) if values == vec!["WS-ITEM IN WS-GROUP"]
        ));
    }

    #[test]
    fn display_upon_console_does_not_treat_destination_as_operand() {
        let statement = parse_statement("DISPLAY \"HELLO\" UPON CONSOLE", SourceSpan::generated());
        assert!(matches!(
            statement.kind,
            StatementKindAst::Display(values) if values == vec!["\"HELLO\""]
        ));
    }

    #[test]
    fn display_function_current_date_keeps_function_as_single_operand() {
        let statement = parse_statement("DISPLAY FUNCTION CURRENT-DATE", SourceSpan::generated());
        assert!(matches!(
            statement.kind,
            StatementKindAst::Display(values) if values == vec!["FUNCTION CURRENT-DATE"]
        ));
    }

    #[test]
    fn display_function_with_spaced_parentheses_keeps_function_as_single_operand() {
        let statement = parse_statement(
            "DISPLAY FUNCTION LENGTH ( WS-FIELD )",
            SourceSpan::generated(),
        );
        assert!(matches!(
            statement.kind,
            StatementKindAst::Display(values) if values == vec!["FUNCTION LENGTH ( WS-FIELD )"]
        ));
    }

    #[test]
    fn display_function_length_of_keeps_argument_as_single_operand() {
        let statement = parse_statement(
            "DISPLAY FUNCTION LENGTH OF WS-FIELD",
            SourceSpan::generated(),
        );
        assert!(matches!(
            statement.kind,
            StatementKindAst::Display(values) if values == vec!["FUNCTION LENGTH OF WS-FIELD"]
        ));
    }

    #[test]
    fn display_function_length_of_keeps_qualified_argument_as_single_operand() {
        let statement = parse_statement(
            "DISPLAY FUNCTION LENGTH OF WS-ITEM OF WS-GROUP",
            SourceSpan::generated(),
        );
        assert!(matches!(
            statement.kind,
            StatementKindAst::Display(values) if values == vec!["FUNCTION LENGTH OF WS-ITEM OF WS-GROUP"]
        ));
    }

    #[test]
    fn display_function_length_of_keeps_reference_modified_argument_as_single_operand() {
        let statement = parse_statement(
            "DISPLAY FUNCTION LENGTH OF WS-TEXT(2:3)",
            SourceSpan::generated(),
        );
        assert!(matches!(
            statement.kind,
            StatementKindAst::Display(values) if values == vec!["FUNCTION LENGTH OF WS-TEXT(2:3)"]
        ));
    }

    #[test]
    fn display_function_preserves_comma_text_inside_function_operand() {
        let statement = parse_statement(
            "DISPLAY FUNCTION LENGTH(\"A\", \"B\")",
            SourceSpan::generated(),
        );
        assert!(matches!(
            statement.kind,
            StatementKindAst::Display(values) if values == vec!["FUNCTION LENGTH(\"A\", \"B\")"]
        ));
    }

    #[test]
    fn display_function_literal_parentheses_do_not_consume_following_statement() {
        let statements = parse_imperative_list(
            "DISPLAY FUNCTION LENGTH(\"(\") DISPLAY \"NEXT\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 2);
        assert!(matches!(
            statements[0].kind,
            StatementKindAst::Display(ref values) if values.as_slice() == ["FUNCTION LENGTH(\"(\")"]
        ));
        assert!(matches!(
            statements[1].kind,
            StatementKindAst::Display(ref values) if values.as_slice() == ["\"NEXT\""]
        ));
    }

    #[test]
    fn display_no_advancing_fails_closed_instead_of_parsing_option_words_as_operands() {
        let statement = parse_statement(
            "DISPLAY \"HELLO\" WITH NO ADVANCING",
            SourceSpan::generated(),
        );
        assert!(matches!(
            statement.kind,
            StatementKindAst::Unsupported(keyword) if keyword == "DISPLAY WITH NO ADVANCING"
        ));
    }

    #[test]
    fn add_to_giving_lowers_to_compute_ast_without_merging_target_words() {
        let statement = parse_statement("ADD WS-A TO WS-B GIVING WS-C", SourceSpan::generated());
        let StatementKindAst::Compute(compute) = statement.kind else {
            panic!("expected ADD GIVING to lower through COMPUTE AST");
        };

        assert_eq!(compute.target, "WS-C");
        assert_eq!(compute.expression, "WS-A + WS-B");
        assert!(compute.on_size_error.is_empty());
        assert!(compute.not_on_size_error.is_empty());
    }

    #[test]
    fn compute_expression_preserves_comma_text_inside_function_operand() {
        let statement = parse_statement(
            "COMPUTE WS-N = FUNCTION LENGTH(\"A\", \"B\")",
            SourceSpan::generated(),
        );
        let StatementKindAst::Compute(compute) = statement.kind else {
            panic!("expected COMPUTE");
        };
        assert_eq!(compute.expression, "FUNCTION LENGTH(\"A\", \"B\")");
    }

    #[test]
    fn compute_target_preserves_subscript_syntax_for_sema() {
        let statement = parse_statement("COMPUTE WS-TABLE(2) = 5", SourceSpan::generated());
        let StatementKindAst::Compute(compute) = statement.kind else {
            panic!("expected COMPUTE");
        };

        assert_eq!(compute.target, "WS-TABLE(2)");
        assert_eq!(compute.expression, "5");
    }

    #[test]
    fn if_condition_preserves_comma_text_inside_function_operand() {
        let statement = parse_statement(
            "IF FUNCTION LENGTH(\"A\", \"B\") = 1 DISPLAY \"BAD\"",
            SourceSpan::generated(),
        );
        let StatementKindAst::If { condition, .. } = statement.kind else {
            panic!("expected IF");
        };
        assert_eq!(condition, "FUNCTION LENGTH(\"A\", \"B\") = 1");
    }

    #[test]
    fn if_condition_keeps_statement_keyword_condition_name_qualification() {
        let statement = parse_statement(
            "IF READY OF A-FLAG OF A-REC DISPLAY \"A\" END-IF",
            SourceSpan::generated(),
        );
        let StatementKindAst::If {
            condition,
            then_statements,
            ..
        } = statement.kind
        else {
            panic!("expected typed IF");
        };
        assert_eq!(condition, "READY OF A-FLAG OF A-REC");
        assert!(matches!(
            then_statements.first().map(|statement| &statement.kind),
            Some(StatementKindAst::Display(values)) if values.as_slice() == ["\"A\""]
        ));
    }

    #[test]
    fn nested_if_binds_else_to_nearest_if() {
        let statement = parse_statement(
            "IF A = \"Y\" IF B = \"Y\" DISPLAY \"B\" ELSE DISPLAY \"INNER\"",
            SourceSpan::generated(),
        );
        let StatementKindAst::If {
            then_statements,
            else_statements,
            ..
        } = statement.kind
        else {
            panic!("expected typed IF");
        };
        assert!(else_statements.is_empty());
        assert_eq!(then_statements.len(), 1);
        assert!(matches!(
            then_statements[0].kind,
            StatementKindAst::If {
                ref then_statements,
                ref else_statements,
                ..
            } if then_statements.len() == 1
                && else_statements.len() == 1
                && matches!(then_statements[0].kind, StatementKindAst::Display(ref values) if values.as_slice() == ["\"B\""])
                && matches!(else_statements[0].kind, StatementKindAst::Display(ref values) if values.as_slice() == ["\"INNER\""])
        ));
    }

    #[test]
    fn if_statement_uses_typed_nested_statement_lists() {
        let statement = parse_statement(
            "IF A = \"Y\" IF B = \"Y\" DISPLAY \"B\" ELSE DISPLAY \"A\" END-IF ELSE DISPLAY \"N\" END-IF",
            SourceSpan::generated(),
        );
        let StatementKindAst::If {
            condition,
            then_statements,
            else_statements,
        } = statement.kind
        else {
            panic!("expected typed IF");
        };
        assert_eq!(condition, "A = \"Y\"");
        assert_eq!(then_statements.len(), 1);
        assert_eq!(else_statements.len(), 1);
        assert!(matches!(
            then_statements[0].kind,
            StatementKindAst::If {
                ref condition,
                ref then_statements,
                ref else_statements,
            } if condition == "B = \"Y\""
                && matches!(then_statements[0].kind, StatementKindAst::Display(ref values) if values.as_slice() == ["\"B\""])
                && matches!(else_statements[0].kind, StatementKindAst::Display(ref values) if values.as_slice() == ["\"A\""])
        ));
        assert!(matches!(
            else_statements[0].kind,
            StatementKindAst::Display(ref values) if values.as_slice() == ["\"N\""]
        ));
    }

    #[test]
    fn if_statement_with_then_and_end_if_uses_typed_nested_statement_lists() {
        let statement = parse_statement(
            "IF A = \"Y\" THEN IF B = \"Y\" THEN DISPLAY \"B\" END-IF ELSE DISPLAY \"N\" END-IF",
            SourceSpan::generated(),
        );
        let StatementKindAst::If {
            condition,
            then_statements,
            else_statements,
        } = statement.kind
        else {
            panic!("expected typed IF");
        };
        assert_eq!(condition, "A = \"Y\"");
        assert_eq!(then_statements.len(), 1);
        assert_eq!(else_statements.len(), 1);
        assert!(matches!(
            then_statements[0].kind,
            StatementKindAst::If {
                ref condition,
                ref then_statements,
                ref else_statements,
            } if condition == "B = \"Y\""
                && then_statements.len() == 1
                && else_statements.is_empty()
                && matches!(then_statements[0].kind, StatementKindAst::Display(ref values) if values.as_slice() == ["\"B\""])
        ));
        assert!(matches!(
            else_statements[0].kind,
            StatementKindAst::Display(ref values) if values.as_slice() == ["\"N\""]
        ));
    }

    #[test]
    fn read_statement_ast_preserves_typed_branch_body() {
        let statements = parse_imperative_list(
            "READ INFILE AT END DISPLAY \"A\" DISPLAY \"B\" END-READ",
            SourceSpan::generated(),
        );
        let StatementKindAst::Read(read) = &statements[0].kind else {
            panic!("expected typed READ AST");
        };
        assert_eq!(read.file, "INFILE");
        assert_eq!(read.at_end.len(), 2);
        assert!(matches!(read.at_end[0].kind, StatementKindAst::Display(_)));
        assert!(matches!(read.at_end[1].kind, StatementKindAst::Display(_)));
    }

    #[test]
    fn read_branch_statement_raw_uses_exact_normalized_sentence_substring() {
        let statements = parse_imperative_list(
            "READ INFILE AT END DISPLAY   \"A.B\" DISPLAY \"A\"\"B\" END-READ",
            SourceSpan::generated(),
        );
        let StatementKindAst::Read(read) = &statements[0].kind else {
            panic!("expected typed READ AST");
        };
        assert_eq!(read.at_end.len(), 2);
        assert_eq!(read.at_end[0].raw, "DISPLAY   \"A.B\"");
        assert_eq!(read.at_end[1].raw, "DISPLAY \"A\"\"B\"");
    }

    #[test]
    fn read_period_scoped_at_end_keeps_branch_body_inside_read() {
        let statements = parse_imperative_list(
            "READ INFILE AT END DISPLAY \"EOF\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 1, "{statements:?}");
        let StatementKindAst::Read(read) = &statements[0].kind else {
            panic!("expected typed READ AST");
        };
        assert_eq!(read.at_end.len(), 1);
        assert!(matches!(
            read.at_end[0].kind,
            StatementKindAst::Display(ref values) if values.as_slice() == ["\"EOF\""]
        ));
        assert_eq!(read.at_end[0].raw, "DISPLAY \"EOF\"");
    }

    #[test]
    fn read_at_end_branch_ignores_nested_search_at_end_phrase() {
        let statements = parse_imperative_list(
            "READ INFILE AT END SEARCH WS-ITEM AT END DISPLAY \"SEARCH-END\" WHEN WS-ITEM = \"A\" DISPLAY \"FOUND\" END-SEARCH NOT AT END DISPLAY \"READ-OK\" END-READ",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 1, "{statements:?}");
        let StatementKindAst::Read(read) = &statements[0].kind else {
            panic!("expected typed READ AST");
        };
        assert_eq!(read.at_end.len(), 1, "{read:?}");
        let StatementKindAst::Search(search) = &read.at_end[0].kind else {
            panic!("expected nested SEARCH AST");
        };
        assert_eq!(search.at_end.len(), 1);
        assert_eq!(search.whens.len(), 1);
        assert_eq!(read.not_at_end.len(), 1);
        assert!(matches!(
            read.not_at_end[0].kind,
            StatementKindAst::Display(ref values) if values.as_slice() == ["\"READ-OK\""]
        ));
    }

    #[test]
    fn read_at_end_branch_ignores_nested_read_not_at_end_phrase() {
        let statements = parse_imperative_list(
            "READ OUTER-FILE AT END READ INNER-FILE AT END DISPLAY \"INNER-END\" NOT AT END DISPLAY \"INNER-OK\" END-READ NOT AT END DISPLAY \"OUTER-OK\" END-READ",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 1, "{statements:?}");
        let StatementKindAst::Read(outer) = &statements[0].kind else {
            panic!("expected outer READ AST");
        };
        assert_eq!(outer.at_end.len(), 1, "{outer:?}");
        let StatementKindAst::Read(inner) = &outer.at_end[0].kind else {
            panic!("expected nested READ AST");
        };
        assert_eq!(inner.at_end.len(), 1);
        assert_eq!(inner.not_at_end.len(), 1);
        assert_eq!(outer.not_at_end.len(), 1);
        assert!(matches!(
            outer.not_at_end[0].kind,
            StatementKindAst::Display(ref values) if values.as_slice() == ["\"OUTER-OK\""]
        ));
    }

    #[test]
    fn return_period_scoped_at_end_keeps_go_to_inside_return() {
        let statements = parse_imperative_list(
            "RETURN SORT-FILE AT END GO TO SORT-DONE",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 1, "{statements:?}");
        let StatementKindAst::Return(ret) = &statements[0].kind else {
            panic!("expected typed RETURN AST");
        };
        assert_eq!(ret.at_end.len(), 1);
        assert!(matches!(
            ret.at_end[0].kind,
            StatementKindAst::GoTo(ref target) if target == "SORT_DONE"
        ));
        assert_eq!(ret.at_end[0].raw, "GO TO SORT-DONE");
    }

    #[test]
    fn return_at_end_branch_ignores_nested_return_not_at_end_phrase() {
        let statements = parse_imperative_list(
            "RETURN OUTER-SORT AT END RETURN INNER-SORT AT END DISPLAY \"INNER-END\" NOT AT END DISPLAY \"INNER-OK\" END-RETURN NOT AT END DISPLAY \"OUTER-OK\" END-RETURN",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 1, "{statements:?}");
        let StatementKindAst::Return(outer) = &statements[0].kind else {
            panic!("expected outer RETURN AST");
        };
        assert_eq!(outer.at_end.len(), 1, "{outer:?}");
        let StatementKindAst::Return(inner) = &outer.at_end[0].kind else {
            panic!("expected nested RETURN AST");
        };
        assert_eq!(inner.at_end.len(), 1);
        assert_eq!(inner.not_at_end.len(), 1);
        assert_eq!(outer.not_at_end.len(), 1);
        assert!(matches!(
            outer.not_at_end[0].kind,
            StatementKindAst::Display(ref values) if values.as_slice() == ["\"OUTER-OK\""]
        ));
    }

    #[test]
    fn string_overflow_branch_parses_nested_evaluate_from_token_slice() {
        let statements = parse_imperative_list(
            "STRING \"A\" DELIMITED BY SIZE INTO WS-TEXT ON OVERFLOW EVALUATE WS-FLAG WHEN \"Y\" DISPLAY \"A\" WHEN OTHER DISPLAY \"B\" END-EVALUATE END-STRING",
            SourceSpan::generated(),
        );
        let StatementKindAst::String(string) = &statements[0].kind else {
            panic!("expected typed STRING AST");
        };
        let StatementKindAst::Evaluate(evaluate) = &string.on_overflow[0].kind else {
            panic!("expected nested EVALUATE AST");
        };
        assert_eq!(evaluate.arms.len(), 2);
        assert_eq!(
            string.on_overflow[0].raw,
            "EVALUATE WS-FLAG WHEN \"Y\" DISPLAY \"A\" WHEN OTHER DISPLAY \"B\" END-EVALUATE"
        );
    }

    #[test]
    fn string_overflow_branch_ignores_nested_string_not_on_overflow() {
        let statements = parse_imperative_list(
            "STRING \"A\" DELIMITED BY SIZE INTO OUTER ON OVERFLOW STRING \"B\" DELIMITED BY SIZE INTO INNER ON OVERFLOW DISPLAY \"INNER-OVER\" NOT ON OVERFLOW DISPLAY \"INNER-OK\" END-STRING NOT ON OVERFLOW DISPLAY \"OUTER-OK\" END-STRING",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 1, "{statements:?}");
        let StatementKindAst::String(outer) = &statements[0].kind else {
            panic!("expected outer STRING AST");
        };
        assert_eq!(outer.on_overflow.len(), 1, "{outer:?}");
        let StatementKindAst::String(inner) = &outer.on_overflow[0].kind else {
            panic!("expected nested STRING AST");
        };
        assert_eq!(inner.on_overflow.len(), 1);
        assert_eq!(inner.not_on_overflow.len(), 1);
        assert_eq!(outer.not_on_overflow.len(), 1);
        assert!(matches!(
            outer.not_on_overflow[0].kind,
            StatementKindAst::Display(ref values) if values.as_slice() == ["\"OUTER-OK\""]
        ));
    }

    #[test]
    fn evaluate_arm_parses_nested_evaluate_without_splitting_outer_arm() {
        let statements = parse_imperative_list(
            "EVALUATE OUTER-FLAG WHEN \"Y\" EVALUATE INNER-FLAG WHEN \"A\" DISPLAY \"INNER-A\" WHEN OTHER DISPLAY \"INNER-OTHER\" END-EVALUATE WHEN OTHER DISPLAY \"OUTER-OTHER\" END-EVALUATE",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 1, "{statements:?}");
        let StatementKindAst::Evaluate(outer) = &statements[0].kind else {
            panic!("expected outer EVALUATE AST");
        };
        assert_eq!(outer.arms.len(), 2);
        assert_eq!(outer.arms[0].patterns, vec!["\"Y\"".to_string()]);
        let StatementKindAst::Evaluate(inner) = &outer.arms[0].statements[0].kind else {
            panic!("expected nested EVALUATE AST");
        };
        assert_eq!(inner.arms.len(), 2);
        assert!(matches!(
            outer.arms[1].statements[0].kind,
            StatementKindAst::Display(ref values) if values.as_slice() == ["\"OUTER-OTHER\""]
        ));
    }

    #[test]
    fn return_at_end_branch_parses_nested_search_from_token_slice() {
        let statements = parse_imperative_list(
            "RETURN SORT-FILE AT END SEARCH WS-ITEM AT END DISPLAY \"END\" WHEN WS-ITEM = \"A\" DISPLAY \"FOUND\" END-SEARCH END-RETURN",
            SourceSpan::generated(),
        );
        let StatementKindAst::Return(ret) = &statements[0].kind else {
            panic!("expected typed RETURN AST");
        };
        let StatementKindAst::Search(search) = &ret.at_end[0].kind else {
            panic!("expected nested SEARCH AST");
        };
        assert_eq!(search.at_end.len(), 1);
        assert_eq!(search.whens.len(), 1);
    }

    #[test]
    fn unsupported_branch_statement_preserves_exact_slice_without_leaking_tokens() {
        let statements = parse_imperative_list(
            "READ INFILE AT END ZORCH BAD DISPLAY \"OK\" END-READ",
            SourceSpan::generated(),
        );
        let StatementKindAst::Read(read) = &statements[0].kind else {
            panic!("expected typed READ AST");
        };
        assert_eq!(read.at_end.len(), 2);
        assert!(matches!(
            read.at_end[0].kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "ZORCH"
        ));
        assert_eq!(read.at_end[0].raw, "ZORCH BAD");
        assert!(matches!(read.at_end[1].kind, StatementKindAst::Display(_)));
    }

    #[test]
    fn next_sentence_remains_explicit_blocked_statement_shape() {
        let statement = parse_statement("NEXT SENTENCE", SourceSpan::generated());
        assert!(matches!(statement.kind, StatementKindAst::NextSentence));
        assert_eq!(statement.raw, "NEXT SENTENCE");
    }

    #[test]
    fn procedure_sentence_uses_token_slice_parser_for_multiple_imperatives() {
        let src = r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SENTCFG.
PROCEDURE DIVISION.
MAIN.
DISPLAY "A" DISPLAY "B".
STOP RUN.
"#;
        let ast = parse_program("sentcfg.cbl", src).expect("program parses");
        let statements = &ast.paragraphs[0].statements;
        assert_eq!(statements.len(), 3, "{statements:?}");
        assert_eq!(ast.paragraphs[0].sentences.len(), 2);
        assert_eq!(ast.paragraphs[0].sentences[0].statements.len(), 2);
        assert_eq!(ast.paragraphs[0].sentences[1].statements.len(), 1);
        assert!(matches!(
            statements[0].kind,
            StatementKindAst::Display(ref values) if values.as_slice() == ["\"A\""]
        ));
        assert!(matches!(
            statements[1].kind,
            StatementKindAst::Display(ref values) if values.as_slice() == ["\"B\""]
        ));
        assert!(matches!(statements[2].kind, StatementKindAst::StopRun));
        assert_eq!(statements[0].raw, "DISPLAY \"A\"");
        assert_eq!(statements[1].raw, "DISPLAY \"B\"");
    }

    #[test]
    fn imperative_parser_does_not_construct_statements_from_joined_raw_text() {
        let source = include_str!("lib.rs");
        for needle in [
            ["statements.push(parse_statement", "(&raw"].concat(),
            ["tokens[idx..next]", ".join(\" \")"].concat(),
        ] {
            assert!(
                !source.contains(&needle),
                "imperative parser raw reparse residue remains: {needle}"
            );
        }
    }

    #[test]
    fn imperative_parser_uses_explicit_statement_consumption_contract() {
        let source = include_str!("lib.rs");
        assert!(
            source.contains("fn parse_statement_at("),
            "missing explicit statement parser contract"
        );

        let start = source
            .find("fn parse_imperative_tokens(")
            .expect("imperative parser exists");
        let end = source[start..]
            .find("fn explicit_scope_statement_end(")
            .map(|offset| start + offset)
            .expect("next helper exists");
        let body = &source[start..end];
        assert!(
            body.contains("parse_statement_at("),
            "imperative parser must delegate statement ownership to parse_statement_at"
        );
        assert!(
            !body.contains("parse_statement_from_words("),
            "imperative parser must not construct statement slices directly"
        );
        assert!(
            !body.contains("find_next_statement_boundary("),
            "boundary heuristics belong inside parse_statement_at"
        );
    }

    #[test]
    fn compute_statement_ast_preserves_typed_size_error_branch() {
        let statements = parse_imperative_list(
            "COMPUTE WS-NUM = 1 / 0 ON SIZE ERROR DISPLAY \"A\" DISPLAY \"B\" END-COMPUTE",
            SourceSpan::generated(),
        );
        let StatementKindAst::Compute(compute) = &statements[0].kind else {
            panic!("expected typed COMPUTE AST");
        };
        assert_eq!(compute.target, "WS-NUM");
        assert_eq!(compute.expression, "1 / 0");
        assert_eq!(compute.on_size_error.len(), 2);
        assert!(matches!(
            compute.on_size_error[0].kind,
            StatementKindAst::Display(_)
        ));
        assert!(matches!(
            compute.on_size_error[1].kind,
            StatementKindAst::Display(_)
        ));
    }

    #[test]
    fn compute_size_error_branch_ignores_nested_compute_not_on_size_error() {
        let statement = parse_statement(
            "COMPUTE OUTER-N = A / B ON SIZE ERROR COMPUTE INNER-N = C / D ON SIZE ERROR DISPLAY \"INNER-BAD\" NOT ON SIZE ERROR DISPLAY \"INNER-OK\" END-COMPUTE NOT ON SIZE ERROR DISPLAY \"OUTER-OK\" END-COMPUTE",
            SourceSpan::generated(),
        );
        let StatementKindAst::Compute(outer) = statement.kind else {
            panic!("expected outer COMPUTE AST");
        };
        assert_eq!(outer.on_size_error.len(), 1, "{outer:?}");
        let StatementKindAst::Compute(inner) = &outer.on_size_error[0].kind else {
            panic!("expected nested COMPUTE AST");
        };
        assert_eq!(inner.on_size_error.len(), 1);
        assert_eq!(inner.not_on_size_error.len(), 1);
        assert_eq!(outer.not_on_size_error.len(), 1);
        assert!(matches!(
            outer.not_on_size_error[0].kind,
            StatementKindAst::Display(ref values) if values.as_slice() == ["\"OUTER-OK\""]
        ));
    }

    #[test]
    fn string_statement_ast_preserves_nested_if_overflow_branch() {
        let statements = parse_imperative_list(
            "STRING \"A\" DELIMITED BY SIZE INTO WS-TEXT ON OVERFLOW IF WS-FLAG = \"Y\" DISPLAY \"Y\" ELSE DISPLAY \"N\" END-IF END-STRING",
            SourceSpan::generated(),
        );
        let StatementKindAst::String(string) = &statements[0].kind else {
            panic!("expected typed STRING AST");
        };
        assert_eq!(string.on_overflow.len(), 1);
        assert!(matches!(
            string.on_overflow[0].kind,
            StatementKindAst::If { .. }
        ));
    }

    #[test]
    fn evaluate_statement_ast_preserves_typed_arm_body() {
        let statements = parse_imperative_list(
            "EVALUATE WS-FLAG WHEN \"Y\" DISPLAY \"A\" DISPLAY \"B\" WHEN OTHER DISPLAY \"N\" END-EVALUATE",
            SourceSpan::generated(),
        );
        let StatementKindAst::Evaluate(evaluate) = &statements[0].kind else {
            panic!("expected typed EVALUATE AST");
        };
        assert_eq!(evaluate.subjects, vec!["WS-FLAG"]);
        assert_eq!(evaluate.arms.len(), 2);
        assert_eq!(evaluate.arms[0].patterns, vec!["\"Y\""]);
        assert_eq!(evaluate.arms[0].statements.len(), 2);
        assert!(matches!(
            evaluate.arms[0].statements[0].kind,
            StatementKindAst::Display(_)
        ));
        assert!(matches!(
            evaluate.arms[0].statements[1].kind,
            StatementKindAst::Display(_)
        ));
    }

    #[test]
    fn search_statement_ast_preserves_typed_at_end_and_when_body() {
        let statements = parse_imperative_list(
            "SEARCH WS-ITEM VARYING WS-IDX AT END DISPLAY \"END\" WHEN WS-ITEM ( WS-IDX ) = \"B\" DISPLAY \"FOUND\" END-SEARCH",
            SourceSpan::generated(),
        );
        let StatementKindAst::Search(search) = &statements[0].kind else {
            panic!("expected typed SEARCH AST");
        };
        assert!(!search.all);
        assert_eq!(search.table, "WS_ITEM");
        assert_eq!(search.index.as_deref(), Some("WS_IDX"));
        assert_eq!(search.at_end.len(), 1);
        assert_eq!(search.whens.len(), 1);
        assert_eq!(search.whens[0].statements.len(), 1);
        assert!(matches!(
            search.whens[0].statements[0].kind,
            StatementKindAst::Display(_)
        ));
    }

    #[test]
    fn data_decl_has_typed_clause_ast() {
        let item = parse_data_decl(
            "05 WS-AMOUNT PIC S9(7)V99 COMP-3 OCCURS 2 TO 4 DEPENDING ON WS-COUNT.",
            SourceSpan::generated(),
            StorageAreaAst::WorkingStorage,
        )
        .expect("data decl");
        assert!(item
            .clause_ast
            .iter()
            .any(|clause| matches!(clause, DataClauseAst::Picture(pic) if pic == "S9(7)V99")));
        assert!(item
            .clause_ast
            .iter()
            .any(|clause| matches!(clause, DataClauseAst::Usage(usage) if usage == "COMP-3")));
        assert!(item.clause_ast.iter().any(|clause| {
            matches!(
                clause,
                DataClauseAst::Occurs {
                    min: 2,
                    max: 4,
                    depending_on: Some(name),
                    ..
                } if name == "WS_COUNT"
            )
        }));
    }

    #[test]
    fn data_occurs_clause_preserves_indexed_by_and_key_metadata() {
        let item = parse_data_decl(
            "05 WS-ITEM OCCURS 3 TIMES ASCENDING KEY IS WS-KEY INDEXED BY WS-IDX, WS-IDY PIC X.",
            SourceSpan::generated(),
            StorageAreaAst::WorkingStorage,
        )
        .expect("data decl");
        assert!(item.clause_ast.iter().any(|clause| {
            matches!(
                clause,
                DataClauseAst::Occurs {
                    min: 3,
                    max: 3,
                    depending_on: None,
                    indexed_by,
                    keys,
                } if indexed_by == &vec!["WS_IDX".to_string(), "WS_IDY".to_string()]
                    && keys == &vec![DataOccursKeyAst {
                        direction: DataOccursKeyDirectionAst::Ascending,
                        name: "WS_KEY".to_string(),
                    }]
            )
        }));
    }

    #[test]
    fn data_picture_clause_skips_optional_is_word() {
        let item = parse_data_decl(
            "05 WS-NAME PICTURE IS X(3).",
            SourceSpan::generated(),
            StorageAreaAst::WorkingStorage,
        )
        .expect("data decl");
        assert!(item
            .clause_ast
            .iter()
            .any(|clause| matches!(clause, DataClauseAst::Picture(pic) if pic == "X(3)")));
    }

    #[test]
    fn data_usage_clause_captures_direct_shorthand() {
        let item = parse_data_decl(
            "05 WS-NATIONAL NATIONAL PIC N(3).",
            SourceSpan::generated(),
            StorageAreaAst::WorkingStorage,
        )
        .expect("data decl");
        assert!(item
            .clause_ast
            .iter()
            .any(|clause| matches!(clause, DataClauseAst::Usage(usage) if usage == "NATIONAL")));

        let pointer = parse_data_decl(
            "01 WS-PTR POINTER.",
            SourceSpan::generated(),
            StorageAreaAst::WorkingStorage,
        )
        .expect("pointer data decl");
        assert!(pointer
            .clause_ast
            .iter()
            .any(|clause| matches!(clause, DataClauseAst::Usage(usage) if usage == "POINTER")));

        let procedure_pointer = parse_data_decl(
            "01 WS-PROC-PTR PROCEDURE-POINTER.",
            SourceSpan::generated(),
            StorageAreaAst::WorkingStorage,
        )
        .expect("procedure pointer data decl");
        assert!(procedure_pointer.clause_ast.iter().any(
            |clause| matches!(clause, DataClauseAst::Usage(usage) if usage == "PROCEDURE-POINTER")
        ));
    }

    #[test]
    fn data_clause_ast_covers_layout_and_value_clauses() {
        let redef = parse_data_decl(
            "05 WS-ALT REDEFINES WS-BASE PIC X(4) USAGE DISPLAY VALUE \"AB\" SYNC.",
            SourceSpan::generated(),
            StorageAreaAst::WorkingStorage,
        )
        .expect("redefines data decl");
        assert!(redef
            .clause_ast
            .iter()
            .any(|clause| matches!(clause, DataClauseAst::Redefines(name) if name == "WS_BASE")));
        assert!(redef
            .clause_ast
            .iter()
            .any(|clause| matches!(clause, DataClauseAst::Picture(pic) if pic == "X(4)")));
        assert!(redef
            .clause_ast
            .iter()
            .any(|clause| matches!(clause, DataClauseAst::Usage(usage) if usage == "DISPLAY")));
        assert!(redef
            .clause_ast
            .iter()
            .any(|clause| matches!(clause, DataClauseAst::Value(value) if value == "AB")));
        assert!(redef
            .clause_ast
            .iter()
            .any(|clause| matches!(clause, DataClauseAst::Sync)));

        let external = parse_data_decl(
            "01 WS-EXTERNAL PIC X EXTERNAL.",
            SourceSpan::generated(),
            StorageAreaAst::WorkingStorage,
        )
        .expect("external data decl");
        assert!(external
            .clause_ast
            .iter()
            .any(|clause| matches!(clause, DataClauseAst::External)));

        let global = parse_data_decl(
            "01 WS-GLOBAL PIC X GLOBAL.",
            SourceSpan::generated(),
            StorageAreaAst::WorkingStorage,
        )
        .expect("global data decl");
        assert!(global
            .clause_ast
            .iter()
            .any(|clause| matches!(clause, DataClauseAst::Global)));

        let signed = parse_data_decl(
            "05 WS-SIGNED PIC S9(4) SIGN IS LEADING SEPARATE.",
            SourceSpan::generated(),
            StorageAreaAst::WorkingStorage,
        )
        .expect("sign data decl");
        assert!(signed.clause_ast.iter().any(|clause| {
            matches!(
                clause,
                DataClauseAst::Sign {
                    position: Some(DataSignPositionAst::Leading),
                    separate: true
                }
            )
        }));

        let justified = parse_data_decl(
            "05 WS-JUST PIC X(4) JUSTIFIED RIGHT.",
            SourceSpan::generated(),
            StorageAreaAst::WorkingStorage,
        )
        .expect("justified data decl");
        assert!(justified
            .clause_ast
            .iter()
            .any(|clause| { matches!(clause, DataClauseAst::Justified { right: true }) }));

        let blank = parse_data_decl(
            "05 WS-BLANK PIC 9(4) BLANK WHEN ZERO.",
            SourceSpan::generated(),
            StorageAreaAst::WorkingStorage,
        )
        .expect("blank when zero data decl");
        assert!(blank
            .clause_ast
            .iter()
            .any(|clause| { matches!(clause, DataClauseAst::BlankWhenZero) }));

        let based = parse_data_decl(
            "01 WS-BASED PIC X(4) BASED ON WS-PTR.",
            SourceSpan::generated(),
            StorageAreaAst::WorkingStorage,
        )
        .expect("based data decl");
        assert!(based.clause_ast.iter().any(|clause| {
            matches!(
                clause,
                DataClauseAst::Based {
                    pointer: Some(pointer)
                } if pointer == "WS_PTR"
            )
        }));

        let bare_based = parse_data_decl(
            "01 WS-BARE-BASED PIC X BASED.",
            SourceSpan::generated(),
            StorageAreaAst::WorkingStorage,
        )
        .expect("bare based data decl");
        assert!(bare_based
            .clause_ast
            .iter()
            .any(|clause| { matches!(clause, DataClauseAst::Based { pointer: None }) }));

        let any_length = parse_data_decl(
            "01 LK-TEXT PIC X ANY LENGTH.",
            SourceSpan::generated(),
            StorageAreaAst::Linkage,
        )
        .expect("any length data decl");
        assert!(any_length
            .clause_ast
            .iter()
            .any(|clause| { matches!(clause, DataClauseAst::AnyLength) }));

        let hyphen_any_length = parse_data_decl(
            "01 LK-BUF PIC X ANY-LENGTH.",
            SourceSpan::generated(),
            StorageAreaAst::Linkage,
        )
        .expect("hyphenated any length data decl");
        assert!(hyphen_any_length
            .clause_ast
            .iter()
            .any(|clause| { matches!(clause, DataClauseAst::AnyLength) }));

        let renames = parse_data_decl(
            "66 WS-RANGE RENAMES WS-FIRST THRU WS-LAST.",
            SourceSpan::generated(),
            StorageAreaAst::WorkingStorage,
        )
        .expect("renames data decl");
        assert!(renames.clause_ast.iter().any(|clause| {
            matches!(
                clause,
                DataClauseAst::Renames {
                    first,
                    last: Some(last),
                } if first == "WS_FIRST" && last == "WS_LAST"
            )
        }));
    }

    #[test]
    fn data_value_clause_preserves_quoted_literal_with_spaces() {
        let item = parse_data_decl(
            "05 WS-GREETING PIC X(11) VALUE \"HELLO WORLD\".",
            SourceSpan::generated(),
            StorageAreaAst::WorkingStorage,
        )
        .expect("data decl");
        assert!(item
            .clause_ast
            .iter()
            .any(|clause| matches!(clause, DataClauseAst::Value(value) if value == "HELLO WORLD")));
    }

    #[test]
    fn data_value_clause_skips_optional_is_word() {
        let item = parse_data_decl(
            "05 WS-FLAG PIC X VALUE IS \"Y\".",
            SourceSpan::generated(),
            StorageAreaAst::WorkingStorage,
        )
        .expect("data decl");
        assert!(item
            .clause_ast
            .iter()
            .any(|clause| matches!(clause, DataClauseAst::Value(value) if value == "Y")));
    }

    #[test]
    fn data_values_clause_preserves_condition_name_ranges() {
        let item = parse_data_decl(
            "88 WS-ERROR VALUES ARE 10 THRU 20, 25.",
            SourceSpan::generated(),
            StorageAreaAst::WorkingStorage,
        )
        .expect("condition-name data decl");
        assert!(item.clause_ast.iter().any(|clause| {
            matches!(
                clause,
                DataClauseAst::Values(values)
                    if values == &vec![
                        DataValueAst::Range {
                            start: "10".to_string(),
                            end: "20".to_string(),
                        },
                        DataValueAst::Single("25".to_string()),
                    ]
            )
        }));
    }

    #[test]
    fn parse_programs_reads_multiple_program_id_units() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. FIRST.\nPROCEDURE DIVISION.\nMAIN.\nSTOP RUN.\nIDENTIFICATION DIVISION.\nPROGRAM-ID. SECOND.\nPROCEDURE DIVISION.\nMAIN.\nSTOP RUN.\n";
        let programs = parse_programs("multi.cbl", src).expect("programs parse");
        assert_eq!(programs.len(), 2);
        assert_eq!(programs[0].name, "FIRST");
        assert_eq!(programs[1].name, "SECOND");
        assert!(programs
            .iter()
            .all(|program| program.diagnostics.is_empty()));
    }
}
