use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFormat {
    Fixed,
    Free,
    Auto,
}

impl SourceFormat {
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "fixed" => Some(Self::Fixed),
            "free" => Some(Self::Free),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Ibm,
    GnuCobol,
    MicroFocus,
}

impl Dialect {
    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "ibm" | "zos" | "z-os" => Some(Self::Ibm),
            "gnu" | "gnucobol" | "gnu-cobol" => Some(Self::GnuCobol),
            "mf" | "microfocus" | "micro-focus" => Some(Self::MicroFocus),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludeTrace {
    pub copybook: String,
    pub resolved_path: PathBuf,
    pub depth: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreprocessedSource {
    pub primary_path: PathBuf,
    pub text: String,
    pub format: SourceFormat,
    pub includes: Vec<IncludeTrace>,
}

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("COPY {copybook} could not be resolved in configured copybook directories")]
    CopybookNotFound { copybook: String },
    #[error("COPY {copybook} matched multiple copybooks: {candidates:?}")]
    AmbiguousCopybook {
        copybook: String,
        candidates: Vec<PathBuf>,
    },
    #[error("COPY recursion exceeded maximum depth {max_depth} at {copybook}")]
    CopyDepthExceeded { copybook: String, max_depth: usize },
    #[error("recursive COPY detected at {copybook}")]
    RecursiveCopy { copybook: String },
    #[error("malformed COPY statement: {statement}")]
    MalformedCopyStatement { statement: String },
    #[error("COPY {copybook} has malformed REPLACING text; expected replacement operands separated by BY")]
    MalformedCopyReplacing { copybook: String },
    #[error("COPY {copybook} uses unsupported clause {clause}")]
    UnsupportedCopyClause { copybook: String, clause: String },
    #[error("malformed REPLACE directive: {statement}")]
    MalformedReplaceDirective { statement: String },
}

pub fn preprocess_file(
    input: &Path,
    copybook_dirs: &[PathBuf],
    requested_format: SourceFormat,
) -> Result<PreprocessedSource, SourceError> {
    let raw = read_to_string(input)?;
    let format = detect_format(&raw, requested_format);
    let normalized = normalize_source(&raw, format);
    let mut includes = Vec::new();
    let mut stack = HashSet::new();
    let expanded = expand_copybooks(
        &normalized,
        input.parent(),
        copybook_dirs,
        format,
        0,
        &mut includes,
        &mut stack,
    )?;
    Ok(PreprocessedSource {
        primary_path: input.to_path_buf(),
        text: expanded,
        format,
        includes,
    })
}

pub fn normalize_source(raw: &str, format: SourceFormat) -> String {
    match detect_format(raw, format) {
        SourceFormat::Fixed => normalize_fixed(raw),
        SourceFormat::Free | SourceFormat::Auto => normalize_free(raw),
    }
}

fn detect_format(raw: &str, requested: SourceFormat) -> SourceFormat {
    if requested != SourceFormat::Auto {
        return requested;
    }
    for line in raw.lines() {
        if let Some(format) = source_format_directive(line) {
            return format;
        }
    }
    if raw.lines().any(looks_like_fixed_indicator_format) {
        return SourceFormat::Fixed;
    }
    if raw.lines().any(looks_like_plain_free_format) {
        SourceFormat::Free
    } else {
        SourceFormat::Fixed
    }
}

fn source_format_directive(line: &str) -> Option<SourceFormat> {
    let directive_text = line_directive_text(line)?;
    let words = cobol_text::split_cobol_words(directive_text);
    if words
        .first()
        .is_some_and(|word| word.eq_ignore_ascii_case(">>SOURCE"))
        && words
            .get(1)
            .is_some_and(|word| word.eq_ignore_ascii_case("FORMAT"))
    {
        let value_idx = if words
            .get(2)
            .is_some_and(|word| word.eq_ignore_ascii_case("IS"))
        {
            3
        } else {
            2
        };
        if words.get(value_idx + 1).is_some() {
            return None;
        }
        let value = words.get(value_idx)?;
        let value = value.strip_suffix('.').unwrap_or(value);
        match value {
            value if value.eq_ignore_ascii_case("FREE") => Some(SourceFormat::Free),
            value if value.eq_ignore_ascii_case("FIXED") => Some(SourceFormat::Fixed),
            _ => None,
        }
    } else {
        None
    }
}

fn line_directive_text(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    if matches!(bytes.get(6).copied(), Some(b'*' | b'/' | b'D' | b'd')) {
        return None;
    }
    let leading = line.trim_start();
    if leading.starts_with("*>") {
        return None;
    }
    if leading.starts_with(">>") {
        let uncommented = strip_source_inline_comment(leading).trim_start();
        return (!uncommented.is_empty()).then_some(uncommented);
    }
    let body = if bytes.len() > 7
        && bytes[..6]
            .iter()
            .all(|byte| byte.is_ascii_digit() || *byte == b' ')
    {
        line.get(7..).unwrap_or(line)
    } else {
        line
    };
    let leading = body.trim_start();
    if leading.starts_with("*>") {
        None
    } else {
        let uncommented = strip_source_inline_comment(body).trim_start();
        (!uncommented.is_empty()).then_some(uncommented)
    }
}

fn looks_like_fixed_indicator_format(line: &str) -> bool {
    let bytes = line.as_bytes();
    if bytes.len() <= 6 {
        return false;
    }
    let sequence_area = &bytes[..6];
    if !sequence_area
        .iter()
        .all(|byte| byte.is_ascii_digit() || *byte == b' ')
    {
        return false;
    }
    let indicator = bytes[6];
    if !matches!(indicator, b'*' | b'/' | b'-' | b'D' | b'd') {
        return false;
    }
    if matches!(indicator, b'D' | b'd')
        && !bytes.get(7).is_some_and(|byte| byte.is_ascii_whitespace())
    {
        return false;
    }
    true
}

fn looks_like_plain_free_format(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with("*>") {
        return false;
    }
    trimmed
        .as_bytes()
        .first()
        .is_some_and(|byte| byte.is_ascii_alphabetic())
}

fn normalize_fixed(raw: &str) -> String {
    let mut out = String::new();
    for line in raw.lines() {
        if line.trim_start().starts_with("*>") {
            continue;
        }
        let indicator = line.as_bytes().get(6).copied().unwrap_or(b' ');
        if matches!(indicator, b'*' | b'/' | b'D' | b'd') {
            continue;
        }
        let area = fixed_source_area(line);
        if indicator == b'-' {
            if out.ends_with('\n') {
                out.pop();
            }
            let active_quote = unclosed_literal_quote_on_current_line(&out);
            let trimmed =
                strip_source_inline_comment_with_initial_quote(area, active_quote).trim_end();
            let continuation = trimmed.trim_start();
            if continuation.is_empty() {
                out.push('\n');
                continue;
            } else if let Some(quote) =
                active_quote.filter(|quote| continuation.starts_with(*quote))
            {
                out.push_str(&continuation[quote.len_utf8()..]);
            } else {
                if !fixed_continuation_joins_hyphenated_word(&out) {
                    out.push(' ');
                }
                out.push_str(continuation);
            }
            out.push('\n');
        } else {
            let trimmed = strip_source_inline_comment(area).trim_end();
            if !trimmed.trim().is_empty() {
                out.push_str(trimmed.trim_start());
                out.push('\n');
            }
        }
    }
    out
}

fn fixed_continuation_joins_hyphenated_word(text: &str) -> bool {
    let Some(before_hyphen) = text.strip_suffix('-') else {
        return false;
    };
    if before_hyphen
        .chars()
        .next_back()
        .is_some_and(char::is_whitespace)
    {
        return false;
    }
    let stem = before_hyphen.trim_end();
    let mut token_start = stem.len();
    for (idx, ch) in stem.char_indices().rev() {
        if !is_cobol_name_char(Some(ch)) {
            break;
        }
        token_start = idx;
    }
    let token = &stem[token_start..];
    !token.is_empty() && token.chars().any(|ch| ch.is_ascii_alphabetic())
}

fn fixed_source_area(line: &str) -> &str {
    const AREA_START: usize = 7;
    const AREA_END: usize = 72;
    if line.len() <= AREA_START || !line.is_char_boundary(AREA_START) {
        return "";
    }
    let mut end = AREA_END.min(line.len());
    while end > AREA_START && !line.is_char_boundary(end) {
        end -= 1;
    }
    line.get(AREA_START..end).unwrap_or("")
}

fn unclosed_literal_quote_on_current_line(text: &str) -> Option<char> {
    let line = text.rsplit_once('\n').map(|(_, line)| line).unwrap_or(text);
    let mut chars = line.chars().peekable();
    let mut quote = None;
    while let Some(ch) = chars.next() {
        if let Some(active_quote) = quote {
            if ch == active_quote {
                if chars.peek().is_some_and(|next| *next == ch) {
                    chars.next();
                } else {
                    quote = None;
                }
            }
        } else if matches!(ch, '\'' | '"') {
            quote = Some(ch);
        }
    }
    quote
}

fn normalize_free(raw: &str) -> String {
    let mut out = String::new();
    for line in raw.lines() {
        let trimmed = strip_source_inline_comment(line).trim();
        if trimmed.starts_with("*>") || trimmed.is_empty() {
            continue;
        }
        out.push_str(trimmed);
        out.push('\n');
    }
    out
}

fn expand_copybooks(
    text: &str,
    primary_dir: Option<&Path>,
    copybook_dirs: &[PathBuf],
    format: SourceFormat,
    depth: usize,
    includes: &mut Vec<IncludeTrace>,
    stack: &mut HashSet<String>,
) -> Result<String, SourceError> {
    let mut active_replacements = Vec::new();
    let mut context = CopyExpansionContext {
        copybook_dirs,
        format,
        includes,
        stack,
    };
    expand_copybooks_inner(
        text,
        primary_dir,
        depth,
        &mut active_replacements,
        &mut context,
    )
}

struct CopyExpansionContext<'a> {
    copybook_dirs: &'a [PathBuf],
    format: SourceFormat,
    includes: &'a mut Vec<IncludeTrace>,
    stack: &'a mut HashSet<String>,
}

fn expand_copybooks_inner(
    text: &str,
    primary_dir: Option<&Path>,
    depth: usize,
    active_replacements: &mut Vec<CopyReplacement>,
    context: &mut CopyExpansionContext<'_>,
) -> Result<String, SourceError> {
    const MAX_COPY_DEPTH: usize = 10;
    let mut out = String::new();
    let lines = text.lines().collect::<Vec<_>>();
    let mut idx = 0usize;
    while idx < lines.len() {
        let line = lines[idx];
        if is_replace_directive_start(line) {
            let statement = collect_directive_source(line, lines.as_slice(), &mut idx);
            let Some(statement_end) = copy_statement_end(&statement) else {
                return Err(SourceError::MalformedReplaceDirective { statement });
            };
            let directive = statement[..statement_end].to_string();
            let trailing_source = statement[statement_end..].trim_start().to_string();
            match parse_replace_directive(&directive) {
                Some(ReplaceDirective::Set(replacements)) => *active_replacements = replacements,
                Some(ReplaceDirective::Off) => active_replacements.clear(),
                None => {
                    return Err(SourceError::MalformedReplaceDirective {
                        statement: directive,
                    })
                }
            }
            if !trailing_source.is_empty() {
                let trailing = expand_copybooks_inner(
                    &trailing_source,
                    primary_dir,
                    depth,
                    active_replacements,
                    context,
                )?;
                out.push_str(&trailing);
            }
        } else if is_copy_statement_start(line) {
            reject_hidden_raw_copy_errors(line, lines.as_slice(), idx, active_replacements)?;
            let statement = collect_replaced_directive_source(
                line,
                lines.as_slice(),
                &mut idx,
                active_replacements,
            );
            let Some(statement_end) = copy_statement_end(&statement) else {
                return Err(SourceError::MalformedCopyStatement { statement });
            };
            let copy_source = statement[..statement_end].to_string();
            let raw_trailing_source = &statement[statement_end..];
            let trailing_source = raw_trailing_source.trim_start().to_string();
            let Some(copy_statement) = parse_copy_statement(&copy_source) else {
                return Err(SourceError::MalformedCopyStatement {
                    statement: copy_source,
                });
            };
            if copy_statement.malformed_replacing {
                return Err(SourceError::MalformedCopyReplacing {
                    copybook: copy_statement.name,
                });
            }
            if let Some(clause) = copy_statement.unsupported_clause.as_ref() {
                return Err(SourceError::UnsupportedCopyClause {
                    copybook: copy_statement.name.clone(),
                    clause: clause.clone(),
                });
            }
            reject_ambiguous_compact_copy_boundary(
                &copy_source,
                raw_trailing_source,
                &copy_statement,
                primary_dir,
                context.copybook_dirs,
            )?;
            let copybook = copy_statement.name;
            if depth >= MAX_COPY_DEPTH {
                return Err(SourceError::CopyDepthExceeded {
                    copybook,
                    max_depth: MAX_COPY_DEPTH,
                });
            }
            let resolved = resolve_copybook(
                &copybook,
                copy_statement.library.as_deref(),
                primary_dir,
                context.copybook_dirs,
            )?
            .ok_or_else(|| SourceError::CopybookNotFound {
                copybook: copybook.clone(),
            })?;
            let key = copybook_path_key(&resolved);
            if !context.stack.insert(key.clone()) {
                return Err(SourceError::RecursiveCopy { copybook });
            }
            context.includes.push(IncludeTrace {
                copybook: copybook.clone(),
                resolved_path: resolved.clone(),
                depth: depth + 1,
            });
            let raw = read_to_string(&resolved)?;
            let normalized = normalize_source(&raw, context.format);
            let mut copybook_replacements = Vec::new();
            let mut expanded = expand_copybooks_inner(
                &normalized,
                resolved.parent(),
                depth + 1,
                &mut copybook_replacements,
                context,
            )?;
            expanded = apply_copy_replacements(&expanded, &copy_statement.replacements);
            if !active_replacements.is_empty() {
                expanded = apply_copy_replacements(&expanded, active_replacements);
            }
            out.push_str(&expanded);
            if !expanded.ends_with('\n') {
                out.push('\n');
            }
            context.stack.remove(&key);
            if !trailing_source.is_empty() {
                let trailing = expand_copybooks_inner(
                    &trailing_source,
                    primary_dir,
                    depth,
                    active_replacements,
                    context,
                )?;
                out.push_str(&trailing);
            }
        } else if let Some(directive_start) = embedded_directive_start(line) {
            let prefix = apply_copy_replacements(&line[..directive_start], active_replacements);
            out.push_str(&prefix);
            let directive_tail = &line[directive_start..];
            let directive_source = if is_copy_statement_start(directive_tail) {
                reject_hidden_raw_copy_errors(
                    directive_tail,
                    lines.as_slice(),
                    idx,
                    active_replacements,
                )?;
                collect_replaced_directive_source(
                    directive_tail,
                    lines.as_slice(),
                    &mut idx,
                    active_replacements,
                )
            } else {
                collect_directive_source(directive_tail, lines.as_slice(), &mut idx)
            };
            let suffix = expand_copybooks_inner(
                &directive_source,
                primary_dir,
                depth,
                active_replacements,
                context,
            )?;
            out.push_str(&suffix);
        } else {
            let line = apply_copy_replacements(line, active_replacements);
            if is_copy_statement_start(&line) || is_replace_directive_start(&line) {
                let directive_source = collect_generated_directive_source(
                    &line,
                    lines.as_slice(),
                    &mut idx,
                    active_replacements,
                );
                let expanded = expand_copybooks_inner(
                    &directive_source,
                    primary_dir,
                    depth,
                    active_replacements,
                    context,
                )?;
                out.push_str(&expanded);
            } else if let Some(directive_start) = embedded_directive_start(&line) {
                out.push_str(&line[..directive_start]);
                let directive_source = collect_generated_directive_source(
                    &line[directive_start..],
                    lines.as_slice(),
                    &mut idx,
                    active_replacements,
                );
                let expanded = expand_copybooks_inner(
                    &directive_source,
                    primary_dir,
                    depth,
                    active_replacements,
                    context,
                )?;
                out.push_str(&expanded);
            } else {
                out.push_str(&line);
                out.push('\n');
            }
        }
        idx += 1;
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CopyStatement {
    name: String,
    library: Option<String>,
    suppress: bool,
    replacements: Vec<CopyReplacement>,
    malformed_replacing: bool,
    unsupported_clause: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CopyReplacement {
    kind: ReplacementKind,
    case_sensitive: bool,
    from: String,
    to: String,
}

impl CopyReplacement {
    fn full(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            kind: ReplacementKind::Full,
            case_sensitive: false,
            from: from.into(),
            to: to.into(),
        }
    }

    fn full_case_sensitive(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            kind: ReplacementKind::Full,
            case_sensitive: true,
            from: from.into(),
            to: to.into(),
        }
    }

    fn leading(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            kind: ReplacementKind::Leading,
            case_sensitive: false,
            from: from.into(),
            to: to.into(),
        }
    }

    fn trailing(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            kind: ReplacementKind::Trailing,
            case_sensitive: false,
            from: from.into(),
            to: to.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplacementKind {
    Full,
    Leading,
    Trailing,
}

enum ReplaceDirective {
    Set(Vec<CopyReplacement>),
    Off,
}

fn is_copy_statement_start(line: &str) -> bool {
    cobol_text::split_cobol_words(line)
        .first()
        .is_some_and(|word| word.trim_end_matches('.').eq_ignore_ascii_case("COPY"))
}

fn is_replace_directive_start(line: &str) -> bool {
    cobol_text::split_cobol_words(line)
        .first()
        .is_some_and(|word| word.trim_end_matches('.').eq_ignore_ascii_case("REPLACE"))
}

fn strip_source_inline_comment(line: &str) -> &str {
    strip_source_inline_comment_with_initial_quote(line, None)
}

fn strip_source_inline_comment_with_initial_quote(line: &str, initial_quote: Option<char>) -> &str {
    let mut idx = 0usize;
    let mut in_pseudo_text = false;
    let mut quote = initial_quote;
    let mut continuation_delimiter_pending = initial_quote;
    while idx < line.len() {
        let Some(ch) = line[idx..].chars().next() else {
            break;
        };
        if let Some(active_quote) = quote {
            if continuation_delimiter_pending.is_some() {
                if ch.is_whitespace() {
                    idx += ch.len_utf8();
                    continue;
                }
                continuation_delimiter_pending = None;
                if ch == active_quote {
                    idx += ch.len_utf8();
                    continue;
                }
            }
            if ch == active_quote {
                let next_idx = idx + ch.len_utf8();
                if line[next_idx..].starts_with(active_quote) {
                    idx = next_idx + active_quote.len_utf8();
                    continue;
                }
                quote = None;
            }
            idx += ch.len_utf8();
            continue;
        }
        if !in_pseudo_text && matches!(ch, '\'' | '"') {
            quote = Some(ch);
            idx += ch.len_utf8();
            continue;
        }
        if line[idx..].starts_with("==") {
            in_pseudo_text = !in_pseudo_text;
            idx += 2;
            continue;
        }
        if !in_pseudo_text && line[idx..].starts_with("*>") {
            return &line[..idx];
        }
        idx += ch.len_utf8();
    }
    line
}

fn copy_statement_end(statement: &str) -> Option<usize> {
    let mut idx = 0usize;
    while idx < statement.len() {
        if statement[idx..].starts_with("==") {
            idx = pseudo_text_end(statement, idx)?;
            continue;
        }
        let Some(ch) = statement[idx..].chars().next() else {
            break;
        };
        if matches!(ch, '\'' | '"') {
            if let Some(end) = cobol_text::quoted_literal_end(statement, idx) {
                idx = end;
                continue;
            }
        }
        if cobol_text::is_sentence_period_outside_literals(statement, idx)
            || period_precedes_adjacent_directive(statement, idx)
        {
            return Some(idx + ch.len_utf8());
        }
        idx += ch.len_utf8();
    }
    None
}

fn period_precedes_adjacent_directive(statement: &str, period_idx: usize) -> bool {
    if !cobol_text::is_period_outside_literals(statement, period_idx) {
        return false;
    }
    let next_idx = period_idx + '.'.len_utf8();
    let Some(tail) = statement.get(next_idx..) else {
        return false;
    };
    !tail.chars().next().map(char::is_whitespace).unwrap_or(true)
        && tail_starts_complete_adjacent_directive(tail)
}

fn tail_starts_complete_adjacent_directive(tail: &str) -> bool {
    if is_copy_statement_start(tail) {
        let Some(end) = copy_statement_end(tail) else {
            return false;
        };
        return parse_copy_statement(&tail[..end]).is_some();
    }
    if is_replace_directive_start(tail) {
        let Some(end) = copy_statement_end(tail) else {
            return false;
        };
        return parse_replace_directive(&tail[..end]).is_some();
    }
    false
}

fn reject_ambiguous_compact_copy_boundary(
    copy_source: &str,
    raw_trailing_source: &str,
    copy_statement: &CopyStatement,
    primary_dir: Option<&Path>,
    copybook_dirs: &[PathBuf],
) -> Result<(), SourceError> {
    let Some(keyword) = compact_adjacent_directive_keyword(raw_trailing_source) else {
        return Ok(());
    };
    let Some(name_operand) = unquoted_copy_name_operand(copy_source) else {
        return Ok(());
    };
    let ambiguous_statement = format!("{}{}", copy_source.trim_end(), raw_trailing_source);
    let adjacent_extension_name = format!("{name_operand}.{keyword}");
    if copy_statement.library.is_none()
        && resolve_copybook(&adjacent_extension_name, None, primary_dir, copybook_dirs)?.is_some()
    {
        return Err(SourceError::MalformedCopyStatement {
            statement: ambiguous_statement,
        });
    }
    if let Some(library) = copy_statement.library.as_deref() {
        let adjacent_extension_library = format!("{library}.{keyword}");
        if resolve_copybook(
            &copy_statement.name,
            Some(&adjacent_extension_library),
            primary_dir,
            copybook_dirs,
        )?
        .is_some()
        {
            return Err(SourceError::MalformedCopyStatement {
                statement: ambiguous_statement,
            });
        }
    }
    Ok(())
}

fn compact_adjacent_directive_keyword(raw_trailing_source: &str) -> Option<String> {
    if raw_trailing_source
        .chars()
        .next()
        .map(char::is_whitespace)
        .unwrap_or(true)
    {
        return None;
    }
    let words = cobol_text::split_cobol_words_spanned(raw_trailing_source);
    let word = words.first()?;
    (word.text.eq_ignore_ascii_case("COPY") || word.text.eq_ignore_ascii_case("REPLACE"))
        .then_some(word.text.clone())
}

fn unquoted_copy_name_operand(copy_source: &str) -> Option<String> {
    let cleaned = cobol_text::strip_trailing_sentence_period_outside_literals(copy_source.trim());
    let words = cobol_text::split_cobol_words_spanned(cleaned);
    let name = words.get(1)?;
    if is_quoted_literal_operand(&name.text) {
        return None;
    }
    clean_copy_name_operand(&name.text)
}

fn collect_directive_source(first_line: &str, lines: &[&str], idx: &mut usize) -> String {
    let mut statement = first_line.to_string();
    while copy_statement_end(&statement).is_none() && *idx + 1 < lines.len() {
        *idx += 1;
        statement.push('\n');
        statement.push_str(lines[*idx]);
    }
    statement
}

fn collect_replaced_directive_source(
    first_line: &str,
    lines: &[&str],
    idx: &mut usize,
    active_replacements: &[CopyReplacement],
) -> String {
    let first_line = apply_copy_replacements(first_line, active_replacements);
    collect_generated_directive_source(&first_line, lines, idx, active_replacements)
}

fn reject_hidden_raw_copy_errors(
    first_line: &str,
    lines: &[&str],
    idx: usize,
    active_replacements: &[CopyReplacement],
) -> Result<(), SourceError> {
    let mut raw_idx = idx;
    let raw_statement = collect_directive_source(first_line, lines, &mut raw_idx);
    let Some(statement_end) = copy_statement_end(&raw_statement) else {
        return Err(SourceError::MalformedCopyStatement {
            statement: raw_statement,
        });
    };
    let raw_copy_source = raw_statement[..statement_end].to_string();
    let Some(raw_copy_statement) = parse_copy_statement(&raw_copy_source) else {
        return Err(SourceError::MalformedCopyStatement {
            statement: raw_copy_source,
        });
    };
    if raw_copy_statement.malformed_replacing {
        return Err(SourceError::MalformedCopyReplacing {
            copybook: raw_copy_statement.name,
        });
    }
    if let Some(clause) = raw_copy_statement.unsupported_clause {
        if is_known_unsupported_copy_clause(&clause)
            || replacement_hides_raw_unsupported_copy_clause(
                first_line,
                lines,
                idx,
                active_replacements,
            )
        {
            return Err(SourceError::UnsupportedCopyClause {
                copybook: raw_copy_statement.name,
                clause,
            });
        }
    }
    Ok(())
}

fn replacement_hides_raw_unsupported_copy_clause(
    first_line: &str,
    lines: &[&str],
    idx: usize,
    active_replacements: &[CopyReplacement],
) -> bool {
    if active_replacements.is_empty() {
        return false;
    }
    let mut replaced_idx = idx;
    let replaced_statement = collect_replaced_directive_source(
        first_line,
        lines,
        &mut replaced_idx,
        active_replacements,
    );
    let Some(replaced_end) = copy_statement_end(&replaced_statement) else {
        return false;
    };
    let replaced_copy_source = &replaced_statement[..replaced_end];
    parse_copy_statement(replaced_copy_source).is_some_and(|statement| {
        !statement.malformed_replacing && statement.unsupported_clause.is_none()
    })
}

fn is_known_unsupported_copy_clause(clause: &str) -> bool {
    matches!(
        clause.to_ascii_uppercase().as_str(),
        "LIST" | "NOLIST" | "PREFIXING" | "SUFFIXING"
    )
}

fn collect_generated_directive_source(
    first_line: &str,
    lines: &[&str],
    idx: &mut usize,
    active_replacements: &[CopyReplacement],
) -> String {
    let mut statement = first_line.to_string();
    while copy_statement_end(&statement).is_none() && *idx + 1 < lines.len() {
        *idx += 1;
        statement.push('\n');
        statement.push_str(&apply_copy_replacements(lines[*idx], active_replacements));
    }
    statement
}

fn embedded_directive_start(line: &str) -> Option<usize> {
    let mut idx = 0usize;
    let mut after_statement_period = false;
    while idx < line.len() {
        if line[idx..].starts_with("==") {
            if let Some(end) = pseudo_text_end(line, idx) {
                idx = end;
                continue;
            }
        }
        let Some(ch) = line[idx..].chars().next() else {
            break;
        };
        if matches!(ch, '\'' | '"') {
            if let Some(end) = cobol_text::quoted_literal_end(line, idx) {
                idx = end;
                continue;
            }
        }
        if after_statement_period {
            if ch.is_whitespace() {
                idx += ch.len_utf8();
                continue;
            }
            let tail = &line[idx..];
            if is_copy_statement_start(tail) || is_replace_directive_start(tail) {
                return Some(idx);
            }
            after_statement_period = false;
        }
        if cobol_text::is_sentence_period_outside_literals(line, idx)
            || period_precedes_adjacent_directive(line, idx)
        {
            after_statement_period = true;
        }
        idx += ch.len_utf8();
    }
    None
}

fn parse_copy_statement(line: &str) -> Option<CopyStatement> {
    let cleaned = cobol_text::strip_trailing_sentence_period_outside_literals(line.trim()).trim();
    let words = cobol_text::split_cobol_words_spanned(cleaned);
    let first = words.first()?;
    if !first.text.eq_ignore_ascii_case("COPY") {
        return None;
    }
    let name = words.get(1)?;
    if !is_quoted_literal_operand(&name.text) && is_copy_clause_keyword(&name.text) {
        return None;
    }
    let copybook_name = clean_copy_name_operand(&name.text)?;
    let remaining = cleaned[name.end..].trim_start().to_string();
    let (library, remaining) = parse_copy_library_clause(&remaining)?;
    let (suppress, remaining) = parse_copy_suppress_clause(&remaining);
    let first_clause = remaining.split_whitespace().next();
    let (replacements, malformed_replacing, unsupported_clause) = match first_clause {
        Some(clause) if clause.eq_ignore_ascii_case("REPLACING") => {
            match parse_replacements(&remaining["REPLACING".len()..]) {
                Some(replacements) => (replacements, false, None),
                None => (Vec::new(), true, None),
            }
        }
        Some(clause) => (Vec::new(), false, Some(clause.to_string())),
        None => (Vec::new(), false, None),
    };
    Some(CopyStatement {
        name: copybook_name,
        library,
        suppress,
        replacements,
        malformed_replacing,
        unsupported_clause,
    })
}

fn parse_copy_library_clause(remaining: &str) -> Option<(Option<String>, String)> {
    let words = cobol_text::split_cobol_words_spanned(remaining);
    let Some(first) = words.first() else {
        return Some((None, String::new()));
    };
    if !(first.text.eq_ignore_ascii_case("IN") || first.text.eq_ignore_ascii_case("OF")) {
        return Some((None, remaining.trim().to_string()));
    }
    let library = words.get(1)?;
    if !is_quoted_literal_operand(&library.text) && is_copy_clause_keyword(&library.text) {
        return None;
    }
    let library_name = clean_copy_name_operand(&library.text)?;
    let rest = remaining[library.end..].trim_start().to_string();
    Some((Some(library_name), rest))
}

fn is_copy_clause_keyword(text: &str) -> bool {
    text.eq_ignore_ascii_case("IN")
        || text.eq_ignore_ascii_case("OF")
        || text.eq_ignore_ascii_case("SUPPRESS")
        || text.eq_ignore_ascii_case("PRINTING")
        || text.eq_ignore_ascii_case("REPLACING")
        || text.eq_ignore_ascii_case("LIST")
        || text.eq_ignore_ascii_case("NOLIST")
        || text.eq_ignore_ascii_case("PREFIXING")
        || text.eq_ignore_ascii_case("SUFFIXING")
}

fn parse_copy_suppress_clause(remaining: &str) -> (bool, String) {
    let words = cobol_text::split_cobol_words_spanned(remaining);
    let Some(first) = words.first() else {
        return (false, String::new());
    };
    if first.text.eq_ignore_ascii_case("SUPPRESS") {
        let rest_start = if words
            .get(1)
            .is_some_and(|word| word.text.eq_ignore_ascii_case("PRINTING"))
        {
            words[1].end
        } else {
            first.end
        };
        return (true, remaining[rest_start..].trim_start().to_string());
    }
    (false, remaining.trim().to_string())
}

fn parse_replace_directive(statement: &str) -> Option<ReplaceDirective> {
    let cleaned =
        cobol_text::strip_trailing_sentence_period_outside_literals(statement.trim()).trim();
    let mut parts = cleaned.splitn(2, char::is_whitespace);
    let keyword = parts.next()?;
    if !keyword.eq_ignore_ascii_case("REPLACE") {
        return None;
    }
    let remaining = parts.next().unwrap_or("").trim_start();
    if remaining.eq_ignore_ascii_case("OFF") {
        return Some(ReplaceDirective::Off);
    }
    parse_replacements(remaining).map(ReplaceDirective::Set)
}

fn clean_copy_name(name: &str) -> String {
    let name = name.trim().trim_end_matches('.').trim();
    unquote_cobol_literal(name).unwrap_or_else(|| name.to_string())
}

fn clean_copy_name_operand(name: &str) -> Option<String> {
    let name = name.trim();
    if name
        .chars()
        .next()
        .is_some_and(|quote| matches!(quote, '\'' | '"'))
        && !is_quoted_literal_operand(name)
    {
        return None;
    }
    if !is_quoted_literal_operand(name) && name.ends_with('.') {
        return None;
    }
    let cleaned = clean_copy_name(name);
    if cleaned.trim().is_empty() || !is_safe_copy_path_operand(&cleaned) {
        return None;
    }
    Some(cleaned)
}

fn is_safe_copy_path_operand(name: &str) -> bool {
    if name.contains('/') || name.contains('\\') || name.contains(':') {
        return false;
    }
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

fn unquote_cobol_literal(text: &str) -> Option<String> {
    let quote = text.chars().next()?;
    if !matches!(quote, '\'' | '"') || !text.ends_with(quote) {
        return None;
    }
    let inner = &text[quote.len_utf8()..text.len() - quote.len_utf8()];
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == quote && chars.peek().is_some_and(|next| *next == quote) {
            chars.next();
        }
        out.push(ch);
    }
    Some(out)
}

fn parse_replacements(text: &str) -> Option<Vec<CopyReplacement>> {
    let mut replacements = Vec::new();
    let mut rest = text.trim();
    if rest.is_empty() {
        return None;
    }
    while !rest.is_empty() {
        let (kind, replacement_rest) = parse_replacement_kind(rest);
        let (from, from_case_sensitive, after_from) = parse_replacement_operand(replacement_rest)?;
        if from.is_empty() {
            return None;
        }
        if matches!(kind, ReplacementKind::Leading | ReplacementKind::Trailing)
            && !is_partial_replacement_source(&from)
        {
            return None;
        }
        let after_by = parse_by_keyword(after_from)?;
        let (to, _, after_to) = parse_replacement_operand(after_by)?;
        replacements.push(match kind {
            ReplacementKind::Full if from_case_sensitive => {
                CopyReplacement::full_case_sensitive(from, to)
            }
            ReplacementKind::Full => CopyReplacement::full(from, to),
            ReplacementKind::Leading => CopyReplacement::leading(from, to),
            ReplacementKind::Trailing => CopyReplacement::trailing(from, to),
        });
        rest = after_to.trim_start();
    }
    (!replacements.is_empty()).then_some(replacements)
}

fn parse_replacement_kind(text: &str) -> (ReplacementKind, &str) {
    let text = text.trim_start();
    let words = cobol_text::split_cobol_words_spanned(text);
    let Some(word) = words.first() else {
        return (ReplacementKind::Full, text);
    };
    if word.text.eq_ignore_ascii_case("LEADING") {
        (ReplacementKind::Leading, text[word.end..].trim_start())
    } else if word.text.eq_ignore_ascii_case("TRAILING") {
        (ReplacementKind::Trailing, text[word.end..].trim_start())
    } else {
        (ReplacementKind::Full, text)
    }
}

fn parse_replacement_operand(text: &str) -> Option<(String, bool, &str)> {
    let text = text.trim_start();
    if text.starts_with("==") {
        let (value, rest) = parse_pseudo_text(text)?;
        let case_sensitive = is_quoted_literal_operand(value.trim());
        return Some((value, case_sensitive, rest));
    }
    let words = cobol_text::split_cobol_words_spanned(text);
    let word = words.first()?;
    let case_sensitive = is_quoted_literal_operand(&word.text);
    if matches!(word.text.chars().next(), Some('\'' | '"')) && !case_sensitive {
        return None;
    }
    Some((word.text.clone(), case_sensitive, &text[word.end..]))
}

fn parse_by_keyword(text: &str) -> Option<&str> {
    let text = text.trim_start();
    let words = cobol_text::split_cobol_words_spanned(text);
    let word = words.first()?;
    if word.text.eq_ignore_ascii_case("BY") {
        Some(text[word.end..].trim_start())
    } else {
        None
    }
}

fn parse_pseudo_text(text: &str) -> Option<(String, &str)> {
    let text = text.trim_start();
    let end = pseudo_text_end(text, 0)?;
    let value = text[2..end - 2].to_string();
    Some((value, &text[end..]))
}

fn is_quoted_literal_operand(text: &str) -> bool {
    let text = text.trim();
    let Some(quote) = text.chars().next() else {
        return false;
    };
    if !matches!(quote, '\'' | '"') {
        return false;
    }
    cobol_text::complete_quoted_literal_end(text, 0).is_some_and(|end| end == text.len())
}

fn pseudo_text_end(text: &str, start: usize) -> Option<usize> {
    if !text.get(start..)?.starts_with("==") {
        return None;
    }
    let mut idx = start + 2;
    let mut quote = None;
    while idx < text.len() {
        let ch = text[idx..].chars().next()?;
        if let Some(active_quote) = quote {
            if ch == active_quote {
                let next_idx = idx + ch.len_utf8();
                if text[next_idx..].starts_with(active_quote) {
                    idx = next_idx + active_quote.len_utf8();
                    continue;
                }
                quote = None;
            }
            idx += ch.len_utf8();
            continue;
        }
        if matches!(ch, '\'' | '"') {
            quote = Some(ch);
            idx += ch.len_utf8();
            continue;
        }
        if text[idx..].starts_with("==") {
            return Some(idx + 2);
        }
        idx += ch.len_utf8();
    }
    None
}

fn apply_copy_replacements(text: &str, replacements: &[CopyReplacement]) -> String {
    let mut out = text.to_string();
    for replacement in replacements {
        out = apply_copy_replacement(&out, replacement);
    }
    out
}

fn apply_copy_replacement(text: &str, replacement: &CopyReplacement) -> String {
    match replacement.kind {
        ReplacementKind::Full => replace_outside_literals(
            text,
            &replacement.from,
            &replacement.to,
            replacement.case_sensitive,
        ),
        ReplacementKind::Leading | ReplacementKind::Trailing => {
            replace_partial_words_outside_literals(text, replacement)
        }
    }
}

fn replace_outside_literals(text: &str, from: &str, to: &str, case_sensitive: bool) -> String {
    if from.trim().is_empty() {
        return text.to_string();
    }

    let mut out = String::with_capacity(text.len());
    let mut idx = 0usize;
    while idx < text.len() {
        let Some(ch) = text[idx..].chars().next() else {
            break;
        };
        if let Some(end) = match_pseudo_text_at(text, idx, from, case_sensitive) {
            out.push_str(to);
            idx = end;
            continue;
        }
        if matches!(ch, '\'' | '"') {
            if let Some(end) = cobol_text::quoted_literal_end(text, idx) {
                out.push_str(&text[idx..end]);
                idx = end;
                continue;
            }
        }
        out.push(ch);
        idx += ch.len_utf8();
    }
    out
}

fn replace_partial_words_outside_literals(text: &str, replacement: &CopyReplacement) -> String {
    if replacement.from.is_empty() {
        return text.to_string();
    }

    let mut out = String::with_capacity(text.len());
    let mut idx = 0usize;
    while idx < text.len() {
        let Some(ch) = text[idx..].chars().next() else {
            break;
        };
        if matches!(ch, '\'' | '"') {
            if let Some(end) = cobol_text::quoted_literal_end(text, idx) {
                out.push_str(&text[idx..end]);
                idx = end;
                continue;
            }
        }
        if is_cobol_name_char(Some(ch)) {
            let start = idx;
            idx += ch.len_utf8();
            while idx < text.len() {
                let Some(word_ch) = text[idx..].chars().next() else {
                    break;
                };
                if !is_cobol_name_char(Some(word_ch)) {
                    break;
                }
                idx += word_ch.len_utf8();
            }
            let word = &text[start..idx];
            match replacement.kind {
                ReplacementKind::Leading
                    if starts_with_ignore_ascii_case(word, &replacement.from) =>
                {
                    out.push_str(&replacement.to);
                    out.push_str(&word[replacement.from.len()..]);
                }
                ReplacementKind::Trailing
                    if ends_with_ignore_ascii_case(word, &replacement.from) =>
                {
                    let keep_end = word.len() - replacement.from.len();
                    out.push_str(&word[..keep_end]);
                    out.push_str(&replacement.to);
                }
                _ => out.push_str(word),
            }
            continue;
        }
        out.push(ch);
        idx += ch.len_utf8();
    }
    out
}

fn match_pseudo_text_at(
    text: &str,
    start: usize,
    pattern: &str,
    case_sensitive: bool,
) -> Option<usize> {
    let first_pattern_ch = pattern.chars().next()?;
    if is_cobol_name_char(Some(first_pattern_ch)) && !is_replacement_start_boundary(text, start) {
        return None;
    }

    let mut pattern_idx = 0usize;
    let mut text_idx = start;
    let mut consumed = false;
    while pattern_idx < pattern.len() {
        let pattern_ch = pattern[pattern_idx..].chars().next()?;
        if matches!(pattern_ch, '\'' | '"') {
            let pattern_end = cobol_text::quoted_literal_end(pattern, pattern_idx)?;
            let text_end = cobol_text::quoted_literal_end(text, text_idx)?;
            let pattern_literal = &pattern[pattern_idx..pattern_end];
            let text_literal = &text[text_idx..text_end];
            if pattern_literal != text_literal {
                return None;
            }
            pattern_idx = pattern_end;
            text_idx = text_end;
            consumed = true;
            continue;
        }
        if pattern_ch.is_whitespace() {
            while pattern_idx < pattern.len() {
                let Some(ch) = pattern[pattern_idx..].chars().next() else {
                    break;
                };
                if !ch.is_whitespace() {
                    break;
                }
                pattern_idx += ch.len_utf8();
            }
            let before_whitespace = text_idx;
            while text_idx < text.len() {
                let Some(ch) = text[text_idx..].chars().next() else {
                    break;
                };
                if !ch.is_whitespace() {
                    break;
                }
                text_idx += ch.len_utf8();
            }
            if text_idx == before_whitespace {
                return None;
            }
            continue;
        }

        let text_ch = text[text_idx..].chars().next()?;
        if !chars_eq(pattern_ch, text_ch, case_sensitive) {
            return None;
        }
        pattern_idx += pattern_ch.len_utf8();
        text_idx += text_ch.len_utf8();
        consumed = true;
    }

    if consumed && is_replacement_end_boundary(text, text_idx) {
        Some(text_idx)
    } else {
        None
    }
}

fn chars_eq(left: char, right: char, case_sensitive: bool) -> bool {
    if case_sensitive {
        left == right
    } else {
        chars_eq_ignore_ascii_case(left, right)
    }
}

fn chars_eq_ignore_ascii_case(left: char, right: char) -> bool {
    if left.is_ascii() && right.is_ascii() {
        left.eq_ignore_ascii_case(&right)
    } else {
        left == right
    }
}

fn starts_with_ignore_ascii_case(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len())
        .map(|head| head.eq_ignore_ascii_case(prefix))
        .unwrap_or(false)
}

fn ends_with_ignore_ascii_case(text: &str, suffix: &str) -> bool {
    if suffix.len() > text.len() {
        return false;
    }
    text.get(text.len() - suffix.len()..)
        .map(|tail| tail.eq_ignore_ascii_case(suffix))
        .unwrap_or(false)
}

fn is_replacement_start_boundary(text: &str, idx: usize) -> bool {
    if idx == 0 || idx >= text.len() {
        return true;
    }
    !is_cobol_name_char(text[..idx].chars().next_back())
}

fn is_replacement_end_boundary(text: &str, idx: usize) -> bool {
    if idx >= text.len() {
        return true;
    }
    !is_cobol_name_char(text[idx..].chars().next())
}

fn is_cobol_name_char(ch: Option<char>) -> bool {
    ch.map(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        .unwrap_or(false)
}

fn is_partial_replacement_source(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|ch| is_cobol_name_char(Some(ch)))
}

fn resolve_copybook(
    name: &str,
    library: Option<&str>,
    primary_dir: Option<&Path>,
    copybook_dirs: &[PathBuf],
) -> Result<Option<PathBuf>, SourceError> {
    const IMPLICIT_COPYBOOK_EXTENSIONS: &[&str] = &[
        "cpy", "CPY", "cbl", "CBL", "cob", "COB", "cobol", "COBOL", "copy", "COPY",
    ];

    let mut base_dirs = Vec::new();
    if let Some(dir) = primary_dir {
        base_dirs.push(dir.to_path_buf());
    }
    base_dirs.extend(copybook_dirs.iter().cloned());
    let mut dirs = Vec::new();
    for dir in base_dirs {
        if let Some(library) = library {
            dirs.push(dir.join(library));
            if dir
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case(library))
            {
                dirs.push(dir);
            }
        } else {
            dirs.push(dir);
        }
    }

    let candidates = if Path::new(name).extension().is_some() {
        vec![PathBuf::from(name)]
    } else {
        let mut candidates = Vec::with_capacity(IMPLICIT_COPYBOOK_EXTENSIONS.len() + 1);
        candidates.push(PathBuf::from(name));
        candidates.extend(
            IMPLICIT_COPYBOOK_EXTENSIONS
                .iter()
                .map(|extension| PathBuf::from(format!("{name}.{extension}"))),
        );
        candidates
    };

    let mut matches = Vec::new();
    let mut seen = HashSet::new();
    for dir in dirs {
        for candidate in &candidates {
            let path = dir.join(candidate);
            if path.is_file() && seen.insert(copybook_path_key(&path)) {
                matches.push(path);
            }
        }
    }

    match matches.len() {
        0 => Ok(None),
        1 => Ok(matches.pop()),
        _ => Err(SourceError::AmbiguousCopybook {
            copybook: name.to_string(),
            candidates: matches,
        }),
    }
}

fn copybook_path_key(path: &Path) -> String {
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let key = path.to_string_lossy().to_string();
    if cfg!(windows) {
        key.to_ascii_lowercase()
    } else {
        key
    }
}

fn read_to_string(path: &Path) -> Result<String, SourceError> {
    fs::read_to_string(path).map_err(|source| SourceError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_format_strips_sequence_area() {
        let raw = "000100 IDENTIFICATION DIVISION.\n000200 PROGRAM-ID. HELLO.\n";
        assert_eq!(
            normalize_source(raw, SourceFormat::Fixed),
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\n"
        );
    }

    #[test]
    fn free_format_drops_comment_lines() {
        let raw = "*> comment\nIDENTIFICATION DIVISION.\n";
        assert_eq!(
            normalize_source(raw, SourceFormat::Free),
            "IDENTIFICATION DIVISION.\n"
        );
    }

    #[test]
    fn auto_format_detects_plain_free_format_cobol() {
        let raw = "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\nPROCEDURE DIVISION.\nDISPLAY \"HELLO\".\n";
        assert_eq!(detect_format(raw, SourceFormat::Auto), SourceFormat::Free);
        assert_eq!(
            normalize_source(raw, detect_format(raw, SourceFormat::Auto)),
            raw
        );
    }

    #[test]
    fn auto_format_detects_indented_free_format_cobol() {
        let raw = "    IDENTIFICATION DIVISION.\n    PROGRAM-ID. HELLO.\n    PROCEDURE DIVISION.\n    DISPLAY \"HELLO\".\n";
        assert_eq!(detect_format(raw, SourceFormat::Auto), SourceFormat::Free);
        assert_eq!(
            normalize_source(raw, detect_format(raw, SourceFormat::Auto)),
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\nPROCEDURE DIVISION.\nDISPLAY \"HELLO\".\n"
        );
    }

    #[test]
    fn normalize_source_auto_detects_fixed_format() {
        let raw = "000100 IDENTIFICATION DIVISION.\n000200 PROGRAM-ID. HELLO.\n";
        assert_eq!(
            normalize_source(raw, SourceFormat::Auto),
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\n"
        );
    }

    #[test]
    fn auto_format_detects_blank_sequence_fixed_debug_indicator() {
        let raw = "       IDENTIFICATION DIVISION.\n      D DISPLAY \"DEBUG\".\n       PROGRAM-ID. HELLO.\n";
        assert_eq!(detect_format(raw, SourceFormat::Auto), SourceFormat::Fixed);
        assert_eq!(
            normalize_source(raw, SourceFormat::Auto),
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\n"
        );
    }

    #[test]
    fn auto_format_detects_blank_sequence_fixed_comment_without_space_after_indicator() {
        let raw = "       IDENTIFICATION DIVISION.\n      *COMMENT\n       PROGRAM-ID. HELLO.\n";
        assert_eq!(detect_format(raw, SourceFormat::Auto), SourceFormat::Fixed);
        assert_eq!(
            normalize_source(raw, SourceFormat::Auto),
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\n"
        );
    }

    #[test]
    fn auto_format_does_not_treat_indented_free_display_as_debug_indicator() {
        let raw = "      DISPLAY \"HELLO\".\n";
        assert_eq!(detect_format(raw, SourceFormat::Auto), SourceFormat::Free);
        assert_eq!(
            normalize_source(raw, SourceFormat::Auto),
            "DISPLAY \"HELLO\".\n"
        );
    }

    #[test]
    fn auto_format_ignores_source_format_directive_in_comment() {
        let raw =
            "*> >>SOURCE FORMAT FREE\n000100 IDENTIFICATION DIVISION.\n000200 PROGRAM-ID. HELLO.\n";
        assert_eq!(detect_format(raw, SourceFormat::Auto), SourceFormat::Fixed);
        assert_eq!(
            normalize_source(raw, SourceFormat::Auto),
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\n"
        );
    }

    #[test]
    fn auto_format_honors_source_format_directive_in_source() {
        let raw = "       >>SOURCE FORMAT FREE\n       IDENTIFICATION DIVISION.\n       PROGRAM-ID. HELLO.\n";
        assert_eq!(detect_format(raw, SourceFormat::Auto), SourceFormat::Free);
        assert_eq!(
            normalize_source(raw, SourceFormat::Auto),
            ">>SOURCE FORMAT FREE\nIDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\n"
        );
    }

    #[test]
    fn auto_format_honors_six_space_indented_free_source_format_directive() {
        let raw =
            "      >>SOURCE FORMAT FREE\n000100 IDENTIFICATION DIVISION.\n000200 PROGRAM-ID. HELLO.\n";
        assert_eq!(detect_format(raw, SourceFormat::Auto), SourceFormat::Free);
        assert_eq!(
            normalize_source(raw, SourceFormat::Auto),
            ">>SOURCE FORMAT FREE\n000100 IDENTIFICATION DIVISION.\n000200 PROGRAM-ID. HELLO.\n"
        );
    }

    #[test]
    fn auto_format_honors_source_format_is_free_directive() {
        let raw = "       >>SOURCE FORMAT IS FREE\n000100 IDENTIFICATION DIVISION.\n000200 PROGRAM-ID. HELLO.\n";
        assert_eq!(detect_format(raw, SourceFormat::Auto), SourceFormat::Free);
        assert_eq!(
            normalize_source(raw, SourceFormat::Auto),
            ">>SOURCE FORMAT IS FREE\n000100 IDENTIFICATION DIVISION.\n000200 PROGRAM-ID. HELLO.\n"
        );
    }

    #[test]
    fn auto_format_honors_source_format_is_fixed_directive() {
        let raw =
            "       >>SOURCE FORMAT IS FIXED\n000100 IDENTIFICATION DIVISION.\n000200 PROGRAM-ID. HELLO.\n";
        assert_eq!(detect_format(raw, SourceFormat::Auto), SourceFormat::Fixed);
        assert_eq!(
            normalize_source(raw, SourceFormat::Auto),
            ">>SOURCE FORMAT IS FIXED\nIDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\n"
        );
    }

    #[test]
    fn auto_format_honors_source_format_directive_with_period() {
        let raw = "       >>SOURCE FORMAT FREE.\n000100 IDENTIFICATION DIVISION.\n000200 PROGRAM-ID. HELLO.\n";
        assert_eq!(detect_format(raw, SourceFormat::Auto), SourceFormat::Free);
        assert_eq!(
            normalize_source(raw, SourceFormat::Auto),
            ">>SOURCE FORMAT FREE.\n000100 IDENTIFICATION DIVISION.\n000200 PROGRAM-ID. HELLO.\n"
        );
    }

    #[test]
    fn auto_format_honors_source_format_is_fixed_directive_with_period() {
        let raw =
            "       >>SOURCE FORMAT IS FIXED.\n000100 IDENTIFICATION DIVISION.\n000200 PROGRAM-ID. HELLO.\n";
        assert_eq!(detect_format(raw, SourceFormat::Auto), SourceFormat::Fixed);
        assert_eq!(
            normalize_source(raw, SourceFormat::Auto),
            ">>SOURCE FORMAT IS FIXED.\nIDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\n"
        );
    }

    #[test]
    fn auto_format_requires_exact_source_format_directive_value() {
        let raw =
            "       >>SOURCE FORMAT FREEFORM\n000100 IDENTIFICATION DIVISION.\n000200 PROGRAM-ID. HELLO.\n";
        assert_eq!(detect_format(raw, SourceFormat::Auto), SourceFormat::Fixed);
        assert_eq!(
            normalize_source(raw, SourceFormat::Auto),
            ">>SOURCE FORMAT FREEFORM\nIDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\n"
        );
    }

    #[test]
    fn auto_format_rejects_source_format_directive_with_extra_words() {
        let raw =
            "       >>SOURCE FORMAT FREE FORM\n000100 IDENTIFICATION DIVISION.\n000200 PROGRAM-ID. HELLO.\n";
        assert_eq!(detect_format(raw, SourceFormat::Auto), SourceFormat::Fixed);
        assert_eq!(
            normalize_source(raw, SourceFormat::Auto),
            ">>SOURCE FORMAT FREE FORM\nIDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\n"
        );
    }

    #[test]
    fn copy_replacing_is_not_silently_ignored() {
        let parsed = parse_copy_statement("COPY CUSTOMER REPLACING ==A== BY ==B==.")
            .expect("copy statement");
        assert_eq!(parsed.name, "CUSTOMER");
        assert!(!parsed.suppress);
        assert_eq!(parsed.replacements, vec![CopyReplacement::full("A", "B")]);
        assert!(!parsed.malformed_replacing);
        assert_eq!(parsed.unsupported_clause, None);
        assert_eq!(parsed.library, None);
    }

    #[test]
    fn copy_statement_parses_library_clause_before_replacing() {
        let parsed = parse_copy_statement("COPY REC OF LIB REPLACING ==A== BY ==B==.")
            .expect("copy statement");
        assert_eq!(parsed.name, "REC");
        assert_eq!(parsed.library.as_deref(), Some("LIB"));
        assert_eq!(parsed.replacements, vec![CopyReplacement::full("A", "B")]);
    }

    #[test]
    fn copy_statement_parses_quoted_name_and_library_with_spaces() {
        let parsed = parse_copy_statement("COPY \"REC FILE\" OF \"COPY LIB\" SUPPRESS.")
            .expect("copy statement");
        assert_eq!(parsed.name, "REC FILE");
        assert_eq!(parsed.library.as_deref(), Some("COPY LIB"));
        assert!(parsed.suppress);

        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_quoted_member_{}",
            std::process::id()
        ));
        let lib = dir.join("COPY LIB");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&lib).expect("library dir");
        fs::write(lib.join("REC FILE.cpy"), "01 WS-FIELD PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "COPY \"REC FILE\" OF \"COPY LIB\".\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 WS-FIELD PIC X."));
        assert_eq!(includes.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_suppress_clause_is_listing_noop() {
        let parsed =
            parse_copy_statement("COPY REC SUPPRESS REPLACING ==OLD-NAME== BY ==NEW-NAME==.")
                .expect("copy statement");
        assert_eq!(parsed.name, "REC");
        assert!(parsed.suppress);
        assert_eq!(
            parsed.replacements,
            vec![CopyReplacement::full("OLD-NAME", "NEW-NAME")]
        );

        let dir =
            std::env::temp_dir().join(format!("cobol_source_copy_suppress_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 OLD-NAME PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "COPY REC SUPPRESS REPLACING ==OLD-NAME== BY ==NEW-NAME==.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expand copy");
        assert!(expanded.contains("01 NEW-NAME PIC X."));
        assert_eq!(includes.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_suppress_printing_clause_is_listing_noop() {
        let parsed = parse_copy_statement(
            "COPY REC SUPPRESS PRINTING REPLACING ==OLD-NAME== BY ==NEW-NAME==.",
        )
        .expect("copy statement");
        assert!(parsed.suppress);
        assert_eq!(parsed.unsupported_clause, None);
        assert_eq!(
            parsed.replacements,
            vec![CopyReplacement::full("OLD-NAME", "NEW-NAME")]
        );
    }

    #[test]
    fn copy_extra_printing_after_suppress_printing_fails_closed() {
        let parsed =
            parse_copy_statement("COPY REC SUPPRESS PRINTING PRINTING.").expect("copy statement");
        assert!(parsed.suppress);
        assert_eq!(parsed.unsupported_clause.as_deref(), Some("PRINTING"));

        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_extra_printing_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC SUPPRESS PRINTING PRINTING.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::UnsupportedCopyClause { copybook, clause })
                if copybook == "REC" && clause == "PRINTING"
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_nolist_dialect_clause_fails_closed() {
        let parsed = parse_copy_statement("COPY REC NOLIST.").expect("copy statement");
        assert!(!parsed.suppress);
        assert_eq!(parsed.unsupported_clause.as_deref(), Some("NOLIST"));

        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_nolist_fails_closed_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 WS-FIELD PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC NOLIST.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::UnsupportedCopyClause { copybook, clause })
                if copybook == "REC" && clause == "NOLIST"
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_printing_without_suppress_fails_closed() {
        let parsed = parse_copy_statement("COPY REC PRINTING.").expect("copy statement");
        assert!(!parsed.suppress);
        assert_eq!(parsed.unsupported_clause.as_deref(), Some("PRINTING"));

        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_printing_fails_closed_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC PRINTING.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::UnsupportedCopyClause { copybook, clause })
                if copybook == "REC" && clause == "PRINTING"
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_replace_cannot_hide_nolist_dialect_clause() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_replace_hides_nolist_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE ==NOLIST== BY ====.\nCOPY REC NOLIST.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::UnsupportedCopyClause { copybook, clause })
                if copybook == "REC" && clause == "NOLIST"
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_replace_cannot_hide_embedded_known_unsupported_dialect_clauses() {
        for clause in ["LIST", "NOLIST", "PREFIXING", "SUFFIXING"] {
            let dir = std::env::temp_dir().join(format!(
                "cobol_source_embedded_copy_replace_hides_{}_{}",
                clause.to_ascii_lowercase(),
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).expect("temp dir");
            fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
            let mut includes = Vec::new();
            let mut stack = HashSet::new();
            let source = format!(
                "REPLACE =={clause}== BY ====.\n01 PREFIX-FIELD PIC X. COPY REC {clause}.\n"
            );
            let result = expand_copybooks(
                &source,
                Some(&dir),
                &[],
                SourceFormat::Free,
                0,
                &mut includes,
                &mut stack,
            );
            assert!(matches!(
                result,
                Err(SourceError::UnsupportedCopyClause { copybook, clause: hidden_clause })
                    if copybook == "REC" && hidden_clause == clause
            ));
            assert!(includes.is_empty());
            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn active_replace_cannot_hide_known_unsupported_dialect_clauses() {
        for clause in ["LIST", "PREFIXING", "SUFFIXING"] {
            let dir = std::env::temp_dir().join(format!(
                "cobol_source_copy_replace_hides_{}_{}",
                clause.to_ascii_lowercase(),
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).expect("temp dir");
            fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
            let mut includes = Vec::new();
            let mut stack = HashSet::new();
            let source = format!("REPLACE =={clause}== BY ====.\nCOPY REC {clause}.\n");
            let result = expand_copybooks(
                &source,
                Some(&dir),
                &[],
                SourceFormat::Free,
                0,
                &mut includes,
                &mut stack,
            );
            assert!(matches!(
                result,
                Err(SourceError::UnsupportedCopyClause { copybook, clause: hidden_clause })
                    if copybook == "REC" && hidden_clause == clause
            ));
            assert!(includes.is_empty());
            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn active_replace_cannot_hide_printing_without_suppress() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_replace_hides_printing_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE ==PRINTING== BY ====.\nCOPY REC PRINTING.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::UnsupportedCopyClause { copybook, clause })
                if copybook == "REC" && clause == "PRINTING"
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_replace_cannot_hide_extra_printing_after_suppress_printing() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_replace_hides_extra_printing_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE ==PRINTING== BY ====.\nCOPY REC SUPPRESS PRINTING PRINTING.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::UnsupportedCopyClause { copybook, clause })
                if copybook == "REC" && clause == "PRINTING"
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_dialect_clause_after_supported_clauses_fails_closed() {
        let parsed = parse_copy_statement("COPY REC OF LIB SUPPRESS PRINTING NOLIST.")
            .expect("copy statement");
        assert_eq!(parsed.name, "REC");
        assert_eq!(parsed.library.as_deref(), Some("LIB"));
        assert!(parsed.suppress);
        assert_eq!(parsed.unsupported_clause.as_deref(), Some("NOLIST"));

        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_supported_then_nolist_{}",
            std::process::id()
        ));
        let lib = dir.join("LIB");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&lib).expect("library dir");
        fs::write(lib.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC OF LIB SUPPRESS PRINTING NOLIST.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::UnsupportedCopyClause { copybook, clause })
                if copybook == "REC" && clause == "NOLIST"
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_replacing_expands_pseudo_text() {
        let dir = std::env::temp_dir().join(format!("cobol_source_copy_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 WS-NAME PIC X(10).\n").expect("copybook");
        let source = "DATA DIVISION.\nCOPY REC REPLACING ==WS-NAME== BY ==WS-OTHER==.\n";
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            source,
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("WS-OTHER"));
        assert!(!expanded.contains("WS-NAME"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_replacing_expands_word_operands() {
        let parsed = parse_copy_statement("COPY REC REPLACING OLD-NAME BY NEW-NAME.")
            .expect("copy statement");
        assert_eq!(
            parsed.replacements,
            vec![CopyReplacement::full("OLD-NAME", "NEW-NAME")]
        );

        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_word_replacing_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 OLD-NAME PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "COPY REC REPLACING OLD-NAME BY NEW-NAME.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 NEW-NAME PIC X."));
        assert!(!expanded.contains("OLD-NAME"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_replacing_expands_literal_operands_without_substring_literal_rewrites() {
        let parsed = parse_copy_statement(
            "COPY REC REPLACING \"OLD VALUE\" BY \"NEW VALUE\" 'A''B' BY 'C''D'.",
        )
        .expect("copy statement");
        assert_eq!(
            parsed.replacements,
            vec![
                CopyReplacement::full_case_sensitive("\"OLD VALUE\"", "\"NEW VALUE\""),
                CopyReplacement::full_case_sensitive("'A''B'", "'C''D'")
            ]
        );

        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_literal_replacing_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(
            dir.join("REC.cpy"),
            "01 A PIC X VALUE \"OLD VALUE\".\n01 B PIC X VALUE \"OLD VALUE EXTRA\".\n01 C PIC X VALUE 'A''B'.\n",
        )
        .expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "COPY REC REPLACING \"OLD VALUE\" BY \"NEW VALUE\" 'A''B' BY 'C''D'.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 A PIC X VALUE \"NEW VALUE\"."));
        assert!(expanded.contains("01 B PIC X VALUE \"OLD VALUE EXTRA\"."));
        assert!(expanded.contains("01 C PIC X VALUE 'C''D'."));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_replacing_literal_operands_are_case_sensitive() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_literal_case_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(
            dir.join("REC.cpy"),
            "01 A PIC X VALUE \"OLD VALUE\".\n01 B PIC X VALUE \"old value\".\n",
        )
        .expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "COPY REC REPLACING \"OLD VALUE\" BY \"NEW VALUE\".\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 A PIC X VALUE \"NEW VALUE\"."));
        assert!(expanded.contains("01 B PIC X VALUE \"old value\"."));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_replacing_literal_pseudo_text_operands_are_case_sensitive() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_literal_pseudo_case_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(
            dir.join("REC.cpy"),
            "01 A PIC X VALUE \"OLD\".\n01 B PIC X VALUE \"old\".\n",
        )
        .expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "COPY REC REPLACING ==\"OLD\"== BY ==\"NEW\"==.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 A PIC X VALUE \"NEW\"."));
        assert!(expanded.contains("01 B PIC X VALUE \"old\"."));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_replacing_mixed_pseudo_text_keeps_literal_case_sensitive() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_mixed_pseudo_case_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(
            dir.join("REC.cpy"),
            "01 A PIC X VALUE \"OLD\".\n01 B PIC X value \"OLD\".\n01 C PIC X VALUE \"old\".\n",
        )
        .expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "COPY REC REPLACING ==VALUE \"OLD\"== BY ==VALUE \"NEW\"==.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 A PIC X VALUE \"NEW\"."));
        assert!(expanded.contains("01 B PIC X VALUE \"NEW\"."));
        assert!(expanded.contains("01 C PIC X VALUE \"old\"."));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_of_library_resolves_relative_library_directory() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_library_relative_{}",
            std::process::id()
        ));
        let lib = dir.join("LIB");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&lib).expect("library dir");
        fs::write(lib.join("REC.cpy"), "01 WS-FIELD PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "COPY REC OF LIB.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 WS-FIELD PIC X."));
        assert_eq!(includes.len(), 1);
        assert!(includes[0].resolved_path.ends_with("LIB/REC.cpy"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn compact_adjacent_copy_fails_closed_when_boundary_could_be_library_extension() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_adjacent_library_extension_{}",
            std::process::id()
        ));
        let lib = dir.join("LIB");
        let lib_copy = dir.join("LIB.COPY");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&lib).expect("library dir");
        fs::create_dir_all(&lib_copy).expect("library extension dir");
        fs::write(lib.join("REC.cpy"), "01 LIB-REC-FIELD PIC X.\n").expect("copybook");
        fs::write(lib_copy.join("REC.cpy"), "01 LIB-COPY-REC-FIELD PIC X.\n").expect("copybook");
        fs::write(dir.join("B.cpy"), "01 B-FIELD PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC OF LIB.COPY B.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyStatement { statement })
                if statement == "COPY REC OF LIB.COPY B."
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_of_library_does_not_fallback_to_base_directory() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_library_no_fallback_{}",
            std::process::id()
        ));
        let lib = dir.join("LIB");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&lib).expect("library dir");
        fs::write(dir.join("REC.cpy"), "01 WRONG-FIELD PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC OF LIB.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::CopybookNotFound { copybook }) if copybook == "REC"
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_member_path_traversal_fails_closed() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_member_traversal_{}",
            std::process::id()
        ));
        let primary = dir.join("primary");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&primary).expect("primary dir");
        fs::write(dir.join("OUTSIDE.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY ../OUTSIDE.\n",
            Some(&primary),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyStatement { statement })
                if statement == "COPY ../OUTSIDE."
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_library_path_traversal_fails_closed() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_library_traversal_{}",
            std::process::id()
        ));
        let primary = dir.join("primary");
        let lib = dir.join("LIB");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&primary).expect("primary dir");
        fs::create_dir_all(&lib).expect("library dir");
        fs::write(lib.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC OF ../LIB.\n",
            Some(&primary),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyStatement { statement })
                if statement == "COPY REC OF ../LIB."
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_member_colon_path_syntax_fails_closed() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC:ALT.\n",
            None,
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyStatement { statement })
                if statement == "COPY REC:ALT."
        ));
        assert!(includes.is_empty());
    }

    #[test]
    fn copy_library_colon_path_syntax_fails_closed() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC OF LIB:ALT.\n",
            None,
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyStatement { statement })
                if statement == "COPY REC OF LIB:ALT."
        ));
        assert!(includes.is_empty());
    }

    #[test]
    fn copy_ambiguous_implicit_member_candidates_fail_closed() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_ambiguous_candidates_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC"), "01 EXTENSIONLESS-FIELD PIC X.\n").expect("copybook");
        fs::write(dir.join("REC.cpy"), "01 CPY-FIELD PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::AmbiguousCopybook { copybook, candidates })
                if copybook == "REC" && candidates.len() == 2
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_ambiguous_search_roots_fail_closed() {
        let root = std::env::temp_dir().join(format!(
            "cobol_source_copy_ambiguous_roots_{}",
            std::process::id()
        ));
        let primary = root.join("primary");
        let secondary = root.join("secondary");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&primary).expect("primary dir");
        fs::create_dir_all(&secondary).expect("secondary dir");
        fs::write(primary.join("REC.cpy"), "01 PRIMARY-FIELD PIC X.\n").expect("copybook");
        fs::write(secondary.join("REC.cpy"), "01 SECONDARY-FIELD PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC.\n",
            Some(&primary),
            &[secondary.clone()],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::AmbiguousCopybook { copybook, candidates })
                if copybook == "REC" && candidates.len() == 2
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn copy_expansion_honors_explicit_unquoted_member_extension() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_explicit_extension_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 WRONG-FIELD PIC X.\n").expect("copybook");
        fs::write(dir.join("REC.cbl"), "01 RIGHT-FIELD PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "COPY REC.cbl.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 RIGHT-FIELD PIC X."));
        assert!(!expanded.contains("WRONG-FIELD"));
        assert!(!expanded.contains("cbl."));
        assert_eq!(includes.len(), 1);
        assert!(includes[0].resolved_path.ends_with("REC.cbl"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_expansion_honors_explicit_copy_extension_without_adjacent_split() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_explicit_copy_extension_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.COPY"), "01 EXPLICIT-COPY-FIELD PIC X.\n").expect("copybook");
        fs::write(dir.join("REC.cpy"), "01 IMPLICIT-CPY-FIELD PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "COPY REC.COPY.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 EXPLICIT-COPY-FIELD PIC X."));
        assert!(!expanded.contains("IMPLICIT-CPY-FIELD"));
        assert_eq!(includes[0].copybook, "REC.COPY");
        assert_eq!(includes.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_expansion_honors_explicit_replace_extension_without_adjacent_split() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_explicit_replace_extension_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(
            dir.join("REC.REPLACE"),
            "01 EXPLICIT-REPLACE-FIELD PIC X.\n",
        )
        .expect("copybook");
        fs::write(dir.join("REC.cpy"), "01 IMPLICIT-CPY-FIELD PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "COPY REC.REPLACE.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 EXPLICIT-REPLACE-FIELD PIC X."));
        assert!(!expanded.contains("IMPLICIT-CPY-FIELD"));
        assert_eq!(includes[0].copybook, "REC.REPLACE");
        assert_eq!(includes.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_resolution_checks_common_dialect_member_extensions() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_common_extensions_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("ACCT.COB"), "01 ACCT-FIELD PIC X.\n").expect("copybook");
        fs::write(dir.join("ADDR.COPY"), "01 ADDR-FIELD PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "COPY ACCT. COPY ADDR.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 ACCT-FIELD PIC X."));
        assert!(expanded.contains("01 ADDR-FIELD PIC X."));
        assert_eq!(includes.len(), 2);
        assert!(includes[0]
            .resolved_path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("ACCT.COB")));
        assert!(includes[1]
            .resolved_path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("ADDR.COPY")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recursive_copy_detection_uses_canonical_resolved_paths() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_recursive_canonical_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.CPY"), "COPY REC.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC.CPY.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::RecursiveCopy { copybook }) if copybook == "REC"
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copybook_path_key_canonicalizes_dot_segments() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_path_key_canonical_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 FIELD PIC X.\n").expect("copybook");
        assert_eq!(
            copybook_path_key(&dir.join("REC.cpy")),
            copybook_path_key(&dir.join(".").join("REC.cpy"))
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_replacing_can_replace_punctuation_pseudo_text() {
        let replaced = apply_copy_replacements(
            "DISPLAY OLD.\n",
            &[CopyReplacement::full(".", " END-DISPLAY.")],
        );
        assert_eq!(replaced, "DISPLAY OLD END-DISPLAY.\n");

        let replaced = apply_copy_replacements(
            "DISPLAY \"OLD.\".\n",
            &[CopyReplacement::full(".", " END-DISPLAY.")],
        );
        assert_eq!(replaced, "DISPLAY \"OLD.\" END-DISPLAY.\n");
    }

    #[test]
    fn copy_replacing_does_not_replace_substrings_or_literals() {
        let text = "01 DATA-NAME PIC X VALUE \"A\".\n01 A PIC X.\n";
        let replaced = apply_copy_replacements(text, &[CopyReplacement::full("A", "B")]);
        assert!(replaced.contains("DATA-NAME"));
        assert!(replaced.contains("\"A\""));
        assert!(replaced.contains("01 B PIC X"));
    }

    #[test]
    fn copy_replacing_does_not_replace_inside_doubled_quote_literals() {
        let text = "01 DATA-NAME PIC X VALUE \"A\"\" OLD-TOKEN\".\n01 OLD-TOKEN PIC X.\n";
        let replaced =
            apply_copy_replacements(text, &[CopyReplacement::full("OLD-TOKEN", "NEW-TOKEN")]);
        assert!(replaced.contains("\"A\"\" OLD-TOKEN\""));
        assert!(replaced.contains("01 NEW-TOKEN PIC X"));
    }

    #[test]
    fn copy_replacing_does_not_replace_inside_doubled_single_quote_literals() {
        let text = "01 DATA-NAME PIC X VALUE 'CAN''T OLD-TOKEN'.\n01 OLD-TOKEN PIC X.\n";
        let replaced =
            apply_copy_replacements(text, &[CopyReplacement::full("OLD-TOKEN", "NEW-TOKEN")]);
        assert!(replaced.contains("'CAN''T OLD-TOKEN'"));
        assert!(replaced.contains("01 NEW-TOKEN PIC X"));
    }

    #[test]
    fn copy_replacing_matches_cobol_words_case_insensitively() {
        let replaced = apply_copy_replacements(
            "01 WS-NAME PIC X.\n",
            &[CopyReplacement::full("ws-name", "WS-OTHER")],
        );
        assert!(replaced.contains("WS-OTHER"));
    }

    #[test]
    fn copy_replacing_respects_cobol_word_boundaries() {
        let replaced = apply_copy_replacements(
            "01 OLD PIC X.\n01 OLD-NAME PIC X.\n01 HOLD PIC X.\n01 OLD_2 PIC X.\n",
            &[CopyReplacement::full("OLD", "NEW")],
        );
        assert!(replaced.contains("01 NEW PIC X"));
        assert!(replaced.contains("01 OLD-NAME PIC X"));
        assert!(replaced.contains("01 HOLD PIC X"));
        assert!(replaced.contains("01 OLD_2 PIC X"));
    }

    #[test]
    fn copy_replacing_matches_pseudo_text_across_whitespace_runs() {
        let replaced = apply_copy_replacements(
            "01 WS-FIELD    PIC     X(5).\n",
            &[CopyReplacement::full("PIC X(5)", "PIC 9(5)")],
        );
        assert_eq!(replaced, "01 WS-FIELD    PIC 9(5).\n");
    }

    #[test]
    fn copy_replacing_matches_pseudo_text_across_line_breaks() {
        let replaced = apply_copy_replacements(
            "01 WS-FIELD\n   PIC X(5).\n",
            &[CopyReplacement::full(
                "WS-FIELD PIC X(5)",
                "WS-RENAMED PIC 9(5)",
            )],
        );
        assert_eq!(replaced, "01 WS-RENAMED PIC 9(5).\n");
    }

    #[test]
    fn copy_replacing_pseudo_text_still_skips_literals() {
        let replaced = apply_copy_replacements(
            "01 A PIC X VALUE \"PIC     X(5)\".\n01 B PIC     X(5).\n",
            &[CopyReplacement::full("PIC X(5)", "PIC 9(5)")],
        );
        assert!(replaced.contains("\"PIC     X(5)\""));
        assert!(replaced.contains("01 B PIC 9(5)."));
    }

    #[test]
    fn copy_replacing_does_not_match_prefix_before_name_character() {
        let replaced = apply_copy_replacements(
            "01 A PIC X(5)X.\n01 B PIC X(5).\n",
            &[CopyReplacement::full("X(5)", "9(5)")],
        );
        assert!(replaced.contains("01 A PIC X(5)X."));
        assert!(replaced.contains("01 B PIC 9(5)."));
    }

    #[test]
    fn replace_directive_applies_until_replace_off() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "replace ==OLD-NAME== BY ==NEW-NAME==.\n01 OLD-NAME PIC X VALUE \"OLD-NAME\".\nRePlAcE oFf.\n01 OLD-NAME PIC X.\n",
            None,
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 NEW-NAME PIC X VALUE \"OLD-NAME\"."));
        assert!(expanded.contains("01 OLD-NAME PIC X."));
        assert!(!expanded.contains("REPLACE"));
    }

    #[test]
    fn replace_directive_accepts_word_and_literal_operands() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "REPLACE OLD-NAME BY NEW-NAME \"OLD VALUE\" BY \"NEW VALUE\".\n01 OLD-NAME PIC X VALUE \"OLD VALUE\".\nREPLACE OFF.\n",
            None,
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 NEW-NAME PIC X VALUE \"NEW VALUE\"."));
        assert!(!expanded.contains("OLD-NAME"));
        assert!(!expanded.contains("\"OLD VALUE\""));
    }

    #[test]
    fn replace_directive_can_delete_pseudo_text() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "REPLACE == DEBUG-LINE== BY ====.\nDISPLAY \"A\" DEBUG-LINE.\nREPLACE OFF.\n",
            None,
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("DISPLAY \"A\"."));
        assert!(!expanded.contains("DEBUG-LINE"));
    }

    #[test]
    fn replace_directive_mixed_pseudo_text_keeps_literal_case_sensitive() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "REPLACE ==VALUE \"OLD\"== BY ==VALUE \"NEW\"==.\n01 A PIC X value \"OLD\".\n01 B PIC X VALUE \"old\".\nREPLACE OFF.\n",
            None,
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 A PIC X VALUE \"NEW\"."));
        assert!(expanded.contains("01 B PIC X VALUE \"old\"."));
    }

    #[test]
    fn replace_directive_supports_leading_and_trailing_partial_words() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "REPLACE LEADING ==OLD== BY ==NEW== TRAILING ==SUF== BY ==END==.\n01 OLD-FIELD PIC X.\n01 MID-SUF PIC X VALUE \"OLD-SUF\".\nREPLACE OFF.\n",
            None,
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 NEW-FIELD PIC X."));
        assert!(expanded.contains("01 MID-END PIC X VALUE \"OLD-SUF\"."));
        assert!(!expanded.contains("01 OLD-FIELD"));
        assert!(!expanded.contains("01 MID-SUF PIC"));
    }

    #[test]
    fn replace_directive_rejects_unmatchable_partial_replacement_source() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE LEADING ==OLD NAME== BY ==NEW==.\n01 OLD-NAME PIC X.\n",
            None,
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedReplaceDirective { statement })
                if statement == "REPLACE LEADING ==OLD NAME== BY ==NEW==."
        ));
    }

    #[test]
    fn replace_directive_applies_to_expanded_copybook_text() {
        let dir =
            std::env::temp_dir().join(format!("cobol_source_replace_copy_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 OLD-NAME PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "REPLACE ==OLD-NAME== BY ==NEW-NAME==.\nCOPY REC.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 NEW-NAME PIC X."));
        assert!(!expanded.contains("OLD-NAME"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_replace_rewrites_copy_member_name_before_resolution() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_copy_member_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 COPIED-FIELD PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "REPLACE ==COPY-MEMBER== BY ==REC==.\nCOPY COPY-MEMBER.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 COPIED-FIELD PIC X."));
        assert_eq!(includes[0].copybook, "REC");
        assert_eq!(includes.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_replace_rewrites_embedded_copy_member_name_before_resolution() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_embedded_copy_member_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 COPIED-FIELD PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "REPLACE ==COPY-MEMBER== BY ==REC==.\n01 PREFIX-FIELD PIC X. COPY COPY-MEMBER.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 PREFIX-FIELD PIC X."));
        assert!(expanded.contains("01 COPIED-FIELD PIC X."));
        assert_eq!(includes[0].copybook, "REC");
        assert_eq!(includes.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_replace_generated_unsupported_copy_clause_fails_closed() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_copy_unsupported_clause_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 COPIED-FIELD PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE ==CLAUSE-MARKER== BY ==PREFIXING==.\nCOPY REC CLAUSE-MARKER.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::UnsupportedCopyClause { copybook, clause })
                if copybook == "REC" && clause == "PREFIXING"
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_replace_generated_copy_member_path_traversal_fails_closed() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_copy_member_traversal_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE ==COPY-MEMBER== BY ==../OUTSIDE==.\nCOPY COPY-MEMBER.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyStatement { statement })
                if statement == "COPY ../OUTSIDE."
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_replace_generated_copy_library_path_traversal_fails_closed() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_copy_library_traversal_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE ==COPY-LIB== BY ==../LIB==.\nCOPY REC OF COPY-LIB.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyStatement { statement })
                if statement == "COPY REC OF ../LIB."
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn replace_directive_on_same_line_applies_to_trailing_copybook() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_same_line_copy_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 OLD-NAME PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "REPLACE ==OLD-NAME== BY ==NEW-NAME==. COPY REC.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 NEW-NAME PIC X."));
        assert!(!expanded.contains("OLD-NAME"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn replace_directive_adjacent_to_copy_period_applies_to_trailing_source() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_adjacent_after_copy_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 COPIED-FIELD PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "COPY REC.REPLACE ==OLD-NAME== BY ==NEW-NAME==. 01 OLD-NAME PIC X.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 COPIED-FIELD PIC X."));
        assert!(expanded.contains("01 NEW-NAME PIC X."));
        assert!(!expanded.contains("OLD-NAME"));
        assert_eq!(includes.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn replace_off_on_same_line_cancels_before_trailing_copybook() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_off_same_line_copy_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 OLD-NAME PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "REPLACE ==OLD-NAME== BY ==NEW-NAME==. REPLACE OFF. COPY REC.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 OLD-NAME PIC X."));
        assert!(!expanded.contains("NEW-NAME"));
        assert_eq!(includes.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn replace_directive_can_generate_standalone_copy_statement() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_generates_copy_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 COPIED-FIELD PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "REPLACE ==COPY-MARKER== BY ==COPY REC.==.\nCOPY-MARKER\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 COPIED-FIELD PIC X."));
        assert!(!expanded.contains("COPY-MARKER"));
        assert!(!expanded.contains("COPY REC"));
        assert_eq!(includes.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn replace_directive_can_generate_multiline_standalone_copy_statement() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_generates_multiline_copy_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 OLD-NAME PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "REPLACE ==COPY-MARKER== BY ==COPY REC==.\nCOPY-MARKER\n    REPLACING ==OLD-NAME== BY ==NEW-NAME==.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 NEW-NAME PIC X."));
        assert!(!expanded.contains("COPY-MARKER"));
        assert!(!expanded.contains("OLD-NAME"));
        assert_eq!(includes.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn generated_malformed_replace_directive_fails_closed() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE ==REPLACE-MARKER== BY ==REPLACE.==.\nREPLACE-MARKER\n01 OLD-NAME PIC X.\n",
            None,
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedReplaceDirective { statement })
                if statement == "REPLACE."
        ));
        assert!(includes.is_empty());
    }

    #[test]
    fn generated_embedded_malformed_replace_directive_fails_closed() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE ==REPLACE-MARKER== BY ==REPLACE.==.\n01 PREFIX-FIELD PIC X. REPLACE-MARKER\n01 OLD-NAME PIC X.\n",
            None,
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedReplaceDirective { statement })
                if statement == "REPLACE."
        ));
        assert!(includes.is_empty());
    }

    #[test]
    fn generated_replace_off_cancels_before_trailing_copybook() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_generates_off_copy_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 OLD-NAME PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "REPLACE ==OLD-NAME== BY ==NEW-NAME== ==OFF-MARKER== BY ==REPLACE OFF.==.\nOFF-MARKER COPY REC.\n01 OLD-NAME PIC X.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert_eq!(expanded.matches("01 OLD-NAME PIC X.").count(), 2);
        assert!(!expanded.contains("NEW-NAME"));
        assert_eq!(includes.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn generated_embedded_replace_off_cancels_before_trailing_copybook() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_generates_embedded_off_copy_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 OLD-NAME PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "REPLACE ==OLD-NAME== BY ==NEW-NAME== ==OFF-MARKER== BY ==REPLACE OFF.==.\n01 PREFIX-FIELD PIC X. OFF-MARKER COPY REC.\n01 OLD-NAME PIC X.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 PREFIX-FIELD PIC X."));
        assert_eq!(expanded.matches("01 OLD-NAME PIC X.").count(), 2);
        assert!(!expanded.contains("NEW-NAME"));
        assert_eq!(includes.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn generated_multiline_copy_statement_applies_replacements_to_continuation_lines() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_generates_copy_continuation_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 OLD-NAME PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "REPLACE ==COPY-MARKER== BY ==COPY REC== ==REPLACING-MARKER== BY ==REPLACING OLD-NAME BY NEW-NAME.==.\nCOPY-MARKER\n    REPLACING-MARKER\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 NEW-NAME PIC X."));
        assert!(!expanded.contains("OLD-NAME"));
        assert!(!expanded.contains("REPLACING-MARKER"));
        assert_eq!(includes.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn generated_unterminated_copy_statement_fails_closed() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE ==COPY-MARKER== BY ==COPY REC==.\nCOPY-MARKER\n    REPLACING ==OLD== BY ==NEW==\n",
            None,
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyStatement { statement })
                if statement.contains("COPY REC") && statement.contains("REPLACING")
        ));
    }

    #[test]
    fn replace_directive_can_generate_embedded_copy_statement() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_generates_embedded_copy_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 COPIED-FIELD PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "REPLACE ==COPY-MARKER== BY ==COPY REC.==.\n01 PREFIX-FIELD PIC X. COPY-MARKER\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 PREFIX-FIELD PIC X."));
        assert!(expanded.contains("01 COPIED-FIELD PIC X."));
        assert!(!expanded.contains("COPY-MARKER"));
        assert!(!expanded.contains("COPY REC"));
        assert_eq!(includes.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn generated_embedded_copy_does_not_reapply_prefix_replacements() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_generates_embedded_copy_prefix_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 COPIED-FIELD PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "REPLACE ==PREFIX-MARKER== BY ==PREFIX-DONE== ==COPY-MARKER== BY ==COPY REC.==.\n01 PREFIX-MARKER PIC X. COPY-MARKER\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 PREFIX-DONE PIC X."));
        assert!(expanded.contains("01 COPIED-FIELD PIC X."));
        assert!(!expanded.contains("PREFIX-MARKER"));
        assert!(!expanded.contains("COPY-MARKER"));
        assert_eq!(expanded.matches("PREFIX-DONE").count(), 1);
        assert_eq!(includes.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn generated_embedded_copy_applies_replacements_to_continuation_lines() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_generates_embedded_copy_continuation_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 OLD-NAME PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "REPLACE ==COPY-MARKER== BY ==COPY REC== ==REPLACING-MARKER== BY ==REPLACING OLD-NAME BY NEW-NAME.==.\n01 PREFIX-FIELD PIC X. COPY-MARKER\n    REPLACING-MARKER\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 PREFIX-FIELD PIC X."));
        assert!(expanded.contains("01 NEW-NAME PIC X."));
        assert!(!expanded.contains("OLD-NAME"));
        assert!(!expanded.contains("REPLACING-MARKER"));
        assert_eq!(includes.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_replace_survives_trailing_source_after_copy() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_after_copy_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 COPIED PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "REPLACE ==OLD-NAME== BY ==NEW-NAME==.\nCOPY REC. 01 OLD-NAME PIC X.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 COPIED PIC X."));
        assert!(expanded.contains("01 NEW-NAME PIC X."));
        assert!(!expanded.contains("OLD-NAME"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_replace_directive_fails_closed() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE ==OLD== WITH ==NEW==.\n01 OLD PIC X.\n",
            None,
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedReplaceDirective { statement })
                if statement.contains("REPLACE")
        ));
    }

    #[test]
    fn replace_without_operands_fails_closed() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE.\n01 OLD PIC X.\n",
            None,
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedReplaceDirective { statement }) if statement == "REPLACE."
        ));
    }

    #[test]
    fn replace_unclosed_quoted_operand_fails_closed() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE \"OLD BY NEW.\n01 OLD PIC X.\n",
            None,
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedReplaceDirective { statement })
                if statement.contains("REPLACE \"OLD BY NEW.")
        ));
    }

    #[test]
    fn replace_also_separator_fails_closed() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE ==OLD== BY ==NEW== ALSO ==A== BY ==B==.\n01 OLD PIC X.\n",
            None,
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedReplaceDirective { statement })
                if statement == "REPLACE ==OLD== BY ==NEW== ALSO ==A== BY ==B==."
        ));
    }

    #[test]
    fn fixed_format_drops_comment_debug_and_page_lines() {
        let raw = "000100 IDENTIFICATION DIVISION.\n000200*COMMENT\n000300/ PAGE\n000400D DISPLAY \"DEBUG\".\n000500 PROGRAM-ID. HELLO.\n";
        assert_eq!(
            normalize_source(raw, SourceFormat::Fixed),
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\n"
        );
    }

    #[test]
    fn fixed_format_utf8_boundary_does_not_leak_after_column_72() {
        let raw = format!("000100 {}éSHOULD-NOT-LEAK\n", "A".repeat(64));
        assert_eq!(
            normalize_source(&raw, SourceFormat::Fixed),
            format!("{}\n", "A".repeat(64))
        );
    }

    #[test]
    fn fixed_format_continuation_stays_on_same_logical_line() {
        let raw = "000100 DISPLAY \"HELLO\n000200-        WORLD\".\n";
        assert_eq!(
            normalize_source(raw, SourceFormat::Fixed),
            "DISPLAY \"HELLO WORLD\".\n"
        );
    }

    #[test]
    fn fixed_format_literal_continuation_drops_repeated_double_quote_delimiter() {
        let raw = "000100 DISPLAY \"HELLO\n000200-        \" WORLD\".\n";
        assert_eq!(
            normalize_source(raw, SourceFormat::Fixed),
            "DISPLAY \"HELLO WORLD\".\n"
        );
    }

    #[test]
    fn fixed_format_literal_continuation_drops_repeated_single_quote_delimiter() {
        let raw = "000100 DISPLAY 'CAN''T\n000200-        ' STOP'.\n";
        assert_eq!(
            normalize_source(raw, SourceFormat::Fixed),
            "DISPLAY 'CAN''T STOP'.\n"
        );
    }

    #[test]
    fn fixed_format_continuation_does_not_split_hyphenated_words() {
        let raw = "000100 MOVE WS-\n000200-        NAME TO OUT-FIELD.\n";
        assert_eq!(
            normalize_source(raw, SourceFormat::Fixed),
            "MOVE WS-NAME TO OUT-FIELD.\n"
        );
    }

    #[test]
    fn fixed_format_continuation_preserves_space_after_minus_operator() {
        let raw = "000100 COMPUTE WS-OUT = WS-A -\n000200-        WS-B.\n";
        assert_eq!(
            normalize_source(raw, SourceFormat::Fixed),
            "COMPUTE WS-OUT = WS-A - WS-B.\n"
        );
    }

    #[test]
    fn fixed_format_continuation_preserves_space_after_numeric_minus_operator() {
        let raw = "000100 COMPUTE WS-OUT = 1-\n000200-        WS-B.\n";
        assert_eq!(
            normalize_source(raw, SourceFormat::Fixed),
            "COMPUTE WS-OUT = 1- WS-B.\n"
        );
    }

    #[test]
    fn fixed_format_comment_only_continuation_does_not_add_blank() {
        let raw = "000100 DISPLAY \"A\"\n000200-        *> comment only\n000300 DISPLAY \"B\".\n";
        assert_eq!(
            normalize_source(raw, SourceFormat::Fixed),
            "DISPLAY \"A\"\nDISPLAY \"B\".\n"
        );
    }

    #[test]
    fn fixed_format_literal_continuation_drops_repeated_delimiter() {
        let raw = "000100 DISPLAY \"HELLO\n000200-        \"WORLD\".\n";
        assert_eq!(
            normalize_source(raw, SourceFormat::Fixed),
            "DISPLAY \"HELLOWORLD\".\n"
        );
    }

    #[test]
    fn fixed_format_literal_continuation_preserves_embedded_doubled_quotes() {
        let raw = "000100 DISPLAY 'CAN\n000200-        '''T STOP'.\n";
        assert_eq!(
            normalize_source(raw, SourceFormat::Fixed),
            "DISPLAY 'CAN''T STOP'.\n"
        );
    }

    #[test]
    fn fixed_format_literal_continuation_keeps_mismatched_delimiter_visible() {
        let raw = "000100 DISPLAY \"HELLO\n000200-        'WORLD\".\n";
        assert_eq!(
            normalize_source(raw, SourceFormat::Fixed),
            "DISPLAY \"HELLO 'WORLD\".\n"
        );
    }

    #[test]
    fn fixed_format_literal_continuation_preserves_comment_marker_as_literal_text() {
        let raw = "000100 DISPLAY \"A\n000200-        *> B\".\n";
        assert_eq!(
            normalize_source(raw, SourceFormat::Fixed),
            "DISPLAY \"A *> B\".\n"
        );
    }

    #[test]
    fn fixed_format_literal_continuation_strips_comment_after_closed_literal() {
        let raw = "000100 DISPLAY \"A\n000200-        \"B\" *> comment\n";
        assert_eq!(
            normalize_source(raw, SourceFormat::Fixed),
            "DISPLAY \"AB\"\n"
        );
    }

    #[test]
    fn free_format_strips_inline_comments_outside_literals_only() {
        let raw = "DISPLAY \"A *> B\" *> comment\nDISPLAY 'C *> D'.\n";
        assert_eq!(
            normalize_source(raw, SourceFormat::Free),
            "DISPLAY \"A *> B\"\nDISPLAY 'C *> D'.\n"
        );
    }

    #[test]
    fn free_format_preserves_comment_marker_inside_copy_pseudo_text() {
        let raw = "COPY REC REPLACING ==OLD *> TOKEN== BY ==NEW==. *> comment\n";
        assert_eq!(
            normalize_source(raw, SourceFormat::Free),
            "COPY REC REPLACING ==OLD *> TOKEN== BY ==NEW==.\n"
        );
    }

    #[test]
    fn fixed_format_strips_inline_comments_outside_literals_only() {
        let raw = "000100 DISPLAY \"A *> B\" *> comment\n000200 DISPLAY 'C *> D'.\n";
        assert_eq!(
            normalize_source(raw, SourceFormat::Fixed),
            "DISPLAY \"A *> B\"\nDISPLAY 'C *> D'.\n"
        );
    }

    #[test]
    fn copy_replacing_statement_can_span_lines() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_multiline_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 OLD-NAME PIC X.\n").expect("copybook");
        let source = "DATA DIVISION.\nCOPY REC\n    REPLACING ==OLD-NAME==\n    BY ==NEW-NAME==.\n";
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            source,
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 NEW-NAME PIC X."));
        assert!(!expanded.contains("OLD-NAME"));
        assert!(!expanded.contains("REPLACING"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_replacing_pseudo_text_delimiter_inside_literal_is_not_terminator() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_pseudo_literal_delim_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 F PIC X VALUE \"A==B\".\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "COPY REC REPLACING ==VALUE \"A==B\"== BY ==VALUE \"C\"==.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 F PIC X VALUE \"C\"."));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_statement_boundary_ignores_periods_inside_literals_and_pseudo_text() {
        let dir =
            std::env::temp_dir().join(format!("cobol_source_copy_periods_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(
            dir.join("REC.cpy"),
            "01 OLD.TOKEN PIC X VALUE \"OLD.TOKEN\".\n",
        )
        .expect("copybook");
        let source =
            "COPY REC\n    REPLACING ==OLD.TOKEN== BY ==NEW.TOKEN==.\nDISPLAY \"AFTER\".\n";
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            source,
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 NEW.TOKEN PIC X VALUE \"OLD.TOKEN\"."));
        assert!(expanded.contains("DISPLAY \"AFTER\"."));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_expansion_preserves_trailing_source_after_copy_period() {
        let dir =
            std::env::temp_dir().join(format!("cobol_source_copy_trailing_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 WS-FIELD PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "COPY REC. DISPLAY \"AFTER\".\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 WS-FIELD PIC X."));
        assert!(expanded.contains("DISPLAY \"AFTER\"."));
        assert!(!expanded.contains("COPY REC"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_expansion_processes_multiple_copy_directives_on_one_line() {
        let dir =
            std::env::temp_dir().join(format!("cobol_source_copy_twice_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("A.cpy"), "01 A-FIELD PIC X.\n").expect("copybook");
        fs::write(dir.join("B.cpy"), "01 B-FIELD PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "COPY A. COPY B.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 A-FIELD PIC X."));
        assert!(expanded.contains("01 B-FIELD PIC X."));
        assert_eq!(includes.len(), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_expansion_processes_adjacent_copy_directives_on_one_line() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_adjacent_twice_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("A.cpy"), "01 A-FIELD PIC X.\n").expect("copybook");
        fs::write(dir.join("B.cpy"), "01 B-FIELD PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "COPY A.COPY B.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 A-FIELD PIC X."));
        assert!(expanded.contains("01 B-FIELD PIC X."));
        assert_eq!(includes.len(), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn compact_adjacent_copy_fails_closed_when_boundary_could_be_member_extension() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_adjacent_member_extension_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("A.COPY"), "01 EXPLICIT-A-COPY PIC X.\n").expect("copybook");
        fs::write(dir.join("B.cpy"), "01 B-FIELD PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY A.COPY B.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyStatement { statement })
                if statement == "COPY A.COPY B."
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn compact_adjacent_replace_fails_closed_when_boundary_could_be_member_extension() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_adjacent_member_extension_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("A.REPLACE"), "01 EXPLICIT-A-REPLACE PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY A.REPLACE OFF.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyStatement { statement })
                if statement == "COPY A.REPLACE OFF."
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_expansion_processes_copy_after_prior_statement_on_same_line() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_embedded_after_statement_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 COPIED-FIELD PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "01 PREFIX-FIELD PIC X. COPY REC.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 PREFIX-FIELD PIC X."));
        assert!(expanded.contains("01 COPIED-FIELD PIC X."));
        assert!(!expanded.contains("COPY REC"));
        assert_eq!(includes.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_expansion_processes_adjacent_copy_after_prior_statement() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_adjacent_after_statement_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 COPIED-FIELD PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "01 PREFIX-FIELD PIC X.COPY REC.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 PREFIX-FIELD PIC X."));
        assert!(expanded.contains("01 COPIED-FIELD PIC X."));
        assert!(!expanded.contains("COPY REC"));
        assert_eq!(includes.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn embedded_copy_statement_can_span_following_lines() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_embedded_copy_multiline_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 OLD-NAME PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "01 PREFIX-FIELD PIC X. COPY REC\n    REPLACING ==OLD-NAME==\n    BY ==NEW-NAME==.\n01 AFTER-FIELD PIC X.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 PREFIX-FIELD PIC X."));
        assert!(expanded.contains("01 NEW-NAME PIC X."));
        assert!(expanded.contains("01 AFTER-FIELD PIC X."));
        assert!(!expanded.contains("REPLACING"));
        assert_eq!(includes.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn replace_directive_after_prior_statement_applies_only_to_trailing_source() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "01 OLD-NAME PIC X. REPLACE ==OLD-NAME== BY ==NEW-NAME==. 01 OLD-NAME PIC X.\n",
            None,
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 OLD-NAME PIC X."));
        assert!(expanded.contains("01 NEW-NAME PIC X."));
        assert!(!expanded.contains("REPLACE"));
    }

    #[test]
    fn embedded_replace_directive_can_span_following_lines() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "01 OLD-NAME PIC X. REPLACE ==OLD-NAME==\n    BY ==NEW-NAME==.\n01 OLD-NAME PIC X.\n",
            None,
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 OLD-NAME PIC X."));
        assert!(expanded.contains("01 NEW-NAME PIC X."));
        assert!(!expanded.contains("REPLACE"));
    }

    #[test]
    fn unterminated_embedded_copy_statement_fails_closed() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "01 PREFIX-FIELD PIC X. COPY REC\n    REPLACING ==OLD== BY ==NEW==\n",
            None,
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyStatement { statement })
                if statement.contains("COPY REC")
        ));
    }

    #[test]
    fn replace_directive_trailing_source_is_not_replaced_twice() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "REPLACE ==.== BY == END.==. DISPLAY \"A\".\n",
            None,
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert_eq!(expanded, "DISPLAY \"A\" END.\n");
    }

    #[test]
    fn embedded_directive_scan_ignores_copy_text_inside_literals() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_embedded_copy_literal_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "DISPLAY \"A. COPY REC.\".\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert_eq!(expanded, "DISPLAY \"A. COPY REC.\".\n");
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_with_unsupported_clause_fails_closed() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_unsupported_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 WS-FIELD PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC UNKNOWN.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::UnsupportedCopyClause { copybook, clause })
                if copybook == "REC" && clause == "UNKNOWN"
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_replacing_rejects_empty_source_pseudo_text() {
        let parsed =
            parse_copy_statement("COPY REC REPLACING ==== BY ==NEW==.").expect("copy statement");
        assert!(parsed.malformed_replacing);
    }

    #[test]
    fn copy_replacing_unclosed_quoted_operand_fails_closed() {
        let parsed =
            parse_copy_statement("COPY REC REPLACING \"OLD BY NEW.").expect("copy statement");
        assert!(parsed.malformed_replacing);

        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_unclosed_replacing_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC REPLACING \"OLD BY NEW.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyStatement { statement })
                if statement.contains("COPY REC REPLACING \"OLD BY NEW.")
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_replace_cannot_terminate_raw_copy_statement() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_replace_terminates_raw_copy_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE ==END-COPY== BY ==.==.\nCOPY REC END-COPY\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyStatement { statement })
                if statement.contains("COPY REC END-COPY")
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_replace_cannot_hide_unknown_raw_copy_clause_by_terminating_early() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_replace_hides_unknown_clause_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE ==END-COPY== BY ==.==.\nCOPY REC END-COPY\n01 AFTER PIC X.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::UnsupportedCopyClause { copybook, clause })
                if copybook == "REC" && clause == "END-COPY"
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_replacing_without_operands_fails_closed() {
        let parsed = parse_copy_statement("COPY REC REPLACING.").expect("copy statement");
        assert!(parsed.malformed_replacing);

        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_empty_replacing_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC REPLACING.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyReplacing { copybook }) if copybook == "REC"
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_replacing_also_separator_fails_closed() {
        let parsed =
            parse_copy_statement("COPY REC REPLACING ==OLD== BY ==NEW== ALSO ==A== BY ==B==.")
                .expect("copy statement");
        assert!(parsed.malformed_replacing);

        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_also_replacing_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC REPLACING ==OLD== BY ==NEW== ALSO ==A== BY ==B==.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyReplacing { copybook }) if copybook == "REC"
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_replace_cannot_hide_malformed_copy_replacing() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_replace_hides_bad_replacing_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE ==REPLACING== BY ====.\nCOPY REC REPLACING.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyReplacing { copybook }) if copybook == "REC"
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_replace_cannot_hide_copy_replacing_also_separator() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_replace_hides_also_replacing_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE ==ALSO== BY ====.\nCOPY REC REPLACING ==OLD== BY ==NEW== ALSO ==A== BY ==B==.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyReplacing { copybook }) if copybook == "REC"
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_replace_cannot_hide_embedded_malformed_copy_replacing() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_embedded_copy_replace_hides_bad_replacing_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE ==REPLACING== BY ====.\n01 PREFIX-FIELD PIC X. COPY REC REPLACING.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyReplacing { copybook }) if copybook == "REC"
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_replacing_leading_partial_word_rewrites_word_prefixes() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_leading_replacing_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(
            dir.join("REC.cpy"),
            "01 OLD-NAME PIC X.\n01 MY-OLD-NAME PIC X.\n01 OLDVALUE PIC X.\n",
        )
        .expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "COPY REC REPLACING LEADING ==OLD== BY ==NEW==.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 NEW-NAME PIC X."));
        assert!(expanded.contains("01 MY-OLD-NAME PIC X."));
        assert!(expanded.contains("01 NEWVALUE PIC X."));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_replacing_rejects_unmatchable_partial_replacement_source() {
        let parsed = parse_copy_statement("COPY REC REPLACING LEADING ==OLD NAME== BY ==NEW==.")
            .expect("copy statement");
        assert!(parsed.malformed_replacing);

        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_bad_partial_replacing_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 OLD-NAME PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC REPLACING LEADING ==OLD NAME== BY ==NEW==.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyReplacing { copybook }) if copybook == "REC"
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_replacing_trailing_partial_word_rewrites_word_suffixes() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_trailing_replacing_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(
            dir.join("REC.cpy"),
            "01 NAME-OLD PIC X.\n01 NAME-OLD-SUFFIX PIC X.\n01 VALUEOLD PIC X.\n",
        )
        .expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "COPY REC REPLACING TRAILING ==OLD== BY ==NEW==.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 NAME-NEW PIC X."));
        assert!(expanded.contains("01 NAME-OLD-SUFFIX PIC X."));
        assert!(expanded.contains("01 VALUENEW PIC X."));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_copy_without_name_fails_closed() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY.\n",
            None,
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyStatement { statement }) if statement == "COPY."
        ));
    }

    #[test]
    fn malformed_copy_empty_quoted_name_fails_closed() {
        assert!(parse_copy_statement("COPY \"\".").is_none());

        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY \"\".\n",
            None,
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyStatement { statement }) if statement == "COPY \"\"."
        ));
        assert!(includes.is_empty());
    }

    #[test]
    fn active_replace_cannot_hide_malformed_copy_empty_quoted_name() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_hides_empty_copy_name_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE ==\"\"== BY ==REC==.\nCOPY \"\".\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyStatement { statement }) if statement == "COPY \"\"."
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_replace_cannot_hide_embedded_malformed_copy_empty_quoted_name() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_hides_embedded_empty_copy_name_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE ==\"\"== BY ==REC==.\n01 PREFIX-FIELD PIC X. COPY \"\".\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyStatement { statement }) if statement == "COPY \"\"."
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_copy_unclosed_quoted_name_fails_closed() {
        assert!(!is_quoted_literal_operand("\"REC"));
        assert!(parse_copy_statement("COPY \"REC.").is_none());
        assert!(parse_copy_statement("COPY \"\"\".").is_none());
    }

    #[test]
    fn malformed_copy_extra_sentence_period_fails_closed() {
        assert!(parse_copy_statement("COPY REC..").is_none());

        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_extra_period_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC..\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyStatement { statement }) if statement == "COPY REC.."
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_member_name_rejects_unquoted_clause_keyword() {
        assert!(parse_copy_statement("COPY OF LIB.").is_none());
        assert!(parse_copy_statement("COPY LIST.").is_none());
        assert!(parse_copy_statement("COPY PREFIXING.").is_none());
        assert!(parse_copy_statement("COPY SUFFIXING.").is_none());

        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_keyword_member_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("OF.cpy"), "01 QUOTED-KEYWORD PIC X.\n").expect("copybook");
        fs::write(dir.join("LIST.cpy"), "01 QUOTED-LIST PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY OF LIB.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyStatement { statement }) if statement == "COPY OF LIB."
        ));
        assert!(includes.is_empty());

        let expanded = expand_copybooks(
            "COPY \"OF\".\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("quoted keyword copybook name expands");
        assert!(expanded.contains("01 QUOTED-KEYWORD PIC X."));
        assert_eq!(includes.len(), 1);
        let expanded = expand_copybooks(
            "COPY \"LIST\".\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("quoted keyword copybook name expands");
        assert!(expanded.contains("01 QUOTED-LIST PIC X."));
        assert_eq!(includes.len(), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_copy_library_clause_fails_closed() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC OF.\n",
            None,
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyStatement { statement }) if statement == "COPY REC OF."
        ));
    }

    #[test]
    fn active_replace_cannot_hide_malformed_copy_library_clauses() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_hides_malformed_copy_library_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        for (replacement_source, copy_source) in [
            ("OF", "COPY REC OF."),
            ("IN", "COPY REC IN."),
            ("OF SUPPRESS", "COPY REC OF SUPPRESS."),
            ("OF LIB:ALT", "COPY REC OF LIB:ALT."),
        ] {
            let mut includes = Vec::new();
            let mut stack = HashSet::new();
            let source = format!("REPLACE =={replacement_source}== BY ====.\n{copy_source}\n");
            let result = expand_copybooks(
                &source,
                Some(&dir),
                &[],
                SourceFormat::Free,
                0,
                &mut includes,
                &mut stack,
            );
            assert!(matches!(
                result,
                Err(SourceError::MalformedCopyStatement { statement }) if statement == copy_source
            ));
            assert!(includes.is_empty());
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_copy_empty_quoted_library_fails_closed() {
        assert!(parse_copy_statement("COPY REC OF \"\".").is_none());

        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_empty_library_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC OF \"\".\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyStatement { statement })
                if statement == "COPY REC OF \"\"."
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_copy_unclosed_quoted_library_fails_closed() {
        assert!(parse_copy_statement("COPY REC OF \"LIB.").is_none());
        assert!(parse_copy_statement("COPY REC OF \"\"\".").is_none());
    }

    #[test]
    fn copy_library_clause_rejects_unquoted_clause_keyword_name() {
        assert!(parse_copy_statement("COPY REC OF SUPPRESS.").is_none());
        assert!(parse_copy_statement("COPY REC OF LIST.").is_none());
        assert!(parse_copy_statement("COPY REC OF PREFIXING.").is_none());
        assert!(parse_copy_statement("COPY REC OF SUFFIXING.").is_none());

        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_keyword_library_{}",
            std::process::id()
        ));
        let lib = dir.join("SUPPRESS");
        let list_lib = dir.join("LIST");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&lib).expect("library dir");
        fs::create_dir_all(&list_lib).expect("library dir");
        fs::write(lib.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        fs::write(list_lib.join("REC.cpy"), "01 QUOTED-LIST-LIB PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC OF SUPPRESS.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyStatement { statement })
                if statement == "COPY REC OF SUPPRESS."
        ));
        assert!(includes.is_empty());

        let expanded = expand_copybooks(
            "COPY REC OF \"SUPPRESS\".\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("quoted keyword library name expands");
        assert!(expanded.contains("01 SHOULD-NOT-EXPAND PIC X."));
        assert_eq!(includes.len(), 1);

        let expanded = expand_copybooks(
            "COPY REC OF \"LIST\".\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("quoted keyword library name expands");
        assert!(expanded.contains("01 QUOTED-LIST-LIB PIC X."));
        assert_eq!(includes.len(), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unterminated_copy_statement_fails_closed() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC REPLACING ==OLD== BY ==NEW==\n",
            None,
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyStatement { statement })
                if statement.contains("COPY REC REPLACING")
        ));
    }
}
