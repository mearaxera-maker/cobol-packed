use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
struct Declaration {
    line: usize,
    source: String,
}

#[derive(Debug)]
struct ItemHeader {
    level: u8,
    name: String,
    line: usize,
    source: String,
}

#[derive(Debug)]
struct CopybookItem {
    level: u8,
    name: String,
    pic: Option<String>,
    usage: Usage,
    line: usize,
    source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Usage {
    Display,
    PackedDecimal,
    Binary,
}

#[derive(Debug)]
struct Picture {
    kind: PictureKind,
    total_digits: Option<u8>,
    scale: u8,
    signed: bool,
    display_len: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PictureKind {
    Text,
    Numeric,
}

#[derive(Debug, serde::Serialize)]
struct CopybookDiagnostic {
    severity: &'static str,
    code: &'static str,
    line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    column: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    record_name: Option<String>,
    field_name: Option<String>,
    message: String,
    help: String,
    source: String,
}

type CopybookResult<T> = std::result::Result<T, Box<CopybookDiagnostic>>;

#[derive(Debug)]
struct GroupPath {
    level: u8,
    name: String,
}

#[derive(Default)]
struct SchemaNameAllocator {
    used: BTreeSet<String>,
    counts: BTreeMap<String, usize>,
}

impl SchemaNameAllocator {
    fn allocate(&mut self, name: &str) -> String {
        let base = cap_schema_name(name, 128);
        let mut next = self.counts.get(&base).copied().unwrap_or(0) + 1;
        loop {
            let suffix = if next == 1 {
                String::new()
            } else {
                format!("_{next}")
            };
            let stem_len = 128usize.saturating_sub(suffix.len());
            let candidate = format!("{}{}", cap_schema_name(&base, stem_len), suffix);
            if self.used.insert(candidate.clone()) {
                self.counts.insert(base, next);
                return candidate;
            }
            next += 1;
        }
    }
}

pub(super) fn from_copybook(args: CopybookArgs) -> Result<(), CliError> {
    let _ = args.include_fillers;
    let _strict = args.strict;

    if matches!(
        args.input_encoding,
        InputEncoding::Csv | InputEncoding::Jsonl
    ) {
        let diag = global_diagnostic(
            "schema from-copybook emits fixed-width schemas; name-based input encodings are unsupported",
            "choose --input-encoding binary for host records or --input-encoding hex for hex line fixtures",
            format!("{:?}", args.input_encoding),
        );
        return fail_with_diagnostic(&args, &diag);
    }

    let encoding = match TextEncoding::from_label(&args.encoding) {
        Some(encoding) if encoding.is_mixed_dbcs() => {
            let diag = global_diagnostic(
                format!(
                    "encoding {encoding} is a mixed DBCS encoding and cannot be used as the display-text default"
                ),
                "choose a single-byte EBCDIC encoding or a schema field type that explicitly models mixed DBCS text",
                args.encoding.clone(),
            );
            return fail_with_diagnostic(&args, &diag);
        }
        Some(encoding) => encoding,
        None => {
            let diag = global_diagnostic(
                format!("unsupported EBCDIC encoding {}", args.encoding),
                "pass one of the encodings listed by `hostlens encodings list`",
                args.encoding.clone(),
            );
            return fail_with_diagnostic(&args, &diag);
        }
    };

    let raw = fs::read(&args.copybook)?;
    let text = match std::str::from_utf8(&raw) {
        Ok(text) => text,
        Err(err) => {
            let diag = global_diagnostic(
                format!("copybook is not UTF-8: {err}"),
                "save the copybook as UTF-8 before running schema from-copybook",
                args.copybook.display().to_string(),
            );
            return fail_with_diagnostic(&args, &diag);
        }
    };

    let schema = match build_schema(text, &args, encoding) {
        Ok(schema) => schema,
        Err(diag) => return fail_with_diagnostic(&args, &diag),
    };
    validate_schema(&schema)?;

    let json = serde_json::to_string_pretty(&schema)?;
    if let Some(path) = &args.output {
        fs::write(path, format!("{json}\n"))?;
    } else {
        println!("{json}");
    }
    Ok(())
}

fn build_schema(text: &str, args: &CopybookArgs, encoding: TextEncoding) -> CopybookResult<Schema> {
    let declarations = declarations(text);
    if declarations.is_empty() {
        return Err(diagnostic_box(global_diagnostic(
            "copybook contains no period-terminated declarations",
            "add a level 01 record declaration before generating a schema",
            String::new(),
        )));
    }

    let headers = declarations
        .iter()
        .map(parse_item_header)
        .collect::<CopybookResult<Vec<_>>>()?;
    let records = headers
        .iter()
        .enumerate()
        .filter(|(_, header)| header.level == 1)
        .map(|(idx, _)| idx)
        .collect::<Vec<_>>();

    if records.is_empty() {
        if let Some(decl) = declarations
            .iter()
            .find(|decl| contains_token(&upper_tokens(&decl.source), "COPY"))
        {
            return Err(diagnostic_box(unsupported_diagnostic(
                decl.line,
                None,
                "COPY",
                &decl.source,
            )));
        }
        return Err(diagnostic_box(global_diagnostic(
            "copybook contains no level 01 record",
            "add a supported level 01 record or pass a copybook member that contains one",
            String::new(),
        )));
    }

    let selected_record = select_record(&headers, &records, args.record_name.as_deref())?;
    let selected_end = records
        .iter()
        .copied()
        .find(|idx| *idx > selected_record)
        .unwrap_or(headers.len());

    let items = declarations[selected_record..selected_end]
        .iter()
        .map(parse_item)
        .collect::<CopybookResult<Vec<_>>>()?;
    schema_from_items(&items, args, encoding)
}

fn declarations(text: &str) -> Vec<Declaration> {
    let mut declarations = Vec::new();
    let mut current = String::new();
    let mut current_line = 0usize;

    for (idx, raw_line) in text.lines().enumerate() {
        let line = idx + 1;
        let Some(content) = preprocess_line(raw_line) else {
            continue;
        };
        let trimmed = content.trim();
        if trimmed.is_empty() {
            continue;
        }

        if current_line == 0 {
            current_line = line;
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(trimmed);

        while let Some(period) = find_declaration_period(&current) {
            let source = current[..period].trim();
            if !source.is_empty() {
                declarations.push(Declaration {
                    line: current_line,
                    source: source.to_string(),
                });
            }
            let remainder = current[period + 1..].trim().to_string();
            current = remainder;
            current_line = if current.is_empty() { 0 } else { line };
        }
    }

    if !current.trim().is_empty() {
        declarations.push(Declaration {
            line: current_line,
            source: current.trim().to_string(),
        });
    }

    declarations
}

fn preprocess_line(raw: &str) -> Option<String> {
    if raw.trim_start().starts_with("*>") {
        return None;
    }

    let chars = raw.chars().collect::<Vec<_>>();
    if chars.len() >= 7
        && chars[..6]
            .iter()
            .all(|ch| ch.is_ascii_digit() || *ch == ' ')
    {
        match chars[6] {
            '*' | '/' => return None,
            '-' | ' ' => return strip_inline_comment(&chars[7..].iter().collect::<String>()),
            _ => {}
        }
    }

    strip_inline_comment(raw)
}

fn strip_inline_comment(raw: &str) -> Option<String> {
    let mut quote: Option<char> = None;
    let mut chars = raw.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        if quote == Some(ch) {
            if chars.peek().is_some_and(|(_, next)| *next == ch) {
                chars.next();
            } else {
                quote = None;
            }
        } else if quote.is_none() && matches!(ch, '\'' | '"') {
            quote = Some(ch);
        } else if quote.is_none() && ch == '*' && chars.peek().is_some_and(|(_, next)| *next == '>')
        {
            let kept = raw[..idx].trim_end();
            return if kept.is_empty() {
                None
            } else {
                Some(kept.to_string())
            };
        }
    }
    Some(raw.to_string())
}

fn find_declaration_period(source: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    let mut chars = source.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        if quote == Some(ch) {
            if chars.peek().is_some_and(|(_, next)| *next == ch) {
                chars.next();
            } else {
                quote = None;
            }
        } else if quote.is_none() && matches!(ch, '\'' | '"') {
            quote = Some(ch);
        } else if quote.is_none() && ch == '.' {
            return Some(idx);
        }
    }
    None
}

fn parse_item_header(declaration: &Declaration) -> CopybookResult<ItemHeader> {
    let tokens = declaration_tokens(&declaration.source);
    if contains_token(&upper_from_tokens(&tokens), "COPY") {
        return Err(diagnostic_box(unsupported_diagnostic(
            declaration.line,
            None,
            "COPY",
            &declaration.source,
        )));
    }
    if tokens.len() < 2 {
        return Err(diagnostic_box(line_diagnostic(
            declaration.line,
            None,
            "malformed copybook declaration; expected `<level> <name>`",
            "use a supported data description entry such as `05 FIELD PIC X(10)`",
            declaration.source.clone(),
        )));
    }
    let level = parse_level(&tokens[0], declaration.line, &declaration.source)?;
    Ok(ItemHeader {
        level,
        name: tokens[1].clone(),
        line: declaration.line,
        source: declaration.source.clone(),
    })
}

fn parse_item(declaration: &Declaration) -> CopybookResult<CopybookItem> {
    let header = parse_item_header(declaration)?;
    if matches!(header.level, 66 | 77 | 88) {
        return Err(diagnostic_box(unsupported_diagnostic(
            header.line,
            Some(header.name),
            format!("level {}", header.level),
            &header.source,
        )));
    }
    if header.level == 0 || header.level > 49 {
        return Err(diagnostic_box(unsupported_diagnostic(
            header.line,
            Some(header.name),
            format!("level {}", header.level),
            &header.source,
        )));
    }

    let tokens = declaration_tokens(&header.source);
    let upper = upper_from_tokens(&tokens);
    if let Some(construct) = unsupported_construct(&upper) {
        return Err(diagnostic_box(unsupported_diagnostic(
            header.line,
            Some(header.name),
            construct,
            &header.source,
        )));
    }

    let pic_idx = upper
        .iter()
        .position(|token| token == "PIC" || token == "PICTURE");
    let Some(pic_idx) = pic_idx else {
        if upper.iter().any(|token| is_usage_token(token)) {
            return Err(diagnostic_box(line_diagnostic(
                header.line,
                Some(header.name.clone()),
                "USAGE clause without PIC is not supported",
                "add a supported PIC clause to elementary items",
                header.source.clone(),
            )));
        }
        return Ok(CopybookItem {
            level: header.level,
            name: header.name,
            pic: None,
            usage: Usage::Display,
            line: header.line,
            source: header.source,
        });
    };

    let mut pic_start = pic_idx + 1;
    if upper.get(pic_start).is_some_and(|token| token == "IS") {
        pic_start += 1;
    }
    let mut pic_end = pic_start;
    while pic_end < tokens.len() {
        let token = &upper[pic_end];
        if is_usage_clause_boundary(token) || token == "VALUE" {
            break;
        }
        pic_end += 1;
    }
    if pic_start == pic_end {
        return Err(diagnostic_box(line_diagnostic(
            header.line,
            Some(header.name),
            "PIC clause is missing a picture string",
            "use a supported picture such as `PIC X(10)` or `PIC S9(5)V99`",
            header.source,
        )));
    }

    let pic = tokens[pic_start..pic_end].join("");
    let mut usage_tokens = Vec::new();
    usage_tokens.extend_from_slice(&upper[2..pic_idx]);
    usage_tokens.extend_from_slice(&upper[pic_end..]);
    let usage = parse_usage(&usage_tokens, header.line, &header.name, &header.source)?;

    Ok(CopybookItem {
        level: header.level,
        name: header.name,
        pic: Some(pic),
        usage,
        line: header.line,
        source: header.source,
    })
}

fn parse_level(raw: &str, line: usize, source: &str) -> CopybookResult<u8> {
    raw.parse::<u8>().map_err(|_| {
        diagnostic_box(line_diagnostic(
            line,
            None,
            format!("invalid COBOL level {raw:?}"),
            "levels must be numeric, for example `01` or `05`",
            source.to_string(),
        ))
    })
}

fn select_record(
    headers: &[ItemHeader],
    records: &[usize],
    record_name: Option<&str>,
) -> CopybookResult<usize> {
    if let Some(record_name) = record_name {
        records
            .iter()
            .copied()
            .find(|idx| names_match(&headers[*idx].name, record_name))
            .ok_or_else(|| {
                diagnostic_box(global_diagnostic(
                    format!("level 01 record {record_name:?} was not found"),
                    "check the copybook record name or omit --record-name when the copybook has one level 01",
                    records
                        .iter()
                        .map(|idx| headers[*idx].name.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                ))
            })
    } else if records.len() == 1 {
        Ok(records[0])
    } else {
        let second = &headers[records[1]];
        Err(diagnostic_box(line_diagnostic(
            second.line,
            Some(second.name.clone()),
            "multiple level 01 records found; pass --record-name to select one",
            "rerun with `--record-name <01-name>`",
            records
                .iter()
                .map(|idx| headers[*idx].name.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        )))
    }
}

fn schema_from_items(
    items: &[CopybookItem],
    args: &CopybookArgs,
    encoding: TextEncoding,
) -> CopybookResult<Schema> {
    let mut fields = Vec::new();
    let mut fillers = Vec::new();
    let mut offset = 0usize;
    let mut filler_count = 0usize;
    let mut groups: Vec<GroupPath> = Vec::new();
    let mut field_names = SchemaNameAllocator::default();

    for (idx, item) in items.iter().enumerate() {
        if idx == 0 && item.level == 1 && item.pic.is_none() {
            continue;
        }
        if item.level <= 1 && idx > 0 {
            return Err(diagnostic_box(unsupported_diagnostic(
                item.line,
                Some(item.name.clone()),
                format!("level {}", item.level),
                &item.source,
            )));
        }

        while groups.last().is_some_and(|group| item.level <= group.level) {
            groups.pop();
        }

        let Some(pic) = &item.pic else {
            if !is_filler(&item.name) {
                groups.push(GroupPath {
                    level: item.level,
                    name: schema_name(&item.name),
                });
            }
            continue;
        };

        let picture = parse_picture(pic).map_err(|message| {
            diagnostic_box(line_diagnostic(
                item.line,
                Some(item.name.clone()),
                format!("{message}: PIC {pic}"),
                "use X/A text pictures or 9/S9 numeric pictures with optional V scale",
                item.source.clone(),
            ))
        })?;
        let storage_len = storage_len(item, &picture)?;

        if is_filler(&item.name) {
            filler_count += 1;
            if !args.drop_fillers {
                fillers.push(FillerSpec {
                    name: filler_name(&groups, filler_count),
                    offset,
                    length: storage_len,
                    description: Some(source_description(item)),
                });
            }
            offset = offset.checked_add(storage_len).ok_or_else(|| {
                diagnostic_box(line_diagnostic(
                    item.line,
                    Some(item.name.clone()),
                    "record length overflow",
                    "reduce the generated record size",
                    item.source.clone(),
                ))
            })?;
            continue;
        }

        let mut field = field_spec(item, &groups, &picture, storage_len, offset, encoding)?;
        field.name = field_names.allocate(&field.name);
        fields.push(field);
        offset = offset.checked_add(storage_len).ok_or_else(|| {
            diagnostic_box(line_diagnostic(
                item.line,
                Some(item.name.clone()),
                "record length overflow",
                "reduce the generated record size",
                item.source.clone(),
            ))
        })?;
    }

    if fields.is_empty() {
        return Err(diagnostic_box(global_diagnostic(
            "selected record contains no supported elementary fields",
            "add at least one non-FILLER item with a supported PIC clause",
            String::new(),
        )));
    }

    Ok(Schema {
        version: 2,
        record_length: Some(offset),
        input_encoding: args.input_encoding,
        on_error: args.on_error,
        output: None,
        verification_scope: args.verification_scope,
        fillers,
        fields,
    })
}

fn field_spec(
    item: &CopybookItem,
    groups: &[GroupPath],
    picture: &Picture,
    length: usize,
    offset: usize,
    encoding: TextEncoding,
) -> CopybookResult<FieldSpec> {
    let field_type = match (picture.kind, item.usage) {
        (PictureKind::Text, Usage::Display) => FieldType::DisplayText,
        (PictureKind::Text, _) => {
            return Err(diagnostic_box(line_diagnostic(
                item.line,
                Some(item.name.clone()),
                format!(
                    "text picture PIC {} cannot use {}",
                    item.pic.as_deref().unwrap_or(""),
                    item.usage.label()
                ),
                "text pictures support only DISPLAY usage",
                item.source.clone(),
            )));
        }
        (PictureKind::Numeric, Usage::Display) => FieldType::ZonedDecimal,
        (PictureKind::Numeric, Usage::PackedDecimal) => FieldType::PackedDecimal,
        (PictureKind::Numeric, Usage::Binary) => FieldType::Binary,
    };

    Ok(FieldSpec {
        name: path_name(groups, &item.name),
        field_type,
        offset: Some(offset),
        length: Some(length),
        total_digits: picture.total_digits,
        scale: if picture.kind == PictureKind::Numeric {
            Some(picture.scale)
        } else {
            None
        },
        signed: if picture.kind == PictureKind::Numeric {
            Some(picture.signed)
        } else {
            None
        },
        sign_mode: CliSignMode::Pfd,
        mode: FieldMode::Lossless,
        encoding: if field_type == FieldType::DisplayText {
            Some(encoding)
        } else {
            None
        },
        required: true,
        description: Some(source_description(item)),
    })
}

fn storage_len(item: &CopybookItem, picture: &Picture) -> CopybookResult<usize> {
    match (picture.kind, item.usage) {
        (PictureKind::Text, Usage::Display) => Ok(picture.display_len),
        (PictureKind::Text, _) => Err(diagnostic_box(line_diagnostic(
            item.line,
            Some(item.name.clone()),
            format!(
                "text picture PIC {} cannot use {}",
                item.pic.as_deref().unwrap_or(""),
                item.usage.label()
            ),
            "text pictures support only DISPLAY usage",
            item.source.clone(),
        ))),
        (PictureKind::Numeric, Usage::Display) => Ok(picture.display_len),
        (PictureKind::Numeric, Usage::PackedDecimal) => {
            Ok(packed_len(picture.total_digits.unwrap_or(0)))
        }
        (PictureKind::Numeric, Usage::Binary) => {
            let digits = picture.total_digits.unwrap_or(0);
            binary_len(digits).map_err(|message| {
                diagnostic_box(line_diagnostic(
                    item.line,
                    Some(item.name.clone()),
                    message,
                    "IBM COMP/BINARY widths are supported for 1..=18 digits",
                    item.source.clone(),
                ))
            })
        }
    }
}

fn parse_picture(raw: &str) -> Result<Picture, String> {
    let pic = raw
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '.')
        .collect::<String>()
        .to_ascii_uppercase();
    if pic.is_empty() {
        return Err("empty picture is unsupported".to_string());
    }

    if pic.chars().any(|ch| matches!(ch, 'X' | 'A')) {
        if pic.starts_with('S') || pic.contains('9') || pic.contains('V') {
            return Err("mixed text/numeric picture is unsupported".to_string());
        }
        let display_len = count_repeated_atoms(&pic, &['X', 'A'])?;
        return Ok(Picture {
            kind: PictureKind::Text,
            total_digits: None,
            scale: 0,
            signed: false,
            display_len,
        });
    }

    let (signed, body) = if let Some(rest) = pic.strip_prefix('S') {
        (true, rest)
    } else {
        (false, pic.as_str())
    };
    if body.contains('S') {
        return Err("sign marker is only supported at the start of numeric pictures".to_string());
    }

    let parts = body.split('V').collect::<Vec<_>>();
    if parts.len() > 2 {
        return Err("numeric picture may contain at most one implied decimal V".to_string());
    }
    let integer_digits = count_repeated_atoms(parts[0], &['9'])?;
    if integer_digits == 0 {
        return Err("numeric picture requires at least one digit before V".to_string());
    }
    let scale_digits = if parts.len() == 2 {
        let scale = count_repeated_atoms(parts[1], &['9'])?;
        if scale == 0 {
            return Err("numeric picture requires at least one digit after V".to_string());
        }
        scale
    } else {
        0
    };
    let total = integer_digits
        .checked_add(scale_digits)
        .ok_or_else(|| "picture digit count overflows".to_string())?;
    let total_digits =
        u8::try_from(total).map_err(|_| "picture digit count exceeds 255".to_string())?;
    let scale = u8::try_from(scale_digits).map_err(|_| "picture scale exceeds 255".to_string())?;

    Ok(Picture {
        kind: PictureKind::Numeric,
        total_digits: Some(total_digits),
        scale,
        signed,
        display_len: total,
    })
}

fn count_repeated_atoms(raw: &str, allowed: &[char]) -> Result<usize, String> {
    let chars = raw.chars().collect::<Vec<_>>();
    let mut idx = 0usize;
    let mut total = 0usize;

    while idx < chars.len() {
        let atom = chars[idx];
        if !allowed.contains(&atom) {
            return Err(format!("unsupported or edited picture character {atom:?}"));
        }
        idx += 1;

        let repeat = if chars.get(idx) == Some(&'(') {
            idx += 1;
            let start = idx;
            while idx < chars.len() && chars[idx].is_ascii_digit() {
                idx += 1;
            }
            if start == idx || chars.get(idx) != Some(&')') {
                return Err("picture repeat must use a positive integer in parentheses".to_string());
            }
            let repeat = chars[start..idx]
                .iter()
                .collect::<String>()
                .parse::<usize>()
                .map_err(|_| "picture repeat count is too large".to_string())?;
            idx += 1;
            repeat
        } else {
            1
        };
        if repeat == 0 {
            return Err("picture repeat count must be greater than zero".to_string());
        }
        total = total
            .checked_add(repeat)
            .ok_or_else(|| "picture repeat count overflows".to_string())?;
    }

    Ok(total)
}

fn parse_usage(
    tokens: &[String],
    line: usize,
    field_name: &str,
    source: &str,
) -> CopybookResult<Usage> {
    let mut usage = Usage::Display;
    let mut explicit = false;
    let mut idx = 0usize;

    while idx < tokens.len() {
        let token = tokens[idx].as_str();
        match token {
            "USAGE" | "IS" => {}
            "DISPLAY" => set_usage(
                &mut usage,
                &mut explicit,
                Usage::Display,
                line,
                field_name,
                source,
            )?,
            "COMP-3" | "PACKED-DECIMAL" => set_usage(
                &mut usage,
                &mut explicit,
                Usage::PackedDecimal,
                line,
                field_name,
                source,
            )?,
            "COMP" | "BINARY" | "COMP-4" => set_usage(
                &mut usage,
                &mut explicit,
                Usage::Binary,
                line,
                field_name,
                source,
            )?,
            "VALUE" => break,
            other if other.starts_with("COMP") || other.starts_with("COMPUTATIONAL") => {
                return Err(diagnostic_box(unsupported_diagnostic(
                    line,
                    Some(field_name.to_string()),
                    other,
                    source,
                )));
            }
            "" => {}
            other => {
                return Err(diagnostic_box(line_diagnostic(
                    line,
                    Some(field_name.to_string()),
                    format!("unsupported clause {other}"),
                    "remove unsupported clauses before generating a HostLens schema",
                    source.to_string(),
                )));
            }
        }
        idx += 1;
    }

    Ok(usage)
}

fn set_usage(
    current: &mut Usage,
    explicit: &mut bool,
    next: Usage,
    line: usize,
    field_name: &str,
    source: &str,
) -> CopybookResult<()> {
    if *explicit && *current != next {
        return Err(diagnostic_box(line_diagnostic(
            line,
            Some(field_name.to_string()),
            "conflicting USAGE clauses",
            "keep exactly one supported USAGE clause",
            source.to_string(),
        )));
    }
    *current = next;
    *explicit = true;
    Ok(())
}

fn unsupported_construct(tokens: &[String]) -> Option<&'static str> {
    if contains_token(tokens, "REDEFINES") {
        Some("REDEFINES")
    } else if contains_token(tokens, "OCCURS") {
        Some("OCCURS")
    } else if contains_token(tokens, "SYNCHRONIZED") {
        Some("SYNCHRONIZED")
    } else if contains_token(tokens, "SYNC") {
        Some("SYNC")
    } else if contains_token(tokens, "JUSTIFIED") || contains_token(tokens, "JUST") {
        Some("JUSTIFIED")
    } else if contains_token(tokens, "SIGN") && contains_token(tokens, "SEPARATE") {
        Some("SIGN IS SEPARATE")
    } else if contains_sequence(tokens, &["BLANK", "WHEN", "ZERO"]) {
        Some("BLANK WHEN ZERO")
    } else if contains_token(tokens, "COPY") {
        Some("COPY")
    } else {
        None
    }
}

fn is_usage_clause_boundary(token: &str) -> bool {
    matches!(
        token,
        "USAGE" | "IS" | "DISPLAY" | "COMP" | "COMP-3" | "COMP-4" | "BINARY" | "PACKED-DECIMAL"
    ) || token.starts_with("COMP")
        || token.starts_with("COMPUTATIONAL")
}

fn is_usage_token(token: &str) -> bool {
    is_usage_clause_boundary(token)
}

fn declaration_tokens(source: &str) -> Vec<String> {
    source
        .split_whitespace()
        .map(|token| {
            token
                .trim_matches(|ch: char| matches!(ch, ',' | ';' | '.'))
                .to_string()
        })
        .filter(|token| !token.is_empty())
        .collect()
}

fn upper_tokens(source: &str) -> Vec<String> {
    upper_from_tokens(&declaration_tokens(source))
}

fn upper_from_tokens(tokens: &[String]) -> Vec<String> {
    tokens
        .iter()
        .map(|token| token.to_ascii_uppercase())
        .collect()
}

fn contains_token(tokens: &[String], needle: &str) -> bool {
    tokens.iter().any(|token| token == needle)
}

fn contains_sequence(tokens: &[String], sequence: &[&str]) -> bool {
    tokens.windows(sequence.len()).any(|window| {
        window
            .iter()
            .zip(sequence)
            .all(|(left, right)| left == right)
    })
}

fn packed_len(digits: u8) -> usize {
    (digits as usize + 2) / 2
}

fn binary_len(digits: u8) -> Result<usize, String> {
    match digits {
        1..=4 => Ok(2),
        5..=9 => Ok(4),
        10..=18 => Ok(8),
        _ => Err(format!("binary COMP supports 1..=18 digits, got {digits}")),
    }
}

fn names_match(copybook_name: &str, requested: &str) -> bool {
    copybook_name.eq_ignore_ascii_case(requested)
        || schema_name(copybook_name) == schema_name(requested)
}

fn is_filler(name: &str) -> bool {
    name.eq_ignore_ascii_case("FILLER")
}

fn path_name(groups: &[GroupPath], leaf: &str) -> String {
    let mut parts = groups
        .iter()
        .filter(|group| !group.name.is_empty())
        .map(|group| group.name.clone())
        .collect::<Vec<_>>();
    parts.push(schema_name(leaf));
    parts.join(".")
}

fn filler_name(groups: &[GroupPath], count: usize) -> String {
    let mut parts = groups
        .iter()
        .filter(|group| !group.name.is_empty())
        .map(|group| group.name.clone())
        .collect::<Vec<_>>();
    parts.push(format!("filler_{count}"));
    parts.join(".")
}

fn schema_name(raw: &str) -> String {
    let mut out = String::new();
    let mut last_was_separator = false;

    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_was_separator = false;
        } else if !last_was_separator {
            out.push('_');
            last_was_separator = true;
        }
    }

    let trimmed = out.trim_matches('_');
    let mut normalized = if trimmed.is_empty() {
        "field".to_string()
    } else {
        trimmed.to_string()
    };
    if normalized
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_digit())
    {
        normalized.insert_str(0, "f_");
    }
    normalized
}

fn cap_schema_name(name: &str, max_len: usize) -> String {
    if name.len() <= max_len {
        return name.to_string();
    }
    if max_len == 0 {
        return String::new();
    }
    let mut capped = name
        .chars()
        .take(max_len)
        .collect::<String>()
        .trim_matches(|ch| matches!(ch, '_' | '-' | '.'))
        .to_string();
    if capped.is_empty() {
        capped = "field".chars().take(max_len).collect();
    }
    capped
}

fn source_description(item: &CopybookItem) -> String {
    let mut description = format!("copybook line {}: {}", item.line, item.source);
    if item.usage == Usage::Binary {
        description.push_str(
            "; IBM mainframe COMP/BINARY width assumption: big-endian signed binary (1..=4 digits=2 bytes, 5..=9 digits=4 bytes, 10..=18 digits=8 bytes)",
        );
    }
    description
}

fn fail_with_diagnostic<T>(
    args: &CopybookArgs,
    diagnostic: &CopybookDiagnostic,
) -> Result<T, CliError> {
    let message = if diagnostic.line == 0 {
        diagnostic.message.clone()
    } else {
        format!("line {}: {}", diagnostic.line, diagnostic.message)
    };
    emit_diagnostic(args.diagnostics, diagnostic)?;
    Err(CliError::config("E_SCHEMA", message))
}

fn emit_diagnostic(
    format: DiagnosticFormat,
    diagnostic: &CopybookDiagnostic,
) -> Result<(), CliError> {
    match format {
        DiagnosticFormat::Json => {
            eprintln!("{}", serde_json::to_string(diagnostic)?);
        }
        DiagnosticFormat::Text => {
            if diagnostic.line == 0 {
                eprintln!("{}: {}", diagnostic.code, diagnostic.message);
            } else {
                eprintln!(
                    "{}: line {}: {}",
                    diagnostic.code, diagnostic.line, diagnostic.message
                );
            }
            if let Some(field_name) = &diagnostic.field_name {
                eprintln!("  field: {field_name}");
            }
            if !diagnostic.source.is_empty() {
                eprintln!("  source: {}", diagnostic.source);
            }
            if !diagnostic.help.is_empty() {
                eprintln!("  help: {}", diagnostic.help);
            }
        }
    }
    Ok(())
}

fn global_diagnostic(
    message: impl Into<String>,
    help: impl Into<String>,
    source: String,
) -> CopybookDiagnostic {
    CopybookDiagnostic {
        severity: "error",
        code: "E_COPYBOOK",
        line: 0,
        column: None,
        record_name: None,
        field_name: None,
        message: message.into(),
        help: help.into(),
        source,
    }
}

fn line_diagnostic(
    line: usize,
    field_name: Option<String>,
    message: impl Into<String>,
    help: impl Into<String>,
    source: String,
) -> CopybookDiagnostic {
    CopybookDiagnostic {
        severity: "error",
        code: "E_COPYBOOK_PARSE",
        line,
        column: None,
        record_name: None,
        field_name,
        message: message.into(),
        help: help.into(),
        source,
    }
}

fn unsupported_diagnostic(
    line: usize,
    field_name: Option<String>,
    construct: impl Into<String>,
    source: &str,
) -> CopybookDiagnostic {
    let construct = construct.into();
    let mut diagnostic = line_diagnostic(
        line,
        field_name,
        format!("unsupported copybook construct {construct}"),
        "remove the unsupported construct or hand-author the schema for this layout",
        source.to_string(),
    );
    diagnostic.code = "E_COPYBOOK_UNSUPPORTED";
    diagnostic
}

fn diagnostic_box(diagnostic: CopybookDiagnostic) -> Box<CopybookDiagnostic> {
    Box::new(diagnostic)
}

impl Usage {
    fn label(self) -> &'static str {
        match self {
            Usage::Display => "DISPLAY",
            Usage::PackedDecimal => "COMP-3/PACKED-DECIMAL",
            Usage::Binary => "COMP/BINARY/COMP-4",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_file(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "hostlens-copybook-{}-{nanos}-{name}",
            std::process::id()
        ))
    }

    fn args(copybook: PathBuf, output: PathBuf) -> CopybookArgs {
        CopybookArgs {
            copybook,
            record_name: None,
            encoding: "cp037".to_string(),
            input_encoding: InputEncoding::Binary,
            on_error: OnError::EmitErrorRow,
            verification_scope: VerificationScope::Field,
            output: Some(output),
            diagnostics: DiagnosticFormat::Text,
            strict: false,
            include_fillers: false,
            drop_fillers: false,
        }
    }

    #[test]
    fn generates_schema_v2_for_mixed_record() {
        let copybook = temp_file("customer.cpy");
        let output = temp_file("customer.schema.json");
        fs::write(
            &copybook,
            r#"
       01 CUSTOMER-REC.
          05 ACCOUNT-ID     PIC X(4).
          05 AMOUNT         PIC S9(5)V99 COMP-3.
          05 TAX            PIC S9(3)V99.
          05 SEQUENCE-NO    PIC 9(9) COMP.
          05 FILLER         PIC X(2).
        "#,
        )
        .unwrap();

        from_copybook(args(copybook.clone(), output.clone())).unwrap();

        let json: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&output).unwrap()).unwrap();
        assert_eq!(json["version"], 2);
        assert_eq!(json["record_length"], 19);
        assert_eq!(json["fields"][0]["name"], "account_id");
        assert_eq!(json["fields"][0]["field_type"], "display-text");
        assert_eq!(json["fields"][0]["encoding"], "cp037");
        assert_eq!(json["fields"][1]["field_type"], "packed-decimal");
        assert_eq!(json["fields"][1]["length"], 4);
        assert_eq!(json["fields"][2]["field_type"], "zoned-decimal");
        assert_eq!(json["fields"][2]["offset"], 8);
        assert_eq!(json["fields"][3]["field_type"], "binary");
        assert_eq!(json["fields"][3]["length"], 4);
        assert_eq!(json["fillers"][0]["offset"], 17);
        assert_eq!(json["fillers"][0]["length"], 2);

        let _ = fs::remove_file(copybook);
        let _ = fs::remove_file(output);
    }

    #[test]
    fn rejects_unsupported_constructs_with_line_numbers() {
        for (expected, line) in [
            (
                "REDEFINES",
                "          05 ALT-AMOUNT REDEFINES AMOUNT PIC X(4).",
            ),
            ("OCCURS", "          05 ITEM PIC X(2) OCCURS 3 TIMES."),
            ("SYNC", "          05 BIN-FIELD PIC 9(4) COMP SYNC."),
            ("level 88", "          88 ACTIVE VALUE 'Y'."),
        ] {
            let copybook = temp_file("bad.cpy");
            let output = temp_file("bad.schema.json");
            fs::write(
                &copybook,
                format!("       01 BAD-REC.\n          05 AMOUNT PIC 9(4).\n{line}\n"),
            )
            .unwrap();

            let err = from_copybook(args(copybook.clone(), output.clone())).unwrap_err();
            assert_eq!(err.code, "E_SCHEMA");
            assert!(err.message.contains(expected), "{:?}", err.message);
            assert!(err.message.contains("line"), "{:?}", err.message);

            let _ = fs::remove_file(copybook);
            let _ = fs::remove_file(output);
        }
    }

    #[test]
    fn requires_record_name_for_multiple_01_records() {
        let copybook = temp_file("multi.cpy");
        let output = temp_file("multi.schema.json");
        fs::write(
            &copybook,
            "       01 FIRST-REC.\n          05 A PIC X(1).\n       01 SECOND-REC.\n          05 B PIC X(1).\n",
        )
        .unwrap();

        let err = from_copybook(args(copybook.clone(), output.clone())).unwrap_err();
        assert_eq!(err.code, "E_SCHEMA");
        assert!(err.message.contains("--record-name"), "{:?}", err.message);

        let _ = fs::remove_file(copybook);
        let _ = fs::remove_file(output);
    }
}
