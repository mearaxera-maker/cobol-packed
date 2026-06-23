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
            "ibm" | "zos" | "z-os" | "z_os" | "ibm-zos" | "ibm_zos" => Some(Self::Ibm),
            "gnu" | "gnucobol" | "gnu-cobol" | "gnu_cobol" => Some(Self::GnuCobol),
            "mf" | "microfocus" | "micro-focus" | "micro_focus" => Some(Self::MicroFocus),
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
    pub source_map: Vec<SourceLineOrigin>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLineOrigin {
    pub file: PathBuf,
    pub line: usize,
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

impl SourceError {
    pub fn code(&self) -> &'static str {
        match self {
            SourceError::Io { .. } => "E_SOURCE_IO",
            SourceError::CopybookNotFound { .. } => "E_COPY_NOT_FOUND",
            SourceError::AmbiguousCopybook { .. } => "E_COPY_AMBIGUOUS",
            SourceError::CopyDepthExceeded { .. } => "E_COPY_DEPTH_EXCEEDED",
            SourceError::RecursiveCopy { .. } => "E_COPY_RECURSIVE",
            SourceError::MalformedCopyStatement { .. } => "E_COPY_MALFORMED",
            SourceError::MalformedCopyReplacing { .. } => "E_COPY_REPLACING_MALFORMED",
            SourceError::UnsupportedCopyClause { .. } => "E_COPY_UNSUPPORTED_CLAUSE",
            SourceError::MalformedReplaceDirective { .. } => "E_REPLACE_MALFORMED",
        }
    }

    pub fn suggested_action(&self) -> &'static str {
        match self {
            SourceError::Io { .. } => {
                "Workaround: verify the input path exists and is readable, then rerun conversion."
            }
            SourceError::CopybookNotFound { .. } => {
                "Workaround: add the missing copybook directory with --copybook-dir or correct the COPY member name."
            }
            SourceError::AmbiguousCopybook { .. } => {
                "Workaround: remove duplicate copybook candidates or pass a narrower --copybook-dir list so COPY resolves to one member."
            }
            SourceError::CopyDepthExceeded { .. } => {
                "Workaround: flatten nested COPY usage or reduce recursive copybook inclusion depth before conversion."
            }
            SourceError::RecursiveCopy { .. } => {
                "Workaround: break the recursive COPY chain or replace one edge with an explicit copybook body."
            }
            SourceError::MalformedCopyStatement { .. } => {
                "Workaround: rewrite the COPY statement as `COPY member.` with a valid member name and one terminating period."
            }
            SourceError::MalformedCopyReplacing { .. } => {
                "Workaround: rewrite COPY REPLACING operands as valid pseudo-text or identifiers separated by BY."
            }
            SourceError::UnsupportedCopyClause { .. } => {
                "Workaround: expand this COPY clause before conversion or replace it with supported COPY REPLACING syntax."
            }
            SourceError::MalformedReplaceDirective { .. } => {
                "Workaround: rewrite the REPLACE directive with valid pseudo-text pairs and an OFF terminator if needed."
            }
        }
    }
}

pub fn preprocess_file(
    input: &Path,
    copybook_dirs: &[PathBuf],
    requested_format: SourceFormat,
) -> Result<PreprocessedSource, SourceError> {
    let raw = read_to_string(input)?;
    let format = detect_format(&raw, requested_format);
    let (normalized, line_origins) = normalize_source_with_line_origins(&raw, format);
    let mut includes = Vec::new();
    let mut source_map = Vec::new();
    let mut stack = HashSet::new();
    let expanded = expand_copybooks_with_map(
        &normalized,
        input,
        input.parent(),
        copybook_dirs,
        format,
        0,
        &mut includes,
        &mut source_map,
        &mut stack,
    )?;
    remap_source_map_lines(&mut source_map, 0, input, &line_origins);
    Ok(PreprocessedSource {
        primary_path: input.to_path_buf(),
        text: expanded,
        format,
        includes,
        source_map,
    })
}

pub fn normalize_source(raw: &str, format: SourceFormat) -> String {
    normalize_source_with_line_origins(raw, format).0
}

fn normalize_source_with_line_origins(raw: &str, format: SourceFormat) -> (String, Vec<usize>) {
    match detect_format(raw, format) {
        SourceFormat::Fixed => normalize_fixed_with_line_origins(raw),
        SourceFormat::Free | SourceFormat::Auto => normalize_free_with_line_origins(raw),
    }
}

fn detect_format(raw: &str, requested: SourceFormat) -> SourceFormat {
    if requested != SourceFormat::Auto {
        return requested;
    }
    if let Some(format) = explicit_source_format_directive(raw) {
        return format;
    }
    if source_context_clear_line_matches(raw, looks_like_fixed_indicator_format) {
        return SourceFormat::Fixed;
    }
    if source_context_clear_line_matches(raw, looks_like_plain_free_format) {
        SourceFormat::Free
    } else {
        SourceFormat::Fixed
    }
}

fn explicit_source_format_directive(raw: &str) -> Option<SourceFormat> {
    let mut directive_context = ContinuationContext::default();
    for line in raw.lines() {
        if directive_context == ContinuationContext::default() {
            if let Some(format) = source_format_directive(line) {
                return Some(format);
            }
        }
        directive_context = source_format_directive_context_after_line(directive_context, line);
    }
    None
}

fn copybook_source_format(raw: &str, inherited_format: SourceFormat) -> SourceFormat {
    explicit_source_format_directive(raw).unwrap_or(inherited_format)
}

fn source_context_clear_line_matches(raw: &str, predicate: impl Fn(&str) -> bool) -> bool {
    let mut context = ContinuationContext::default();
    for line in raw.lines() {
        if context == ContinuationContext::default() && predicate(line) {
            return true;
        }
        context = source_format_directive_context_after_line(context, line);
    }
    false
}

fn source_format_directive_context_after_line(
    mut context: ContinuationContext,
    line: &str,
) -> ContinuationContext {
    let Some(line) = source_format_directive_context_text(line) else {
        return context;
    };
    let mut idx = 0usize;
    while idx < line.len() {
        if let Some(active_quote) = context.active_quote {
            let Some(ch) = line[idx..].chars().next() else {
                break;
            };
            let next_idx = idx + ch.len_utf8();
            if ch == active_quote {
                if line[next_idx..].starts_with(active_quote) {
                    idx = next_idx + active_quote.len_utf8();
                    continue;
                }
                context.active_quote = None;
            }
            idx = next_idx;
            continue;
        }
        if context.inside_pseudo_text {
            if line[idx..].starts_with("==") {
                context.inside_pseudo_text = false;
                idx += 2;
                continue;
            }
            let Some(ch) = line[idx..].chars().next() else {
                break;
            };
            if matches!(ch, '\'' | '"') {
                context.active_quote = Some(ch);
            }
            idx += ch.len_utf8();
            continue;
        }
        if line[idx..].starts_with("*>") {
            break;
        }
        if line[idx..].starts_with("==") {
            context.inside_pseudo_text = true;
            idx += 2;
            continue;
        }
        let Some(ch) = line[idx..].chars().next() else {
            break;
        };
        if matches!(ch, '\'' | '"') {
            context.active_quote = Some(ch);
        }
        idx += ch.len_utf8();
    }
    context
}

fn source_format_directive_context_text(line: &str) -> Option<&str> {
    let bytes = line.as_bytes();
    let leading = line.trim_start();
    if leading.starts_with("*>") {
        return None;
    }
    if leading.starts_with(">>") {
        return Some(leading);
    }
    if matches!(bytes.get(6).copied(), Some(b'*' | b'/' | b'D' | b'd')) {
        return None;
    }
    if bytes.len() > 7
        && bytes[..6]
            .iter()
            .all(|byte| byte.is_ascii_digit() || *byte == b' ')
    {
        Some(line.get(7..).unwrap_or(line))
    } else {
        Some(line)
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
    if matches!(
        bytes.get(6).copied(),
        Some(b'*' | b'/' | b'-' | b'D' | b'd')
    ) {
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
    if indicator == b'*'
        && bytes.get(7).is_some_and(|byte| *byte == b'>')
        && sequence_area.iter().all(|byte| *byte == b' ')
    {
        return false;
    }
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

fn normalize_fixed_with_line_origins(raw: &str) -> (String, Vec<usize>) {
    let mut out = String::new();
    let mut line_origins = Vec::new();
    for (line_idx, line) in raw.lines().enumerate() {
        let source_line = line_idx + 1;
        if line.trim_start().starts_with("*>") {
            continue;
        }
        let indicator = line.as_bytes().get(6).copied().unwrap_or(b' ');
        if matches!(indicator, b'*' | b'/' | b'D' | b'd') {
            continue;
        }
        let area = fixed_source_area(line);
        if indicator == b'-' {
            let continued_existing_line = out.ends_with('\n');
            if out.ends_with('\n') {
                out.pop();
            }
            let continuation_context = continuation_context_on_current_line(&out);
            let trimmed =
                strip_source_inline_comment_with_continuation_context(area, continuation_context)
                    .trim_end();
            let continuation = trimmed.trim_start();
            if continuation.is_empty() {
                if !out.is_empty() {
                    out.push('\n');
                }
                continue;
            } else if let Some(quote) = continuation_context
                .active_quote
                .filter(|quote| continuation.starts_with(*quote))
            {
                out.push_str(&continuation[quote.len_utf8()..]);
            } else if continuation_context.inside_pseudo_text && continuation.starts_with("==") {
                out.push_str(continuation);
            } else {
                if !out.is_empty() && !fixed_continuation_joins_hyphenated_word(&out) {
                    out.push(' ');
                }
                out.push_str(continuation);
            }
            out.push('\n');
            if !continued_existing_line {
                line_origins.push(source_line);
            }
        } else {
            let trimmed = cobol_text::strip_inline_comment_outside_literals(area).trim_end();
            if !trimmed.trim().is_empty() {
                out.push_str(trimmed.trim_start());
                out.push('\n');
                line_origins.push(source_line);
            }
        }
    }
    (out, line_origins)
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ContinuationContext {
    active_quote: Option<char>,
    inside_pseudo_text: bool,
}

fn continuation_context_on_current_line(text: &str) -> ContinuationContext {
    let line = text.rsplit_once('\n').map(|(_, line)| line).unwrap_or(text);
    let mut idx = 0usize;
    let mut quote = None;
    let mut inside_pseudo_text = false;
    while idx < line.len() {
        if let Some(active_quote) = quote {
            let Some(ch) = line[idx..].chars().next() else {
                break;
            };
            let next_idx = idx + ch.len_utf8();
            if ch == active_quote {
                if line[next_idx..].starts_with(active_quote) {
                    idx = next_idx + active_quote.len_utf8();
                    continue;
                } else {
                    quote = None;
                }
            }
            idx = next_idx;
            continue;
        }
        if inside_pseudo_text && line[idx..].starts_with("==") {
            inside_pseudo_text = false;
            idx += 2;
            continue;
        }
        let Some(ch) = line[idx..].chars().next() else {
            break;
        };
        if matches!(ch, '\'' | '"') {
            quote = Some(ch);
            idx += ch.len_utf8();
            continue;
        }
        if !inside_pseudo_text && line[idx..].starts_with("==") {
            inside_pseudo_text = true;
            idx += 2;
            continue;
        }
        idx += ch.len_utf8();
    }
    ContinuationContext {
        active_quote: quote,
        inside_pseudo_text,
    }
}

fn normalize_free_with_line_origins(raw: &str) -> (String, Vec<usize>) {
    let mut out = String::new();
    let mut line_origins = Vec::new();
    let mut context = ContinuationContext::default();
    for (line_idx, line) in raw.lines().enumerate() {
        let stripped = strip_source_inline_comment_with_continuation_context(line, context);
        let normalized = if context == ContinuationContext::default() {
            if source_format_directive(stripped).is_some() {
                line_directive_text(stripped).unwrap_or(stripped).trim()
            } else {
                stripped.trim()
            }
        } else {
            stripped.trim_end()
        };
        if context == ContinuationContext::default()
            && (normalized.starts_with("*>") || normalized.is_empty())
        {
            context = source_format_directive_context_after_line(context, stripped);
            continue;
        }
        out.push_str(normalized);
        out.push('\n');
        line_origins.push(line_idx + 1);
        context = source_format_directive_context_after_line(context, normalized);
    }
    (out, line_origins)
}

#[allow(dead_code)]
fn expand_copybooks(
    text: &str,
    primary_dir: Option<&Path>,
    copybook_dirs: &[PathBuf],
    format: SourceFormat,
    depth: usize,
    includes: &mut Vec<IncludeTrace>,
    stack: &mut HashSet<String>,
) -> Result<String, SourceError> {
    let mut source_map = Vec::new();
    let origin = primary_dir.unwrap_or_else(|| Path::new("<source>"));
    expand_copybooks_with_map(
        text,
        origin,
        primary_dir,
        copybook_dirs,
        format,
        depth,
        includes,
        &mut source_map,
        stack,
    )
}

fn expand_copybooks_with_map(
    text: &str,
    origin_path: &Path,
    primary_dir: Option<&Path>,
    copybook_dirs: &[PathBuf],
    format: SourceFormat,
    depth: usize,
    includes: &mut Vec<IncludeTrace>,
    source_map: &mut Vec<SourceLineOrigin>,
    stack: &mut HashSet<String>,
) -> Result<String, SourceError> {
    let mut active_replacements = Vec::new();
    let mut context = CopyExpansionContext {
        copybook_dirs,
        format,
        includes,
        source_map,
        stack,
    };
    expand_copybooks_inner(
        text,
        origin_path,
        1,
        primary_dir,
        depth,
        false,
        false,
        &mut active_replacements,
        &mut context,
    )
}

struct CopyExpansionContext<'a> {
    copybook_dirs: &'a [PathBuf],
    format: SourceFormat,
    includes: &'a mut Vec<IncludeTrace>,
    source_map: &'a mut Vec<SourceLineOrigin>,
    stack: &'a mut HashSet<String>,
}

fn push_mapped_source_line(
    out: &mut String,
    source_map: &mut Vec<SourceLineOrigin>,
    origin_path: &Path,
    origin_line: usize,
    text: &str,
) {
    if text.trim().is_empty() {
        return;
    }
    for line in text.lines() {
        out.push_str(line);
        out.push('\n');
        source_map.push(SourceLineOrigin {
            file: origin_path.to_path_buf(),
            line: origin_line,
        });
    }
}

fn push_mapped_replaced_source_chunk(
    out: &mut String,
    source_map: &mut Vec<SourceLineOrigin>,
    origin_path: &Path,
    origin_line_start: usize,
    original_text: &str,
    replaced_text: &str,
) {
    if original_text.trim().is_empty() || replaced_text.trim().is_empty() {
        return;
    }
    let source_map_start = source_map.len();
    source_map.extend(
        original_text
            .lines()
            .enumerate()
            .map(|(idx, _)| SourceLineOrigin {
                file: origin_path.to_path_buf(),
                line: origin_line_start + idx,
            }),
    );
    adjust_replaced_source_map(source_map, source_map_start, original_text, replaced_text);
    out.push_str(replaced_text);
    if !replaced_text.ends_with('\n') {
        out.push('\n');
    }
}

fn replaced_chunk_line_origins(
    origin_path: &Path,
    origin_line_start: usize,
    original_text: &str,
    replaced_text: &str,
) -> Vec<SourceLineOrigin> {
    let mut line_origins = original_text
        .lines()
        .enumerate()
        .map(|(idx, _)| SourceLineOrigin {
            file: origin_path.to_path_buf(),
            line: origin_line_start + idx,
        })
        .collect::<Vec<_>>();
    adjust_replaced_source_map(&mut line_origins, 0, original_text, replaced_text);
    line_origins
}

fn remap_generated_replaced_chunk_source_map(
    source_map: &mut [SourceLineOrigin],
    start: usize,
    origin_path: &Path,
    replaced_origin_line_start: usize,
    replaced_line_origins: &[SourceLineOrigin],
) {
    for origin in source_map.iter_mut().skip(start) {
        if origin.file == origin_path {
            if let Some(mapped) = origin
                .line
                .checked_sub(replaced_origin_line_start)
                .and_then(|idx| replaced_line_origins.get(idx))
            {
                *origin = mapped.clone();
            }
        }
    }
}

fn adjust_replaced_source_map(
    source_map: &mut Vec<SourceLineOrigin>,
    start: usize,
    original_text: &str,
    replaced_text: &str,
) {
    let original_origins = source_map[start..].to_vec();
    let original_lines = original_text.lines().collect::<Vec<_>>();
    let replaced_lines = replaced_text.lines().collect::<Vec<_>>();
    if original_text == replaced_text {
        return;
    }

    if replaced_lines.is_empty() {
        source_map.truncate(start);
        return;
    }

    let Some(default_origin) = original_origins
        .first()
        .cloned()
        .or_else(|| source_map.get(start.saturating_sub(1)).cloned())
    else {
        return;
    };

    let mut prefix = 0usize;
    while prefix < original_lines.len()
        && prefix < replaced_lines.len()
        && original_lines[prefix] == replaced_lines[prefix]
    {
        prefix += 1;
    }

    let mut suffix = 0usize;
    while suffix < original_lines.len().saturating_sub(prefix)
        && suffix < replaced_lines.len().saturating_sub(prefix)
        && original_lines[original_lines.len() - 1 - suffix]
            == replaced_lines[replaced_lines.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let changed_origin = original_origins
        .get(prefix)
        .cloned()
        .or_else(|| {
            prefix
                .checked_sub(1)
                .and_then(|idx| original_origins.get(idx).cloned())
        })
        .unwrap_or_else(|| default_origin.clone());

    let mut remapped = Vec::with_capacity(replaced_lines.len());
    for idx in 0..prefix {
        remapped.push(
            original_origins
                .get(idx)
                .cloned()
                .unwrap_or_else(|| default_origin.clone()),
        );
    }
    let changed_len = replaced_lines.len().saturating_sub(prefix + suffix);
    remapped.extend(std::iter::repeat_n(changed_origin.clone(), changed_len));
    for idx in 0..suffix {
        let original_idx = original_lines.len() - suffix + idx;
        remapped.push(
            original_origins
                .get(original_idx)
                .cloned()
                .unwrap_or_else(|| changed_origin.clone()),
        );
    }

    source_map.truncate(start);
    source_map.extend(remapped);
}

fn active_replacements_can_span_lines(replacements: &[CopyReplacement]) -> bool {
    replacements.iter().any(|replacement| {
        matches!(replacement.kind, ReplacementKind::Full)
            && replacement.from.chars().any(char::is_whitespace)
    })
}

fn collect_plain_source_chunk(lines: &[&str], start: usize) -> (String, usize) {
    let mut chunk = String::new();
    let mut end = start;
    while end < lines.len() {
        let line = lines[end];
        if end != start
            && (is_copy_statement_start(line)
                || is_replace_directive_start(line)
                || embedded_directive_start(line).is_some())
        {
            break;
        }
        chunk.push_str(line);
        chunk.push('\n');
        end += 1;
    }
    (chunk, end)
}

fn replaced_chunk_contains_generated_directive(text: &str) -> bool {
    text.lines().any(|line| {
        is_copy_statement_start(line)
            || is_replace_directive_start(line)
            || embedded_directive_start(line).is_some()
    })
}

fn remap_source_map_lines(
    source_map: &mut [SourceLineOrigin],
    start: usize,
    file: &Path,
    line_origins: &[usize],
) {
    for origin in source_map.iter_mut().skip(start) {
        if origin.file == file {
            if let Some(line) = origin
                .line
                .checked_sub(1)
                .and_then(|idx| line_origins.get(idx))
            {
                origin.line = *line;
            }
        }
    }
}

fn expand_copybooks_inner(
    text: &str,
    origin_path: &Path,
    origin_line_start: usize,
    primary_dir: Option<&Path>,
    depth: usize,
    copy_replacing_active: bool,
    generated_by_replacement: bool,
    active_replacements: &mut Vec<CopyReplacement>,
    context: &mut CopyExpansionContext<'_>,
) -> Result<String, SourceError> {
    const MAX_COPY_DEPTH: usize = 10;
    let mut out = String::new();
    let lines = text.lines().collect::<Vec<_>>();
    let mut idx = 0usize;
    while idx < lines.len() {
        let origin_line = origin_line_start + idx;
        let line = lines[idx];
        if is_replace_directive_start(line) {
            let statement = collect_directive_source(line, lines.as_slice(), &mut idx);
            let Some(statement_end) = replace_directive_end(&statement) else {
                return Err(SourceError::MalformedReplaceDirective { statement });
            };
            let directive = statement[..statement_end].to_string();
            let trailing_source = statement[statement_end..].trim_start().to_string();
            let trailing_origin_line =
                directive_trailing_origin_line(&statement, statement_end, origin_line);
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
                    origin_path,
                    trailing_origin_line,
                    primary_dir,
                    depth,
                    copy_replacing_active,
                    false,
                    active_replacements,
                    context,
                )?;
                out.push_str(&trailing);
            }
        } else if is_copy_statement_start(line) {
            let raw_copy_source =
                reject_hidden_raw_copy_errors(line, lines.as_slice(), idx, active_replacements)?;
            let statement = if generated_by_replacement {
                collect_generated_directive_source(
                    line,
                    lines.as_slice(),
                    &mut idx,
                    active_replacements,
                )
            } else {
                collect_replaced_directive_source(
                    line,
                    lines.as_slice(),
                    &mut idx,
                    active_replacements,
                )
            };
            let Some(statement_end) = copy_statement_end(&statement) else {
                return Err(classify_unterminated_copy_statement(statement));
            };
            let copy_source = statement[..statement_end].to_string();
            let raw_trailing_source = &statement[statement_end..];
            let trailing_source = raw_trailing_source.trim_start().to_string();
            let trailing_origin_line =
                directive_trailing_origin_line(&statement, statement_end, origin_line);
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
            if copy_replacing_active && !copy_statement.replacements.is_empty() {
                return Err(SourceError::UnsupportedCopyClause {
                    copybook: copy_statement.name.clone(),
                    clause: "REPLACING".to_string(),
                });
            }
            if !generated_by_replacement
                && !active_replacements.is_empty()
                && !copy_statement.replacements.is_empty()
            {
                return Err(SourceError::UnsupportedCopyClause {
                    copybook: copy_statement.name.clone(),
                    clause: "REPLACING".to_string(),
                });
            }
            if !active_replacements.is_empty()
                && !generated_by_replacement
                && raw_copy_source != copy_source
                && (copy_source_has_replacements(&raw_copy_source)
                    || !copy_statement.replacements.is_empty())
            {
                return Err(SourceError::UnsupportedCopyClause {
                    copybook: copy_statement.name.clone(),
                    clause: "REPLACING".to_string(),
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
            let raw = match read_to_string(&resolved) {
                Ok(raw) => raw,
                Err(err) => {
                    context.stack.remove(&key);
                    return Err(err);
                }
            };
            let inherited_format = context.format;
            let copybook_format = copybook_source_format(&raw, inherited_format);
            let (normalized, copybook_line_origins) =
                normalize_source_with_line_origins(&raw, copybook_format);
            let mut copybook_replacements = Vec::new();
            let source_map_start = context.source_map.len();
            context.format = copybook_format;
            let expanded_result = expand_copybooks_inner(
                &normalized,
                &resolved,
                1,
                resolved.parent(),
                depth + 1,
                copy_replacing_active || !copy_statement.replacements.is_empty(),
                false,
                &mut copybook_replacements,
                context,
            );
            context.format = inherited_format;
            context.stack.remove(&key);
            let mut expanded = expanded_result?;
            remap_source_map_lines(
                context.source_map,
                source_map_start,
                &resolved,
                &copybook_line_origins,
            );
            let original_expanded = expanded.clone();
            expanded = apply_copy_replacements(&expanded, &copy_statement.replacements);
            if !active_replacements.is_empty() {
                expanded =
                    apply_active_replacements_to_plain_source(&expanded, active_replacements);
            }
            if replaced_chunk_contains_generated_directive(&expanded) {
                let mut replaced_line_origins = context.source_map[source_map_start..].to_vec();
                adjust_replaced_source_map(
                    &mut replaced_line_origins,
                    0,
                    &original_expanded,
                    &expanded,
                );
                context.source_map.truncate(source_map_start);
                let expanded = expand_copybooks_inner(
                    &expanded,
                    &resolved,
                    1,
                    resolved.parent(),
                    depth + 1,
                    copy_replacing_active || !copy_statement.replacements.is_empty(),
                    true,
                    active_replacements,
                    context,
                )?;
                remap_generated_replaced_chunk_source_map(
                    context.source_map,
                    source_map_start,
                    &resolved,
                    1,
                    &replaced_line_origins,
                );
                out.push_str(&expanded);
                if !expanded.is_empty() && !expanded.ends_with('\n') {
                    out.push('\n');
                }
            } else {
                adjust_replaced_source_map(
                    context.source_map,
                    source_map_start,
                    &original_expanded,
                    &expanded,
                );
                out.push_str(&expanded);
                if !expanded.is_empty() && !expanded.ends_with('\n') {
                    out.push('\n');
                }
            }
            if !trailing_source.is_empty() {
                let trailing = expand_copybooks_inner(
                    &trailing_source,
                    origin_path,
                    trailing_origin_line,
                    primary_dir,
                    depth,
                    copy_replacing_active,
                    generated_by_replacement,
                    active_replacements,
                    context,
                )?;
                out.push_str(&trailing);
            }
        } else if let Some(directive_start) = embedded_directive_start(line) {
            let prefix = apply_copy_replacements(&line[..directive_start], active_replacements);
            push_mapped_source_line(
                &mut out,
                context.source_map,
                origin_path,
                origin_line,
                &prefix,
            );
            let directive_tail = &line[directive_start..];
            let directive_source = if is_copy_statement_start(directive_tail) {
                let raw_copy_source = reject_hidden_raw_copy_errors(
                    directive_tail,
                    lines.as_slice(),
                    idx,
                    active_replacements,
                )?;
                let directive_source = collect_replaced_directive_source(
                    directive_tail,
                    lines.as_slice(),
                    &mut idx,
                    active_replacements,
                );
                if !active_replacements.is_empty() && !generated_by_replacement {
                    if let Some(replaced_end) = copy_statement_end(&directive_source) {
                        let replaced_copy_source = &directive_source[..replaced_end];
                        if let Some(copy_statement) = parse_copy_statement(replaced_copy_source) {
                            if copy_source_has_replacements(&raw_copy_source)
                                || !copy_statement.replacements.is_empty()
                            {
                                return Err(SourceError::UnsupportedCopyClause {
                                    copybook: copy_statement.name,
                                    clause: "REPLACING".to_string(),
                                });
                            }
                        }
                    }
                }
                directive_source
            } else {
                collect_directive_source(directive_tail, lines.as_slice(), &mut idx)
            };
            let suffix = expand_copybooks_inner(
                &directive_source,
                origin_path,
                origin_line,
                primary_dir,
                depth,
                copy_replacing_active,
                generated_by_replacement,
                active_replacements,
                context,
            )?;
            out.push_str(&suffix);
        } else {
            if !active_replacements.is_empty()
                && active_replacements_can_span_lines(active_replacements)
            {
                let (chunk, next_idx) = collect_plain_source_chunk(lines.as_slice(), idx);
                let replaced =
                    apply_active_replacements_to_plain_source(&chunk, active_replacements);
                if replaced_chunk_contains_generated_directive(&replaced) {
                    let source_map_start = context.source_map.len();
                    let replaced_line_origins =
                        replaced_chunk_line_origins(origin_path, origin_line, &chunk, &replaced);
                    let expanded = expand_copybooks_inner(
                        &replaced,
                        origin_path,
                        origin_line,
                        primary_dir,
                        depth,
                        copy_replacing_active,
                        true,
                        active_replacements,
                        context,
                    )?;
                    remap_generated_replaced_chunk_source_map(
                        context.source_map,
                        source_map_start,
                        origin_path,
                        origin_line,
                        &replaced_line_origins,
                    );
                    out.push_str(&expanded);
                    idx = next_idx;
                    continue;
                } else {
                    push_mapped_replaced_source_chunk(
                        &mut out,
                        context.source_map,
                        origin_path,
                        origin_line,
                        &chunk,
                        &replaced,
                    );
                    idx = next_idx;
                    continue;
                }
            }
            let line = apply_active_replacements_to_plain_source(line, active_replacements);
            if is_copy_statement_start(&line) || is_replace_directive_start(&line) {
                let directive_source = collect_generated_directive_source(
                    &line,
                    lines.as_slice(),
                    &mut idx,
                    active_replacements,
                );
                let expanded = expand_copybooks_inner(
                    &directive_source,
                    origin_path,
                    origin_line,
                    primary_dir,
                    depth,
                    copy_replacing_active,
                    true,
                    active_replacements,
                    context,
                )?;
                out.push_str(&expanded);
            } else if let Some(directive_start) = embedded_directive_start(&line) {
                push_mapped_source_line(
                    &mut out,
                    context.source_map,
                    origin_path,
                    origin_line,
                    &line[..directive_start],
                );
                let directive_source = collect_generated_directive_source(
                    &line[directive_start..],
                    lines.as_slice(),
                    &mut idx,
                    active_replacements,
                );
                let expanded = expand_copybooks_inner(
                    &directive_source,
                    origin_path,
                    origin_line,
                    primary_dir,
                    depth,
                    copy_replacing_active,
                    true,
                    active_replacements,
                    context,
                )?;
                out.push_str(&expanded);
            } else {
                push_mapped_source_line(
                    &mut out,
                    context.source_map,
                    origin_path,
                    origin_line,
                    &line,
                );
            }
        }
        idx += 1;
    }
    Ok(out)
}

fn directive_trailing_origin_line(
    statement: &str,
    statement_end: usize,
    origin_line: usize,
) -> usize {
    origin_line
        + statement[..statement_end]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
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
    cobol_text::strip_inline_comment_outside_literals(line)
}

fn strip_source_inline_comment_with_continuation_context(
    line: &str,
    context: ContinuationContext,
) -> &str {
    if context == ContinuationContext::default() {
        return cobol_text::strip_inline_comment_outside_literals(line);
    }

    let mut idx = 0usize;
    let mut quote = context.active_quote;
    let mut inside_pseudo_text = context.inside_pseudo_text;
    let mut continuation_delimiter_pending = context.active_quote;
    let mut unmatched_quote_seen = false;
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
        if inside_pseudo_text {
            if line[idx..].starts_with("==") {
                inside_pseudo_text = false;
                idx += 2;
                continue;
            }
            if matches!(ch, '\'' | '"') {
                quote = Some(ch);
            }
            idx += ch.len_utf8();
            continue;
        }
        if matches!(ch, '\'' | '"') {
            if let Some(end) = cobol_text::complete_quoted_literal_end(line, idx) {
                idx = end;
                continue;
            }
            unmatched_quote_seen = true;
            idx += ch.len_utf8();
            continue;
        }
        if !unmatched_quote_seen && line[idx..].starts_with("==") {
            if let Some(end) = cobol_text::pseudo_text_end(line, idx) {
                idx = end;
                continue;
            }
            idx += ch.len_utf8();
            continue;
        }
        if line[idx..].starts_with("*>") {
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
            idx = cobol_text::pseudo_text_end(statement, idx)?;
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

fn replace_directive_end(statement: &str) -> Option<usize> {
    let mut idx = 0usize;
    while idx < statement.len() {
        if statement[idx..].starts_with("==") {
            idx = cobol_text::pseudo_text_end(statement, idx)?;
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
) -> Result<String, SourceError> {
    let mut raw_idx = idx;
    let raw_statement = collect_directive_source(first_line, lines, &mut raw_idx);
    let Some(statement_end) = copy_statement_end(&raw_statement) else {
        return Err(classify_unterminated_copy_statement(raw_statement));
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
    Ok(raw_copy_source)
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
        "AS" | "DISJOINING"
            | "INDEXED"
            | "JOINING"
            | "LIST"
            | "NOLIST"
            | "PREFIX"
            | "PREFIXING"
            | "RESOURCE"
            | "SUFFIX"
            | "SUFFIXING"
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

fn classify_unterminated_copy_statement(statement: String) -> SourceError {
    if let Some(copybook) = copy_replacing_unclosed_pseudo_text_copybook(&statement) {
        return SourceError::MalformedCopyReplacing { copybook };
    }
    SourceError::MalformedCopyStatement { statement }
}

fn copy_replacing_unclosed_pseudo_text_copybook(statement: &str) -> Option<String> {
    let cleaned = statement.trim();
    let words = cobol_text::split_cobol_words_spanned(cleaned);
    let first = words.first()?;
    if !first.text.eq_ignore_ascii_case("COPY") {
        return None;
    }
    let name = words.get(1)?;
    let copybook = clean_copy_name_operand(&name.text)?;
    let replacing = words
        .iter()
        .find(|word| word.text.eq_ignore_ascii_case("REPLACING"))?;
    contains_unclosed_pseudo_text(&cleaned[replacing.end..]).then_some(copybook)
}

fn contains_unclosed_pseudo_text(text: &str) -> bool {
    let mut idx = 0usize;
    while idx < text.len() {
        if text[idx..].starts_with("==") {
            let Some(end) = cobol_text::pseudo_text_end(text, idx) else {
                return true;
            };
            idx = end;
            continue;
        }
        let Some(ch) = text[idx..].chars().next() else {
            break;
        };
        if matches!(ch, '\'' | '"') {
            if let Some(end) = cobol_text::quoted_literal_end(text, idx) {
                idx = end;
                continue;
            }
        }
        idx += ch.len_utf8();
    }
    false
}

fn embedded_directive_start(line: &str) -> Option<usize> {
    let mut idx = 0usize;
    let mut after_statement_period = false;
    while idx < line.len() {
        if line[idx..].starts_with("==") {
            if let Some(end) = cobol_text::pseudo_text_end(line, idx) {
                idx = end;
                continue;
            }
            return None;
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
    if !is_quoted_literal_operand(&name.text) && is_prefix_copy_dialect_keyword(&name.text) {
        return parse_unsupported_prefix_copy_statement(cleaned, name);
    }
    if !is_quoted_literal_operand(&name.text) && is_copy_clause_keyword(&name.text) {
        return None;
    }
    let copybook_name = clean_copy_name_operand(&name.text)?;
    let remaining = trim_leading_clause_separators(&cleaned[name.end..]).to_string();
    let (library, remaining) = parse_copy_library_clause(&remaining)?;
    let (suppress, remaining) = parse_copy_suppress_clause(&remaining);
    let remaining_words = cobol_text::split_cobol_words_spanned(&remaining);
    let first_clause = remaining_words.first();
    let (replacements, malformed_replacing, unsupported_clause) = match first_clause {
        Some(clause) if clause.text.eq_ignore_ascii_case("REPLACING") => {
            match parse_replacements(&remaining[clause.end..], false) {
                Some(replacements) => (replacements, false, None),
                None => (Vec::new(), true, None),
            }
        }
        Some(clause) => (Vec::new(), false, Some(clause.text.clone())),
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

fn copy_source_has_replacements(copy_source: &str) -> bool {
    parse_copy_statement(copy_source).is_some_and(|statement| !statement.replacements.is_empty())
}

fn is_prefix_copy_dialect_keyword(text: &str) -> bool {
    text.eq_ignore_ascii_case("INDEXED") || text.eq_ignore_ascii_case("RESOURCE")
}

fn parse_unsupported_prefix_copy_statement(
    cleaned: &str,
    prefix: &cobol_text::SpannedWord,
) -> Option<CopyStatement> {
    let remaining = trim_leading_clause_separators(&cleaned[prefix.end..]).to_string();
    let words = cobol_text::split_cobol_words_spanned(&remaining);
    let name = words.first()?;
    if !is_quoted_literal_operand(&name.text) && is_copy_clause_keyword(&name.text) {
        return None;
    }
    let copybook_name = clean_copy_name_operand(&name.text)?;
    let (library, remaining) = parse_copy_library_clause(&remaining[name.end..])?;
    let (suppress, _) = parse_copy_suppress_clause(&remaining);
    Some(CopyStatement {
        name: copybook_name,
        library,
        suppress,
        replacements: Vec::new(),
        malformed_replacing: false,
        unsupported_clause: Some(prefix.text.to_ascii_uppercase()),
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
    let rest = trim_leading_clause_separators(&remaining[library.end..]).to_string();
    Some((Some(library_name), rest))
}

fn is_copy_clause_keyword(text: &str) -> bool {
    text.eq_ignore_ascii_case("IN")
        || text.eq_ignore_ascii_case("OF")
        || text.eq_ignore_ascii_case("SUPPRESS")
        || text.eq_ignore_ascii_case("PRINTING")
        || text.eq_ignore_ascii_case("REPLACING")
        || text.eq_ignore_ascii_case("INDEXED")
        || text.eq_ignore_ascii_case("RESOURCE")
        || text.eq_ignore_ascii_case("LIST")
        || text.eq_ignore_ascii_case("NOLIST")
        || text.eq_ignore_ascii_case("PREFIXING")
        || text.eq_ignore_ascii_case("SUFFIXING")
        || text.eq_ignore_ascii_case("JOINING")
        || text.eq_ignore_ascii_case("DISJOINING")
        || text.eq_ignore_ascii_case("PREFIX")
        || text.eq_ignore_ascii_case("SUFFIX")
        || text.eq_ignore_ascii_case("AS")
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
        return (
            true,
            trim_leading_clause_separators(&remaining[rest_start..]).to_string(),
        );
    }
    (false, remaining.trim().to_string())
}

fn trim_leading_clause_separators(text: &str) -> &str {
    let mut remaining = text.trim_start();
    loop {
        let Some(ch) = remaining.chars().next() else {
            return remaining;
        };
        if matches!(ch, ',' | ';') {
            let next_idx = ch.len_utf8();
            if remaining[next_idx..]
                .chars()
                .next()
                .is_some_and(char::is_whitespace)
            {
                remaining = remaining[next_idx..].trim_start();
                continue;
            }
        }
        return remaining;
    }
}

fn parse_replace_directive(statement: &str) -> Option<ReplaceDirective> {
    let cleaned =
        cobol_text::strip_trailing_sentence_period_outside_literals(statement.trim()).trim();
    let words = cobol_text::split_cobol_words_spanned(cleaned);
    let keyword = words.first()?;
    if !keyword.text.eq_ignore_ascii_case("REPLACE") {
        return None;
    }
    let remaining = trim_leading_clause_separators(&cleaned[keyword.end..]);
    if remaining.eq_ignore_ascii_case("OFF") {
        return Some(ReplaceDirective::Off);
    }
    parse_replacements(remaining, true).map(ReplaceDirective::Set)
}

fn clean_copy_name(name: &str) -> String {
    let name = name.trim().trim_end_matches('.').trim();
    unquote_cobol_literal(name).unwrap_or_else(|| name.to_string())
}

fn clean_copy_name_operand(name: &str) -> Option<String> {
    let name = name.trim();
    let quoted = is_quoted_literal_operand(name);
    if name
        .chars()
        .next()
        .is_some_and(|quote| matches!(quote, '\'' | '"'))
        && !quoted
    {
        return None;
    }
    if !quoted && name.ends_with('.') {
        return None;
    }
    let cleaned = clean_copy_name(name);
    if cleaned.trim().is_empty()
        || !is_safe_copy_path_operand(&cleaned)
        || (!quoted && !is_unquoted_copy_path_operand(&cleaned))
    {
        return None;
    }
    Some(cleaned)
}

fn is_unquoted_copy_path_operand(name: &str) -> bool {
    name.len() <= 30
        && name
            .chars()
            .all(|ch| is_unquoted_copy_path_char(ch) || ch == '.')
        && name.split('.').all(is_valid_unquoted_copy_path_segment)
}

fn is_unquoted_copy_path_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '-'
}

fn is_valid_unquoted_copy_path_segment(segment: &str) -> bool {
    !segment.is_empty() && !segment.starts_with('-') && !segment.ends_with('-')
}

fn is_safe_copy_path_operand(name: &str) -> bool {
    if name.contains('/')
        || name.contains('\\')
        || name.contains(':')
        || has_windows_invalid_filename_char(name)
        || has_windows_ambiguous_terminal_char(name)
        || is_windows_reserved_device_name(name)
    {
        return false;
    }
    let mut components = Path::new(name).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
}

fn has_windows_ambiguous_terminal_char(name: &str) -> bool {
    name.ends_with(' ') || name.ends_with('.')
}

fn has_windows_invalid_filename_char(name: &str) -> bool {
    name.chars()
        .any(|ch| matches!(ch, '<' | '>' | '"' | '|' | '?' | '*') || ch.is_control())
}

fn is_windows_reserved_device_name(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim_end_matches([' ', '.']);
    let upper = stem.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || upper
        .strip_prefix("COM")
        .is_some_and(|suffix| matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"))
        || upper.strip_prefix("LPT").is_some_and(|suffix| {
            matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
        })
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

fn parse_replacements(text: &str, allow_copy_word: bool) -> Option<Vec<CopyReplacement>> {
    let mut replacements = Vec::new();
    let mut rest = trim_leading_clause_separators(text);
    if rest.is_empty() {
        return None;
    }
    while !rest.is_empty() {
        let (kind, replacement_rest) = parse_replacement_kind(rest);
        let (from, from_case_sensitive, from_pseudo_text, after_from) =
            parse_replacement_operand(replacement_rest)?;
        if from.trim().is_empty() {
            return None;
        }
        if !allow_copy_word && contains_reserved_copy_word(&from) {
            return None;
        }
        let is_partial = matches!(kind, ReplacementKind::Leading | ReplacementKind::Trailing);
        if is_partial && !from_pseudo_text {
            return None;
        }
        if matches!(kind, ReplacementKind::Leading | ReplacementKind::Trailing)
            && !is_partial_replacement_source(&from)
        {
            return None;
        }
        let after_by = parse_by_keyword(after_from)?;
        let (to, _, to_pseudo_text, after_to) = parse_replacement_operand(after_by)?;
        if !allow_copy_word && contains_reserved_copy_word(&to) {
            return None;
        }
        if is_partial && !to_pseudo_text {
            return None;
        }
        if is_partial && !is_partial_replacement_target(&to) {
            return None;
        }
        replacements.push(match kind {
            ReplacementKind::Full if from_case_sensitive => {
                CopyReplacement::full_case_sensitive(from, to)
            }
            ReplacementKind::Full => CopyReplacement::full(from, to),
            ReplacementKind::Leading => CopyReplacement::leading(from, to),
            ReplacementKind::Trailing => CopyReplacement::trailing(from, to),
        });
        rest = trim_leading_clause_separators(after_to);
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

fn parse_replacement_operand(text: &str) -> Option<(String, bool, bool, &str)> {
    let text = text.trim_start();
    if text.starts_with("==") {
        let (value, rest) = parse_pseudo_text(text)?;
        let case_sensitive = is_quoted_literal_operand(value.trim());
        return Some((value, case_sensitive, true, rest));
    }
    let words = cobol_text::split_cobol_words_spanned(text);
    let word = words.first()?;
    let case_sensitive = is_quoted_literal_operand(&word.text);
    if matches!(word.text.chars().next(), Some('\'' | '"')) && !case_sensitive {
        return None;
    }
    if !case_sensitive && !is_bare_replacement_word_operand(&word.text) {
        return None;
    }
    Some((word.text.clone(), case_sensitive, false, &text[word.end..]))
}

fn is_bare_replacement_word_operand(text: &str) -> bool {
    !text.is_empty()
        && !is_replacement_control_keyword(text)
        && text
            .chars()
            .all(|ch| is_cobol_name_char(Some(ch)) || is_replacement_nonseparator_char(ch))
}

fn is_replacement_control_keyword(text: &str) -> bool {
    matches!(
        text.to_ascii_uppercase().as_str(),
        "ALSO" | "BY" | "LEADING" | "REPLACING" | "TRAILING"
    )
}

fn is_replacement_nonseparator_char(ch: char) -> bool {
    matches!(ch, '+' | '*' | '/' | '$' | '<' | '>' | '=')
}

fn contains_reserved_copy_word(text: &str) -> bool {
    let mut idx = 0usize;
    while idx < text.len() {
        let Some(ch) = text[idx..].chars().next() else {
            break;
        };
        if matches!(ch, '\'' | '"') {
            if let Some(end) = cobol_text::quoted_literal_end(text, idx) {
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
            if text[start..idx].eq_ignore_ascii_case("COPY") {
                return true;
            }
            continue;
        }
        idx += ch.len_utf8();
    }
    false
}

fn parse_by_keyword(text: &str) -> Option<&str> {
    let text = trim_leading_clause_separators(text);
    let words = cobol_text::split_cobol_words_spanned(text);
    let word = words.first()?;
    if word.text.eq_ignore_ascii_case("BY") {
        Some(trim_leading_clause_separators(&text[word.end..]))
    } else {
        None
    }
}

fn parse_pseudo_text(text: &str) -> Option<(String, &str)> {
    let text = text.trim_start();
    let end = cobol_text::pseudo_text_end(text, 0)?;
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

fn apply_copy_replacements(text: &str, replacements: &[CopyReplacement]) -> String {
    let mut out = text.to_string();
    for replacement in replacements {
        out = apply_copy_replacement(&out, replacement);
    }
    out
}

fn apply_active_replacements_to_plain_source(
    text: &str,
    replacements: &[CopyReplacement],
) -> String {
    if let Some((start, end, replacement_text)) = find_generated_replace_off(text, replacements) {
        let mut out = apply_copy_replacements(&text[..start], replacements);
        out.push_str(&replacement_text);
        out.push_str(&text[end..]);
        out
    } else {
        apply_copy_replacements(text, replacements)
    }
}

fn find_generated_replace_off(
    text: &str,
    replacements: &[CopyReplacement],
) -> Option<(usize, usize, String)> {
    let generated_off_replacements = replacements
        .iter()
        .filter_map(|replacement| {
            replacement_generated_replace_off_text(replacement, replacements)
                .map(|replacement_text| (replacement, replacement_text))
        })
        .collect::<Vec<_>>();
    if generated_off_replacements.is_empty() {
        return None;
    }

    let mut idx = 0usize;
    while idx < text.len() {
        let Some(ch) = text[idx..].chars().next() else {
            break;
        };
        if text[idx..].starts_with("==") {
            if let Some(end) = cobol_text::pseudo_text_end(text, idx) {
                idx = end;
                continue;
            }
            break;
        }
        if matches!(ch, '\'' | '"') {
            if let Some(end) = cobol_text::quoted_literal_end(text, idx) {
                idx = end;
                continue;
            }
        }
        for (replacement, replacement_text) in &generated_off_replacements {
            if let Some(end) =
                match_pseudo_text_at(text, idx, &replacement.from, replacement.case_sensitive)
            {
                return Some((idx, end, replacement_text.clone()));
            }
        }
        idx += ch.len_utf8();
    }
    None
}

fn replacement_generated_replace_off_text(
    replacement: &CopyReplacement,
    replacements: &[CopyReplacement],
) -> Option<String> {
    if !matches!(replacement.kind, ReplacementKind::Full) {
        return None;
    }
    if replacement_text_is_replace_off(&replacement.to) {
        return Some(replacement.to.clone());
    }
    let mut generated = replacement.to.clone();
    for nested in replacements {
        generated = apply_copy_replacement(&generated, nested);
        if replacement_text_is_replace_off(&generated) {
            return Some(generated);
        }
    }
    None
}

fn replacement_text_is_replace_off(generated: &str) -> bool {
    let generated = generated.trim_start();
    if !is_replace_directive_start(generated) {
        return false;
    }
    let Some(statement_end) = copy_statement_end(generated) else {
        return false;
    };
    matches!(
        parse_replace_directive(&generated[..statement_end]),
        Some(ReplaceDirective::Off)
    )
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
        if text[idx..].starts_with("==") {
            if let Some(end) = cobol_text::pseudo_text_end(text, idx) {
                out.push_str(&text[idx..end]);
                idx = end;
                continue;
            }
            out.push_str(&text[idx..]);
            break;
        }
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
        if text[idx..].starts_with("==") {
            if let Some(end) = cobol_text::pseudo_text_end(text, idx) {
                out.push_str(&text[idx..end]);
                idx = end;
                continue;
            }
            out.push_str(&text[idx..]);
            break;
        }
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
    let mut ended_after_pattern_whitespace = false;
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
            ended_after_pattern_whitespace = pattern_idx >= pattern.len();
            continue;
        }
        ended_after_pattern_whitespace = false;

        let text_ch = text[text_idx..].chars().next()?;
        if !chars_eq(pattern_ch, text_ch, case_sensitive) {
            return None;
        }
        pattern_idx += pattern_ch.len_utf8();
        text_idx += text_ch.len_utf8();
        consumed = true;
    }

    if consumed && (ended_after_pattern_whitespace || is_replacement_end_boundary(text, text_idx)) {
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

fn is_partial_replacement_target(text: &str) -> bool {
    text.chars().all(|ch| is_cobol_name_char(Some(ch)))
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
    fn source_error_exposes_stable_code_and_action_metadata() {
        let errors = vec![
            SourceError::Io {
                path: PathBuf::from("missing.cbl"),
                source: std::io::Error::new(std::io::ErrorKind::NotFound, "missing"),
            },
            SourceError::CopybookNotFound {
                copybook: "REC".to_string(),
            },
            SourceError::AmbiguousCopybook {
                copybook: "REC".to_string(),
                candidates: vec![PathBuf::from("a/REC"), PathBuf::from("b/REC")],
            },
            SourceError::CopyDepthExceeded {
                copybook: "REC".to_string(),
                max_depth: 32,
            },
            SourceError::RecursiveCopy {
                copybook: "REC".to_string(),
            },
            SourceError::MalformedCopyStatement {
                statement: "COPY.".to_string(),
            },
            SourceError::MalformedCopyReplacing {
                copybook: "REC".to_string(),
            },
            SourceError::UnsupportedCopyClause {
                copybook: "REC".to_string(),
                clause: "SUPPRESS".to_string(),
            },
            SourceError::MalformedReplaceDirective {
                statement: "REPLACE.".to_string(),
            },
        ];

        let expected_codes = [
            "E_SOURCE_IO",
            "E_COPY_NOT_FOUND",
            "E_COPY_AMBIGUOUS",
            "E_COPY_DEPTH_EXCEEDED",
            "E_COPY_RECURSIVE",
            "E_COPY_MALFORMED",
            "E_COPY_REPLACING_MALFORMED",
            "E_COPY_UNSUPPORTED_CLAUSE",
            "E_REPLACE_MALFORMED",
        ];

        for (error, expected_code) in errors.iter().zip(expected_codes) {
            assert_eq!(error.code(), expected_code);
            assert!(
                error.suggested_action().contains("Workaround:"),
                "{expected_code} missing actionable workaround"
            );
        }
    }

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
    fn auto_format_does_not_treat_indented_free_inline_comment_as_fixed_indicator() {
        let raw = "      *> comment\n      IDENTIFICATION DIVISION.\n      PROGRAM-ID. HELLO.\n";
        assert_eq!(detect_format(raw, SourceFormat::Auto), SourceFormat::Free);
        assert_eq!(
            normalize_source(raw, SourceFormat::Auto),
            "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\n"
        );
    }

    #[test]
    fn auto_format_does_not_treat_fixed_indicator_text_inside_literal_as_fixed() {
        let raw = "DISPLAY \"A\n000100*NOT A FIXED COMMENT\nDISPLAY \"B\".\n";
        assert_eq!(detect_format(raw, SourceFormat::Auto), SourceFormat::Free);
        assert_eq!(
            normalize_source(raw, SourceFormat::Auto),
            "DISPLAY \"A\n000100*NOT A FIXED COMMENT\nDISPLAY \"B\".\n"
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
    fn auto_format_ignores_source_format_directive_on_fixed_continuation_line() {
        let raw = "000100 DISPLAY \"A\n000200-       >>SOURCE FORMAT FREE\n000300-       \"\".\n";
        assert_eq!(detect_format(raw, SourceFormat::Auto), SourceFormat::Fixed);
        assert_eq!(
            normalize_source(raw, SourceFormat::Auto),
            "DISPLAY \"A >>SOURCE FORMAT FREE\".\n"
        );
    }

    #[test]
    fn auto_format_ignores_source_format_directive_inside_unclosed_literal() {
        let raw = "DISPLAY \"A\n>>SOURCE FORMAT FIXED\nDISPLAY \"B\".\n";
        assert_eq!(detect_format(raw, SourceFormat::Auto), SourceFormat::Free);
        assert_eq!(
            normalize_source(raw, SourceFormat::Auto),
            "DISPLAY \"A\n>>SOURCE FORMAT FIXED\nDISPLAY \"B\".\n"
        );
    }

    #[test]
    fn auto_format_ignores_source_format_directive_inside_unclosed_pseudo_text() {
        let raw = "COPY REC REPLACING ==OLD\n>>SOURCE FORMAT FIXED\nTOKEN== BY ==NEW==.\n";
        assert_eq!(detect_format(raw, SourceFormat::Auto), SourceFormat::Free);
        assert_eq!(
            normalize_source(raw, SourceFormat::Auto),
            "COPY REC REPLACING ==OLD\n>>SOURCE FORMAT FIXED\nTOKEN== BY ==NEW==.\n"
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
    fn auto_format_strips_sequence_area_from_fixed_position_free_directive() {
        let raw =
            "000100 >>SOURCE FORMAT FREE\n000200 IDENTIFICATION DIVISION.\n000300 PROGRAM-ID. HELLO.\n";
        assert_eq!(detect_format(raw, SourceFormat::Auto), SourceFormat::Free);
        assert_eq!(
            normalize_source(raw, SourceFormat::Auto),
            ">>SOURCE FORMAT FREE\n000200 IDENTIFICATION DIVISION.\n000300 PROGRAM-ID. HELLO.\n"
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
    fn copy_semicolon_separator_between_clauses_expands() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_semicolon_separator_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 OLD-NAME PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "COPY REC; REPLACING ==OLD-NAME== BY ==NEW-NAME==.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("semicolon separator copy expands");
        assert!(expanded.contains("01 NEW-NAME PIC X."));
        assert!(!expanded.contains("OLD-NAME"));
        assert_eq!(includes.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_replacing_accepts_separator_punctuation_between_pairs() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_replacing_pair_separators_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(
            dir.join("REC.cpy"),
            "01 OLD-NAME PIC X.\n01 OLD-VALUE PIC X.\n",
        )
        .expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "COPY REC REPLACING ==OLD-NAME==; BY ==NEW-NAME==, ==OLD-VALUE== BY ==NEW-VALUE==.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("replacement separator copy expands");
        assert!(expanded.contains("01 NEW-NAME PIC X."));
        assert!(expanded.contains("01 NEW-VALUE PIC X."));
        assert!(!expanded.contains("OLD-NAME"));
        assert!(!expanded.contains("OLD-VALUE"));
        assert_eq!(includes.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_replacing_accepts_separator_after_keyword() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_replacing_keyword_separator_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 OLD-NAME PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "COPY REC REPLACING, ==OLD-NAME== BY ==NEW-NAME==.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("replacement keyword separator copy expands");
        assert!(expanded.contains("01 NEW-NAME PIC X."));
        assert!(!expanded.contains("OLD-NAME"));
        assert_eq!(includes.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_member_comma_without_separator_space_fails_closed() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_member_comma_no_space_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC,.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyStatement { statement }) if statement == "COPY REC,."
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
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
        for clause in [
            "AS",
            "DISJOINING",
            "INDEXED",
            "JOINING",
            "LIST",
            "NOLIST",
            "PRINTING",
            "PREFIX",
            "PREFIXING",
            "RESOURCE",
            "SUFFIX",
            "SUFFIXING",
        ] {
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
        for clause in [
            "AS",
            "DISJOINING",
            "INDEXED",
            "JOINING",
            "LIST",
            "NOLIST",
            "PRINTING",
            "PREFIX",
            "PREFIXING",
            "RESOURCE",
            "SUFFIX",
            "SUFFIXING",
        ] {
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
    fn active_replace_cannot_hide_dialect_clause_after_supported_clauses() {
        let unsupported_after_suppress = [
            "AS",
            "DISJOINING",
            "INDEXED",
            "JOINING",
            "LIST",
            "NOLIST",
            "PREFIX",
            "PREFIXING",
            "RESOURCE",
            "SUFFIX",
            "SUFFIXING",
        ];
        let mut cases = Vec::new();
        for clause in unsupported_after_suppress {
            cases.push((clause, format!("COPY REC SUPPRESS {clause}.\n")));
            cases.push((
                clause,
                format!("01 PREFIX-FIELD PIC X. COPY REC SUPPRESS {clause}.\n"),
            ));
            cases.push((clause, format!("COPY REC OF LIB SUPPRESS {clause}.\n")));
            cases.push((
                clause,
                format!("01 PREFIX-FIELD PIC X. COPY REC IN LIB SUPPRESS {clause}.\n"),
            ));
        }
        cases.extend([
            (
                "NOLIST",
                "COPY REC OF LIB SUPPRESS PRINTING NOLIST.\n".to_string(),
            ),
            (
                "NOLIST",
                "01 PREFIX-FIELD PIC X. COPY REC IN LIB SUPPRESS PRINTING NOLIST.\n".to_string(),
            ),
            (
                "PRINTING",
                "COPY REC OF LIB SUPPRESS PRINTING PRINTING.\n".to_string(),
            ),
            (
                "PRINTING",
                "01 PREFIX-FIELD PIC X. COPY REC IN LIB SUPPRESS PRINTING PRINTING.\n".to_string(),
            ),
        ]);
        for (clause, source) in cases {
            let dir = std::env::temp_dir().join(format!(
                "cobol_source_copy_replace_hides_supported_then_{}_{}",
                clause.to_ascii_lowercase(),
                std::process::id()
            ));
            let lib = dir.join("LIB");
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&lib).expect("library dir");
            fs::write(lib.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
            let mut includes = Vec::new();
            let mut stack = HashSet::new();
            let input = format!("REPLACE =={clause}== BY ====.\n{source}");
            let result = expand_copybooks(
                &input,
                Some(&dir),
                &[],
                SourceFormat::Free,
                0,
                &mut includes,
                &mut stack,
            );
            assert!(matches!(
                result,
                Err(SourceError::UnsupportedCopyClause {
                    copybook,
                    clause: hidden_clause,
                }) if copybook == "REC" && hidden_clause == clause
            ));
            assert!(includes.is_empty());
            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn copy_dialect_clause_after_supported_clauses_fails_closed() {
        for clause in [
            "AS",
            "DISJOINING",
            "INDEXED",
            "JOINING",
            "LIST",
            "NOLIST",
            "PREFIX",
            "PREFIXING",
            "RESOURCE",
            "SUFFIX",
            "SUFFIXING",
        ] {
            for (source, library) in [
                (format!("COPY REC SUPPRESS {clause}.\n"), None),
                (format!("COPY REC OF LIB SUPPRESS {clause}.\n"), Some("LIB")),
                (format!("COPY REC IN LIB SUPPRESS {clause}.\n"), Some("LIB")),
            ] {
                let parsed = parse_copy_statement(&source).expect("copy statement");
                assert_eq!(parsed.name, "REC");
                assert_eq!(parsed.library.as_deref(), library);
                assert!(parsed.suppress);
                assert_eq!(parsed.unsupported_clause.as_deref(), Some(clause));

                let dir = std::env::temp_dir().join(format!(
                    "cobol_source_copy_supported_then_{}_{}",
                    clause.to_ascii_lowercase(),
                    std::process::id()
                ));
                let lib = dir.join("LIB");
                let _ = fs::remove_dir_all(&dir);
                fs::create_dir_all(&dir).expect("temp dir");
                fs::create_dir_all(&lib).expect("library dir");
                fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
                fs::write(lib.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
                let mut includes = Vec::new();
                let mut stack = HashSet::new();
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
                    Err(SourceError::UnsupportedCopyClause {
                        copybook,
                        clause: actual_clause,
                    }) if copybook == "REC" && actual_clause == clause
                ));
                assert!(includes.is_empty());
                let _ = fs::remove_dir_all(&dir);
            }
        }
        for (source, library, clause) in [
            (
                "COPY REC OF LIB SUPPRESS PRINTING NOLIST.\n",
                Some("LIB"),
                "NOLIST",
            ),
            (
                "COPY REC IN LIB SUPPRESS PRINTING PRINTING.\n",
                Some("LIB"),
                "PRINTING",
            ),
        ] {
            let parsed = parse_copy_statement(source).expect("copy statement");
            assert_eq!(parsed.name, "REC");
            assert_eq!(parsed.library.as_deref(), library);
            assert!(parsed.suppress);
            assert_eq!(parsed.unsupported_clause.as_deref(), Some(clause));

            let dir = std::env::temp_dir().join(format!(
                "cobol_source_copy_supported_then_{}_{}",
                clause.to_ascii_lowercase(),
                std::process::id()
            ));
            let lib = dir.join("LIB");
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).expect("temp dir");
            fs::create_dir_all(&lib).expect("library dir");
            fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
            fs::write(lib.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
            let mut includes = Vec::new();
            let mut stack = HashSet::new();
            let result = expand_copybooks(
                source,
                Some(&dir),
                &[],
                SourceFormat::Free,
                0,
                &mut includes,
                &mut stack,
            );
            assert!(matches!(
                result,
                Err(SourceError::UnsupportedCopyClause {
                    copybook,
                    clause: actual_clause,
                }) if copybook == "REC" && actual_clause == clause
            ));
            assert!(includes.is_empty());
            let _ = fs::remove_dir_all(&dir);
        }
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
    fn active_replace_with_copy_replacing_fails_closed() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_active_replace_copy_replacing_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 OLD-NAME PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE ==OLD-NAME== BY ==GLOBAL-NAME==.\nCOPY REC REPLACING ==OLD-NAME== BY ==LOCAL-NAME==.\n",
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
                if copybook == "REC" && clause == "REPLACING"
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_replace_cannot_remove_copy_replacing_clause() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_active_replace_removes_copy_replacing_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 OLD-NAME PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE ==REPLACING== BY ==== ==OLD-NAME== BY ==== ==BY== BY ==== ==LOCAL-NAME== BY ====.\nCOPY REC REPLACING OLD-NAME BY LOCAL-NAME.\n",
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
                if copybook == "REC" && clause == "REPLACING"
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_replace_with_embedded_copy_replacing_fails_closed() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_active_replace_embedded_copy_replacing_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 OLD-NAME PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE ==OLD-NAME== BY ==GLOBAL-NAME==.\n01 PREFIX-FIELD PIC X. COPY REC REPLACING ==OLD-NAME== BY ==LOCAL-NAME==.\n",
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
                if copybook == "REC" && clause == "REPLACING"
        ));
        assert!(includes.is_empty());
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
    fn copy_replacing_accepts_nonseparator_operand_characters() {
        let parsed =
            parse_copy_statement("COPY REC REPLACING + BY - $ BY =.").expect("copy statement");
        assert_eq!(
            parsed.replacements,
            vec![
                CopyReplacement::full("+", "-"),
                CopyReplacement::full("$", "=")
            ]
        );

        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_nonseparator_replacing_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "COMPUTE WS-A = 1 + 2.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "COPY REC REPLACING + BY -.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("COMPUTE WS-A = 1 - 2."));
        assert!(!expanded.contains("1 + 2"));
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
    fn copy_member_windows_reserved_device_name_fails_closed() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY CON.\n",
            None,
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyStatement { statement }) if statement == "COPY CON."
        ));
        assert!(includes.is_empty());
    }

    #[test]
    fn copy_library_windows_reserved_device_name_fails_closed() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC OF NUL.\n",
            None,
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyStatement { statement }) if statement == "COPY REC OF NUL."
        ));
        assert!(includes.is_empty());
    }

    #[test]
    fn quoted_copy_member_windows_reserved_device_alias_fails_closed() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        for source in ["COPY \"CON \".\n", "COPY \"CONOUT$\".\n"] {
            let result = expand_copybooks(
                source,
                None,
                &[],
                SourceFormat::Free,
                0,
                &mut includes,
                &mut stack,
            );
            assert!(
                matches!(result, Err(SourceError::MalformedCopyStatement { ref statement }) if statement == source.trim()),
                "expected malformed COPY for {source:?}, got {result:?}"
            );
            assert!(includes.is_empty());
        }
    }

    #[test]
    fn quoted_copy_member_trailing_space_or_dot_fails_closed() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        for source in ["COPY \"REC \".\n", "COPY \"REC.\".\n"] {
            let result = expand_copybooks(
                source,
                None,
                &[],
                SourceFormat::Free,
                0,
                &mut includes,
                &mut stack,
            );
            assert!(
                matches!(result, Err(SourceError::MalformedCopyStatement { ref statement }) if statement == source.trim()),
                "expected malformed COPY for {source:?}, got {result:?}"
            );
            assert!(includes.is_empty());
        }
    }

    #[test]
    fn quoted_copy_library_trailing_space_or_dot_fails_closed() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        for source in ["COPY REC OF \"LIB \".\n", "COPY REC OF \"LIB.\".\n"] {
            let result = expand_copybooks(
                source,
                None,
                &[],
                SourceFormat::Free,
                0,
                &mut includes,
                &mut stack,
            );
            assert!(
                matches!(result, Err(SourceError::MalformedCopyStatement { ref statement }) if statement == source.trim()),
                "expected malformed COPY for {source:?}, got {result:?}"
            );
            assert!(includes.is_empty());
        }
    }

    #[test]
    fn quoted_copy_member_windows_invalid_filename_char_fails_closed() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        for source in ["COPY \"REC*ALT\".\n", "COPY \"REC?ALT\".\n"] {
            let result = expand_copybooks(
                source,
                None,
                &[],
                SourceFormat::Free,
                0,
                &mut includes,
                &mut stack,
            );
            assert!(
                matches!(result, Err(SourceError::MalformedCopyStatement { ref statement }) if statement == source.trim()),
                "expected malformed COPY for {source:?}, got {result:?}"
            );
            assert!(includes.is_empty());
        }
    }

    #[test]
    fn quoted_copy_library_windows_invalid_filename_char_fails_closed() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        for source in ["COPY REC OF \"LIB|ALT\".\n", "COPY REC OF \"LIB<ALT\".\n"] {
            let result = expand_copybooks(
                source,
                None,
                &[],
                SourceFormat::Free,
                0,
                &mut includes,
                &mut stack,
            );
            assert!(
                matches!(result, Err(SourceError::MalformedCopyStatement { ref statement }) if statement == source.trim()),
                "expected malformed COPY for {source:?}, got {result:?}"
            );
            assert!(includes.is_empty());
        }
    }

    #[test]
    fn quoted_copy_embedded_double_quote_filename_char_fails_closed() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        for source in ["COPY \"REC\"\"ALT\".\n", "COPY REC OF \"LIB\"\"ALT\".\n"] {
            let result = expand_copybooks(
                source,
                None,
                &[],
                SourceFormat::Free,
                0,
                &mut includes,
                &mut stack,
            );
            assert!(
                matches!(result, Err(SourceError::MalformedCopyStatement { ref statement }) if statement == source.trim()),
                "expected malformed COPY for {source:?}, got {result:?}"
            );
            assert!(includes.is_empty());
        }
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
    fn copy_member_unquoted_punctuation_fails_closed() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_member_punctuation_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC;ALT.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC;ALT.\n",
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
                if statement == "COPY REC;ALT."
        ));
        assert!(includes.is_empty());

        let expanded = expand_copybooks(
            "COPY \"REC;ALT\".\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("quoted punctuation copybook name expands");
        assert!(expanded.contains("01 SHOULD-NOT-EXPAND PIC X."));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_member_unquoted_edge_hyphen_fails_closed() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_member_edge_hyphen_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("-REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        fs::write(dir.join("REC-.cpy"), "01 TRAILING-HYPHEN PIC X.\n").expect("copybook");

        for source in ["COPY -REC.\n", "COPY REC-.\n"] {
            let mut includes = Vec::new();
            let mut stack = HashSet::new();
            let result = expand_copybooks(
                source,
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
                    if statement == source.trim()
            ));
            assert!(includes.is_empty());
        }

        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "COPY \"-REC\".\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("quoted edge-hyphen copybook name expands");
        assert!(expanded.contains("01 SHOULD-NOT-EXPAND PIC X."));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_member_unquoted_underscore_fails_closed() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_member_underscore_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC_FILE.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");

        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC_FILE.\n",
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
                if statement == "COPY REC_FILE."
        ));
        assert!(includes.is_empty());

        let expanded = expand_copybooks(
            "COPY \"REC_FILE\".\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("quoted underscore copybook name expands");
        assert!(expanded.contains("01 SHOULD-NOT-EXPAND PIC X."));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_member_unquoted_overlength_fails_closed() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_member_overlength_{}",
            std::process::id()
        ));
        let long_name = "RECNAME123456789012345678901234";
        assert_eq!(long_name.len(), 31);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(
            dir.join(format!("{long_name}.cpy")),
            "01 SHOULD-NOT-EXPAND PIC X.\n",
        )
        .expect("copybook");

        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            &format!("COPY {long_name}.\n"),
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
                if statement == format!("COPY {long_name}.")
        ));
        assert!(includes.is_empty());

        let expanded = expand_copybooks(
            &format!("COPY \"{long_name}\".\n"),
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("quoted overlength copybook name expands");
        assert!(expanded.contains("01 SHOULD-NOT-EXPAND PIC X."));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_member_unquoted_empty_dot_segment_fails_closed() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_member_empty_dot_segment_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC..COPY"), "01 QUOTED-ONLY PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC..COPY.\n",
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
                if statement == "COPY REC..COPY."
        ));
        assert!(includes.is_empty());

        let expanded = expand_copybooks(
            "COPY \"REC..COPY\".\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("quoted copybook name with repeated dot expands");
        assert!(expanded.contains("01 QUOTED-ONLY PIC X."));
        let _ = fs::remove_dir_all(&dir);
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
    fn copy_library_unquoted_punctuation_fails_closed() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_library_punctuation_{}",
            std::process::id()
        ));
        let lib = dir.join("LIB;ALT");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&lib).expect("library dir");
        fs::write(lib.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC OF LIB;ALT.\n",
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
                if statement == "COPY REC OF LIB;ALT."
        ));
        assert!(includes.is_empty());

        let expanded = expand_copybooks(
            "COPY REC OF \"LIB;ALT\".\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("quoted punctuation library name expands");
        assert!(expanded.contains("01 SHOULD-NOT-EXPAND PIC X."));
        let _ = fs::remove_dir_all(&dir);
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
            std::slice::from_ref(&secondary),
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
    fn failed_nested_copy_expansion_clears_recursion_stack() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_failed_nested_stack_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("OUTER.cpy"), "COPY MISSING.\n").expect("outer copybook");
        let origin = dir.join("MAIN.cbl");
        let mut includes = Vec::new();
        let mut source_map = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks_with_map(
            "COPY OUTER.\n",
            &origin,
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut source_map,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::CopybookNotFound { copybook }) if copybook == "MISSING"
        ));
        assert!(stack.is_empty(), "recursion stack leaked after COPY error");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_trailing_source_map_keeps_original_source_line() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_trailing_source_map_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 COPIED-FIELD PIC X.\n").expect("copybook");
        let origin = dir.join("MAIN.cbl");
        let copybook = dir.join("REC.cpy");
        let mut includes = Vec::new();
        let mut source_map = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks_with_map(
            "01 BEFORE PIC X.\n01 SECOND PIC X.\nCOPY REC. 01 AFTER PIC X.\n",
            &origin,
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut source_map,
            &mut stack,
        )
        .expect("expanded");
        assert_eq!(
            expanded,
            "01 BEFORE PIC X.\n01 SECOND PIC X.\n01 COPIED-FIELD PIC X.\n01 AFTER PIC X.\n"
        );
        assert_eq!(
            source_map,
            vec![
                SourceLineOrigin {
                    file: origin.clone(),
                    line: 1,
                },
                SourceLineOrigin {
                    file: origin.clone(),
                    line: 2,
                },
                SourceLineOrigin {
                    file: copybook,
                    line: 1,
                },
                SourceLineOrigin {
                    file: origin,
                    line: 3,
                },
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn embedded_copy_source_map_keeps_prefix_origin_line() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_embedded_copy_source_map_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 COPIED-FIELD PIC X.\n").expect("copybook");
        let origin = dir.join("MAIN.cbl");
        let copybook = dir.join("REC.cpy");
        let mut includes = Vec::new();
        let mut source_map = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks_with_map(
            "01 PREFIX-FIELD PIC X. COPY REC.\n",
            &origin,
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut source_map,
            &mut stack,
        )
        .expect("expanded");
        assert_eq!(
            expanded,
            "01 PREFIX-FIELD PIC X. \n01 COPIED-FIELD PIC X.\n"
        );
        assert_eq!(
            source_map,
            vec![
                SourceLineOrigin {
                    file: origin,
                    line: 1,
                },
                SourceLineOrigin {
                    file: copybook,
                    line: 1,
                },
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_replacing_multiline_output_extends_source_map() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_replacing_multiline_source_map_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 OLD PIC X.\n").expect("copybook");
        let origin = dir.join("MAIN.cbl");
        let copybook = dir.join("REC.cpy");
        let mut includes = Vec::new();
        let mut source_map = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks_with_map(
            "COPY REC REPLACING ==01 OLD PIC X.== BY ==01 NEW PIC X.\n01 INSERT PIC X.==.\n",
            &origin,
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut source_map,
            &mut stack,
        )
        .expect("expanded");
        assert_eq!(expanded, "01 NEW PIC X.\n01 INSERT PIC X.\n");
        assert_eq!(
            source_map,
            vec![
                SourceLineOrigin {
                    file: copybook.clone(),
                    line: 1,
                },
                SourceLineOrigin {
                    file: copybook,
                    line: 1,
                },
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_replacing_inserted_middle_line_maps_to_replaced_origin() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_replacing_inserted_line_source_map_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 OLD PIC X.\n01 KEEP PIC X.\n").expect("copybook");
        let origin = dir.join("MAIN.cbl");
        let copybook = dir.join("REC.cpy");
        let mut includes = Vec::new();
        let mut source_map = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks_with_map(
            "COPY REC REPLACING ==01 OLD PIC X.== BY ==01 NEW PIC X.\n01 INSERT PIC X.==.\n",
            &origin,
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut source_map,
            &mut stack,
        )
        .expect("expanded");
        assert_eq!(
            expanded,
            "01 NEW PIC X.\n01 INSERT PIC X.\n01 KEEP PIC X.\n"
        );
        assert_eq!(
            source_map,
            vec![
                SourceLineOrigin {
                    file: copybook.clone(),
                    line: 1,
                },
                SourceLineOrigin {
                    file: copybook.clone(),
                    line: 1,
                },
                SourceLineOrigin {
                    file: copybook,
                    line: 2,
                },
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_replacing_delete_entire_copybook_does_not_emit_phantom_line() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_replacing_delete_all_source_map_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 OLD PIC X.\n").expect("copybook");
        let origin = dir.join("MAIN.cbl");
        let mut includes = Vec::new();
        let mut source_map = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks_with_map(
            "COPY REC REPLACING ==01 OLD PIC X.\n== BY ====.\n",
            &origin,
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut source_map,
            &mut stack,
        )
        .expect("expanded");
        assert_eq!(expanded, "");
        assert!(source_map.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_replacing_deleted_line_preserves_suffix_origin() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_replacing_delete_line_source_map_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 OLD PIC X.\n01 KEEP PIC X.\n").expect("copybook");
        let origin = dir.join("MAIN.cbl");
        let copybook = dir.join("REC.cpy");
        let mut includes = Vec::new();
        let mut source_map = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks_with_map(
            "COPY REC REPLACING ==01 OLD PIC X.\n== BY ====.\n",
            &origin,
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut source_map,
            &mut stack,
        )
        .expect("expanded");
        assert_eq!(expanded, "01 KEEP PIC X.\n");
        assert_eq!(
            source_map,
            vec![SourceLineOrigin {
                file: copybook,
                line: 2,
            }]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_replace_multiline_output_extends_source_map_for_source_line() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_multiline_source_map_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let origin = dir.join("MAIN.cbl");
        let mut includes = Vec::new();
        let mut source_map = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks_with_map(
            "REPLACE ==01 OLD PIC X.== BY ==01 NEW PIC X.\n01 INSERT PIC X.==.\n01 OLD PIC X.\n",
            &origin,
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut source_map,
            &mut stack,
        )
        .expect("expanded");
        assert_eq!(expanded, "01 NEW PIC X.\n01 INSERT PIC X.\n");
        assert_eq!(
            source_map,
            vec![
                SourceLineOrigin {
                    file: origin.clone(),
                    line: 3,
                },
                SourceLineOrigin {
                    file: origin,
                    line: 3,
                },
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_replace_balanced_insert_delete_keeps_insert_origin() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_balanced_insert_delete_map_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let origin = dir.join("MAIN.cbl");
        let mut includes = Vec::new();
        let mut source_map = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks_with_map(
            "REPLACE ==01 OLD PIC X.== BY ==01 NEW PIC X.\n01 INSERT PIC X.== ==01 DELETE PIC X.\n== BY ====.\n01 OLD PIC X.\n01 DELETE PIC X.\n",
            &origin,
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut source_map,
            &mut stack,
        )
        .expect("expanded");
        assert_eq!(expanded, "01 NEW PIC X.\n01 INSERT PIC X.\n");
        assert_eq!(
            source_map,
            vec![
                SourceLineOrigin {
                    file: origin.clone(),
                    line: 4,
                },
                SourceLineOrigin {
                    file: origin,
                    line: 4,
                },
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_replace_multiline_source_updates_source_map_suffix() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_multiline_source_delete_map_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let origin = dir.join("MAIN.cbl");
        let mut includes = Vec::new();
        let mut source_map = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks_with_map(
            "REPLACE ==01 OLD PIC X.\n01 DROP PIC X.== BY ==01 NEW PIC X.==.\n01 OLD PIC X.\n01 DROP PIC X.\n01 KEEP PIC X.\n",
            &origin,
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut source_map,
            &mut stack,
        )
        .expect("expanded");
        assert_eq!(expanded, "01 NEW PIC X.\n01 KEEP PIC X.\n");
        assert_eq!(
            source_map,
            vec![
                SourceLineOrigin {
                    file: origin.clone(),
                    line: 3,
                },
                SourceLineOrigin {
                    file: origin,
                    line: 5,
                },
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_replace_multiline_source_can_generate_copy_directive() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_multiline_generated_copy_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 COPIED PIC X.\n").expect("copybook");
        let origin = dir.join("MAIN.cbl");
        let copybook = dir.join("REC.cpy");
        let mut includes = Vec::new();
        let mut source_map = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks_with_map(
            "REPLACE ==01 MARKER PIC X.\n01 END-MARKER PIC X.== BY ==COPY REC.==.\n01 MARKER PIC X.\n01 END-MARKER PIC X.\n",
            &origin,
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut source_map,
            &mut stack,
        )
        .expect("expanded");
        assert_eq!(expanded, "01 COPIED PIC X.\n");
        assert_eq!(
            source_map,
            vec![SourceLineOrigin {
                file: copybook,
                line: 1,
            }]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_replace_can_generate_copy_directive_inside_copybook_text() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_generates_copy_inside_copybook_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("OUTER.cpy"), "COPY-MARKER\n01 KEEP PIC X.\n").expect("outer copybook");
        fs::write(dir.join("INNER.cpy"), "01 INNER PIC X.\n").expect("inner copybook");
        let origin = dir.join("MAIN.cbl");
        let outer = dir.join("OUTER.cpy");
        let inner = dir.join("INNER.cpy");
        let mut includes = Vec::new();
        let mut source_map = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks_with_map(
            "REPLACE ==COPY-MARKER== BY ==COPY INNER.==.\nCOPY OUTER.\n",
            &origin,
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut source_map,
            &mut stack,
        )
        .expect("expanded");
        assert_eq!(expanded, "01 INNER PIC X.\n01 KEEP PIC X.\n");
        assert_eq!(
            source_map,
            vec![
                SourceLineOrigin {
                    file: inner,
                    line: 1,
                },
                SourceLineOrigin {
                    file: outer,
                    line: 2,
                },
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_replacing_generated_copy_directive_fails_closed() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_replacing_generates_copy_fails_closed_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("OUTER.cpy"), "COPY-MARKER\n01 KEEP PIC X.\n").expect("outer copybook");
        fs::write(dir.join("INNER.cpy"), "01 INNER PIC X.\n").expect("inner copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY OUTER REPLACING ==COPY-MARKER== BY ==COPY INNER.==.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyReplacing { copybook }) if copybook == "OUTER"
        ));
        assert!(includes.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_replace_generated_replace_off_inside_copybook_cancels_following_source() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_generates_off_inside_copybook_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("OUTER.cpy"), "OFF-MARKER\n").expect("outer copybook");
        let origin = dir.join("MAIN.cbl");
        let mut includes = Vec::new();
        let mut source_map = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks_with_map(
            "REPLACE ==OLD-NAME== BY ==NEW-NAME== ==OFF-MARKER== BY ==REPLACE OFF.==.\nCOPY OUTER.\n01 OLD-NAME PIC X.\n",
            &origin,
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut source_map,
            &mut stack,
        )
        .expect("expanded");
        assert_eq!(expanded, "01 OLD-NAME PIC X.\n");
        assert_eq!(
            source_map,
            vec![SourceLineOrigin {
                file: origin,
                line: 3,
            }]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_replace_multiline_generated_copy_preserves_suffix_origin() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_multiline_generated_copy_suffix_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 COPIED PIC X.\n").expect("copybook");
        let origin = dir.join("MAIN.cbl");
        let copybook = dir.join("REC.cpy");
        let mut includes = Vec::new();
        let mut source_map = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks_with_map(
            "REPLACE ==01 MARKER PIC X.\n01 END-MARKER PIC X.== BY ==COPY REC.==.\n01 MARKER PIC X.\n01 END-MARKER PIC X.\n01 KEEP PIC X.\n",
            &origin,
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut source_map,
            &mut stack,
        )
        .expect("expanded");
        assert_eq!(expanded, "01 COPIED PIC X.\n01 KEEP PIC X.\n");
        assert_eq!(
            source_map,
            vec![
                SourceLineOrigin {
                    file: copybook,
                    line: 1,
                },
                SourceLineOrigin {
                    file: origin,
                    line: 5,
                },
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn preprocess_file_source_map_preserves_free_format_original_line_after_comments() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_preprocess_free_origin_map_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let source = dir.join("MAIN.cbl");
        fs::write(&source, "*> leading comment\n01 FIELD PIC X.\n").expect("source");
        let preprocessed = preprocess_file(&source, &[], SourceFormat::Free).expect("preprocess");
        assert_eq!(preprocessed.text, "01 FIELD PIC X.\n");
        assert_eq!(
            preprocessed.source_map,
            vec![SourceLineOrigin {
                file: source.clone(),
                line: 2,
            }]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn preprocess_file_source_map_preserves_fixed_format_original_line_after_dropped_lines() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_preprocess_fixed_origin_map_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let source = dir.join("MAIN.cbl");
        fs::write(
            &source,
            "000100*COMMENT\n000200D DISPLAY \"DEBUG\".\n000300 01 FIELD PIC X.\n",
        )
        .expect("source");
        let preprocessed = preprocess_file(&source, &[], SourceFormat::Fixed).expect("preprocess");
        assert_eq!(preprocessed.text, "01 FIELD PIC X.\n");
        assert_eq!(
            preprocessed.source_map,
            vec![SourceLineOrigin {
                file: source.clone(),
                line: 3,
            }]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn preprocess_file_source_map_preserves_fixed_continuation_start_line() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_preprocess_fixed_continuation_origin_map_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let source = dir.join("MAIN.cbl");
        fs::write(
            &source,
            "000100 DISPLAY \"HELLO\n000200-        WORLD\".\n000300 01 FIELD PIC X.\n",
        )
        .expect("source");
        let preprocessed = preprocess_file(&source, &[], SourceFormat::Fixed).expect("preprocess");
        assert_eq!(
            preprocessed.text,
            "DISPLAY \"HELLO WORLD\".\n01 FIELD PIC X.\n"
        );
        assert_eq!(
            preprocessed.source_map,
            vec![
                SourceLineOrigin {
                    file: source.clone(),
                    line: 1,
                },
                SourceLineOrigin {
                    file: source.clone(),
                    line: 3,
                },
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn preprocess_file_source_map_preserves_copybook_original_line_after_comments() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_preprocess_copybook_origin_map_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let source = dir.join("MAIN.cbl");
        let copybook = dir.join("REC.cpy");
        fs::write(&source, "COPY REC.\n").expect("source");
        fs::write(&copybook, "*> copybook comment\n01 COPIED PIC X.\n").expect("copybook");
        let preprocessed = preprocess_file(&source, &[], SourceFormat::Free).expect("preprocess");
        assert_eq!(preprocessed.text, "01 COPIED PIC X.\n");
        assert_eq!(
            preprocessed.source_map,
            vec![SourceLineOrigin {
                file: copybook,
                line: 2,
            }]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn preprocess_file_source_map_preserves_copybook_fixed_continuation_start_line() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_preprocess_copybook_fixed_continuation_origin_map_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let source = dir.join("MAIN.cbl");
        let copybook = dir.join("REC.cpy");
        fs::write(&source, "000100 COPY REC.\n").expect("source");
        fs::write(
            &copybook,
            "000100 01 COPIED PIC X VALUE \"HELLO\n000200-        WORLD\".\n000300 01 NEXT PIC X.\n",
        )
        .expect("copybook");
        let preprocessed = preprocess_file(&source, &[], SourceFormat::Fixed).expect("preprocess");
        assert_eq!(
            preprocessed.text,
            "01 COPIED PIC X VALUE \"HELLO WORLD\".\n01 NEXT PIC X.\n"
        );
        assert_eq!(
            preprocessed.source_map,
            vec![
                SourceLineOrigin {
                    file: copybook.clone(),
                    line: 1,
                },
                SourceLineOrigin {
                    file: copybook,
                    line: 3,
                },
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn preprocess_file_source_map_covers_mixed_replace_copy_and_normalization() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_preprocess_mixed_origin_map_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let source = dir.join("MAIN.cbl");
        let copybook = dir.join("REC.cpy");
        fs::write(
            &source,
            "*> main comment\nREPLACE ==01 OLD PIC X.== BY ==01 NEW PIC X.\n01 INSERT PIC X.==.\nCOPY REC.\n",
        )
        .expect("source");
        fs::write(
            &copybook,
            "*> copybook comment\n01 OLD PIC X.\n01 KEEP PIC X.\n",
        )
        .expect("copybook");
        let preprocessed = preprocess_file(&source, &[], SourceFormat::Free).expect("preprocess");
        assert_eq!(
            preprocessed.text,
            "01 NEW PIC X.\n01 INSERT PIC X.\n01 KEEP PIC X.\n"
        );
        assert_eq!(
            preprocessed.text.lines().count(),
            preprocessed.source_map.len()
        );
        assert_eq!(
            preprocessed.source_map,
            vec![
                SourceLineOrigin {
                    file: copybook.clone(),
                    line: 2,
                },
                SourceLineOrigin {
                    file: copybook.clone(),
                    line: 2,
                },
                SourceLineOrigin {
                    file: copybook,
                    line: 3,
                },
            ]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn nested_copy_replacing_chain_fails_closed() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_nested_replacing_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(
            dir.join("OUTER.cpy"),
            "COPY INNER REPLACING ==OLD-NAME== BY ==INNER-NAME==.\n",
        )
        .expect("outer copybook");
        fs::write(dir.join("INNER.cpy"), "01 OLD-NAME PIC X.\n").expect("inner copybook");

        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY OUTER REPLACING ==OLD-NAME== BY ==OUTER-NAME==.\n",
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
                if copybook == "INNER" && clause == "REPLACING"
        ));
        assert!(!includes.iter().any(|include| include.copybook == "INNER"));
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
    fn copy_replacing_matches_multiline_pseudo_text_with_period_before_line_break() {
        let replaced = apply_copy_replacements(
            "01 OLD PIC X.\n01 DROP PIC X.\n01 KEEP PIC X.\n",
            &[CopyReplacement::full(
                "01 OLD PIC X.\n01 DROP PIC X.",
                "01 NEW PIC X.",
            )],
        );
        assert_eq!(replaced, "01 NEW PIC X.\n01 KEEP PIC X.\n");
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
    fn replacement_preserves_unmatched_complete_pseudo_text() {
        let replaced = apply_copy_replacements(
            "COPY REC REPLACING ==OLD-NAME== BY ==LOCAL-NAME==.\n01 OLD-NAME PIC X.\n",
            &[CopyReplacement::full("OLD-NAME", "GLOBAL-NAME")],
        );
        assert_eq!(
            replaced,
            "COPY REC REPLACING ==OLD-NAME== BY ==LOCAL-NAME==.\n01 GLOBAL-NAME PIC X.\n"
        );
    }

    #[test]
    fn partial_replacement_preserves_unmatched_complete_pseudo_text() {
        let replaced = apply_copy_replacements(
            "COPY REC REPLACING ==OLD-NAME== BY ==LOCAL-NAME==.\n01 OLD-NAME PIC X.\n",
            &[CopyReplacement::leading("OLD", "GLOBAL")],
        );
        assert_eq!(
            replaced,
            "COPY REC REPLACING ==OLD-NAME== BY ==LOCAL-NAME==.\n01 GLOBAL-NAME PIC X.\n"
        );
    }

    #[test]
    fn replacement_preserves_unclosed_pseudo_text() {
        let replaced = apply_copy_replacements(
            "COPY REC REPLACING ==OLD-NAME OLD-NAME.\n01 OLD-NAME PIC X.\n",
            &[CopyReplacement::full("OLD-NAME", "GLOBAL-NAME")],
        );
        assert_eq!(
            replaced,
            "COPY REC REPLACING ==OLD-NAME OLD-NAME.\n01 OLD-NAME PIC X.\n"
        );
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
    fn replace_directive_accepts_separator_after_keyword() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "REPLACE; ==OLD-NAME== BY ==NEW-NAME==.\n01 OLD-NAME PIC X.\nREPLACE; OFF.\n01 OLD-NAME PIC X.\n",
            None,
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("replace separator after keyword");
        assert!(expanded.contains("01 NEW-NAME PIC X."));
        assert_eq!(expanded.matches("01 OLD-NAME PIC X.").count(), 1);
        assert!(!expanded.contains("REPLACE"));
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
    fn replace_directive_rejects_blank_source_pseudo_text() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE ==   == BY ==NEW==.\n01 OLD-NAME PIC X.\n",
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
                if statement == "REPLACE ==   == BY ==NEW==."
        ));
    }

    #[test]
    fn replace_directive_rejects_bare_punctuation_operands() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE : BY SEMICOLON.\n01 OLD-NAME PIC X.\n",
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
                if statement == "REPLACE : BY SEMICOLON."
        ));
    }

    #[test]
    fn replace_directive_rejects_non_pseudo_partial_word_operands() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE LEADING OLD BY NEW.\n01 OLD-NAME PIC X.\n",
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
                if statement == "REPLACE LEADING OLD BY NEW."
        ));
    }

    #[test]
    fn replace_directive_rejects_multi_word_partial_replacement_target() {
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "REPLACE LEADING ==OLD== BY ==NEW NAME==.\n01 OLD-NAME PIC X.\n",
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
                if statement == "REPLACE LEADING ==OLD== BY ==NEW NAME==."
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
        for clause in [
            "AS",
            "DISJOINING",
            "INDEXED",
            "JOINING",
            "LIST",
            "NOLIST",
            "PRINTING",
            "PREFIX",
            "PREFIXING",
            "RESOURCE",
            "SUFFIX",
            "SUFFIXING",
        ] {
            for source in [
                "COPY REC CLAUSE-MARKER.\n".to_string(),
                "01 PREFIX-FIELD PIC X. COPY REC CLAUSE-MARKER.\n".to_string(),
            ] {
                let dir = std::env::temp_dir().join(format!(
                    "cobol_source_replace_copy_unsupported_clause_{}_{}",
                    clause.to_ascii_lowercase(),
                    std::process::id()
                ));
                let _ = fs::remove_dir_all(&dir);
                fs::create_dir_all(&dir).expect("temp dir");
                fs::write(dir.join("REC.cpy"), "01 COPIED-FIELD PIC X.\n").expect("copybook");
                let mut includes = Vec::new();
                let mut stack = HashSet::new();
                let input = format!("REPLACE ==CLAUSE-MARKER== BY =={clause}==.\n{source}");
                let result = expand_copybooks(
                    &input,
                    Some(&dir),
                    &[],
                    SourceFormat::Free,
                    0,
                    &mut includes,
                    &mut stack,
                );
                assert!(matches!(
                    result,
                    Err(SourceError::UnsupportedCopyClause {
                        copybook,
                        clause: actual_clause,
                    }) if copybook == "REC" && actual_clause == clause
                ));
                assert!(includes.is_empty());
                let _ = fs::remove_dir_all(&dir);
            }
        }
    }

    #[test]
    fn active_replace_generated_extra_clause_after_suppress_printing_fails_closed() {
        for clause in ["NOLIST", "PRINTING"] {
            for source in [
                "COPY REC SUPPRESS PRINTING CLAUSE-MARKER.\n".to_string(),
                "01 PREFIX-FIELD PIC X. COPY REC SUPPRESS PRINTING CLAUSE-MARKER.\n".to_string(),
                "COPY REC OF LIB SUPPRESS PRINTING CLAUSE-MARKER.\n".to_string(),
                "01 PREFIX-FIELD PIC X. COPY REC OF LIB SUPPRESS PRINTING CLAUSE-MARKER.\n"
                    .to_string(),
                "COPY REC IN LIB SUPPRESS PRINTING CLAUSE-MARKER.\n".to_string(),
                "01 PREFIX-FIELD PIC X. COPY REC IN LIB SUPPRESS PRINTING CLAUSE-MARKER.\n"
                    .to_string(),
            ] {
                let dir = std::env::temp_dir().join(format!(
                    "cobol_source_replace_copy_suppress_printing_extra_{}_{}",
                    clause.to_ascii_lowercase(),
                    std::process::id()
                ));
                let _ = fs::remove_dir_all(&dir);
                let lib = dir.join("LIB");
                fs::create_dir_all(&dir).expect("temp dir");
                fs::create_dir_all(&lib).expect("library dir");
                fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
                fs::write(lib.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n")
                    .expect("library copybook");
                let mut includes = Vec::new();
                let mut stack = HashSet::new();
                let input = format!("REPLACE ==CLAUSE-MARKER== BY =={clause}==.\n{source}");
                let result = expand_copybooks(
                    &input,
                    Some(&dir),
                    &[],
                    SourceFormat::Free,
                    0,
                    &mut includes,
                    &mut stack,
                );
                assert!(matches!(
                    result,
                    Err(SourceError::UnsupportedCopyClause {
                        copybook,
                        clause: actual_clause,
                    }) if copybook == "REC" && actual_clause == clause
                ));
                assert!(includes.is_empty());
                let _ = fs::remove_dir_all(&dir);
            }
        }
    }

    #[test]
    fn active_replace_generated_extra_clause_after_suppress_fails_closed() {
        for clause in [
            "AS",
            "DISJOINING",
            "INDEXED",
            "JOINING",
            "LIST",
            "NOLIST",
            "PREFIX",
            "PREFIXING",
            "RESOURCE",
            "SUFFIX",
            "SUFFIXING",
        ] {
            for source in [
                "COPY REC SUPPRESS CLAUSE-MARKER.\n".to_string(),
                "01 PREFIX-FIELD PIC X. COPY REC SUPPRESS CLAUSE-MARKER.\n".to_string(),
                "COPY REC OF LIB SUPPRESS CLAUSE-MARKER.\n".to_string(),
                "01 PREFIX-FIELD PIC X. COPY REC IN LIB SUPPRESS CLAUSE-MARKER.\n".to_string(),
            ] {
                let dir = std::env::temp_dir().join(format!(
                    "cobol_source_replace_copy_suppress_extra_{}_{}",
                    clause.to_ascii_lowercase(),
                    std::process::id()
                ));
                let _ = fs::remove_dir_all(&dir);
                let lib = dir.join("LIB");
                fs::create_dir_all(&dir).expect("temp dir");
                fs::create_dir_all(&lib).expect("library dir");
                fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
                fs::write(lib.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n")
                    .expect("library copybook");
                let mut includes = Vec::new();
                let mut stack = HashSet::new();
                let input = format!("REPLACE ==CLAUSE-MARKER== BY =={clause}==.\n{source}");
                let result = expand_copybooks(
                    &input,
                    Some(&dir),
                    &[],
                    SourceFormat::Free,
                    0,
                    &mut includes,
                    &mut stack,
                );
                assert!(matches!(
                    result,
                    Err(SourceError::UnsupportedCopyClause {
                        copybook,
                        clause: actual_clause,
                    }) if copybook == "REC" && actual_clause == clause
                ));
                assert!(includes.is_empty());
                let _ = fs::remove_dir_all(&dir);
            }
        }
    }

    #[test]
    fn active_replace_generated_acu_prefix_copy_forms_fail_closed() {
        for (clause, copybook) in [("RESOURCE", "RES"), ("INDEXED", "REC")] {
            for source in [
                "COPY-MARKER\n".to_string(),
                "01 PREFIX-FIELD PIC X. COPY-MARKER\n".to_string(),
            ] {
                let dir = std::env::temp_dir().join(format!(
                    "cobol_source_replace_copy_acu_prefix_{}_{}",
                    clause.to_ascii_lowercase(),
                    std::process::id()
                ));
                let _ = fs::remove_dir_all(&dir);
                fs::create_dir_all(&dir).expect("temp dir");
                fs::write(
                    dir.join(format!("{copybook}.cpy")),
                    "01 SHOULD-NOT-EXPAND PIC X.\n",
                )
                .expect("copybook");
                let mut includes = Vec::new();
                let mut stack = HashSet::new();
                let input =
                    format!("REPLACE ==COPY-MARKER== BY ==COPY {clause} {copybook}.==.\n{source}");
                let result = expand_copybooks(
                    &input,
                    Some(&dir),
                    &[],
                    SourceFormat::Free,
                    0,
                    &mut includes,
                    &mut stack,
                );
                assert!(matches!(
                    result,
                    Err(SourceError::UnsupportedCopyClause {
                        copybook: actual_copybook,
                        clause: actual_clause,
                    }) if actual_copybook == copybook && actual_clause == clause
                ));
                assert!(includes.is_empty());
                let _ = fs::remove_dir_all(&dir);
            }
        }
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
    fn generated_replace_off_cancels_before_trailing_source_in_multiline_chunk() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_generates_off_suffix_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let origin = dir.join("MAIN.cbl");
        let mut includes = Vec::new();
        let mut source_map = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks_with_map(
            "REPLACE ==OLD-NAME== BY ==NEW-NAME== ==OFF-MARKER== BY ==REPLACE OFF.==.\nOFF-MARKER\n01 OLD-NAME PIC X.\n",
            &origin,
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut source_map,
            &mut stack,
        )
        .expect("expanded");
        assert_eq!(expanded, "01 OLD-NAME PIC X.\n");
        assert_eq!(
            source_map,
            vec![SourceLineOrigin {
                file: origin,
                line: 3,
            }]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn generated_replace_off_cancels_before_same_line_trailing_source() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_generates_off_same_line_suffix_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let origin = dir.join("MAIN.cbl");
        let mut includes = Vec::new();
        let mut source_map = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks_with_map(
            "REPLACE ==OLD-NAME== BY ==NEW-NAME== ==OFF-MARKER== BY ==REPLACE OFF.==.\nOFF-MARKER 01 OLD-NAME PIC X.\n",
            &origin,
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut source_map,
            &mut stack,
        )
        .expect("expanded");
        assert_eq!(expanded, "01 OLD-NAME PIC X.\n");
        assert_eq!(
            source_map,
            vec![SourceLineOrigin {
                file: origin,
                line: 2,
            }]
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn generated_copy_before_replace_off_same_line_uses_active_then_cancels() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_replace_generates_copy_then_off_same_line_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 OLD-NAME PIC X.\n").expect("copybook");
        let origin = dir.join("MAIN.cbl");
        let copybook = dir.join("REC.cpy");
        let mut includes = Vec::new();
        let mut source_map = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks_with_map(
            "REPLACE ==OLD-NAME== BY ==NEW-NAME== ==COPY-MARKER== BY ==COPY REC.== ==OFF-MARKER== BY ==REPLACE OFF.==.\nCOPY-MARKER OFF-MARKER 01 OLD-NAME PIC X.\n",
            &origin,
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut source_map,
            &mut stack,
        )
        .expect("expanded");
        assert_eq!(expanded, "01 NEW-NAME PIC X.\n01 OLD-NAME PIC X.\n");
        assert_eq!(
            source_map,
            vec![
                SourceLineOrigin {
                    file: copybook,
                    line: 1,
                },
                SourceLineOrigin {
                    file: origin,
                    line: 2,
                }
            ]
        );
        assert_eq!(includes.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn chained_replacement_generated_replace_off_cancels_before_same_line_trailing_source() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_chained_replace_off_same_line_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        let origin = dir.join("MAIN.cbl");
        let mut includes = Vec::new();
        let mut source_map = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks_with_map(
            "REPLACE ==A== BY ==B== ==B== BY ==REPLACE OFF.== ==OLD-NAME== BY ==NEW-NAME==.\nA 01 OLD-NAME PIC X.\n",
            &origin,
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut source_map,
            &mut stack,
        )
        .expect("expanded");
        assert_eq!(expanded, "01 OLD-NAME PIC X.\n");
        assert_eq!(
            source_map,
            vec![SourceLineOrigin {
                file: origin,
                line: 2,
            }]
        );
        assert!(includes.is_empty());
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
    fn fixed_format_leading_continuation_does_not_insert_leading_space() {
        let raw = "000100-        DISPLAY \"ORPHAN\".\n";
        assert_eq!(
            normalize_source(raw, SourceFormat::Fixed),
            "DISPLAY \"ORPHAN\".\n"
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
    fn fixed_format_continuation_preserves_multiline_pseudo_text_state() {
        let raw = "000100 COPY REC REPLACING ==OLD\n000200-        TOKEN== BY ==NEW-TOKEN==.\n";
        assert_eq!(
            normalize_source(raw, SourceFormat::Fixed),
            "COPY REC REPLACING ==OLD TOKEN== BY ==NEW-TOKEN==.\n"
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
    fn fixed_format_leading_comment_only_continuation_does_not_emit_blank_line() {
        let raw = "000100-        *> comment only\n000200 DISPLAY \"B\".\n";
        assert_eq!(
            normalize_source(raw, SourceFormat::Fixed),
            "DISPLAY \"B\".\n"
        );
    }

    #[test]
    fn fixed_format_leading_continuation_strips_comment_after_unclosed_quote() {
        let raw = "000100-        DISPLAY \"A *> comment\n";
        assert_eq!(normalize_source(raw, SourceFormat::Fixed), "DISPLAY \"A\n");
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
    fn fixed_format_continuation_preserves_comment_marker_inside_open_pseudo_text() {
        let raw =
            "000100 COPY REC REPLACING ==OLD\n000200-        *>TOKEN== BY ==NEW==. *> comment\n";
        assert_eq!(
            normalize_source(raw, SourceFormat::Fixed),
            "COPY REC REPLACING ==OLD *>TOKEN== BY ==NEW==.\n"
        );
    }

    #[test]
    fn fixed_format_copy_replacing_keeps_split_pseudo_text_delimiter_tight() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_fixed_split_pseudo_delim_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "000100 01 OLD PIC X.\n").expect("copybook");
        let raw = "000100 COPY REC REPLACING ==OLD\n000200-        == BY ==NEW==.\n";
        let normalized = normalize_source(raw, SourceFormat::Fixed);
        assert_eq!(normalized, "COPY REC REPLACING ==OLD== BY ==NEW==.\n");
        let parsed = parse_copy_statement(&normalized).expect("copy statement");
        assert_eq!(
            parsed.replacements,
            vec![CopyReplacement::full("OLD", "NEW")]
        );
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            &normalized,
            Some(&dir),
            &[],
            SourceFormat::Fixed,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert_eq!(expanded, "01 NEW PIC X.\n");
        assert_eq!(includes.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn fixed_source_honors_free_format_directive_in_copybook() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_fixed_includes_free_copybook_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(
            dir.join("REC.cpy"),
            "       >>SOURCE FORMAT FREE\n01 FREE-FIELD PIC X.\n",
        )
        .expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "COPY REC.\n",
            Some(&dir),
            &[],
            SourceFormat::Fixed,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains(">>SOURCE FORMAT FREE"));
        assert!(expanded.contains("01 FREE-FIELD PIC X."));
        assert!(!expanded.lines().any(|line| line == "E-FIELD PIC X."));
        assert_eq!(includes.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn nested_copybook_inherits_parent_copybook_effective_format() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_nested_copybook_format_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(
            dir.join("OUTER.cpy"),
            "       >>SOURCE FORMAT FREE\nCOPY INNER.\n",
        )
        .expect("outer copybook");
        fs::write(dir.join("INNER.cpy"), "01 INNER-FIELD PIC X.\n").expect("inner copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "COPY OUTER.\n",
            Some(&dir),
            &[],
            SourceFormat::Fixed,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert!(expanded.contains("01 INNER-FIELD PIC X."));
        assert!(!expanded.lines().any(|line| line == "NER-FIELD PIC X."));
        assert_eq!(includes.len(), 2);
        let _ = fs::remove_dir_all(&dir);
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
    fn fixed_format_literal_continuation_strips_comment_after_later_unclosed_quote() {
        let raw = "000100 DISPLAY \"A\n000200-        \"B\" \"C *> comment\n";
        assert_eq!(
            normalize_source(raw, SourceFormat::Fixed),
            "DISPLAY \"AB\" \"C\n"
        );
    }

    #[test]
    fn source_inline_comment_strips_comment_after_unclosed_quote_without_active_continuation() {
        assert_eq!(
            strip_source_inline_comment("DISPLAY \"A *> comment"),
            "DISPLAY \"A "
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
    fn free_format_preserves_pseudo_text_delimiter_inside_literal_while_stripping_comment() {
        let raw = "COPY REC REPLACING ==VALUE \"A==B\" *> TOKEN== BY ==NEW==. *> comment\n";
        assert_eq!(
            normalize_source(raw, SourceFormat::Free),
            "COPY REC REPLACING ==VALUE \"A==B\" *> TOKEN== BY ==NEW==.\n"
        );
    }

    #[test]
    fn free_format_strips_comment_after_unclosed_pseudo_text_marker() {
        let raw = "DISPLAY == *> comment\n";
        assert_eq!(normalize_source(raw, SourceFormat::Free), "DISPLAY ==\n");
    }

    #[test]
    fn free_format_strips_comment_after_unclosed_quote() {
        let raw = "DISPLAY \"A *> comment\n";
        assert_eq!(normalize_source(raw, SourceFormat::Free), "DISPLAY \"A\n");
    }

    #[test]
    fn free_format_preserves_leading_spaces_inside_open_literal() {
        let raw = "DISPLAY \"A\n  B\".\n";
        assert_eq!(
            normalize_source(raw, SourceFormat::Free),
            "DISPLAY \"A\n  B\".\n"
        );
    }

    #[test]
    fn free_format_strips_comment_inside_pseudo_text_after_unclosed_quote() {
        let raw = "DISPLAY \"A ==OLD *> TOKEN== *> comment\n";
        assert_eq!(
            normalize_source(raw, SourceFormat::Free),
            "DISPLAY \"A ==OLD\n"
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
    fn fixed_format_strips_comment_after_unclosed_quote() {
        let raw = "000100 DISPLAY \"A *> comment\n";
        assert_eq!(normalize_source(raw, SourceFormat::Fixed), "DISPLAY \"A\n");
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
    fn embedded_directive_scan_ignores_copy_text_inside_unclosed_pseudo_text() {
        let dir = std::env::temp_dir().join(format!(
            "cobol_source_embedded_copy_unclosed_pseudo_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "DISPLAY ==. COPY REC.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("expanded");
        assert_eq!(expanded, "DISPLAY ==. COPY REC.\n");
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
    fn copy_replacing_unclosed_pseudo_text_fails_closed() {
        let parsed = parse_copy_statement("COPY REC REPLACING ==OLD-NAME BY NEW-NAME.")
            .expect("copy statement");
        assert!(parsed.malformed_replacing);

        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_unclosed_pseudo_replacing_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC REPLACING ==OLD-NAME BY NEW-NAME.\n",
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
    fn copy_replacing_rejects_bare_punctuation_operands() {
        let parsed =
            parse_copy_statement("COPY REC REPLACING : BY SEMICOLON.").expect("copy statement");
        assert!(parsed.malformed_replacing);

        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_bare_punctuation_replacing_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 OLD-NAME PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC REPLACING : BY SEMICOLON.\n",
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
    fn copy_replacing_rejects_bare_separator_keywords_as_operands() {
        for source in [
            "COPY REC REPLACING BY BY NEW-NAME.",
            "COPY REC REPLACING ALSO BY NEW-NAME.",
            "COPY REC REPLACING REPLACING BY NEW-NAME.",
            "COPY REC REPLACING OLD-NAME BY BY.",
            "COPY REC REPLACING OLD-NAME BY ALSO.",
            "COPY REC REPLACING OLD-NAME BY LEADING.",
            "COPY REC REPLACING OLD-NAME BY REPLACING.",
            "COPY REC REPLACING OLD-NAME BY TRAILING.",
        ] {
            let parsed = parse_copy_statement(source).expect("copy statement");
            assert!(
                parsed.malformed_replacing,
                "bare separator keyword accepted in {source}"
            );
        }

        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_separator_keyword_replacing_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC REPLACING BY BY NEW-NAME.\n",
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
    fn copy_replacing_rejects_blank_source_pseudo_text() {
        let parsed =
            parse_copy_statement("COPY REC REPLACING ==   == BY ==NEW==.").expect("copy statement");
        assert!(parsed.malformed_replacing);

        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_blank_source_replacing_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 OLD-NAME PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC REPLACING ==   == BY ==NEW==.\n",
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
    fn copy_replacing_rejects_non_pseudo_partial_word_operands() {
        let parsed =
            parse_copy_statement("COPY REC REPLACING LEADING OLD BY NEW.").expect("copy statement");
        assert!(parsed.malformed_replacing);

        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_bad_bare_partial_replacing_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 OLD-NAME PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC REPLACING LEADING OLD BY NEW.\n",
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
    fn copy_replacing_rejects_multi_word_partial_replacement_target() {
        let parsed = parse_copy_statement("COPY REC REPLACING LEADING ==OLD== BY ==NEW NAME==.")
            .expect("copy statement");
        assert!(parsed.malformed_replacing);

        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_bad_partial_target_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("REC.cpy"), "01 OLD-NAME PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY REC REPLACING LEADING ==OLD== BY ==NEW NAME==.\n",
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
    fn copy_replacing_rejects_copy_word_replacement_operands() {
        for source in [
            "COPY REC REPLACING ==COPY== BY ==DISPLAY==.\n",
            "COPY REC REPLACING ==OLD== BY ==COPY==.\n",
            "COPY REC REPLACING COPY BY DISPLAY.\n",
        ] {
            let parsed = parse_copy_statement(source).expect("copy statement");
            assert!(parsed.malformed_replacing);

            let dir = std::env::temp_dir().join(format!(
                "cobol_source_copy_bad_copy_operand_{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).expect("temp dir");
            fs::write(dir.join("REC.cpy"), "01 COPY PIC X.\n01 OLD PIC X.\n").expect("copybook");
            let mut includes = Vec::new();
            let mut stack = HashSet::new();
            let result = expand_copybooks(
                source,
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
    fn copy_member_name_rejects_unquoted_micro_focus_copy_clause_keywords() {
        for source in [
            "COPY JOINING.",
            "COPY DISJOINING.",
            "COPY PREFIX.",
            "COPY SUFFIX.",
            "COPY AS.",
        ] {
            assert!(
                parse_copy_statement(source).is_none(),
                "{source} must not be accepted as an unquoted copy member"
            );
        }

        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_mf_keyword_member_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("JOINING.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n").expect("copybook");
        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let result = expand_copybooks(
            "COPY JOINING.\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        );
        assert!(matches!(
            result,
            Err(SourceError::MalformedCopyStatement { statement }) if statement == "COPY JOINING."
        ));
        assert!(includes.is_empty());

        let expanded = expand_copybooks(
            "COPY \"JOINING\".\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("quoted dialect keyword copybook name expands");
        assert!(expanded.contains("01 SHOULD-NOT-EXPAND PIC X."));
        assert_eq!(includes.len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_member_name_rejects_unquoted_acu_copy_format_keywords() {
        for source in ["COPY RESOURCE.", "COPY INDEXED."] {
            assert!(
                parse_copy_statement(source).is_none(),
                "{source} must not be accepted as an unquoted copy member"
            );
        }

        let dir = std::env::temp_dir().join(format!(
            "cobol_source_copy_acu_keyword_member_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("RESOURCE.cpy"), "01 RESOURCE-FIELD PIC X.\n").expect("copybook");
        fs::write(dir.join("INDEXED.cpy"), "01 INDEXED-FIELD PIC X.\n").expect("copybook");

        for source in ["COPY RESOURCE.\n", "COPY INDEXED.\n"] {
            let mut includes = Vec::new();
            let mut stack = HashSet::new();
            let result = expand_copybooks(
                source,
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
                    if statement == source.trim()
            ));
            assert!(includes.is_empty());
        }

        let mut includes = Vec::new();
        let mut stack = HashSet::new();
        let expanded = expand_copybooks(
            "COPY \"RESOURCE\".\nCOPY \"INDEXED\".\n",
            Some(&dir),
            &[],
            SourceFormat::Free,
            0,
            &mut includes,
            &mut stack,
        )
        .expect("quoted dialect keyword copybook names expand");
        assert!(expanded.contains("01 RESOURCE-FIELD PIC X."));
        assert!(expanded.contains("01 INDEXED-FIELD PIC X."));
        assert_eq!(includes.len(), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn acu_prefix_copy_forms_fail_closed_as_unsupported_clauses() {
        for (source, copybook, clause) in [
            ("COPY RESOURCE RES.\n", "RES", "RESOURCE"),
            ("COPY INDEXED REC.\n", "REC", "INDEXED"),
        ] {
            let parsed = parse_copy_statement(source).expect("copy statement");
            assert_eq!(parsed.name, copybook);
            assert_eq!(parsed.unsupported_clause.as_deref(), Some(clause));

            let dir = std::env::temp_dir().join(format!(
                "cobol_source_acu_prefix_copy_{}_{}",
                clause.to_ascii_lowercase(),
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).expect("temp dir");
            fs::write(
                dir.join(format!("{copybook}.cpy")),
                "01 SHOULD-NOT-EXPAND PIC X.\n",
            )
            .expect("copybook");
            let mut includes = Vec::new();
            let mut stack = HashSet::new();
            let result = expand_copybooks(
                source,
                Some(&dir),
                &[],
                SourceFormat::Free,
                0,
                &mut includes,
                &mut stack,
            );
            assert!(matches!(
                result,
                Err(SourceError::UnsupportedCopyClause {
                    copybook: actual_copybook,
                    clause: actual_clause,
                }) if actual_copybook == copybook && actual_clause == clause
            ));
            assert!(includes.is_empty());
            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn active_replace_cannot_hide_acu_prefix_copy_forms() {
        for (clause, copybook) in [("RESOURCE", "RES"), ("INDEXED", "REC")] {
            for source in [
                format!("COPY {clause} {copybook}.\n"),
                format!("01 PREFIX-FIELD PIC X. COPY {clause} {copybook}.\n"),
            ] {
                let dir = std::env::temp_dir().join(format!(
                    "cobol_source_replace_hides_acu_prefix_{}_{}",
                    clause.to_ascii_lowercase(),
                    std::process::id()
                ));
                let _ = fs::remove_dir_all(&dir);
                fs::create_dir_all(&dir).expect("temp dir");
                fs::write(
                    dir.join(format!("{copybook}.cpy")),
                    "01 SHOULD-NOT-EXPAND PIC X.\n",
                )
                .expect("copybook");
                let mut includes = Vec::new();
                let mut stack = HashSet::new();
                let input = format!("REPLACE =={clause}== BY ==REC==.\n{source}");
                let result = expand_copybooks(
                    &input,
                    Some(&dir),
                    &[],
                    SourceFormat::Free,
                    0,
                    &mut includes,
                    &mut stack,
                );
                assert!(matches!(
                    result,
                    Err(SourceError::UnsupportedCopyClause {
                        copybook: actual_copybook,
                        clause: actual_clause,
                    }) if actual_copybook == copybook && actual_clause == clause
                ));
                assert!(includes.is_empty());
                let _ = fs::remove_dir_all(&dir);
            }
        }
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
    fn active_replace_generated_copy_library_clause_keyword_fails_closed() {
        for library_clause in ["OF", "IN"] {
            for keyword in [
                "AS",
                "DISJOINING",
                "INDEXED",
                "JOINING",
                "LIST",
                "NOLIST",
                "PREFIX",
                "PREFIXING",
                "RESOURCE",
                "SUFFIX",
                "SUFFIXING",
                "SUPPRESS",
            ] {
                for source in [
                    format!("COPY REC {library_clause} COPY-LIB.\n"),
                    format!("01 PREFIX-FIELD PIC X. COPY REC {library_clause} COPY-LIB.\n"),
                ] {
                    let dir = std::env::temp_dir().join(format!(
                        "cobol_source_replace_generates_bad_copy_library_{}_{}_{}",
                        library_clause.to_ascii_lowercase(),
                        keyword.to_ascii_lowercase(),
                        std::process::id()
                    ));
                    let _ = fs::remove_dir_all(&dir);
                    fs::create_dir_all(&dir).expect("temp dir");
                    fs::write(dir.join("REC.cpy"), "01 SHOULD-NOT-EXPAND PIC X.\n")
                        .expect("copybook");
                    let mut includes = Vec::new();
                    let mut stack = HashSet::new();
                    let input = format!("REPLACE ==COPY-LIB== BY =={keyword}==.\n{source}");
                    let expected = format!("COPY REC {library_clause} {keyword}.");
                    let result = expand_copybooks(
                        &input,
                        Some(&dir),
                        &[],
                        SourceFormat::Free,
                        0,
                        &mut includes,
                        &mut stack,
                    );
                    assert!(matches!(
                        result,
                        Err(SourceError::MalformedCopyStatement { statement }) if statement == expected
                    ));
                    assert!(includes.is_empty());
                    let _ = fs::remove_dir_all(&dir);
                }
            }
        }
    }

    #[test]
    fn copy_library_clause_rejects_unquoted_clause_keyword_name() {
        for source in [
            "COPY REC OF AS.",
            "COPY REC OF DISJOINING.",
            "COPY REC OF INDEXED.",
            "COPY REC OF JOINING.",
            "COPY REC OF LIST.",
            "COPY REC OF NOLIST.",
            "COPY REC OF PREFIX.",
            "COPY REC OF PREFIXING.",
            "COPY REC OF RESOURCE.",
            "COPY REC OF SUFFIX.",
            "COPY REC OF SUFFIXING.",
            "COPY REC OF SUPPRESS.",
        ] {
            assert!(
                parse_copy_statement(source).is_none(),
                "{source} must not be accepted as an unquoted copy library"
            );
        }

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
