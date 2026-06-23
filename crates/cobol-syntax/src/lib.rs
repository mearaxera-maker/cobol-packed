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
    TypeDef {
        strong: bool,
    },
    TypeOf {
        name: String,
    },
    SameAs {
        name: String,
    },
    GroupUsage(String),
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
    BlockedNextSentence,
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
    Accept {
        target: String,
        source: Option<String>,
        options: Vec<String>,
    },
    Initialize {
        targets: Vec<String>,
        options: Vec<String>,
    },
    Cancel {
        targets: Vec<String>,
    },
    Entry {
        name: String,
        using: Vec<String>,
    },
    Chain {
        target: String,
        using: Vec<String>,
    },
    Unlock {
        file: String,
        options: Vec<String>,
    },
    Generate {
        target: String,
        options: Vec<String>,
    },
    Initiate {
        targets: Vec<String>,
    },
    Terminate {
        targets: Vec<String>,
    },
    Purge {
        target: String,
        options: Vec<String>,
    },
    Suppress {
        target: Option<String>,
        options: Vec<String>,
    },
    Enable {
        target: String,
        options: Vec<String>,
    },
    Disable {
        target: String,
        options: Vec<String>,
    },
    Send {
        target: String,
        options: Vec<String>,
    },
    Receive {
        target: String,
        options: Vec<String>,
    },
    Enter {
        language: String,
        options: Vec<String>,
    },
    Merge {
        file: String,
        options: Vec<String>,
    },
    Start {
        file: String,
        options: Vec<String>,
        invalid_key: ImperativeListAst,
        not_invalid_key: ImperativeListAst,
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
    ExitProgram,
    Goback,
    Stop(String),
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
    pub rounded: bool,
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
    pub invalid_key: ImperativeListAst,
    pub not_invalid_key: ImperativeListAst,
    pub on_exception: ImperativeListAst,
    pub not_on_exception: ImperativeListAst,
    pub branch_phrases: Vec<String>,
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

impl SyntaxError {
    pub fn code(&self) -> &'static str {
        match self {
            SyntaxError::EmptyProgram => "E_SYNTAX_EMPTY_PROGRAM",
        }
    }

    pub fn suggested_action(&self) -> &'static str {
        match self {
            SyntaxError::EmptyProgram => {
                "Workaround: ensure the source contains IDENTIFICATION DIVISION, PROGRAM-ID, and a complete COBOL program unit after preprocessing."
            }
        }
    }
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
                if parts
                    .get(idx + 2)
                    .filter(|_| {
                        parts
                            .get(idx + 1)
                            .map(|word| word.eq_ignore_ascii_case("IS"))
                            .unwrap_or(false)
                    })
                    .is_some()
                {
                    let (usage, next) = parse_usage_value(&parts, idx + 2);
                    out.push(DataClauseAst::Usage(usage));
                    idx = next;
                } else if parts.get(idx + 1).is_some() {
                    let (usage, next) = parse_usage_value(&parts, idx + 1);
                    out.push(DataClauseAst::Usage(usage));
                    idx = next;
                } else {
                    out.push(DataClauseAst::Other(parts[idx].to_string()));
                    idx += 1;
                }
            }
            "COMP" | "COMP-1" | "COMP-2" | "COMP-3" | "COMP-4" | "COMP-5" | "COMPUTATIONAL"
            | "COMPUTATIONAL-1" | "COMPUTATIONAL-2" | "COMPUTATIONAL-3" | "COMPUTATIONAL-4"
            | "COMPUTATIONAL-5" | "COMPUTATIONAL-X" | "COMPUTATIONAL-N" | "COMPUTATIONAL-6"
            | "COMP-X" | "COMP-N" | "COMP-6" | "OBJECT" | "BINARY" | "BINARY-CHAR"
            | "BINARY-SHORT" | "BINARY-LONG" | "BINARY-DOUBLE" | "PACKED-DECIMAL" | "NATIONAL"
            | "DISPLAY-1" | "DBCS" | "KANJI" | "POINTER" | "PROCEDURE-POINTER" => {
                let (usage, next) = parse_usage_value(&parts, idx);
                out.push(DataClauseAst::Usage(usage));
                idx = next;
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
                idx += if parts
                    .get(idx + 1)
                    .map(|word| {
                        data_clause_word_eq(word, "LEFT") || data_clause_word_eq(word, "RIGHT")
                    })
                    .unwrap_or(false)
                {
                    2
                } else {
                    1
                };
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
                    .map(|word| data_clause_word_is_zero_alias(word))
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
            "ANY"
                if parts
                    .get(idx + 1)
                    .map(|word| data_clause_word_eq(word, "LENGTH"))
                    .unwrap_or(false) =>
            {
                out.push(DataClauseAst::AnyLength);
                idx += 2;
            }
            "ANY" => {
                out.push(DataClauseAst::Other(parts[idx].to_string()));
                idx += 1;
            }
            "GROUP-USAGE" => {
                let value_idx = if parts
                    .get(idx + 1)
                    .map(|word| data_clause_word_eq(word, "IS"))
                    .unwrap_or(false)
                {
                    idx + 2
                } else {
                    idx + 1
                };
                if let Some(value) = parts.get(value_idx) {
                    out.push(DataClauseAst::GroupUsage(
                        value.trim_end_matches('.').to_ascii_uppercase(),
                    ));
                    idx = value_idx + 1;
                } else {
                    out.push(DataClauseAst::Other(parts[idx].to_string()));
                    idx += 1;
                }
            }
            "TYPEDEF" => {
                let strong = parts
                    .get(idx + 1)
                    .map(|word| data_clause_word_eq(word, "STRONG"))
                    .unwrap_or(false);
                out.push(DataClauseAst::TypeDef { strong });
                idx += if strong { 2 } else { 1 };
            }
            "TYPE" => {
                let value_idx = if parts
                    .get(idx + 1)
                    .map(|word| data_clause_word_eq(word, "IS"))
                    .unwrap_or(false)
                {
                    idx + 2
                } else {
                    idx + 1
                };
                if let Some(value) = parts.get(value_idx) {
                    out.push(DataClauseAst::TypeOf {
                        name: sanitize_cobol_name(value),
                    });
                    idx = value_idx + 1;
                } else {
                    out.push(DataClauseAst::Other(parts[idx].to_string()));
                    idx += 1;
                }
            }
            "SAME"
                if parts
                    .get(idx + 1)
                    .map(|word| data_clause_word_eq(word, "AS"))
                    .unwrap_or(false) =>
            {
                if let Some(value) = parts.get(idx + 2) {
                    out.push(DataClauseAst::SameAs {
                        name: sanitize_cobol_name(value),
                    });
                    idx += 3;
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

fn parse_usage_value(parts: &[String], idx: usize) -> (String, usize) {
    let usage = parts[idx].trim_end_matches('.').to_ascii_uppercase();
    if usage == "OBJECT"
        && parts
            .get(idx + 1)
            .map(|word| data_clause_word_eq(word, "REFERENCE"))
            .unwrap_or(false)
    {
        ("OBJECT REFERENCE".to_string(), idx + 2)
    } else {
        (usage, idx + 1)
    }
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

        let (start, next_cursor) = parse_data_value_atom(parts, cursor);
        if start.is_empty() {
            cursor = next_cursor;
            continue;
        }

        if parts
            .get(next_cursor)
            .map(|word| word.eq_ignore_ascii_case("THRU") || word.eq_ignore_ascii_case("THROUGH"))
            .unwrap_or(false)
        {
            if let Some(end) = parts.get(next_cursor + 1) {
                values.push(DataValueAst::Range {
                    start,
                    end: clean_data_value_literal(end),
                });
                cursor = next_cursor + 2;
                continue;
            }
        }

        values.push(DataValueAst::Single(start));
        cursor = next_cursor;
    }

    match values.as_slice() {
        [DataValueAst::Single(value)] => (DataClauseAst::Value(value.clone()), cursor),
        [] => (DataClauseAst::Other(parts[idx].to_string()), idx + 1),
        _ => (DataClauseAst::Values(values), cursor),
    }
}

fn parse_data_value_atom(parts: &[String], cursor: usize) -> (String, usize) {
    let start = clean_data_value_literal(&parts[cursor]);
    if start.eq_ignore_ascii_case("ALL") {
        if let Some(value) = parts.get(cursor + 1) {
            let value = clean_data_value_literal(value);
            if !value.is_empty() && !is_data_clause_starter(&value) {
                return (format!("ALL {value}"), cursor + 2);
            }
        }
    }
    (start, cursor + 1)
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

fn data_clause_word_is_zero_alias(word: &str) -> bool {
    matches!(
        word.trim_end_matches('.')
            .trim_end_matches(',')
            .to_ascii_uppercase()
            .as_str(),
        "ZERO" | "ZEROS" | "ZEROES"
    )
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
            | "COMPUTATIONAL"
            | "COMPUTATIONAL-1"
            | "COMPUTATIONAL-2"
            | "COMPUTATIONAL-3"
            | "COMPUTATIONAL-4"
            | "COMPUTATIONAL-5"
            | "COMPUTATIONAL-X"
            | "COMPUTATIONAL-N"
            | "COMPUTATIONAL-6"
            | "COMP-X"
            | "COMP-N"
            | "COMP-6"
            | "OBJECT"
            | "BINARY"
            | "BINARY-CHAR"
            | "BINARY-SHORT"
            | "BINARY-LONG"
            | "BINARY-DOUBLE"
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
            | "GROUP-USAGE"
            | "TYPEDEF"
            | "TYPE"
            | "SAME"
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
            | "COMPUTATIONAL"
            | "COMPUTATIONAL-1"
            | "COMPUTATIONAL-2"
            | "COMPUTATIONAL-3"
            | "COMPUTATIONAL-4"
            | "COMPUTATIONAL-5"
            | "COMPUTATIONAL-X"
            | "COMPUTATIONAL-N"
            | "COMPUTATIONAL-6"
            | "COMP-X"
            | "COMP-N"
            | "COMP-6"
            | "OBJECT"
            | "BINARY"
            | "BINARY-CHAR"
            | "BINARY-SHORT"
            | "BINARY-LONG"
            | "BINARY-DOUBLE"
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
            | "GROUP-USAGE"
            | "TYPEDEF"
            | "TYPE"
            | "SAME"
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
        "ADD" => parse_add_statement(spanned_words, sentence_raw, &words, span.clone()),
        "SUBTRACT" => parse_binary_arithmetic_statement(
            spanned_words,
            sentence_raw,
            &words,
            span.clone(),
            "FROM",
            ArithmeticStatementAst::Subtract,
        )
        .or_else(|| {
            parse_arithmetic_giving_statement(
                spanned_words,
                sentence_raw,
                &words,
                span.clone(),
                "FROM",
                ArithmeticStatementAst::Subtract,
            )
        })
        .unwrap_or_else(|| StatementKindAst::Unsupported("SUBTRACT".to_string())),
        "MULTIPLY" => parse_binary_arithmetic_statement(
            spanned_words,
            sentence_raw,
            &words,
            span.clone(),
            "BY",
            ArithmeticStatementAst::Multiply,
        )
        .or_else(|| {
            parse_arithmetic_giving_statement(
                spanned_words,
                sentence_raw,
                &words,
                span.clone(),
                "BY",
                ArithmeticStatementAst::Multiply,
            )
        })
        .unwrap_or_else(|| StatementKindAst::Unsupported("MULTIPLY".to_string())),
        "DIVIDE" => parse_binary_arithmetic_statement(
            spanned_words,
            sentence_raw,
            &words,
            span.clone(),
            "INTO",
            ArithmeticStatementAst::Divide,
        )
        .or_else(|| {
            parse_arithmetic_giving_statement(
                spanned_words,
                sentence_raw,
                &words,
                span.clone(),
                "INTO",
                ArithmeticStatementAst::Divide,
            )
        })
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
            StatementKindAst::BlockedNextSentence
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
        "GOBACK" if words.len() == 1 => StatementKindAst::Goback,
        "GOBACK" => StatementKindAst::Unsupported("GOBACK".to_string()),
        "ACCEPT" => parse_accept(&words)
            .unwrap_or_else(|| StatementKindAst::Unsupported("ACCEPT".to_string())),
        "INITIALIZE" => parse_initialize(&words)
            .unwrap_or_else(|| StatementKindAst::Unsupported("INITIALIZE".to_string())),
        "CANCEL" => parse_cancel(&words)
            .unwrap_or_else(|| StatementKindAst::Unsupported("CANCEL".to_string())),
        "ENTRY" => parse_entry(&words)
            .unwrap_or_else(|| StatementKindAst::Unsupported("ENTRY".to_string())),
        "CHAIN" => parse_chain(&words)
            .unwrap_or_else(|| StatementKindAst::Unsupported("CHAIN".to_string())),
        "UNLOCK" => parse_unlock(&words)
            .unwrap_or_else(|| StatementKindAst::Unsupported("UNLOCK".to_string())),
        "GENERATE" => parse_generate(&words)
            .unwrap_or_else(|| StatementKindAst::Unsupported("GENERATE".to_string())),
        "INITIATE" => parse_initiate(&words)
            .unwrap_or_else(|| StatementKindAst::Unsupported("INITIATE".to_string())),
        "TERMINATE" => parse_terminate(&words)
            .unwrap_or_else(|| StatementKindAst::Unsupported("TERMINATE".to_string())),
        "PURGE" => parse_purge(&words)
            .unwrap_or_else(|| StatementKindAst::Unsupported("PURGE".to_string())),
        "SUPPRESS" => parse_suppress(&words)
            .unwrap_or_else(|| StatementKindAst::Unsupported("SUPPRESS".to_string())),
        "ENABLE" => parse_enable_disable(&words, "ENABLE")
            .unwrap_or_else(|| StatementKindAst::Unsupported("ENABLE".to_string())),
        "DISABLE" => parse_enable_disable(&words, "DISABLE")
            .unwrap_or_else(|| StatementKindAst::Unsupported("DISABLE".to_string())),
        "SEND" => parse_target_options_statement(&words, "SEND")
            .map(|(target, options)| StatementKindAst::Send { target, options })
            .unwrap_or_else(|| StatementKindAst::Unsupported("SEND".to_string())),
        "RECEIVE" => parse_target_options_statement(&words, "RECEIVE")
            .map(|(target, options)| StatementKindAst::Receive { target, options })
            .unwrap_or_else(|| StatementKindAst::Unsupported("RECEIVE".to_string())),
        "ENTER" => parse_target_options_statement(&words, "ENTER")
            .map(|(language, options)| StatementKindAst::Enter { language, options })
            .unwrap_or_else(|| StatementKindAst::Unsupported("ENTER".to_string())),
        "MERGE" => parse_target_options_statement(&words, "MERGE")
            .map(|(file, options)| StatementKindAst::Merge { file, options })
            .unwrap_or_else(|| StatementKindAst::Unsupported("MERGE".to_string())),
        "START" => parse_start(spanned_words, &words, sentence_raw, span.clone())
            .unwrap_or_else(|| StatementKindAst::Unsupported("START".to_string())),
        "STOP" => {
            if words.len() == 2
                && words.get(1).map(|word| word.eq_ignore_ascii_case("RUN")) == Some(true)
            {
                StatementKindAst::StopRun
            } else if words.len() == 2 {
                StatementKindAst::Stop(words[1].clone())
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
        "WRITE" => parse_write_file_ast(spanned_words, &words, sentence_raw, span.clone())
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
        "EXIT" if words.len() == 1 => StatementKindAst::Continue,
        "EXIT"
            if words.len() == 2
                && words
                    .get(1)
                    .map(|word| word.eq_ignore_ascii_case("PROGRAM"))
                    == Some(true) =>
        {
            StatementKindAst::ExitProgram
        }
        "EXIT" => StatementKindAst::Unsupported("EXIT".to_string()),
        "ALTER" => parse_alter(&words),
        "EXEC" => StatementKindAst::Unsupported(first),
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
    if first_branch <= 1 {
        return None;
    }
    let prefix = words.get(2..first_branch).unwrap_or_default();
    let into = match prefix {
        [] => None,
        [keyword, ..] if keyword.eq_ignore_ascii_case("INTO") && first_branch > 3 => {
            let (target, consumed) = qualified_operand_at(words, 3);
            if 3 + consumed != first_branch {
                return None;
            }
            Some(target)
        }
        _ => return None,
    };
    if !read_branches_have_bodies_ast(words) {
        return None;
    }
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

fn read_branch_bounds_ast(words: &[String], branch: ReadBranchAst) -> Option<(usize, usize)> {
    let start = find_read_branch_idx_ast(words, branch)?;
    let marker_len = match branch {
        ReadBranchAst::AtEnd | ReadBranchAst::OnException => 2,
        ReadBranchAst::NotAtEnd => 3,
    };
    let body_start = start + marker_len;
    let terminator = matching_explicit_scope_end_idx(words, 0)
        .filter(|idx| words[*idx].eq_ignore_ascii_case("END-READ"))
        .unwrap_or(words.len());
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
    Some((body_start, end))
}

fn read_branches_have_bodies_ast(words: &[String]) -> bool {
    [
        ReadBranchAst::AtEnd,
        ReadBranchAst::NotAtEnd,
        ReadBranchAst::OnException,
    ]
    .into_iter()
    .all(|branch| match read_branch_bounds_ast(words, branch) {
        Some((body_start, end)) => body_start < end,
        None => true,
    })
}

fn read_branch_ast(
    tokens: &[SpannedWord],
    words: &[String],
    raw_source: &str,
    branch: ReadBranchAst,
    span: SourceSpan,
) -> ImperativeListAst {
    let Some((body_start, end)) = read_branch_bounds_ast(words, branch) else {
        return Vec::new();
    };
    branch_statements_ast(
        tokens.get(body_start..end).unwrap_or_default(),
        raw_source,
        span,
    )
}

#[derive(Debug, Clone, Copy)]
enum WriteBranchAst {
    InvalidKey,
    NotInvalidKey,
    OnException,
    NotOnException,
}

fn parse_write_file_ast(
    tokens: &[SpannedWord],
    words: &[String],
    raw_source: &str,
    span: SourceSpan,
) -> Option<WriteFileAst> {
    if words.len() < 2 || !words[0].eq_ignore_ascii_case("WRITE") {
        return None;
    }
    let first_branch = first_write_branch_idx_ast(words).unwrap_or(words.len());
    if !write_prefix_is_consumed_ast(words, first_branch) {
        return None;
    }
    if !write_branches_have_bodies_ast(words) {
        return None;
    }
    Some(WriteFileAst {
        record: words.get(1)?.clone(),
        advancing: parse_write_advancing_ast(words.get(..first_branch).unwrap_or(words)),
        invalid_key: write_branch_ast(
            tokens,
            words,
            raw_source,
            WriteBranchAst::InvalidKey,
            span.clone(),
        ),
        not_invalid_key: write_branch_ast(
            tokens,
            words,
            raw_source,
            WriteBranchAst::NotInvalidKey,
            span.clone(),
        ),
        on_exception: write_branch_ast(
            tokens,
            words,
            raw_source,
            WriteBranchAst::OnException,
            span.clone(),
        ),
        not_on_exception: write_branch_ast(
            tokens,
            words,
            raw_source,
            WriteBranchAst::NotOnException,
            span,
        ),
        branch_phrases: write_branch_phrases_ast(words),
    })
}

fn write_prefix_is_consumed_ast(tokens: &[String], end: usize) -> bool {
    if end == 2 {
        return true;
    }
    if end < 5 {
        return false;
    }
    let Some(direction) = tokens.get(2) else {
        return false;
    };
    let Some(advancing) = tokens.get(3) else {
        return false;
    };
    if !(direction.eq_ignore_ascii_case("AFTER") || direction.eq_ignore_ascii_case("BEFORE"))
        || !advancing.eq_ignore_ascii_case("ADVANCING")
    {
        return false;
    }
    let operand = tokens
        .get(4)
        .map(|token| token.trim_end_matches('.').to_ascii_uppercase())
        .unwrap_or_default();
    let is_page = matches!(operand.as_str(), "PAGE" | "TOP-OF-PAGE" | "TOP_OF_PAGE");
    if is_page {
        return end == 5;
    }
    if operand.parse::<usize>().is_err() {
        return false;
    }
    end == 5
        || (end == 6
            && tokens.get(5).is_some_and(|token| {
                token.eq_ignore_ascii_case("LINE") || token.eq_ignore_ascii_case("LINES")
            }))
}

fn first_write_branch_idx_ast(tokens: &[String]) -> Option<usize> {
    [
        find_write_branch_idx_ast(tokens, WriteBranchAst::InvalidKey),
        find_write_branch_idx_ast(tokens, WriteBranchAst::NotInvalidKey),
        find_write_branch_idx_ast(tokens, WriteBranchAst::OnException),
        find_write_branch_idx_ast(tokens, WriteBranchAst::NotOnException),
    ]
    .into_iter()
    .flatten()
    .min()
}

fn find_write_branch_idx_ast(tokens: &[String], branch: WriteBranchAst) -> Option<usize> {
    match branch {
        WriteBranchAst::InvalidKey => find_invalid_key_branch_idx_ast(tokens, true),
        WriteBranchAst::NotInvalidKey => find_invalid_key_branch_idx_ast(tokens, false),
        WriteBranchAst::OnException => find_top_level_idx(tokens, 1, |idx, token| {
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
        WriteBranchAst::NotOnException => find_top_level_idx(tokens, 1, |idx, token| {
            token.eq_ignore_ascii_case("NOT")
                && tokens
                    .get(idx + 1)
                    .is_some_and(|token| token.eq_ignore_ascii_case("ON"))
                && tokens
                    .get(idx + 2)
                    .is_some_and(|token| token.eq_ignore_ascii_case("EXCEPTION"))
        }),
    }
}

fn write_branch_ast(
    tokens: &[SpannedWord],
    words: &[String],
    raw_source: &str,
    branch: WriteBranchAst,
    span: SourceSpan,
) -> ImperativeListAst {
    let Some((body_start, end)) = write_branch_bounds_ast(words, branch) else {
        return Vec::new();
    };
    branch_statements_ast(
        tokens.get(body_start..end).unwrap_or_default(),
        raw_source,
        span,
    )
}

fn write_branch_bounds_ast(words: &[String], branch: WriteBranchAst) -> Option<(usize, usize)> {
    let start = find_write_branch_idx_ast(words, branch)?;
    let marker_len = match branch {
        WriteBranchAst::InvalidKey | WriteBranchAst::OnException => 2,
        WriteBranchAst::NotInvalidKey | WriteBranchAst::NotOnException => 3,
    };
    let body_start = start + marker_len;
    let terminator = matching_explicit_scope_end_idx(words, 0)
        .filter(|idx| words[*idx].eq_ignore_ascii_case("END-WRITE"))
        .unwrap_or(words.len());
    let mut end = terminator;
    for candidate in [
        find_write_branch_idx_ast(words, WriteBranchAst::InvalidKey),
        find_write_branch_idx_ast(words, WriteBranchAst::NotInvalidKey),
        find_write_branch_idx_ast(words, WriteBranchAst::OnException),
        find_write_branch_idx_ast(words, WriteBranchAst::NotOnException),
    ]
    .into_iter()
    .flatten()
    {
        if candidate > start {
            end = end.min(candidate);
        }
    }
    Some((body_start, end))
}

fn write_branches_have_bodies_ast(words: &[String]) -> bool {
    [
        WriteBranchAst::InvalidKey,
        WriteBranchAst::NotInvalidKey,
        WriteBranchAst::OnException,
        WriteBranchAst::NotOnException,
    ]
    .into_iter()
    .all(|branch| match write_branch_bounds_ast(words, branch) {
        Some((body_start, end)) => body_start < end,
        None => true,
    })
}

fn write_branch_phrases_ast(tokens: &[String]) -> Vec<String> {
    [
        (WriteBranchAst::InvalidKey, "INVALID KEY"),
        (WriteBranchAst::NotInvalidKey, "NOT INVALID KEY"),
        (WriteBranchAst::OnException, "ON EXCEPTION"),
        (WriteBranchAst::NotOnException, "NOT ON EXCEPTION"),
    ]
    .into_iter()
    .filter_map(|(branch, phrase)| {
        find_write_branch_idx_ast(tokens, branch).map(|_| phrase.to_string())
    })
    .collect()
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
    if first_invalid_key_or_scope_idx_ast(words, "END-REWRITE") != 2 {
        return None;
    }
    if !invalid_key_branches_have_bodies_ast(words, "END-REWRITE") {
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
    if first_invalid_key_or_scope_idx_ast(words, "END-DELETE") != 2 {
        return None;
    }
    if !invalid_key_branches_have_bodies_ast(words, "END-DELETE") {
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
        find_top_level_idx(tokens, 1, |idx, token| {
            token.eq_ignore_ascii_case("INVALID")
                && tokens
                    .get(idx + 1)
                    .is_some_and(|token| token.eq_ignore_ascii_case("KEY"))
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
                    .is_some_and(|token| token.eq_ignore_ascii_case("INVALID"))
                && tokens
                    .get(idx + 2)
                    .is_some_and(|token| token.eq_ignore_ascii_case("KEY"))
        })
    }
}

fn first_invalid_key_or_scope_idx_ast(tokens: &[String], terminator: &str) -> usize {
    [
        find_invalid_key_branch_idx_ast(tokens, true),
        find_invalid_key_branch_idx_ast(tokens, false),
        matching_explicit_scope_end_idx(tokens, 0)
            .filter(|idx| tokens[*idx].eq_ignore_ascii_case(terminator)),
    ]
    .into_iter()
    .flatten()
    .min()
    .unwrap_or(tokens.len())
}

fn invalid_key_branch_ast(
    tokens: &[SpannedWord],
    words: &[String],
    raw_source: &str,
    invalid_key: bool,
    terminator: &str,
    span: SourceSpan,
) -> ImperativeListAst {
    let Some((body_start, end)) = invalid_key_branch_bounds_ast(words, invalid_key, terminator)
    else {
        return Vec::new();
    };
    branch_statements_ast(
        tokens.get(body_start..end).unwrap_or_default(),
        raw_source,
        span,
    )
}

fn invalid_key_branch_bounds_ast(
    words: &[String],
    invalid_key: bool,
    terminator: &str,
) -> Option<(usize, usize)> {
    let start = find_invalid_key_branch_idx_ast(words, invalid_key)?;
    let marker_len = if invalid_key { 2 } else { 3 };
    let body_start = start + marker_len;
    let other = find_invalid_key_branch_idx_ast(words, !invalid_key).unwrap_or(words.len());
    let terminator = matching_explicit_scope_end_idx(words, 0)
        .filter(|idx| words[*idx].eq_ignore_ascii_case(terminator))
        .unwrap_or(words.len());
    let mut end = words.len();
    for candidate in [other, terminator] {
        if candidate > start {
            end = end.min(candidate);
        }
    }
    Some((body_start, end))
}

fn invalid_key_branches_have_bodies_ast(words: &[String], terminator: &str) -> bool {
    [true, false].into_iter().all(|invalid_key| {
        match invalid_key_branch_bounds_ast(words, invalid_key, terminator) {
            Some((body_start, end)) => body_start < end,
            None => true,
        }
    })
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
    let file = sanitize_cobol_name(tokens.get(1)?);
    let mut cursor = 2;
    let key = if tokens
        .get(cursor)
        .is_some_and(|token| token.eq_ignore_ascii_case("ON"))
    {
        cursor += 1;
        let (key, next) = parse_sort_key_at_ast(tokens, cursor)?;
        cursor = next;
        Some(key)
    } else if sort_direction_ast(tokens.get(cursor)).is_some() {
        let (key, next) = parse_sort_key_at_ast(tokens, cursor)?;
        cursor = next;
        Some(key)
    } else {
        None
    };
    let input_range = if tokens
        .get(cursor)
        .is_some_and(|token| token.eq_ignore_ascii_case("INPUT"))
    {
        let (range, next) = parse_sort_procedure_range_at_ast(tokens, cursor, "INPUT")?;
        cursor = next;
        Some(range)
    } else {
        None
    };
    let (output_range, next) = parse_sort_procedure_range_at_ast(tokens, cursor, "OUTPUT")?;
    if next != tokens.len() {
        return None;
    }
    Some(SortProcedureAst {
        file,
        key,
        input_range,
        output_range,
    })
}

fn parse_sort_key_at_ast(tokens: &[String], idx: usize) -> Option<(SortKeyAst, usize)> {
    let direction = sort_direction_ast(tokens.get(idx))?;
    if tokens
        .get(idx + 1)
        .map(|token| token.eq_ignore_ascii_case("KEY"))
        != Some(true)
    {
        return None;
    }
    let name = sanitize_cobol_name(tokens.get(idx + 2)?);
    Some((SortKeyAst { direction, name }, idx + 3))
}

fn sort_direction_ast(token: Option<&String>) -> Option<SortDirectionAst> {
    let token = token?;
    if token.eq_ignore_ascii_case("ASCENDING") {
        Some(SortDirectionAst::Ascending)
    } else if token.eq_ignore_ascii_case("DESCENDING") {
        Some(SortDirectionAst::Descending)
    } else {
        None
    }
}

fn parse_sort_procedure_range_at_ast(
    tokens: &[String],
    idx: usize,
    phrase: &str,
) -> Option<(ProcedureRangeAst, usize)> {
    if tokens
        .get(idx)
        .map(|token| token.eq_ignore_ascii_case(phrase))
        != Some(true)
    {
        return None;
    }
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
    let next = target_idx + if through.is_some() { 3 } else { 1 };
    Some((ProcedureRangeAst { target, through }, next))
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
    let from = if let Some(idx) = from {
        let (source, consumed) = qualified_data_reference_at(tokens, idx + 1)?;
        if idx + 1 + consumed != tokens.len() {
            return None;
        }
        Some(source)
    } else {
        None
    };
    Some(ReleaseSortRecordAst {
        record: tokens.get(1)?.clone(),
        from,
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
    let into = return_prefix_into_ast(words, branch_start)?;
    if !return_branches_have_bodies_ast(words) {
        return None;
    }
    Some(ReturnSortRecordAst {
        file: sanitize_cobol_name(words.get(1)?),
        into,
        at_end: return_branch_ast(tokens, words, raw_source, true, span.clone()),
        not_at_end: return_branch_ast(tokens, words, raw_source, false, span),
    })
}

fn return_prefix_into_ast(tokens: &[String], end: usize) -> Option<Option<String>> {
    match tokens.get(2..end).unwrap_or_default() {
        [] => Some(None),
        [keyword, target] if keyword.eq_ignore_ascii_case("INTO") => Some(Some(target.clone())),
        _ => None,
    }
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

fn return_branch_bounds_ast(words: &[String], at_end: bool) -> Option<(usize, usize)> {
    let start = find_return_branch_idx_ast(words, at_end)?;
    let marker_len = if at_end { 2 } else { 3 };
    let body_start = start + marker_len;
    let other = find_return_branch_idx_ast(words, !at_end).unwrap_or(words.len());
    let terminator = matching_explicit_scope_end_idx(words, 0)
        .filter(|idx| words[*idx].eq_ignore_ascii_case("END-RETURN"))
        .unwrap_or(words.len());
    let mut end = terminator;
    for candidate in [other, terminator] {
        if candidate > start {
            end = end.min(candidate);
        }
    }
    Some((body_start, end))
}

fn return_branches_have_bodies_ast(words: &[String]) -> bool {
    [true, false]
        .into_iter()
        .all(|at_end| match return_branch_bounds_ast(words, at_end) {
            Some((body_start, end)) => body_start < end,
            None => true,
        })
}

fn return_branch_ast(
    tokens: &[SpannedWord],
    words: &[String],
    raw_source: &str,
    at_end: bool,
    span: SourceSpan,
) -> ImperativeListAst {
    let Some((body_start, end)) = return_branch_bounds_ast(words, at_end) else {
        return Vec::new();
    };
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
    let phrase_idx = tallying.or(replacing).or(converting)?;
    if phrase_idx != 2 {
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
    let option_end = first_overflow_or_scope_idx_ast(words, "END-STRING");
    let pointer =
        string_pointer_option_ast(words.get(into_idx + 2..option_end).unwrap_or_default())?;
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
    if !overflow_branches_have_bodies_ast(words, "END-STRING") {
        return None;
    }
    Some(StringOpAst {
        pieces,
        target: words.get(into_idx + 1)?.clone(),
        pointer,
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
    let option_end = first_overflow_or_scope_idx_ast(words, "END-UNSTRING");
    let (pointer, tallying) =
        unstring_options_ast(words.get(targets_end..option_end).unwrap_or_default())?;
    if !overflow_branches_have_bodies_ast(words, "END-UNSTRING") {
        return None;
    }
    Some(UnstringOpAst {
        source: words.get(1)?.clone(),
        delimiter,
        targets,
        pointer,
        tallying,
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

fn string_pointer_option_ast(tokens: &[String]) -> Option<Option<String>> {
    match tokens {
        [] => Some(None),
        [with, pointer, target]
            if with.eq_ignore_ascii_case("WITH") && pointer.eq_ignore_ascii_case("POINTER") =>
        {
            Some(Some(target.clone()))
        }
        _ => None,
    }
}

fn unstring_options_ast(tokens: &[String]) -> Option<(Option<String>, Option<String>)> {
    let mut idx = 0usize;
    let mut pointer = None;
    let mut tallying = None;
    while idx < tokens.len() {
        if tokens
            .get(idx)
            .is_some_and(|token| token.eq_ignore_ascii_case("WITH"))
            && tokens
                .get(idx + 1)
                .is_some_and(|token| token.eq_ignore_ascii_case("POINTER"))
            && pointer.is_none()
        {
            pointer = Some(tokens.get(idx + 2)?.clone());
            idx += 3;
        } else if tokens
            .get(idx)
            .is_some_and(|token| token.eq_ignore_ascii_case("TALLYING"))
            && tokens
                .get(idx + 1)
                .is_some_and(|token| token.eq_ignore_ascii_case("IN"))
            && tallying.is_none()
        {
            tallying = Some(tokens.get(idx + 2)?.clone());
            idx += 3;
        } else {
            return None;
        }
    }
    Some((pointer, tallying))
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

fn first_overflow_or_scope_idx_ast(tokens: &[String], terminator: &str) -> usize {
    [
        find_overflow_branch_idx_ast(tokens, true),
        find_overflow_branch_idx_ast(tokens, false),
        matching_explicit_scope_end_idx(tokens, 0)
            .filter(|idx| tokens[*idx].eq_ignore_ascii_case(terminator)),
    ]
    .into_iter()
    .flatten()
    .min()
    .unwrap_or(tokens.len())
}

fn overflow_branch_bounds_ast(
    words: &[String],
    overflow: bool,
    terminator: &str,
) -> Option<(usize, usize)> {
    let start = find_overflow_branch_idx_ast(words, overflow)?;
    let marker_len = if overflow { 2 } else { 3 };
    let body_start = start + marker_len;
    let other = find_overflow_branch_idx_ast(words, !overflow).unwrap_or(words.len());
    let terminator = matching_explicit_scope_end_idx(words, 0)
        .filter(|idx| words[*idx].eq_ignore_ascii_case(terminator))
        .unwrap_or(words.len());
    let mut end = terminator;
    for candidate in [other, terminator] {
        if candidate > start {
            end = end.min(candidate);
        }
    }
    Some((body_start, end))
}

fn overflow_branches_have_bodies_ast(words: &[String], terminator: &str) -> bool {
    [true, false].into_iter().all(|overflow| {
        match overflow_branch_bounds_ast(words, overflow, terminator) {
            Some((body_start, end)) => body_start < end,
            None => true,
        }
    })
}

fn overflow_branch_ast(
    tokens: &[SpannedWord],
    words: &[String],
    raw_source: &str,
    overflow: bool,
    terminator: &str,
    span: SourceSpan,
) -> ImperativeListAst {
    let Some((body_start, end)) = overflow_branch_bounds_ast(words, overflow, terminator) else {
        return Vec::new();
    };
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
            let mut start = if words
                .get(idx + 1)
                .map(|word| word.eq_ignore_ascii_case("TO"))
                == Some(true)
            {
                idx + 2
            } else {
                idx + 1
            };
            if words
                .get(start)
                .map(|word| word.eq_ignore_ascii_case("DYNAMIC"))
                == Some(true)
            {
                start += 1;
            }
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
    if sep <= 1 || sep + 1 >= words.len() {
        return None;
    }
    if !move_source_is_consumed_ast(words, 1, sep) {
        return None;
    }
    let (_, consumed) = qualified_data_reference_at(words, sep + 1)?;
    if sep + 1 + consumed != words.len() {
        return None;
    }
    let source = words.get(1..sep)?.join(" ");
    let target = words.get(sep + 1..)?.join(" ");
    Some((source, target))
}

fn move_source_is_consumed_ast(words: &[String], start: usize, end: usize) -> bool {
    if end <= start {
        return false;
    }
    if end == start + 1 {
        return true;
    }
    if words
        .get(start)
        .is_some_and(|word| word.eq_ignore_ascii_case("FUNCTION"))
    {
        return true;
    }
    qualified_data_reference_at(words, start)
        .map(|(_, consumed)| start + consumed == end)
        .unwrap_or(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArithmeticStatementAst {
    Add,
    Subtract,
    Multiply,
    Divide,
}

fn parse_add_statement(
    tokens: &[SpannedWord],
    raw_source: &str,
    words: &[String],
    span: SourceSpan,
) -> StatementKindAst {
    if words.iter().any(|word| word.eq_ignore_ascii_case("GIVING")) {
        return parse_add_giving_statement(tokens, raw_source, words, span)
            .unwrap_or_else(|| StatementKindAst::Unsupported("ADD GIVING".to_string()));
    }
    parse_binary_arithmetic_statement(
        tokens,
        raw_source,
        words,
        span,
        "TO",
        ArithmeticStatementAst::Add,
    )
    .unwrap_or_else(|| StatementKindAst::Unsupported("ADD".to_string()))
}

fn parse_add_giving_statement(
    tokens: &[SpannedWord],
    raw_source: &str,
    words: &[String],
    span: SourceSpan,
) -> Option<StatementKindAst> {
    let to_idx = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("TO"))?;
    let giving_idx = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("GIVING"))?;
    if to_idx <= 1 || giving_idx <= to_idx + 1 || giving_idx + 1 >= words.len() {
        return None;
    }
    if !arithmetic_operand_is_consumed_ast(words, 1, to_idx)
        || !arithmetic_operand_is_consumed_ast(words, to_idx + 1, giving_idx)
    {
        return None;
    }
    let target_end = arithmetic_target_end_idx(words, giving_idx + 1);
    let (target, rounded) =
        arithmetic_target_ast(tokens, raw_source, words, giving_idx + 1, target_end)?;
    let source = raw_from_words(tokens.get(1..to_idx)?, raw_source)
        .trim()
        .to_string();
    let addend = raw_from_words(tokens.get(to_idx + 1..giving_idx)?, raw_source)
        .trim()
        .to_string();
    if !size_error_branches_have_bodies_ast(words) {
        return None;
    }
    Some(StatementKindAst::Compute(ComputeAst {
        target,
        expression: format!("{source} + {addend}"),
        rounded,
        on_size_error: compute_size_error_branch_ast(tokens, words, raw_source, true, span.clone()),
        not_on_size_error: compute_size_error_branch_ast(tokens, words, raw_source, false, span),
    }))
}

fn parse_arithmetic_giving_statement(
    tokens: &[SpannedWord],
    raw_source: &str,
    words: &[String],
    span: SourceSpan,
    separator: &str,
    operation: ArithmeticStatementAst,
) -> Option<StatementKindAst> {
    let sep = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case(separator))?;
    let giving_idx = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("GIVING"))?;
    if sep <= 1 || giving_idx <= sep + 1 || giving_idx + 1 >= words.len() {
        return None;
    }
    if !arithmetic_operand_is_consumed_ast(words, 1, sep)
        || !arithmetic_operand_is_consumed_ast(words, sep + 1, giving_idx)
    {
        return None;
    }

    let source = raw_from_words(tokens.get(1..sep)?, raw_source)
        .trim()
        .to_string();
    let receiver = raw_from_words(tokens.get(sep + 1..giving_idx)?, raw_source)
        .trim()
        .to_string();
    if source.is_empty() || receiver.is_empty() {
        return None;
    }

    let target_end = arithmetic_target_end_idx(words, giving_idx + 1);
    let (target, rounded) =
        arithmetic_target_ast(tokens, raw_source, words, giving_idx + 1, target_end)?;
    let expression = match operation {
        ArithmeticStatementAst::Add => format!("{source} + {receiver}"),
        ArithmeticStatementAst::Subtract => format!("{receiver} - {source}"),
        ArithmeticStatementAst::Multiply => format!("{source} * {receiver}"),
        ArithmeticStatementAst::Divide => format!("{receiver} / {source}"),
    };
    if !size_error_branches_have_bodies_ast(words) {
        return None;
    }

    Some(StatementKindAst::Compute(ComputeAst {
        target,
        expression,
        rounded,
        on_size_error: compute_size_error_branch_ast(tokens, words, raw_source, true, span.clone()),
        not_on_size_error: compute_size_error_branch_ast(tokens, words, raw_source, false, span),
    }))
}

fn parse_binary_arithmetic_statement(
    tokens: &[SpannedWord],
    raw_source: &str,
    words: &[String],
    span: SourceSpan,
    separator: &str,
    operation: ArithmeticStatementAst,
) -> Option<StatementKindAst> {
    let sep = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case(separator))?;
    if sep <= 1 {
        return None;
    }
    let target_end = arithmetic_target_end_idx(words, sep + 1);
    if target_end <= sep + 1 {
        return None;
    }
    if !arithmetic_operand_is_consumed_ast(words, 1, sep) {
        return None;
    }
    let source = raw_from_words(tokens.get(1..sep)?, raw_source)
        .trim()
        .to_string();
    let (target, rounded) = arithmetic_target_ast(tokens, raw_source, words, sep + 1, target_end)?;

    if !rounded && !arithmetic_has_size_error_branch(words) {
        return Some(match operation {
            ArithmeticStatementAst::Add => StatementKindAst::Add { source, target },
            ArithmeticStatementAst::Subtract => StatementKindAst::Subtract { source, target },
            ArithmeticStatementAst::Multiply => StatementKindAst::Multiply { source, target },
            ArithmeticStatementAst::Divide => StatementKindAst::Divide { source, target },
        });
    }

    let expression = match operation {
        ArithmeticStatementAst::Add => format!("{target} + {source}"),
        ArithmeticStatementAst::Subtract => format!("{target} - {source}"),
        ArithmeticStatementAst::Multiply => format!("{target} * {source}"),
        ArithmeticStatementAst::Divide => format!("{target} / {source}"),
    };
    if !size_error_branches_have_bodies_ast(words) {
        return None;
    }
    Some(StatementKindAst::Compute(ComputeAst {
        target,
        expression,
        rounded,
        on_size_error: compute_size_error_branch_ast(tokens, words, raw_source, true, span.clone()),
        not_on_size_error: compute_size_error_branch_ast(tokens, words, raw_source, false, span),
    }))
}

fn arithmetic_operand_is_consumed_ast(words: &[String], start: usize, end: usize) -> bool {
    if end <= start {
        return false;
    }
    if end == start + 1 {
        return true;
    }
    if words
        .get(start)
        .is_some_and(|word| word.eq_ignore_ascii_case("FUNCTION"))
    {
        return true;
    }
    qualified_data_reference_at(words, start)
        .map(|(_, consumed)| start + consumed == end)
        .unwrap_or(false)
}

fn arithmetic_target_ast(
    tokens: &[SpannedWord],
    raw_source: &str,
    words: &[String],
    start: usize,
    end: usize,
) -> Option<(String, bool)> {
    if end <= start {
        return None;
    }
    let rounded = words
        .get(end - 1)
        .is_some_and(|word| word.eq_ignore_ascii_case("ROUNDED"));
    let target_end = if rounded { end - 1 } else { end };
    if target_end <= start {
        return None;
    }
    if words
        .get(start..target_end)?
        .iter()
        .any(|word| word.eq_ignore_ascii_case("ROUNDED"))
    {
        return None;
    }
    let (_, consumed) = qualified_data_reference_at(words, start)?;
    if start + consumed != target_end {
        return None;
    }
    let target = raw_from_words(tokens.get(start..target_end)?, raw_source)
        .trim()
        .to_string();
    Some((target, rounded))
}

fn arithmetic_target_end_idx(words: &[String], start: usize) -> usize {
    first_compute_option_idx_ast(words, start)
        .unwrap_or_else(|| matching_explicit_scope_end_idx(words, 0).unwrap_or(words.len()))
}

fn arithmetic_has_size_error_branch(words: &[String]) -> bool {
    find_size_error_branch_idx_ast(words, true, 1).is_some()
        || find_size_error_branch_idx_ast(words, false, 1).is_some()
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
        let (_, source_consumed) = qualified_data_reference_at(words, 2)?;
        if 2 + source_consumed != sep {
            return None;
        }
        let (_, target_consumed) = qualified_data_reference_at(words, sep + 1)?;
        if sep + 1 + target_consumed != words.len() {
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
    if eq < 2 {
        return None;
    }
    let target_words = words.get(1..eq)?;
    let rounded = target_words
        .last()
        .is_some_and(|word| word.eq_ignore_ascii_case("ROUNDED"));
    if target_words[..target_words.len().saturating_sub(usize::from(rounded))]
        .iter()
        .any(|word| word.eq_ignore_ascii_case("ROUNDED"))
    {
        return None;
    }
    let target_end = if rounded { eq.saturating_sub(1) } else { eq };
    if target_end <= 1 {
        return None;
    }
    if target_end != 2
        && !target_words
            .iter()
            .any(|word| word.eq_ignore_ascii_case("OF") || word.eq_ignore_ascii_case("IN"))
    {
        return None;
    }
    let target = raw_from_words(tokens.get(1..target_end)?, raw_source)
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
    if !size_error_branches_have_bodies_ast(words) {
        return None;
    }
    Some(ComputeAst {
        target,
        expression,
        rounded,
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

fn size_error_branch_bounds_ast(words: &[String], size_error: bool) -> Option<(usize, usize)> {
    let start = find_size_error_branch_idx_ast(words, size_error, 1)?;
    let marker_len = if size_error { 3 } else { 4 };
    let body_start = start + marker_len;
    let other =
        find_size_error_branch_idx_ast(words, !size_error, body_start).unwrap_or(words.len());
    let terminator = matching_explicit_scope_end_idx(words, 0).unwrap_or(words.len());
    let mut end = terminator;
    for candidate in [other, terminator] {
        if candidate > start {
            end = end.min(candidate);
        }
    }
    Some((body_start, end))
}

fn size_error_branches_have_bodies_ast(words: &[String]) -> bool {
    [true, false].into_iter().all(|size_error| {
        match size_error_branch_bounds_ast(words, size_error) {
            Some((body_start, end)) => body_start < end,
            None => true,
        }
    })
}

fn compute_size_error_branch_ast(
    tokens: &[SpannedWord],
    words: &[String],
    raw_source: &str,
    size_error: bool,
    span: SourceSpan,
) -> ImperativeListAst {
    let Some((body_start, end)) = size_error_branch_bounds_ast(words, size_error) else {
        return Vec::new();
    };
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
    let subject_groups = split_also_groups_ast(&body_words[..first_when]);
    if subject_groups.is_empty() || subject_groups.iter().any(Vec::is_empty) {
        return None;
    }
    let subjects = subject_groups
        .into_iter()
        .map(|group| group.join(" "))
        .collect::<Vec<_>>();
    let subject_count = subjects.len();
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
    if action_idx == 0 {
        return None;
    }
    let pattern_tokens = &words[..action_idx];
    let body_tokens = &tokens[action_idx..];
    let patterns = if pattern_tokens.len() == 1 && pattern_tokens[0].eq_ignore_ascii_case("OTHER") {
        vec!["OTHER".to_string(); subject_count]
    } else if subject_count <= 1 {
        vec![pattern_tokens.join(" ")]
    } else {
        let pattern_groups = split_also_groups_ast(pattern_tokens);
        if pattern_groups.len() != subject_count || pattern_groups.iter().any(Vec::is_empty) {
            return None;
        }
        pattern_groups
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
    let has_at_end = body_words
        .get(cursor)
        .map(|token| token.eq_ignore_ascii_case("AT"))
        == Some(true)
        && body_words
            .get(cursor + 1)
            .map(|token| token.eq_ignore_ascii_case("END"))
            == Some(true);
    let at_end = if has_at_end {
        if cursor + 2 >= first_when {
            return None;
        }
        branch_statements_ast(
            body_tokens.get(cursor + 2..first_when).unwrap_or_default(),
            raw_source,
            span.clone(),
        )
    } else {
        if cursor != first_when {
            return None;
        }
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
    let list_start = idx;
    while idx < tokens.len() {
        if imperative_token_is_terminator(&words, list_start, idx, terminators) {
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

fn imperative_token_is_terminator(
    words: &[String],
    list_start: usize,
    idx: usize,
    terminators: &[&str],
) -> bool {
    let Some(word) = words.get(idx) else {
        return false;
    };
    if !terminators
        .iter()
        .any(|terminator| word.eq_ignore_ascii_case(terminator))
    {
        return false;
    }
    if !word.eq_ignore_ascii_case("ELSE") && !word.eq_ignore_ascii_case("END-IF") {
        return true;
    }

    let mut if_depth = 0usize;
    for token in words.iter().take(idx).skip(list_start) {
        if token.eq_ignore_ascii_case("IF") {
            if_depth = if_depth.saturating_add(1);
        } else if token.eq_ignore_ascii_case("END-IF") {
            if_depth = if_depth.saturating_sub(1);
        }
    }
    if_depth == 0
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
        "ADD" => "END-ADD",
        "COMPUTE" => "END-COMPUTE",
        "DIVIDE" => "END-DIVIDE",
        "EVALUATE" => "END-EVALUATE",
        "MULTIPLY" => "END-MULTIPLY",
        "SEARCH" => "END-SEARCH",
        "START" => "END-START",
        "SUBTRACT" => "END-SUBTRACT",
        "READ" => "END-READ",
        "WRITE" => "END-WRITE",
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
        "WRITE" | "REWRITE" | "DELETE" | "START" => {
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
            | "ENABLE"
            | "DISABLE"
            | "EVALUATE"
            | "SEND"
            | "RECEIVE"
            | "SEARCH"
            | "SET"
            | "PERFORM"
            | "ENTRY"
            | "CHAIN"
            | "UNLOCK"
            | "GENERATE"
            | "INITIATE"
            | "TERMINATE"
            | "PURGE"
            | "SUPPRESS"
            | "GO"
            | "GOBACK"
            | "INITIALIZE"
            | "STOP"
            | "CALL"
            | "CANCEL"
            | "START"
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
            | "EXIT"
            | "ALTER"
            | "ENTER"
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
        if to_idx + 2 != words.len() {
            return None;
        }
        Some(StatementKindAst::SetCondition {
            condition,
            value: true,
        })
    } else if value.eq_ignore_ascii_case("FALSE") {
        if to_idx + 2 != words.len() {
            return None;
        }
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

fn parse_accept(words: &[String]) -> Option<StatementKindAst> {
    let target = words.get(1)?.clone();
    let with_idx = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("WITH"));
    let options = if let Some(with_idx) = with_idx {
        if with_idx + 1 >= words.len() {
            return None;
        }
        accept_option_phrase(words.get(with_idx + 1..)?)?
    } else {
        Vec::new()
    };
    let end = with_idx.unwrap_or(words.len());
    let source = match end {
        2 => None,
        len if len >= 4 && words[2].eq_ignore_ascii_case("FROM") => {
            Some(accept_source_phrase(words.get(3..end)?)?)
        }
        _ => return None,
    };
    Some(StatementKindAst::Accept {
        target,
        source,
        options,
    })
}

fn accept_source_phrase(words: &[String]) -> Option<String> {
    match words {
        [source] if is_accept_single_source(source) => Some(source.clone()),
        [source, format]
            if source.eq_ignore_ascii_case("DATE") && format.eq_ignore_ascii_case("YYYYMMDD") =>
        {
            Some(words.join(" "))
        }
        _ => None,
    }
}

fn is_accept_single_source(word: &str) -> bool {
    matches!(
        word.to_ascii_uppercase().as_str(),
        "DATE"
            | "DAY"
            | "DAY-OF-WEEK"
            | "TIME"
            | "COMMAND-LINE"
            | "ENVIRONMENT-NAME"
            | "ENVIRONMENT-VALUE"
            | "ARGUMENT-NUMBER"
    )
}

fn accept_option_phrase(words: &[String]) -> Option<Vec<String>> {
    match words {
        [option] if is_accept_single_option(option) => Some(vec![option.clone()]),
        [no, echo] if no.eq_ignore_ascii_case("NO") && echo.eq_ignore_ascii_case("ECHO") => {
            Some(vec![words.join(" ")])
        }
        _ => None,
    }
}

fn is_accept_single_option(word: &str) -> bool {
    matches!(
        word.to_ascii_uppercase().as_str(),
        "ECHO" | "PROMPT" | "UPDATE" | "SECURE"
    )
}

fn parse_initialize(words: &[String]) -> Option<StatementKindAst> {
    if words.len() < 2 {
        return None;
    }
    let option_idx = words.iter().position(|word| {
        word.eq_ignore_ascii_case("REPLACING") || word.eq_ignore_ascii_case("WITH")
    });
    let end = option_idx.unwrap_or(words.len());
    let targets = words
        .get(1..end)?
        .iter()
        .filter(|word| !word.as_str().trim_matches(',').is_empty())
        .map(|word| word.trim_end_matches(',').to_string())
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return None;
    }
    let options = if let Some(option_idx) = option_idx {
        if !initialize_options_are_complete(words.get(option_idx..)?) {
            return None;
        }
        vec![words[option_idx..].join(" ")]
    } else {
        Vec::new()
    };
    Some(StatementKindAst::Initialize { targets, options })
}

fn initialize_options_are_complete(words: &[String]) -> bool {
    let mut idx = 0usize;
    while idx < words.len() {
        if words[idx].eq_ignore_ascii_case("WITH") {
            if !words
                .get(idx + 1)
                .is_some_and(|word| word.eq_ignore_ascii_case("FILLER"))
            {
                return false;
            }
            idx += 2;
        } else if words[idx].eq_ignore_ascii_case("REPLACING") {
            let by_idx =
                words
                    .iter()
                    .enumerate()
                    .skip(idx + 1)
                    .find_map(|(candidate_idx, word)| {
                        word.eq_ignore_ascii_case("BY").then_some(candidate_idx)
                    });
            let Some(by_idx) = by_idx else {
                return false;
            };
            if by_idx == idx + 1 || by_idx + 1 >= words.len() {
                return false;
            }
            idx = words.len();
        } else {
            return false;
        }
    }
    true
}

fn parse_cancel(words: &[String]) -> Option<StatementKindAst> {
    if words.len() < 2 || !words[0].eq_ignore_ascii_case("CANCEL") {
        return None;
    }
    let targets = words
        .get(1..)?
        .iter()
        .map(|word| word.trim_end_matches(','))
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return None;
    }
    Some(StatementKindAst::Cancel { targets })
}

fn parse_entry(words: &[String]) -> Option<StatementKindAst> {
    if words.len() < 2 || !words[0].eq_ignore_ascii_case("ENTRY") {
        return None;
    }
    let name = words[1].trim_end_matches(',').to_string();
    if name.is_empty() {
        return None;
    }
    let using = match words.len() {
        2 => Vec::new(),
        len if len > 3
            && words
                .get(2)
                .is_some_and(|word| word.eq_ignore_ascii_case("USING")) =>
        {
            entry_chain_using_operands_ast(&words[3..], "END-ENTRY")?
        }
        _ => return None,
    };
    Some(StatementKindAst::Entry { name, using })
}

fn parse_chain(words: &[String]) -> Option<StatementKindAst> {
    if words.len() < 2 || !words[0].eq_ignore_ascii_case("CHAIN") {
        return None;
    }
    let target = words[1].trim_end_matches(',').to_string();
    if target.is_empty() {
        return None;
    }
    let using = match words.len() {
        2 => Vec::new(),
        len if len > 3
            && words
                .get(2)
                .is_some_and(|word| word.eq_ignore_ascii_case("USING")) =>
        {
            entry_chain_using_operands_ast(&words[3..], "END-CHAIN")?
        }
        _ => return None,
    };
    Some(StatementKindAst::Chain { target, using })
}

fn entry_chain_using_operands_ast(words: &[String], terminator: &str) -> Option<Vec<String>> {
    let mut using = Vec::new();
    for (idx, word) in words.iter().enumerate() {
        if entry_chain_using_boundary_ast(words, idx, terminator) {
            return None;
        }
        let trimmed = word.trim_end_matches(',');
        if !trimmed.is_empty() {
            using.push(trimmed.to_string());
        }
    }
    (!using.is_empty()).then_some(using)
}

fn entry_chain_using_boundary_ast(words: &[String], idx: usize, terminator: &str) -> bool {
    let Some(word) = words.get(idx) else {
        return false;
    };
    let word = word.trim_end_matches(',');
    word.eq_ignore_ascii_case(terminator)
        || is_statement_start(word)
        || is_call_using_boundary_ast(words, idx)
}

fn parse_unlock(words: &[String]) -> Option<StatementKindAst> {
    if words.len() < 2 || !words[0].eq_ignore_ascii_case("UNLOCK") {
        return None;
    }
    let file = words[1].trim_end_matches(',').to_string();
    if file.is_empty() {
        return None;
    }
    let options = words
        .get(2..)
        .unwrap_or_default()
        .iter()
        .map(|word| word.trim_end_matches(','))
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect();
    Some(StatementKindAst::Unlock { file, options })
}

fn parse_generate(words: &[String]) -> Option<StatementKindAst> {
    if words.len() < 2 || !words[0].eq_ignore_ascii_case("GENERATE") {
        return None;
    }
    let target = words[1].trim_end_matches(',').to_string();
    if target.is_empty() {
        return None;
    }
    let options = words
        .get(2..)
        .unwrap_or_default()
        .iter()
        .map(|word| word.trim_end_matches(','))
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect();
    Some(StatementKindAst::Generate { target, options })
}

fn parse_initiate(words: &[String]) -> Option<StatementKindAst> {
    if words.len() < 2 || !words[0].eq_ignore_ascii_case("INITIATE") {
        return None;
    }
    let targets = words
        .get(1..)?
        .iter()
        .map(|word| word.trim_end_matches(','))
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return None;
    }
    Some(StatementKindAst::Initiate { targets })
}

fn parse_terminate(words: &[String]) -> Option<StatementKindAst> {
    if words.len() < 2 || !words[0].eq_ignore_ascii_case("TERMINATE") {
        return None;
    }
    let targets = words
        .get(1..)?
        .iter()
        .map(|word| word.trim_end_matches(','))
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return None;
    }
    Some(StatementKindAst::Terminate { targets })
}

fn parse_purge(words: &[String]) -> Option<StatementKindAst> {
    if words.len() < 2 || !words[0].eq_ignore_ascii_case("PURGE") {
        return None;
    }
    let target = words[1].trim_end_matches(',').to_string();
    if target.is_empty() {
        return None;
    }
    let options = words
        .get(2..)
        .unwrap_or_default()
        .iter()
        .map(|word| word.trim_end_matches(','))
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect();
    Some(StatementKindAst::Purge { target, options })
}

fn parse_suppress(words: &[String]) -> Option<StatementKindAst> {
    if words.is_empty() || !words[0].eq_ignore_ascii_case("SUPPRESS") {
        return None;
    }
    let mut options = Vec::new();
    let mut idx = 1;
    if words
        .get(idx)
        .is_some_and(|word| word.eq_ignore_ascii_case("PRINTING"))
    {
        options.push(words[idx].trim_end_matches(',').to_string());
        idx += 1;
    }
    let target = if let Some(word) = words.get(idx) {
        let target = word.trim_end_matches(',');
        if target.is_empty() {
            None
        } else {
            idx += 1;
            Some(target.to_string())
        }
    } else {
        None
    };
    if idx != words.len() {
        return None;
    }
    Some(StatementKindAst::Suppress { target, options })
}

fn parse_enable_disable(words: &[String], keyword: &str) -> Option<StatementKindAst> {
    if words.len() < 2 || !words[0].eq_ignore_ascii_case(keyword) {
        return None;
    }
    let mut options = Vec::new();
    let target_idx = if words.get(1).is_some_and(|word| {
        matches!(
            word.to_ascii_uppercase().as_str(),
            "INPUT" | "OUTPUT" | "I-O"
        )
    }) {
        options.push(words[1].trim_end_matches(',').to_string());
        2
    } else {
        1
    };
    let target = words.get(target_idx)?.trim_end_matches(',').to_string();
    if target.is_empty() {
        return None;
    }
    options.extend(
        words
            .get((target_idx + 1)..)
            .unwrap_or_default()
            .iter()
            .map(|word| word.trim_end_matches(','))
            .filter(|word| !word.is_empty())
            .map(str::to_string),
    );
    if keyword.eq_ignore_ascii_case("ENABLE") {
        Some(StatementKindAst::Enable { target, options })
    } else {
        Some(StatementKindAst::Disable { target, options })
    }
}

fn parse_target_options_statement(
    words: &[String],
    keyword: &str,
) -> Option<(String, Vec<String>)> {
    if words.len() < 2 || !words[0].eq_ignore_ascii_case(keyword) {
        return None;
    }
    let target = words[1].trim_end_matches(',').to_string();
    if target.is_empty() {
        return None;
    }
    let options = words
        .get(2..)
        .unwrap_or_default()
        .iter()
        .map(|word| word.trim_end_matches(','))
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect();
    Some((target, options))
}

fn parse_start(
    tokens: &[SpannedWord],
    words: &[String],
    raw_source: &str,
    span: SourceSpan,
) -> Option<StatementKindAst> {
    if words.len() < 2 || !words[0].eq_ignore_ascii_case("START") {
        return None;
    }
    let file = words[1].trim_end_matches(',').to_string();
    if file.is_empty() {
        return None;
    }
    let mut options = Vec::new();
    let mut idx = 2usize;
    while idx < words.len() {
        if is_start_branch_phrase(words, idx) {
            break;
        }
        let phrase_start = idx;
        idx += 1;
        while idx < words.len() && !is_start_branch_phrase(words, idx) {
            idx += 1;
        }
        let phrase = words[phrase_start..idx].join(" ");
        if !phrase.trim().is_empty() {
            options.push(phrase);
        }
    }
    if !invalid_key_branches_have_bodies_ast(words, "END-START") {
        return None;
    }
    Some(StatementKindAst::Start {
        file,
        options,
        invalid_key: invalid_key_branch_ast(
            tokens,
            words,
            raw_source,
            true,
            "END-START",
            span.clone(),
        ),
        not_invalid_key: invalid_key_branch_ast(
            tokens,
            words,
            raw_source,
            false,
            "END-START",
            span,
        ),
    })
}

fn is_start_branch_phrase(words: &[String], idx: usize) -> bool {
    words
        .get(idx)
        .is_some_and(|word| word.eq_ignore_ascii_case("INVALID"))
        || (words
            .get(idx)
            .is_some_and(|word| word.eq_ignore_ascii_case("NOT"))
            && words
                .get(idx + 1)
                .is_some_and(|word| word.eq_ignore_ascii_case("INVALID"))
            && words
                .get(idx + 2)
                .is_some_and(|word| word.eq_ignore_ascii_case("KEY")))
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
            let Some(using) = group_qualified_call_operands(&words[3..]) else {
                return StatementKindAst::Unsupported("CALL".to_string());
            };
            using
        }
        _ => return StatementKindAst::Unsupported("CALL".to_string()),
    };
    StatementKindAst::Call {
        target: target.clone(),
        using,
    }
}

fn group_qualified_call_operands(words: &[String]) -> Option<Vec<String>> {
    let mut operands = Vec::new();
    let mut idx = 0usize;
    while idx < words.len() {
        let (operand, consumed) = qualified_call_operand_at(words, idx)?;
        idx += consumed;
        operands.push(operand);
    }
    (!operands.is_empty()).then_some(operands)
}

fn qualified_data_reference_at(words: &[String], start: usize) -> Option<(String, usize)> {
    let mut operand = words.get(start)?.clone();
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
    Some((operand, consumed))
}

fn qualified_call_operand_at(words: &[String], start: usize) -> Option<(String, usize)> {
    let word = words.get(start)?;
    if word.trim_end_matches(',').is_empty() {
        return None;
    }
    if is_call_using_boundary_ast(words, start) {
        return None;
    }
    if word.eq_ignore_ascii_case("BY")
        && words
            .get(start + 1)
            .is_some_and(|word| is_call_using_mode(word))
    {
        let mode = words.get(start + 1)?;
        let (argument, argument_consumed) = qualified_call_operand_at(words, start + 2)?;
        return Some((format!("{word} {mode} {argument}"), argument_consumed + 2));
    }
    qualified_data_reference_at(words, start)
}

fn is_call_using_boundary_ast(words: &[String], idx: usize) -> bool {
    words
        .get(idx)
        .is_some_and(|word| word.eq_ignore_ascii_case("END-CALL"))
        || words
            .get(idx)
            .is_some_and(|word| word.eq_ignore_ascii_case("ON"))
            && words
                .get(idx + 1)
                .is_some_and(|word| word.eq_ignore_ascii_case("EXCEPTION"))
        || words
            .get(idx)
            .is_some_and(|word| word.eq_ignore_ascii_case("NOT"))
            && words
                .get(idx + 1)
                .is_some_and(|word| word.eq_ignore_ascii_case("ON"))
            && words
                .get(idx + 2)
                .is_some_and(|word| word.eq_ignore_ascii_case("EXCEPTION"))
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
    qualified_data_reference_at(words, start).expect("qualified operand start must be in range")
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
    if !perform_tail_is_consumed(words) {
        return StatementKindAst::Unsupported("PERFORM".to_string());
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

fn perform_tail_is_consumed(words: &[String]) -> bool {
    let mut idx = 2usize;
    while idx < words.len() {
        if words[idx].eq_ignore_ascii_case("THRU") || words[idx].eq_ignore_ascii_case("THROUGH") {
            if idx + 1 >= words.len() {
                return false;
            }
            idx += 2;
        } else if idx + 1 < words.len() && words[idx + 1].eq_ignore_ascii_case("TIMES") {
            idx += 2;
        } else if words[idx].eq_ignore_ascii_case("UNTIL") {
            let start = idx + 1;
            idx = start;
            while idx < words.len() && !is_perform_until_trailer(&words[idx]) {
                idx += 1;
            }
            if idx == start {
                return false;
            }
        } else if words[idx].eq_ignore_ascii_case("VARYING") {
            let start = idx + 1;
            idx = start;
            while idx < words.len() && !is_perform_varying_trailer(&words[idx]) {
                idx += 1;
            }
            if idx == start {
                return false;
            }
        } else if words[idx].eq_ignore_ascii_case("WITH") {
            if words
                .get(idx + 1)
                .is_some_and(|word| word.eq_ignore_ascii_case("TEST"))
                && words.get(idx + 2).is_some_and(|word| {
                    word.eq_ignore_ascii_case("BEFORE") || word.eq_ignore_ascii_case("AFTER")
                })
            {
                idx += 3;
            } else {
                return false;
            }
        } else {
            return false;
        }
    }
    true
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
            | "CANCEL"
            | "CHAIN"
            | "CLOSE"
            | "COMPUTE"
            | "CONTINUE"
            | "DELETE"
            | "DISPLAY"
            | "DIVIDE"
            | "DISABLE"
            | "DIVISION"
            | "ENABLE"
            | "ENTER"
            | "ENTRY"
            | "EVALUATE"
            | "EXAMINE"
            | "EXEC"
            | "EXIT"
            | "GENERATE"
            | "GO"
            | "GOBACK"
            | "IF"
            | "INITIATE"
            | "INITIALIZE"
            | "INSPECT"
            | "MERGE"
            | "MOVE"
            | "MULTIPLY"
            | "NEXT"
            | "OPEN"
            | "PERFORM"
            | "PURGE"
            | "READ"
            | "RECEIVE"
            | "READY"
            | "RELEASE"
            | "RESET"
            | "RETURN"
            | "REWRITE"
            | "SEARCH"
            | "SECTION"
            | "SEND"
            | "SET"
            | "SORT"
            | "START"
            | "STOP"
            | "STRING"
            | "SUPPRESS"
            | "SUBTRACT"
            | "TERMINATE"
            | "UNLOCK"
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

fn is_sentence_period(text: &str, byte_idx: usize, _current: &str) -> bool {
    cobol_text::is_sentence_period_outside_literals(text, byte_idx)
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
    fn select_dynamic_assign_preserves_identifier_target() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. X.\nENVIRONMENT DIVISION.\nINPUT-OUTPUT SECTION.\nFILE-CONTROL.\nSELECT INFILE ASSIGN TO DYNAMIC WS-PATH.\n";
        let ast = parse_program("x.cbl", src).expect("program parses");
        assert_eq!(ast.files.len(), 1);
        assert_eq!(ast.files[0].name, "INFILE");
        assert_eq!(ast.files[0].assign.as_deref(), Some("WS-PATH"));
        assert!(!ast.files[0].assign_is_literal);
    }

    #[test]
    fn select_file_status_preserves_multiline_status_item() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. X.\nENVIRONMENT DIVISION.\nINPUT-OUTPUT SECTION.\nFILE-CONTROL.\nSELECT INFILE ASSIGN TO \"INFILE\"\n    ORGANIZATION IS SEQUENTIAL\n    ACCESS MODE IS SEQUENTIAL\n    FILE STATUS IS STATUS-FIELD.\n";
        let ast = parse_program("x.cbl", src).expect("program parses");
        assert_eq!(ast.files.len(), 1);
        assert_eq!(ast.files[0].name, "INFILE");
        assert_eq!(ast.files[0].file_status.as_deref(), Some("STATUS_FIELD"));
    }

    #[test]
    fn malformed_sd_without_name_does_not_panic_after_select() {
        let src = "IDENTIFICATION DIVISION.\nPROGRAM-ID. X.\nENVIRONMENT DIVISION.\nINPUT-OUTPUT SECTION.\nFILE-CONTROL.\nSELECT INFILE ASSIGN TO \"INFILE\".\nDATA DIVISION.\nFILE SECTION.\nSD .\n01 SORT-REC PIC X.\nPROCEDURE DIVISION.\nMAIN.\nSTOP RUN.\n";
        let ast = parse_program("x.cbl", src).expect("program parses without panic");

        assert_eq!(ast.files.len(), 1);
        assert_eq!(ast.files[0].name, "INFILE");
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
    fn one_word_exit_is_noop_statement() {
        let statement = parse_statement("EXIT", SourceSpan::generated());
        assert!(matches!(statement.kind, StatementKindAst::Continue));

        let extended = parse_statement("EXIT PROGRAM", SourceSpan::generated());
        assert!(matches!(extended.kind, StatementKindAst::ExitProgram));

        let statements = parse_imperative_list(
            "DISPLAY \"A\" EXIT PROGRAM DISPLAY \"B\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 3);
        assert!(matches!(statements[1].kind, StatementKindAst::ExitProgram));
    }

    #[test]
    fn accept_statement_parses_basic_target_and_optional_source() {
        let basic = parse_statement("ACCEPT WS-NAME", SourceSpan::generated());
        assert!(matches!(
            basic.kind,
            StatementKindAst::Accept {
                ref target,
                source: None,
                ref options
            } if target == "WS-NAME" && options.is_empty()
        ));

        let sourced = parse_statement("ACCEPT WS-DATE FROM DATE", SourceSpan::generated());
        assert!(matches!(
            sourced.kind,
            StatementKindAst::Accept {
                ref target,
                source: Some(ref source),
                ref options
            } if target == "WS-DATE" && source == "DATE" && options.is_empty()
        ));

        let source_phrase =
            parse_statement("ACCEPT WS-DATE FROM DATE YYYYMMDD", SourceSpan::generated());
        assert!(matches!(
            source_phrase.kind,
            StatementKindAst::Accept {
                ref target,
                source: Some(ref source),
                ref options
            } if target == "WS-DATE" && source == "DATE YYYYMMDD" && options.is_empty()
        ));

        let with_options =
            parse_statement("ACCEPT WS-PASSWORD WITH NO ECHO", SourceSpan::generated());
        assert!(matches!(
            with_options.kind,
            StatementKindAst::Accept {
                ref target,
                source: None,
                ref options
            } if target == "WS-PASSWORD" && options == &vec!["NO ECHO".to_string()]
        ));

        let source_with_options = parse_statement(
            "ACCEPT WS-DATE FROM DATE YYYYMMDD WITH NO ECHO",
            SourceSpan::generated(),
        );
        assert!(matches!(
            source_with_options.kind,
            StatementKindAst::Accept {
                ref target,
                source: Some(ref source),
                ref options
            } if target == "WS-DATE"
                && source == "DATE YYYYMMDD"
                && options == &vec!["NO ECHO".to_string()]
        ));
    }

    #[test]
    fn accept_rejects_trailing_tokens_after_known_phrases() {
        let bad_source = parse_statement(
            "ACCEPT WS-DATE FROM DATE YYYYMMDD GARBAGE",
            SourceSpan::generated(),
        );
        assert!(matches!(
            bad_source.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "ACCEPT"
        ));

        let bad_options = parse_statement(
            "ACCEPT WS-PASSWORD WITH NO ECHO GARBAGE",
            SourceSpan::generated(),
        );
        assert!(matches!(
            bad_options.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "ACCEPT"
        ));
    }

    #[test]
    fn set_condition_rejects_trailing_tokens_after_boolean_value() {
        let statement = parse_statement("SET WS-OK TO TRUE EXTRA", SourceSpan::generated());
        assert!(matches!(
            statement.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "SET"
        ));
    }

    #[test]
    fn initialize_statement_parses_targets_and_splits_from_neighbors() {
        let statement = parse_statement("INITIALIZE WS-A WS-B", SourceSpan::generated());
        assert!(matches!(
            statement.kind,
            StatementKindAst::Initialize { ref targets, ref options }
                if targets == &vec!["WS-A".to_string(), "WS-B".to_string()]
                    && options.is_empty()
        ));

        let replacing = parse_statement(
            "INITIALIZE WS-A REPLACING NUMERIC DATA BY ZERO",
            SourceSpan::generated(),
        );
        assert!(matches!(
            replacing.kind,
            StatementKindAst::Initialize { ref targets, ref options }
                if targets == &vec!["WS-A".to_string()]
                    && options == &vec!["REPLACING NUMERIC DATA BY ZERO".to_string()]
        ));

        let with_filler = parse_statement("INITIALIZE WS-A WITH FILLER", SourceSpan::generated());
        assert!(matches!(
            with_filler.kind,
            StatementKindAst::Initialize { ref targets, ref options }
                if targets == &vec!["WS-A".to_string()]
                    && options == &vec!["WITH FILLER".to_string()]
        ));

        let statements = parse_imperative_list(
            "DISPLAY \"A\" INITIALIZE WS-A STOP RUN",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 3, "{statements:?}");
        assert!(matches!(
            statements[1].kind,
            StatementKindAst::Initialize { .. }
        ));
    }

    #[test]
    fn initialize_rejects_incomplete_option_clauses() {
        let with = parse_statement("INITIALIZE WS-A WITH", SourceSpan::generated());
        assert!(matches!(
            with.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "INITIALIZE"
        ));

        let replacing = parse_statement(
            "INITIALIZE WS-A REPLACING NUMERIC DATA BY",
            SourceSpan::generated(),
        );
        assert!(matches!(
            replacing.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "INITIALIZE"
        ));
    }

    #[test]
    fn cancel_statement_parses_targets_and_splits_from_neighbors() {
        let statement = parse_statement("CANCEL \"SUBPROG\" WS-DYNAMIC", SourceSpan::generated());
        assert!(matches!(
            statement.kind,
            StatementKindAst::Cancel { ref targets }
                if targets == &vec!["\"SUBPROG\"".to_string(), "WS-DYNAMIC".to_string()]
        ));

        let statements = parse_imperative_list(
            "DISPLAY \"A\" CANCEL \"SUBPROG\" DISPLAY \"B\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 3, "{statements:?}");
        assert!(matches!(
            statements[1].kind,
            StatementKindAst::Cancel { .. }
        ));
    }

    #[test]
    fn entry_statement_parses_literal_and_using_arguments_and_splits_from_neighbors() {
        let statement = parse_statement(
            "ENTRY \"ALT-ENTRY\" USING LK-A LK-B",
            SourceSpan::generated(),
        );
        let StatementKindAst::Entry { name, using } = statement.kind else {
            panic!("expected ENTRY statement");
        };
        assert_eq!(name, "\"ALT-ENTRY\"");
        assert_eq!(using, vec!["LK-A".to_string(), "LK-B".to_string()]);

        let statements = parse_imperative_list(
            "DISPLAY \"A\" ENTRY \"ALT-ENTRY\" USING LK-A DISPLAY \"B\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 3, "{statements:?}");
        assert!(matches!(statements[1].kind, StatementKindAst::Entry { .. }));
    }

    #[test]
    fn chain_statement_parses_target_and_using_arguments_and_splits_from_neighbors() {
        let statement = parse_statement(
            "CHAIN \"NEXTPROG\" USING LK-A LK-B",
            SourceSpan::generated(),
        );
        let StatementKindAst::Chain { target, using } = statement.kind else {
            panic!("expected CHAIN statement");
        };
        assert_eq!(target, "\"NEXTPROG\"");
        assert_eq!(using, vec!["LK-A".to_string(), "LK-B".to_string()]);

        let statements = parse_imperative_list(
            "DISPLAY \"A\" CHAIN \"NEXTPROG\" USING LK-A DISPLAY \"B\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 3, "{statements:?}");
        assert!(matches!(statements[1].kind, StatementKindAst::Chain { .. }));
    }

    #[test]
    fn entry_and_chain_using_clauses_require_arguments() {
        let entry = parse_statement("ENTRY \"ALT-ENTRY\" USING ,", SourceSpan::generated());
        assert!(matches!(
            entry.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "ENTRY"
        ));

        let chain = parse_statement("CHAIN \"NEXTPROG\" USING ,", SourceSpan::generated());
        assert!(matches!(
            chain.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "CHAIN"
        ));
    }

    #[test]
    fn entry_and_chain_using_reject_scope_markers_as_arguments() {
        let entry_statements = parse_imperative_list(
            "ENTRY \"ALT-ENTRY\" USING LK-A END-ENTRY DISPLAY \"AFTER\"",
            SourceSpan::generated(),
        );
        assert_eq!(entry_statements.len(), 2, "{entry_statements:?}");
        assert!(matches!(
            entry_statements[0].kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "ENTRY"
        ));
        assert!(matches!(
            entry_statements[1].kind,
            StatementKindAst::Display(_)
        ));

        let chain_statements = parse_imperative_list(
            "CHAIN \"NEXTPROG\" USING LK-A END-CHAIN DISPLAY \"AFTER\"",
            SourceSpan::generated(),
        );
        assert_eq!(chain_statements.len(), 2, "{chain_statements:?}");
        assert!(matches!(
            chain_statements[0].kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "CHAIN"
        ));
        assert!(matches!(
            chain_statements[1].kind,
            StatementKindAst::Display(_)
        ));
    }

    #[test]
    fn unlock_statement_parses_file_options_and_splits_from_neighbors() {
        let statement = parse_statement("UNLOCK CUSTOMER-FILE RECORD", SourceSpan::generated());
        let StatementKindAst::Unlock { file, options } = statement.kind else {
            panic!("expected UNLOCK statement");
        };
        assert_eq!(file, "CUSTOMER-FILE");
        assert_eq!(options, vec!["RECORD".to_string()]);

        let statements = parse_imperative_list(
            "DISPLAY \"A\" UNLOCK CUSTOMER-FILE RECORD DISPLAY \"B\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 3, "{statements:?}");
        assert!(matches!(
            statements[1].kind,
            StatementKindAst::Unlock { .. }
        ));
    }

    #[test]
    fn generate_statement_parses_target_options_and_splits_from_neighbors() {
        let statement = parse_statement("GENERATE SALES-DETAIL REPORT", SourceSpan::generated());
        let StatementKindAst::Generate { target, options } = statement.kind else {
            panic!("expected GENERATE statement");
        };
        assert_eq!(target, "SALES-DETAIL");
        assert_eq!(options, vec!["REPORT".to_string()]);

        let statements = parse_imperative_list(
            "DISPLAY \"A\" GENERATE SALES-DETAIL REPORT DISPLAY \"B\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 3, "{statements:?}");
        assert!(matches!(
            statements[1].kind,
            StatementKindAst::Generate { .. }
        ));
    }

    #[test]
    fn initiate_statement_parses_targets_and_splits_from_neighbors() {
        let statement = parse_statement(
            "INITIATE SALES-REPORT SUMMARY-REPORT",
            SourceSpan::generated(),
        );
        let StatementKindAst::Initiate { targets } = statement.kind else {
            panic!("expected INITIATE statement");
        };
        assert_eq!(
            targets,
            vec!["SALES-REPORT".to_string(), "SUMMARY-REPORT".to_string()]
        );

        let statements = parse_imperative_list(
            "DISPLAY \"A\" INITIATE SALES-REPORT DISPLAY \"B\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 3, "{statements:?}");
        assert!(matches!(
            statements[1].kind,
            StatementKindAst::Initiate { .. }
        ));
    }

    #[test]
    fn terminate_statement_parses_targets_and_splits_from_neighbors() {
        let statement = parse_statement(
            "TERMINATE SALES-REPORT SUMMARY-REPORT",
            SourceSpan::generated(),
        );
        let StatementKindAst::Terminate { targets } = statement.kind else {
            panic!("expected TERMINATE statement");
        };
        assert_eq!(
            targets,
            vec!["SALES-REPORT".to_string(), "SUMMARY-REPORT".to_string()]
        );

        let statements = parse_imperative_list(
            "DISPLAY \"A\" TERMINATE SALES-REPORT DISPLAY \"B\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 3, "{statements:?}");
        assert!(matches!(
            statements[1].kind,
            StatementKindAst::Terminate { .. }
        ));
    }

    #[test]
    fn purge_statement_parses_target_options_and_splits_from_neighbors() {
        let statement = parse_statement("PURGE PRINT-QUEUE MESSAGE", SourceSpan::generated());
        let StatementKindAst::Purge { target, options } = statement.kind else {
            panic!("expected PURGE statement");
        };
        assert_eq!(target, "PRINT-QUEUE");
        assert_eq!(options, vec!["MESSAGE".to_string()]);

        let statements = parse_imperative_list(
            "DISPLAY \"A\" PURGE PRINT-QUEUE MESSAGE DISPLAY \"B\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 3, "{statements:?}");
        assert!(matches!(statements[1].kind, StatementKindAst::Purge { .. }));
    }

    #[test]
    fn suppress_statement_parses_optional_target_and_splits_from_neighbors() {
        let statement = parse_statement("SUPPRESS PRINTING DETAIL-LINE", SourceSpan::generated());
        let StatementKindAst::Suppress { target, options } = statement.kind else {
            panic!("expected SUPPRESS statement");
        };
        assert_eq!(target.as_deref(), Some("DETAIL-LINE"));
        assert_eq!(options, vec!["PRINTING".to_string()]);

        let no_target = parse_statement("SUPPRESS", SourceSpan::generated());
        assert!(matches!(
            no_target.kind,
            StatementKindAst::Suppress {
                target: None,
                ref options
            } if options.is_empty()
        ));

        let statements = parse_imperative_list(
            "DISPLAY \"A\" SUPPRESS PRINTING DETAIL-LINE DISPLAY \"B\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 3, "{statements:?}");
        assert!(matches!(
            statements[1].kind,
            StatementKindAst::Suppress { .. }
        ));
    }

    #[test]
    fn suppress_rejects_unconsumed_target_tail() {
        let statement = parse_statement(
            "SUPPRESS PRINTING DETAIL-LINE EXTRA",
            SourceSpan::generated(),
        );
        assert!(matches!(
            statement.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "SUPPRESS"
        ));
    }

    #[test]
    fn enable_disable_statements_parse_target_options_and_split_from_neighbors() {
        let enable = parse_statement("ENABLE INPUT TERM-1 WITH KEY", SourceSpan::generated());
        let StatementKindAst::Enable { target, options } = enable.kind else {
            panic!("expected ENABLE statement");
        };
        assert_eq!(target, "TERM-1");
        assert_eq!(
            options,
            vec!["INPUT".to_string(), "WITH".to_string(), "KEY".to_string()]
        );

        let disable = parse_statement("DISABLE OUTPUT TERM-1", SourceSpan::generated());
        let StatementKindAst::Disable { target, options } = disable.kind else {
            panic!("expected DISABLE statement");
        };
        assert_eq!(target, "TERM-1");
        assert_eq!(options, vec!["OUTPUT".to_string()]);

        let statements = parse_imperative_list(
            "DISPLAY \"A\" ENABLE INPUT TERM-1 DISABLE OUTPUT TERM-1 DISPLAY \"B\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 4, "{statements:?}");
        assert!(matches!(
            statements[1].kind,
            StatementKindAst::Enable { .. }
        ));
        assert!(matches!(
            statements[2].kind,
            StatementKindAst::Disable { .. }
        ));
    }

    #[test]
    fn send_receive_statements_parse_target_options_and_split_from_neighbors() {
        let send = parse_statement("SEND TERM-1 FROM OUT-MSG WITH EGI", SourceSpan::generated());
        let StatementKindAst::Send { target, options } = send.kind else {
            panic!("expected SEND statement");
        };
        assert_eq!(target, "TERM-1");
        assert_eq!(
            options,
            vec![
                "FROM".to_string(),
                "OUT-MSG".to_string(),
                "WITH".to_string(),
                "EGI".to_string()
            ]
        );

        let receive = parse_statement(
            "RECEIVE TERM-1 MESSAGE INTO IN-MSG",
            SourceSpan::generated(),
        );
        let StatementKindAst::Receive { target, options } = receive.kind else {
            panic!("expected RECEIVE statement");
        };
        assert_eq!(target, "TERM-1");
        assert_eq!(
            options,
            vec![
                "MESSAGE".to_string(),
                "INTO".to_string(),
                "IN-MSG".to_string()
            ]
        );

        let statements = parse_imperative_list(
            "DISPLAY \"A\" SEND TERM-1 FROM OUT-MSG RECEIVE TERM-1 MESSAGE INTO IN-MSG DISPLAY \"B\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 4, "{statements:?}");
        assert!(matches!(statements[1].kind, StatementKindAst::Send { .. }));
        assert!(matches!(
            statements[2].kind,
            StatementKindAst::Receive { .. }
        ));
    }

    #[test]
    fn merge_statement_parses_file_options_and_splits_from_neighbors() {
        let statement = parse_statement(
            "MERGE SORT-FILE ON ASCENDING KEY SORT-KEY USING INPUT-1 INPUT-2 GIVING OUTPUT-FILE",
            SourceSpan::generated(),
        );
        let StatementKindAst::Merge { file, options } = statement.kind else {
            panic!("expected MERGE statement");
        };
        assert_eq!(file, "SORT-FILE");
        assert_eq!(
            options,
            vec![
                "ON".to_string(),
                "ASCENDING".to_string(),
                "KEY".to_string(),
                "SORT-KEY".to_string(),
                "USING".to_string(),
                "INPUT-1".to_string(),
                "INPUT-2".to_string(),
                "GIVING".to_string(),
                "OUTPUT-FILE".to_string()
            ]
        );

        let statements = parse_imperative_list(
            "DISPLAY \"A\" MERGE SORT-FILE USING INPUT-1 GIVING OUTPUT-FILE DISPLAY \"B\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 3, "{statements:?}");
        assert!(matches!(statements[1].kind, StatementKindAst::Merge { .. }));
    }

    #[test]
    fn sort_statement_parses_procedure_ranges() {
        let statement = parse_statement(
            "SORT SORT-FILE ASCENDING KEY SORT-REC INPUT PROCEDURE IS LOAD-SORT OUTPUT PROCEDURE IS DRAIN-SORT THRU SORT-DONE",
            SourceSpan::generated(),
        );
        let StatementKindAst::Sort(sort) = statement.kind else {
            panic!("expected SORT statement");
        };
        assert_eq!(sort.file, "SORT_FILE");
        assert!(matches!(
            sort.key,
            Some(SortKeyAst {
                direction: SortDirectionAst::Ascending,
                ref name
            }) if name == "SORT_REC"
        ));
        assert_eq!(
            sort.input_range,
            Some(ProcedureRangeAst {
                target: "LOAD_SORT".to_string(),
                through: None
            })
        );
        assert_eq!(
            sort.output_range,
            ProcedureRangeAst {
                target: "DRAIN_SORT".to_string(),
                through: Some("SORT_DONE".to_string())
            }
        );
    }

    #[test]
    fn sort_procedure_ranges_reject_unconsumed_tokens() {
        for source in [
            "SORT SORT-FILE ASCENDING KEY SORT-REC INPUT PROCEDURE IS LOAD-SORT GARBAGE OUTPUT PROCEDURE IS DRAIN-SORT",
            "SORT SORT-FILE ASCENDING KEY SORT-REC INPUT PROCEDURE IS LOAD-SORT OUTPUT PROCEDURE IS DRAIN-SORT GARBAGE",
        ] {
            let statement = parse_statement(source, SourceSpan::generated());
            assert!(
                matches!(statement.kind, StatementKindAst::Unsupported(ref keyword) if keyword == "SORT"),
                "{source}: {statement:?}"
            );
        }
    }

    #[test]
    fn enter_statement_parses_language_options_and_splits_from_neighbors() {
        let statement = parse_statement("ENTER LANGUAGE ASSEMBLER", SourceSpan::generated());
        let StatementKindAst::Enter { language, options } = statement.kind else {
            panic!("expected ENTER statement");
        };
        assert_eq!(language, "LANGUAGE");
        assert_eq!(options, vec!["ASSEMBLER".to_string()]);

        let statements = parse_imperative_list(
            "DISPLAY \"A\" ENTER LANGUAGE ASSEMBLER DISPLAY \"B\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 3, "{statements:?}");
        assert!(matches!(statements[1].kind, StatementKindAst::Enter { .. }));
    }

    #[test]
    fn start_statement_parses_file_and_options_and_splits_from_neighbors() {
        let statement = parse_statement(
            "START CUSTOMER-FILE KEY IS GREATER THAN CUSTOMER-ID INVALID KEY DISPLAY \"MISS\"",
            SourceSpan::generated(),
        );
        let StatementKindAst::Start {
            file,
            options,
            invalid_key,
            not_invalid_key,
        } = statement.kind
        else {
            panic!("expected START statement");
        };
        assert_eq!(file, "CUSTOMER-FILE");
        assert_eq!(options, vec!["KEY IS GREATER THAN CUSTOMER-ID".to_string()]);
        assert!(
            matches!(invalid_key.first().map(|statement| &statement.kind), Some(StatementKindAst::Display(values)) if values.as_slice() == ["\"MISS\""]),
            "{invalid_key:?}"
        );
        assert!(not_invalid_key.is_empty(), "{not_invalid_key:?}");

        let statements = parse_imperative_list(
            "DISPLAY \"A\" START CUSTOMER-FILE KEY IS EQUAL TO CUSTOMER-ID DISPLAY \"B\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 3, "{statements:?}");
        assert!(matches!(statements[1].kind, StatementKindAst::Start { .. }));
    }

    #[test]
    fn start_invalid_key_branch_stays_inside_imperative_list_statement() {
        let statements = parse_imperative_list(
            "START CUSTOMER-FILE KEY IS GREATER THAN CUSTOMER-ID INVALID KEY DISPLAY \"MISS\"",
            SourceSpan::generated(),
        );

        assert_eq!(statements.len(), 1, "{statements:?}");
        let StatementKindAst::Start {
            options,
            invalid_key,
            not_invalid_key,
            ..
        } = &statements[0].kind
        else {
            panic!("expected START statement");
        };
        assert_eq!(
            options,
            &vec!["KEY IS GREATER THAN CUSTOMER-ID".to_string()]
        );
        assert!(
            matches!(invalid_key.first().map(|statement| &statement.kind), Some(StatementKindAst::Display(values)) if values.as_slice() == ["\"MISS\""]),
            "{invalid_key:?}"
        );
        assert!(not_invalid_key.is_empty(), "{not_invalid_key:?}");
    }

    #[test]
    fn start_invalid_key_branch_ignores_nested_start_not_invalid_phrase() {
        let statements = parse_imperative_list(
            "START OUTER-FILE KEY IS EQUAL TO OUTER-ID INVALID KEY START INNER-FILE KEY IS EQUAL TO INNER-ID INVALID KEY DISPLAY \"INNER-BAD\" NOT INVALID KEY DISPLAY \"INNER-OK\" END-START NOT INVALID KEY DISPLAY \"OUTER-OK\" END-START",
            SourceSpan::generated(),
        );

        assert_eq!(statements.len(), 1, "{statements:?}");
        let StatementKindAst::Start {
            invalid_key,
            not_invalid_key,
            ..
        } = &statements[0].kind
        else {
            panic!("expected outer START statement");
        };
        assert_eq!(invalid_key.len(), 1, "{invalid_key:?}");
        let StatementKindAst::Start {
            invalid_key: inner_invalid_key,
            not_invalid_key: inner_not_invalid_key,
            ..
        } = &invalid_key[0].kind
        else {
            panic!("expected nested START in outer INVALID KEY branch");
        };
        assert!(
            matches!(
                inner_invalid_key.first().map(|statement| &statement.kind),
                Some(StatementKindAst::Display(values)) if values.as_slice() == ["\"INNER-BAD\""]
            ),
            "{inner_invalid_key:?}"
        );
        assert!(
            matches!(
                inner_not_invalid_key.first().map(|statement| &statement.kind),
                Some(StatementKindAst::Display(values)) if values.as_slice() == ["\"INNER-OK\""]
            ),
            "{inner_not_invalid_key:?}"
        );
        assert_eq!(not_invalid_key.len(), 1, "{not_invalid_key:?}");
        assert!(
            matches!(
                not_invalid_key.first().map(|statement| &statement.kind),
                Some(StatementKindAst::Display(values)) if values.as_slice() == ["\"OUTER-OK\""]
            ),
            "{not_invalid_key:?}"
        );
    }

    #[test]
    fn inspect_rejects_unconsumed_subject_tail() {
        for source in [
            "INSPECT WS-T GARBAGE TALLYING WS-C FOR ALL \"A\"",
            "INSPECT WS-T GARBAGE REPLACING ALL \"A\" BY \"B\"",
            "INSPECT WS-T GARBAGE CONVERTING \"A\" TO \"B\"",
        ] {
            let statement = parse_statement(source, SourceSpan::generated());
            assert!(
                matches!(
                    statement.kind,
                    StatementKindAst::Unsupported(ref keyword) if keyword == "INSPECT"
                ),
                "expected unsupported INSPECT for {source}, got {:?}",
                statement.kind
            );
        }
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
    fn call_using_rejects_empty_or_missing_arguments() {
        let empty = parse_statement("CALL \"SUB\" USING ,", SourceSpan::generated());
        assert!(matches!(
            empty.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "CALL"
        ));

        let missing_by_arg =
            parse_statement("CALL \"SUB\" USING BY REFERENCE", SourceSpan::generated());
        assert!(matches!(
            missing_by_arg.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "CALL"
        ));
    }

    #[test]
    fn call_using_rejects_exception_clause_markers_as_operands() {
        for source in [
            "CALL \"SUB\" USING WS-ARG ON EXCEPTION DISPLAY \"BAD\" END-CALL",
            "CALL \"SUB\" USING WS-ARG NOT ON EXCEPTION DISPLAY \"OK\" END-CALL",
        ] {
            let statement = parse_statement(source, SourceSpan::generated());
            assert!(
                matches!(
                    statement.kind,
                    StatementKindAst::Unsupported(ref keyword) if keyword == "CALL"
                ),
                "expected unsupported CALL for {source}, got {:?}",
                statement.kind
            );
        }
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
    fn perform_rejects_unconsumed_target_tail() {
        let valid_times = parse_statement("PERFORM CHECK-ROW 3 TIMES", SourceSpan::generated());
        assert!(matches!(
            valid_times.kind,
            StatementKindAst::Perform {
                ref target,
                times: Some(ref times),
                ..
            } if target == "CHECK_ROW" && times == "3"
        ));

        let invalid = parse_statement("PERFORM WS-TARGET GARBAGE", SourceSpan::generated());
        assert!(matches!(
            invalid.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "PERFORM"
        ));
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
    fn release_from_rejects_dangling_qualified_source() {
        let valid = parse_statement(
            "RELEASE SORT-REC FROM WS-SRC OF WS-GROUP",
            SourceSpan::generated(),
        );
        assert!(matches!(
            valid.kind,
            StatementKindAst::Release(ref release)
                if release.record == "SORT-REC"
                    && release.from.as_deref() == Some("WS-SRC OF WS-GROUP")
        ));

        let invalid = parse_statement("RELEASE SORT-REC FROM WS-SRC OF", SourceSpan::generated());
        assert!(matches!(
            invalid.kind,
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
    fn stop_literal_is_preserved() {
        let statement = parse_statement("STOP \"PAUSE\"", SourceSpan::generated());
        assert!(matches!(
            statement.kind,
            StatementKindAst::Stop(ref value) if value == "\"PAUSE\""
        ));
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
    fn move_to_rejects_unqualified_multiword_target() {
        let valid = parse_statement("MOVE 1 TO WS-N OF WS-GROUP", SourceSpan::generated());
        assert!(matches!(
            valid.kind,
            StatementKindAst::Move { ref target, .. } if target == "WS-N OF WS-GROUP"
        ));

        let invalid = parse_statement("MOVE 1 TO WS-N GARBAGE", SourceSpan::generated());
        assert!(matches!(
            invalid.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "MOVE"
        ));
    }

    #[test]
    fn move_to_rejects_unqualified_multiword_source() {
        let valid = parse_statement("MOVE WS-SRC OF WS-GROUP TO WS-N", SourceSpan::generated());
        assert!(matches!(
            valid.kind,
            StatementKindAst::Move { ref source, .. } if source == "WS-SRC OF WS-GROUP"
        ));

        let invalid = parse_statement("MOVE WS-SRC GARBAGE TO WS-N", SourceSpan::generated());
        assert!(matches!(
            invalid.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "MOVE"
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
        assert!(!compute.rounded);
        assert!(compute.on_size_error.is_empty());
        assert!(compute.not_on_size_error.is_empty());
    }

    #[test]
    fn add_rounded_target_lowers_to_compute_ast_without_merging_target_words() {
        let statement = parse_statement("ADD 1 TO WS-N ROUNDED", SourceSpan::generated());
        let StatementKindAst::Compute(compute) = statement.kind else {
            panic!("expected ADD ROUNDED to lower through COMPUTE AST");
        };

        assert_eq!(compute.target, "WS-N");
        assert_eq!(compute.expression, "WS-N + 1");
        assert!(compute.rounded);
        assert!(compute.on_size_error.is_empty());
        assert!(compute.not_on_size_error.is_empty());
    }

    #[test]
    fn add_to_rejects_unqualified_multiword_target() {
        let valid = parse_statement("ADD 1 TO WS-N OF WS-GROUP", SourceSpan::generated());
        assert!(matches!(
            valid.kind,
            StatementKindAst::Add { ref target, .. } if target == "WS-N OF WS-GROUP"
        ));

        let invalid = parse_statement("ADD 1 TO WS-N GARBAGE", SourceSpan::generated());
        assert!(matches!(
            invalid.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "ADD"
        ));
    }

    #[test]
    fn arithmetic_rejects_unqualified_multiword_operands() {
        let qualified_source =
            parse_statement("ADD WS-A OF WS-GROUP TO WS-N", SourceSpan::generated());
        assert!(matches!(
            qualified_source.kind,
            StatementKindAst::Add { ref source, .. } if source == "WS-A OF WS-GROUP"
        ));

        let invalid_source = parse_statement("ADD WS-A GARBAGE TO WS-N", SourceSpan::generated());
        assert!(matches!(
            invalid_source.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "ADD"
        ));

        let qualified_receiver = parse_statement(
            "SUBTRACT WS-A FROM WS-B OF WS-GROUP GIVING WS-C",
            SourceSpan::generated(),
        );
        assert!(matches!(
            qualified_receiver.kind,
            StatementKindAst::Compute(ref compute)
                if compute.expression == "WS-B OF WS-GROUP - WS-A"
        ));

        let invalid_receiver = parse_statement(
            "SUBTRACT WS-A FROM WS-B GARBAGE GIVING WS-C",
            SourceSpan::generated(),
        );
        assert!(matches!(
            invalid_receiver.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "SUBTRACT"
        ));
    }

    #[test]
    fn add_giving_rounded_lowers_to_compute_ast_with_metadata() {
        let statement = parse_statement(
            "ADD WS-A TO WS-B GIVING WS-C ROUNDED",
            SourceSpan::generated(),
        );
        let StatementKindAst::Compute(compute) = statement.kind else {
            panic!("expected ADD GIVING ROUNDED to lower through COMPUTE AST");
        };

        assert_eq!(compute.target, "WS-C");
        assert_eq!(compute.expression, "WS-A + WS-B");
        assert!(compute.rounded);
        assert!(compute.on_size_error.is_empty());
        assert!(compute.not_on_size_error.is_empty());
    }

    #[test]
    fn subtract_giving_lowers_to_compute_ast_without_merging_target_words() {
        let statement = parse_statement(
            "SUBTRACT WS-A FROM WS-B GIVING WS-C",
            SourceSpan::generated(),
        );
        let StatementKindAst::Compute(compute) = statement.kind else {
            panic!("expected SUBTRACT GIVING to lower through COMPUTE AST");
        };

        assert_eq!(compute.target, "WS-C");
        assert_eq!(compute.expression, "WS-B - WS-A");
        assert!(!compute.rounded);
        assert!(compute.on_size_error.is_empty());
        assert!(compute.not_on_size_error.is_empty());
    }

    #[test]
    fn multiply_giving_rounded_lowers_to_compute_ast_with_metadata() {
        let statement = parse_statement(
            "MULTIPLY WS-A BY WS-B GIVING WS-C ROUNDED",
            SourceSpan::generated(),
        );
        let StatementKindAst::Compute(compute) = statement.kind else {
            panic!("expected MULTIPLY GIVING ROUNDED to lower through COMPUTE AST");
        };

        assert_eq!(compute.target, "WS-C");
        assert_eq!(compute.expression, "WS-A * WS-B");
        assert!(compute.rounded);
        assert!(compute.on_size_error.is_empty());
        assert!(compute.not_on_size_error.is_empty());
    }

    #[test]
    fn divide_giving_size_error_lowers_to_compute_ast_with_branch() {
        let statement = parse_statement(
            "DIVIDE WS-D INTO WS-N GIVING WS-Q ON SIZE ERROR DISPLAY \"DIV\" END-DIVIDE",
            SourceSpan::generated(),
        );
        let StatementKindAst::Compute(compute) = statement.kind else {
            panic!("expected DIVIDE GIVING to lower through COMPUTE AST");
        };

        assert_eq!(compute.target, "WS-Q");
        assert_eq!(compute.expression, "WS-N / WS-D");
        assert!(!compute.rounded);
        assert_eq!(compute.on_size_error.len(), 1);
        assert!(matches!(
            compute.on_size_error[0].kind,
            StatementKindAst::Display(ref values) if values.as_slice() == ["\"DIV\""]
        ));
        assert!(compute.not_on_size_error.is_empty());
    }

    #[test]
    fn add_size_error_statement_lowers_to_compute_ast_with_branches() {
        let statement = parse_statement(
            "ADD 1 TO WS-N ON SIZE ERROR DISPLAY \"SIZE\" NOT ON SIZE ERROR DISPLAY \"OK\" END-ADD",
            SourceSpan::generated(),
        );
        let StatementKindAst::Compute(compute) = statement.kind else {
            panic!("expected ADD with SIZE ERROR to lower through COMPUTE AST");
        };

        assert_eq!(compute.target, "WS-N");
        assert_eq!(compute.expression, "WS-N + 1");
        assert_eq!(compute.on_size_error.len(), 1);
        assert_eq!(compute.not_on_size_error.len(), 1);
        assert!(matches!(
            compute.on_size_error[0].kind,
            StatementKindAst::Display(ref values) if values.as_slice() == ["\"SIZE\""]
        ));
        assert!(matches!(
            compute.not_on_size_error[0].kind,
            StatementKindAst::Display(ref values) if values.as_slice() == ["\"OK\""]
        ));
    }

    #[test]
    fn divide_size_error_statement_lowers_to_compute_ast() {
        let statement = parse_statement(
            "DIVIDE WS-D INTO WS-N ON SIZE ERROR DISPLAY \"DIV\" END-DIVIDE",
            SourceSpan::generated(),
        );
        let StatementKindAst::Compute(compute) = statement.kind else {
            panic!("expected DIVIDE with SIZE ERROR to lower through COMPUTE AST");
        };

        assert_eq!(compute.target, "WS-N");
        assert_eq!(compute.expression, "WS-N / WS-D");
        assert_eq!(compute.on_size_error.len(), 1);
        assert!(compute.not_on_size_error.is_empty());
    }

    #[test]
    fn size_error_branches_require_body_before_next_branch_or_scope() {
        let compute = parse_statement(
            "COMPUTE WS-N = 1 ON SIZE ERROR NOT ON SIZE ERROR DISPLAY \"OK\" END-COMPUTE",
            SourceSpan::generated(),
        );
        assert!(matches!(
            compute.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "COMPUTE"
        ));

        let add = parse_statement(
            "ADD 1 TO WS-N ON SIZE ERROR NOT ON SIZE ERROR DISPLAY \"OK\" END-ADD",
            SourceSpan::generated(),
        );
        assert!(matches!(
            add.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "ADD"
        ));
    }

    #[test]
    fn compute_rounded_target_sets_metadata_without_merging_target_words() {
        let statement = parse_statement("COMPUTE N ROUNDED = N + 1", SourceSpan::generated());
        let StatementKindAst::Compute(compute) = statement.kind else {
            panic!("expected COMPUTE");
        };

        assert_eq!(compute.target, "N");
        assert_eq!(compute.expression, "N + 1");
        assert!(compute.rounded);
    }

    #[test]
    fn compute_misplaced_rounded_remains_unsupported() {
        let statement = parse_statement("COMPUTE ROUNDED N = N + 1", SourceSpan::generated());
        assert!(matches!(
            statement.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "COMPUTE"
        ));
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
    fn compute_target_preserves_qualified_subscript_syntax_for_sema() {
        let statement =
            parse_statement("COMPUTE WS-ITEM(2) OF WS-ROW = 4", SourceSpan::generated());
        let StatementKindAst::Compute(compute) = statement.kind else {
            panic!("expected COMPUTE");
        };

        assert_eq!(compute.target, "WS-ITEM(2) OF WS-ROW");
        assert_eq!(compute.expression, "4");
    }

    #[test]
    fn compute_unqualified_multiword_target_remains_unsupported() {
        let statement = parse_statement("COMPUTE A B = 1", SourceSpan::generated());
        assert!(matches!(
            statement.kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "COMPUTE"
        ));
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
    fn write_invalid_key_branch_preserves_typed_body() {
        let statement = parse_statement(
            "WRITE OUT-REC INVALID KEY DISPLAY \"BAD\" END-WRITE",
            SourceSpan::generated(),
        );
        let StatementKindAst::Write(write) = &statement.kind else {
            panic!("expected typed WRITE AST");
        };
        assert_eq!(write.record, "OUT-REC");
        assert_eq!(write.branch_phrases, vec!["INVALID KEY".to_string()]);
        assert_eq!(write.invalid_key.len(), 1);
        assert!(matches!(
            write.invalid_key[0].kind,
            StatementKindAst::Display(_)
        ));
    }

    #[test]
    fn write_on_exception_branch_preserves_typed_body() {
        let statement = parse_statement(
            "WRITE OUT-REC ON EXCEPTION DISPLAY \"BAD\" END-WRITE",
            SourceSpan::generated(),
        );
        let StatementKindAst::Write(write) = &statement.kind else {
            panic!("expected typed WRITE AST");
        };
        assert_eq!(write.record, "OUT-REC");
        assert_eq!(write.branch_phrases, vec!["ON EXCEPTION".to_string()]);
        assert_eq!(write.on_exception.len(), 1);
        assert!(matches!(
            write.on_exception[0].kind,
            StatementKindAst::Display(_)
        ));
    }

    #[test]
    fn write_with_unconsumed_prefix_tokens_fails_closed_as_one_slice() {
        let statements = parse_imperative_list(
            "WRITE OUT-REC GARBAGE INVALID KEY DISPLAY \"BAD\" END-WRITE DISPLAY \"AFTER\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 2, "{statements:?}");
        assert!(
            matches!(statements[0].kind, StatementKindAst::Unsupported(ref keyword) if keyword == "WRITE"),
            "{statements:?}"
        );
        assert_eq!(
            statements[0].raw,
            "WRITE OUT-REC GARBAGE INVALID KEY DISPLAY \"BAD\" END-WRITE"
        );
        assert!(matches!(statements[1].kind, StatementKindAst::Display(_)));
    }

    #[test]
    fn write_branches_require_body_before_next_branch_or_scope() {
        for source in [
            "WRITE OUT-REC INVALID KEY NOT INVALID KEY DISPLAY \"OK\" END-WRITE DISPLAY \"AFTER\"",
            "WRITE OUT-REC ON EXCEPTION END-WRITE DISPLAY \"AFTER\"",
        ] {
            let statements = parse_imperative_list(source, SourceSpan::generated());
            assert_eq!(statements.len(), 2, "{source}: {statements:?}");
            assert!(
                matches!(statements[0].kind, StatementKindAst::Unsupported(ref keyword) if keyword == "WRITE"),
                "{source}: {statements:?}"
            );
            assert!(matches!(statements[1].kind, StatementKindAst::Display(_)));
        }
    }

    #[test]
    fn rewrite_with_unconsumed_prefix_tokens_fails_closed_as_one_slice() {
        let statements = parse_imperative_list(
            "REWRITE OUT-REC GARBAGE INVALID KEY DISPLAY \"BAD\" END-REWRITE DISPLAY \"AFTER\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 2, "{statements:?}");
        assert!(matches!(
            statements[0].kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "REWRITE"
        ));
        assert_eq!(
            statements[0].raw,
            "REWRITE OUT-REC GARBAGE INVALID KEY DISPLAY \"BAD\" END-REWRITE"
        );
        assert!(matches!(statements[1].kind, StatementKindAst::Display(_)));
    }

    #[test]
    fn invalid_key_branches_require_body_before_next_branch_or_scope() {
        for (source, keyword) in [
            (
                "REWRITE OUT-REC INVALID KEY NOT INVALID KEY DISPLAY \"OK\" END-REWRITE DISPLAY \"AFTER\"",
                "REWRITE",
            ),
            (
                "DELETE INFILE NOT INVALID KEY END-DELETE DISPLAY \"AFTER\"",
                "DELETE",
            ),
            (
                "START INFILE KEY IS EQUAL TO WS-ID INVALID KEY END-START DISPLAY \"AFTER\"",
                "START",
            ),
        ] {
            let statements = parse_imperative_list(source, SourceSpan::generated());
            assert_eq!(statements.len(), 2, "{source}: {statements:?}");
            assert!(
                matches!(statements[0].kind, StatementKindAst::Unsupported(ref actual) if actual == keyword),
                "{source}: {statements:?}"
            );
            assert!(matches!(statements[1].kind, StatementKindAst::Display(_)));
        }
    }

    #[test]
    fn delete_with_unconsumed_prefix_tokens_fails_closed_as_one_slice() {
        let statements = parse_imperative_list(
            "DELETE INFILE GARBAGE INVALID KEY DISPLAY \"BAD\" END-DELETE DISPLAY \"AFTER\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 2, "{statements:?}");
        assert!(matches!(
            statements[0].kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "DELETE"
        ));
        assert_eq!(
            statements[0].raw,
            "DELETE INFILE GARBAGE INVALID KEY DISPLAY \"BAD\" END-DELETE"
        );
        assert!(matches!(statements[1].kind, StatementKindAst::Display(_)));
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
    fn read_with_unconsumed_prefix_tokens_fails_closed_as_one_slice() {
        let statements = parse_imperative_list(
            "READ INFILE GARBAGE AT END DISPLAY \"EOF\" END-READ DISPLAY \"AFTER\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 2, "{statements:?}");
        assert!(matches!(
            statements[0].kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "READ"
        ));
        assert_eq!(
            statements[0].raw,
            "READ INFILE GARBAGE AT END DISPLAY \"EOF\" END-READ"
        );
        assert!(matches!(statements[1].kind, StatementKindAst::Display(_)));
    }

    #[test]
    fn read_and_return_at_end_branches_require_body_before_next_branch_or_scope() {
        let read_statements = parse_imperative_list(
            "READ INFILE AT END NOT AT END DISPLAY \"OK\" END-READ DISPLAY \"AFTER\"",
            SourceSpan::generated(),
        );
        assert_eq!(read_statements.len(), 2, "{read_statements:?}");
        assert!(matches!(
            read_statements[0].kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "READ"
        ));
        assert_eq!(
            read_statements[0].raw,
            "READ INFILE AT END NOT AT END DISPLAY \"OK\" END-READ"
        );
        assert!(matches!(
            read_statements[1].kind,
            StatementKindAst::Display(_)
        ));

        let return_statements = parse_imperative_list(
            "RETURN SORT-FILE AT END NOT AT END DISPLAY \"OK\" END-RETURN DISPLAY \"AFTER\"",
            SourceSpan::generated(),
        );
        assert_eq!(return_statements.len(), 2, "{return_statements:?}");
        assert!(matches!(
            return_statements[0].kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "RETURN"
        ));
        assert_eq!(
            return_statements[0].raw,
            "RETURN SORT-FILE AT END NOT AT END DISPLAY \"OK\" END-RETURN"
        );
        assert!(matches!(
            return_statements[1].kind,
            StatementKindAst::Display(_)
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
    fn return_with_unconsumed_prefix_tokens_fails_closed_as_one_slice() {
        let statements = parse_imperative_list(
            "RETURN SORT-FILE GARBAGE AT END DISPLAY \"END\" END-RETURN DISPLAY \"AFTER\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 2, "{statements:?}");
        assert!(matches!(
            statements[0].kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "RETURN"
        ));
        assert_eq!(
            statements[0].raw,
            "RETURN SORT-FILE GARBAGE AT END DISPLAY \"END\" END-RETURN"
        );
        assert!(matches!(statements[1].kind, StatementKindAst::Display(_)));
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
    fn string_with_unconsumed_option_tokens_fails_closed_as_one_slice() {
        let statements = parse_imperative_list(
            "STRING \"A\" DELIMITED BY SIZE INTO WS-TEXT GARBAGE ON OVERFLOW DISPLAY \"BAD\" END-STRING DISPLAY \"AFTER\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 2, "{statements:?}");
        assert!(matches!(
            statements[0].kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "STRING"
        ));
        assert_eq!(
            statements[0].raw,
            "STRING \"A\" DELIMITED BY SIZE INTO WS-TEXT GARBAGE ON OVERFLOW DISPLAY \"BAD\" END-STRING"
        );
        assert!(matches!(statements[1].kind, StatementKindAst::Display(_)));
    }

    #[test]
    fn unstring_with_unconsumed_option_tokens_fails_closed_as_one_slice() {
        let statements = parse_imperative_list(
            "UNSTRING WS-SRC DELIMITED BY SPACE INTO WS-A WITH GARBAGE ON OVERFLOW DISPLAY \"BAD\" END-UNSTRING DISPLAY \"AFTER\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 2, "{statements:?}");
        assert!(matches!(
            statements[0].kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "UNSTRING"
        ));
        assert_eq!(
            statements[0].raw,
            "UNSTRING WS-SRC DELIMITED BY SPACE INTO WS-A WITH GARBAGE ON OVERFLOW DISPLAY \"BAD\" END-UNSTRING"
        );
        assert!(matches!(statements[1].kind, StatementKindAst::Display(_)));
    }

    #[test]
    fn string_and_unstring_overflow_branches_require_body_before_next_branch_or_scope() {
        let string_statements = parse_imperative_list(
            "STRING \"A\" DELIMITED BY SIZE INTO WS-TEXT ON OVERFLOW NOT ON OVERFLOW DISPLAY \"OK\" END-STRING DISPLAY \"AFTER\"",
            SourceSpan::generated(),
        );
        assert_eq!(string_statements.len(), 2, "{string_statements:?}");
        assert!(matches!(
            string_statements[0].kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "STRING"
        ));
        assert_eq!(
            string_statements[0].raw,
            "STRING \"A\" DELIMITED BY SIZE INTO WS-TEXT ON OVERFLOW NOT ON OVERFLOW DISPLAY \"OK\" END-STRING"
        );
        assert!(matches!(
            string_statements[1].kind,
            StatementKindAst::Display(_)
        ));

        let unstring_statements = parse_imperative_list(
            "UNSTRING WS-SRC DELIMITED BY SPACE INTO WS-A ON OVERFLOW NOT ON OVERFLOW DISPLAY \"OK\" END-UNSTRING DISPLAY \"AFTER\"",
            SourceSpan::generated(),
        );
        assert_eq!(unstring_statements.len(), 2, "{unstring_statements:?}");
        assert!(matches!(
            unstring_statements[0].kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "UNSTRING"
        ));
        assert_eq!(
            unstring_statements[0].raw,
            "UNSTRING WS-SRC DELIMITED BY SPACE INTO WS-A ON OVERFLOW NOT ON OVERFLOW DISPLAY \"OK\" END-UNSTRING"
        );
        assert!(matches!(
            unstring_statements[1].kind,
            StatementKindAst::Display(_)
        ));
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
        assert!(matches!(
            statement.kind,
            StatementKindAst::BlockedNextSentence
        ));
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
    fn goback_statement_is_distinct_from_stop_run() {
        let goback = parse_statement("GOBACK", SourceSpan::generated());
        assert!(matches!(goback.kind, StatementKindAst::Goback));

        let stop_run = parse_statement("STOP RUN", SourceSpan::generated());
        assert!(matches!(stop_run.kind, StatementKindAst::StopRun));
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
    fn syntax_sentence_periods_delegate_to_cobol_text_boundaries() {
        let source = "REPLACE ==END== BY ==. ==.\nDISPLAY \"A.B\".\nDISPLAY 1.23.\nSTOP RUN.";
        let helper_periods = cobol_text::literal_aware_char_indices(source)
            .filter(|item| item.ch == '.')
            .map(|item| {
                (
                    item.byte_idx,
                    cobol_text::is_sentence_period_outside_literals(source, item.byte_idx),
                )
            })
            .collect::<Vec<_>>();
        assert!(
            helper_periods.iter().any(|(_, sentence)| !sentence),
            "fixture must include non-sentence periods"
        );

        let sentences = split_sentences(source, "periods.cbl");
        assert_eq!(
            sentences
                .iter()
                .map(|sentence| sentence.raw.as_str())
                .collect::<Vec<_>>(),
            vec![
                "REPLACE ==END== BY ==. ==",
                "DISPLAY \"A.B\"",
                "DISPLAY 1.23",
                "STOP RUN",
            ]
        );
    }

    #[test]
    fn imperative_token_slice_consumption_keeps_branch_statements_separate() {
        let statements = parse_imperative_list(
            "IF WS-FLAG = \"Y\" DISPLAY \"A\" ELSE DISPLAY \"B\" END-IF DISPLAY \"C\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 2, "{statements:?}");
        let StatementKindAst::If {
            then_statements,
            else_statements,
            ..
        } = &statements[0].kind
        else {
            panic!("expected IF statement: {statements:?}");
        };
        assert_eq!(then_statements.len(), 1);
        assert_eq!(else_statements.len(), 1);
        assert_eq!(then_statements[0].raw, "DISPLAY \"A\"");
        assert_eq!(else_statements[0].raw, "DISPLAY \"B\"");
        assert_eq!(statements[1].raw, "DISPLAY \"C\"");
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
    fn evaluate_without_subject_fails_closed_as_one_slice() {
        let statements = parse_imperative_list(
            "EVALUATE WHEN \"Y\" DISPLAY \"A\" END-EVALUATE DISPLAY \"AFTER\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 2, "{statements:?}");
        assert!(matches!(
            statements[0].kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "EVALUATE"
        ));
        assert_eq!(
            statements[0].raw,
            "EVALUATE WHEN \"Y\" DISPLAY \"A\" END-EVALUATE"
        );
        assert!(matches!(statements[1].kind, StatementKindAst::Display(_)));
    }

    #[test]
    fn evaluate_when_without_pattern_fails_closed_as_one_slice() {
        let statements = parse_imperative_list(
            "EVALUATE WS-FLAG WHEN DISPLAY \"A\" END-EVALUATE DISPLAY \"AFTER\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 2, "{statements:?}");
        assert!(matches!(
            statements[0].kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "EVALUATE"
        ));
        assert_eq!(
            statements[0].raw,
            "EVALUATE WS-FLAG WHEN DISPLAY \"A\" END-EVALUATE"
        );
        assert!(matches!(statements[1].kind, StatementKindAst::Display(_)));
    }

    #[test]
    fn evaluate_also_patterns_must_match_subject_count() {
        let statements = parse_imperative_list(
            "EVALUATE WS-A ALSO WS-B WHEN \"Y\" DISPLAY \"A\" END-EVALUATE DISPLAY \"AFTER\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 2, "{statements:?}");
        assert!(matches!(
            statements[0].kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "EVALUATE"
        ));
        assert_eq!(
            statements[0].raw,
            "EVALUATE WS-A ALSO WS-B WHEN \"Y\" DISPLAY \"A\" END-EVALUATE"
        );
        assert!(matches!(statements[1].kind, StatementKindAst::Display(_)));
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
    fn search_with_unconsumed_prefix_tokens_fails_closed_as_one_slice() {
        let statements = parse_imperative_list(
            "SEARCH WS-ITEM GARBAGE WHEN WS-ITEM = \"B\" DISPLAY \"FOUND\" END-SEARCH DISPLAY \"AFTER\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 2, "{statements:?}");
        assert!(matches!(
            statements[0].kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "SEARCH"
        ));
        assert_eq!(
            statements[0].raw,
            "SEARCH WS-ITEM GARBAGE WHEN WS-ITEM = \"B\" DISPLAY \"FOUND\" END-SEARCH"
        );
        assert!(matches!(statements[1].kind, StatementKindAst::Display(_)));
    }

    #[test]
    fn search_at_end_requires_branch_body_before_when() {
        let statements = parse_imperative_list(
            "SEARCH WS-ITEM AT END WHEN WS-ITEM = \"B\" DISPLAY \"FOUND\" END-SEARCH DISPLAY \"AFTER\"",
            SourceSpan::generated(),
        );
        assert_eq!(statements.len(), 2, "{statements:?}");
        assert!(matches!(
            statements[0].kind,
            StatementKindAst::Unsupported(ref keyword) if keyword == "SEARCH"
        ));
        assert_eq!(
            statements[0].raw,
            "SEARCH WS-ITEM AT END WHEN WS-ITEM = \"B\" DISPLAY \"FOUND\" END-SEARCH"
        );
        assert!(matches!(statements[1].kind, StatementKindAst::Display(_)));
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

        let object_reference = parse_data_decl(
            "01 WS-OBJ USAGE IS OBJECT REFERENCE.",
            SourceSpan::generated(),
            StorageAreaAst::WorkingStorage,
        )
        .expect("object reference data decl");
        assert!(object_reference.clause_ast.iter().any(
            |clause| matches!(clause, DataClauseAst::Usage(usage) if usage == "OBJECT REFERENCE")
        ));
        assert!(!object_reference
            .clause_ast
            .iter()
            .any(|clause| matches!(clause, DataClauseAst::Other(value) if value.eq_ignore_ascii_case("REFERENCE"))));

        let computational_packed = parse_data_decl(
            "05 WS-PACKED PIC S9(5) COMPUTATIONAL-3.",
            SourceSpan::generated(),
            StorageAreaAst::WorkingStorage,
        )
        .expect("computational packed data decl");
        assert!(computational_packed.clause_ast.iter().any(
            |clause| matches!(clause, DataClauseAst::Usage(usage) if usage == "COMPUTATIONAL-3")
        ));

        for (raw, expected) in [
            ("05 WS-COMP-X PIC X(4) COMP-X.", "COMP-X"),
            ("05 WS-COMP-N PIC X(4) COMP-N.", "COMP-N"),
            ("05 WS-COMP6 PIC S9(9) COMP-6.", "COMP-6"),
            ("05 WS-COMPX PIC X(4) COMPUTATIONAL-X.", "COMPUTATIONAL-X"),
            ("05 WS-COMPN PIC X(4) COMPUTATIONAL-N.", "COMPUTATIONAL-N"),
            (
                "05 WS-COMP6-LONG PIC S9(9) COMPUTATIONAL-6.",
                "COMPUTATIONAL-6",
            ),
            ("05 WS-BIN-CHAR PIC S9(2) BINARY-CHAR.", "BINARY-CHAR"),
            ("05 WS-BIN-SHORT PIC S9(4) BINARY-SHORT.", "BINARY-SHORT"),
            ("05 WS-BIN-LONG PIC S9(9) BINARY-LONG.", "BINARY-LONG"),
            (
                "05 WS-BIN-DOUBLE PIC S9(18) BINARY-DOUBLE.",
                "BINARY-DOUBLE",
            ),
        ] {
            let item =
                parse_data_decl(raw, SourceSpan::generated(), StorageAreaAst::WorkingStorage)
                    .expect("dialect binary usage data decl");
            assert!(
                item.clause_ast.iter().any(
                    |clause| matches!(clause, DataClauseAst::Usage(usage) if usage == expected)
                ),
                "{expected} should be captured as a usage clause: {:?}",
                item.clause_ast
            );
        }
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

        let sync_right = parse_data_decl(
            "05 WS-BIN PIC 9(4) SYNCHRONIZED RIGHT.",
            SourceSpan::generated(),
            StorageAreaAst::WorkingStorage,
        )
        .expect("synchronized right data decl");
        assert!(sync_right
            .clause_ast
            .iter()
            .any(|clause| matches!(clause, DataClauseAst::Sync)));
        assert!(!sync_right
            .clause_ast
            .iter()
            .any(|clause| matches!(clause, DataClauseAst::Other(value) if value.trim_end_matches('.').eq_ignore_ascii_case("RIGHT"))));

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

        for source in [
            "05 WS-BLANKS PIC 9(4) BLANK WHEN ZEROS.",
            "05 WS-BLANKZ PIC 9(4) BLANK WHEN ZEROES.",
        ] {
            let blank_alias = parse_data_decl(
                source,
                SourceSpan::generated(),
                StorageAreaAst::WorkingStorage,
            )
            .expect("blank when zero alias data decl");
            assert!(blank_alias
                .clause_ast
                .iter()
                .any(|clause| { matches!(clause, DataClauseAst::BlankWhenZero) }));
        }

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

        let typedef = parse_data_decl(
            "01 CUSTOMER-TYPE TYPEDEF STRONG.",
            SourceSpan::generated(),
            StorageAreaAst::WorkingStorage,
        )
        .expect("typedef data decl");
        assert!(typedef
            .clause_ast
            .iter()
            .any(|clause| { matches!(clause, DataClauseAst::TypeDef { strong: true }) }));

        let typed = parse_data_decl(
            "01 CUSTOMER-REC TYPE CUSTOMER-TYPE.",
            SourceSpan::generated(),
            StorageAreaAst::WorkingStorage,
        )
        .expect("type data decl");
        assert!(typed.clause_ast.iter().any(|clause| {
            matches!(clause, DataClauseAst::TypeOf { name } if name == "CUSTOMER_TYPE")
        }));

        let same_as = parse_data_decl(
            "01 CUSTOMER-COPY SAME AS CUSTOMER-REC.",
            SourceSpan::generated(),
            StorageAreaAst::WorkingStorage,
        )
        .expect("same as data decl");
        assert!(same_as.clause_ast.iter().any(|clause| {
            matches!(clause, DataClauseAst::SameAs { name } if name == "CUSTOMER_REC")
        }));

        let group_usage = parse_data_decl(
            "01 NATIONAL-GROUP GROUP-USAGE IS NATIONAL.",
            SourceSpan::generated(),
            StorageAreaAst::WorkingStorage,
        )
        .expect("group usage data decl");
        assert!(group_usage.clause_ast.iter().any(|clause| {
            matches!(clause, DataClauseAst::GroupUsage(usage) if usage == "NATIONAL")
        }));

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
    fn data_value_clause_preserves_all_literal_phrase() {
        let item = parse_data_decl(
            "05 WS-FILL PIC X(4) VALUE ALL \"X\".",
            SourceSpan::generated(),
            StorageAreaAst::WorkingStorage,
        )
        .expect("data decl");
        assert!(item
            .clause_ast
            .iter()
            .any(|clause| matches!(clause, DataClauseAst::Value(value) if value == "ALL X")));
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
