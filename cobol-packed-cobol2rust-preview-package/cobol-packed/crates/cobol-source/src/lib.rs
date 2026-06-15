use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

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
    #[error("COPY recursion exceeded maximum depth {max_depth} at {copybook}")]
    CopyDepthExceeded { copybook: String, max_depth: usize },
    #[error("recursive COPY detected at {copybook}")]
    RecursiveCopy { copybook: String },
    #[error("COPY {copybook} uses REPLACING; COPY REPLACING is not implemented in the converter preview")]
    CopyReplacingUnsupported { copybook: String },
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
    match format {
        SourceFormat::Fixed => normalize_fixed(raw),
        SourceFormat::Free | SourceFormat::Auto => normalize_free(raw),
    }
}

fn detect_format(raw: &str, requested: SourceFormat) -> SourceFormat {
    if requested != SourceFormat::Auto {
        return requested;
    }
    if raw
        .lines()
        .any(|line| line.to_ascii_uppercase().contains(">>SOURCE FORMAT FREE"))
    {
        SourceFormat::Free
    } else {
        SourceFormat::Fixed
    }
}

fn normalize_fixed(raw: &str) -> String {
    let mut out = String::new();
    for line in raw.lines() {
        let indicator = line.as_bytes().get(6).copied().unwrap_or(b' ');
        if matches!(indicator, b'*' | b'/' | b'D' | b'd') {
            continue;
        }
        let area = if line.len() > 7 {
            line.get(7..72)
                .unwrap_or_else(|| line.get(7..).unwrap_or(""))
        } else {
            ""
        };
        let trimmed = area.trim_end();
        if indicator == b'-' {
            out.push(' ');
            out.push_str(trimmed.trim_start());
        } else if !trimmed.trim().is_empty() {
            out.push_str(trimmed.trim_start());
            out.push('\n');
        }
    }
    out
}

fn normalize_free(raw: &str) -> String {
    let mut out = String::new();
    for line in raw.lines() {
        let trimmed = line.trim();
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
    const MAX_COPY_DEPTH: usize = 10;
    let mut out = String::new();
    for line in text.lines() {
        if let Some(copy_statement) = parse_copy_statement(line) {
            if copy_statement.has_replacing {
                return Err(SourceError::CopyReplacingUnsupported {
                    copybook: copy_statement.name,
                });
            }
            let copybook = copy_statement.name;
            if depth >= MAX_COPY_DEPTH {
                return Err(SourceError::CopyDepthExceeded {
                    copybook,
                    max_depth: MAX_COPY_DEPTH,
                });
            }
            let resolved =
                resolve_copybook(&copybook, primary_dir, copybook_dirs).ok_or_else(|| {
                    SourceError::CopybookNotFound {
                        copybook: copybook.clone(),
                    }
                })?;
            let key = resolved.to_string_lossy().to_string();
            if !stack.insert(key.clone()) {
                return Err(SourceError::RecursiveCopy { copybook });
            }
            includes.push(IncludeTrace {
                copybook: copybook.clone(),
                resolved_path: resolved.clone(),
                depth: depth + 1,
            });
            let raw = read_to_string(&resolved)?;
            let normalized = normalize_source(&raw, format);
            let expanded = expand_copybooks(
                &normalized,
                resolved.parent(),
                copybook_dirs,
                format,
                depth + 1,
                includes,
                stack,
            )?;
            out.push_str(&expanded);
            if !expanded.ends_with('\n') {
                out.push('\n');
            }
            stack.remove(&key);
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CopyStatement {
    name: String,
    has_replacing: bool,
}

fn parse_copy_statement(line: &str) -> Option<CopyStatement> {
    let cleaned = line.trim().trim_end_matches('.');
    let mut parts = cleaned.split_whitespace();
    let first = parts.next()?;
    if !first.eq_ignore_ascii_case("COPY") {
        return None;
    }
    let name = parts.next()?;
    Some(CopyStatement {
        name: name
            .trim_matches('"')
            .trim_matches('\'')
            .trim_end_matches('.')
            .to_string(),
        has_replacing: parts.any(|part| part.eq_ignore_ascii_case("REPLACING")),
    })
}

fn resolve_copybook(
    name: &str,
    primary_dir: Option<&Path>,
    copybook_dirs: &[PathBuf],
) -> Option<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = primary_dir {
        dirs.push(dir.to_path_buf());
    }
    dirs.extend(copybook_dirs.iter().cloned());

    let candidates = if Path::new(name).extension().is_some() {
        vec![PathBuf::from(name)]
    } else {
        vec![
            PathBuf::from(name),
            PathBuf::from(format!("{name}.cpy")),
            PathBuf::from(format!("{name}.CPY")),
            PathBuf::from(format!("{name}.cbl")),
            PathBuf::from(format!("{name}.CBL")),
        ]
    };

    for dir in dirs {
        for candidate in &candidates {
            let path = dir.join(candidate);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
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
    fn copy_replacing_is_not_silently_ignored() {
        let parsed = parse_copy_statement("COPY CUSTOMER REPLACING ==A== BY ==B==.")
            .expect("copy statement");
        assert_eq!(parsed.name, "CUSTOMER");
        assert!(parsed.has_replacing);
    }
}
