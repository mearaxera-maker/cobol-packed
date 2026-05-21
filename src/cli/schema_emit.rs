use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

pub(super) fn emit_rust(args: EmitRustArgs) -> Result<(), CliError> {
    let (schema, _) = load_schema(&args.schema)?;
    let source = render_rust(&schema, &args)?;
    if let Some(path) = args.output {
        fs::write(path, source)?;
    } else {
        print!("{source}");
    }
    Ok(())
}

fn render_rust(schema: &Schema, args: &EmitRustArgs) -> Result<String, CliError> {
    validate_type_name(&args.struct_name)?;

    let vis = visibility(args.visibility);
    let fields = rust_fields(&schema.fields)?;
    let constants = rust_constants(&schema.fields)?;

    let mut out = String::new();
    out.push_str("// Generated from a validated HostLens schema.\n");
    out.push_str("// Rust values are decoded fields, not a storage-layout view.\n");
    if let Some(module_name) = &args.module_name {
        writeln!(out, "// Module: {}", ascii_comment(module_name))
            .map_err(|err| CliError::internal(format!("failed to render Rust source: {err}")))?;
    }
    out.push('\n');

    if let Some(record_len) = schema.record_length {
        writeln!(out, "{vis}const RECORD_LEN: usize = {record_len};")
            .map_err(|err| CliError::internal(format!("failed to render Rust source: {err}")))?;
        out.push('\n');
    }

    for constant in &constants {
        writeln!(
            out,
            "{vis}const {}: usize = {};",
            constant.offset_ident, constant.offset
        )
        .map_err(|err| CliError::internal(format!("failed to render Rust source: {err}")))?;
        writeln!(
            out,
            "{vis}const {}: usize = {};",
            constant.len_ident, constant.length
        )
        .map_err(|err| CliError::internal(format!("failed to render Rust source: {err}")))?;
    }
    if !constants.is_empty() {
        out.push('\n');
    }

    let derives = derive_names(&args.derive);
    if !derives.is_empty() {
        writeln!(out, "#[derive({})]", derives.join(", "))
            .map_err(|err| CliError::internal(format!("failed to render Rust source: {err}")))?;
    }
    writeln!(out, "{vis}struct {} {{", args.struct_name)
        .map_err(|err| CliError::internal(format!("failed to render Rust source: {err}")))?;
    for field in &fields {
        writeln!(out, "    /// {}", field_comment(field.spec, field.length))
            .map_err(|err| CliError::internal(format!("failed to render Rust source: {err}")))?;
        writeln!(out, "    {vis}{}: {},", field.ident, rust_type(field.spec))
            .map_err(|err| CliError::internal(format!("failed to render Rust source: {err}")))?;
    }
    out.push_str("}\n");

    if args.raw_slices && !constants.is_empty() {
        out.push('\n');
        writeln!(out, "impl {} {{", args.struct_name)
            .map_err(|err| CliError::internal(format!("failed to render Rust source: {err}")))?;
        for (field, constant) in fields.iter().zip(constants.iter()) {
            writeln!(
                out,
                "    {vis}fn {}_raw(record: &[u8]) -> Option<&[u8]> {{",
                field.ident
            )
            .map_err(|err| CliError::internal(format!("failed to render Rust source: {err}")))?;
            writeln!(
                out,
                "        record.get({}..{} + {})",
                constant.offset_ident, constant.offset_ident, constant.len_ident
            )
            .map_err(|err| CliError::internal(format!("failed to render Rust source: {err}")))?;
            out.push_str("    }\n");
        }
        out.push_str("}\n");
    }

    Ok(out)
}

struct RustField<'a> {
    spec: &'a FieldSpec,
    ident: String,
    length: usize,
}

struct RustConstant {
    offset_ident: String,
    len_ident: String,
    offset: usize,
    length: usize,
}

fn rust_fields(fields: &[FieldSpec]) -> Result<Vec<RustField<'_>>, CliError> {
    let mut allocator = IdentAllocator::default();
    let mut out = Vec::with_capacity(fields.len());
    for field in fields {
        let plan = plan_field(field)?;
        out.push(RustField {
            spec: field,
            ident: allocator.allocate_field(&field.name),
            length: field.length.unwrap_or(plan.expected_len),
        });
    }
    Ok(out)
}

fn rust_constants(fields: &[FieldSpec]) -> Result<Vec<RustConstant>, CliError> {
    let mut allocator = IdentAllocator::default();
    let mut out = Vec::new();
    for field in fields {
        if let Some(offset) = field.offset {
            let plan = plan_field(field)?;
            let base = allocator.allocate_const_base(&field.name);
            out.push(RustConstant {
                offset_ident: rust_const_ident(&base, "OFFSET"),
                len_ident: rust_const_ident(&base, "LEN"),
                offset,
                length: field.length.unwrap_or(plan.expected_len),
            });
        }
    }
    Ok(out)
}

#[derive(Default)]
struct IdentAllocator {
    used: BTreeSet<String>,
    counts: BTreeMap<String, usize>,
}

impl IdentAllocator {
    fn allocate_field(&mut self, name: &str) -> String {
        self.allocate(rust_field_ident(name))
    }

    fn allocate_const_base(&mut self, name: &str) -> String {
        self.allocate(rust_const_base(name))
    }

    fn allocate(&mut self, base: String) -> String {
        let mut next = self.counts.get(&base).copied().unwrap_or(0) + 1;
        loop {
            let candidate = if next == 1 {
                base.clone()
            } else {
                format!("{base}_{next}")
            };
            if self.used.insert(candidate.clone()) {
                self.counts.insert(base, next);
                return candidate;
            }
            next += 1;
        }
    }
}

fn validate_type_name(name: &str) -> Result<(), CliError> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err(CliError::config(
            "E_SCHEMA",
            "Rust struct name cannot be empty",
        ));
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return Err(CliError::config(
            "E_SCHEMA",
            format!("Rust struct name {name:?} must start with ASCII letter or underscore"),
        ));
    }
    if !chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric()) {
        return Err(CliError::config(
            "E_SCHEMA",
            format!("Rust struct name {name:?} must be an ASCII Rust identifier"),
        ));
    }
    if name == "_" || is_rust_keyword(name) {
        return Err(CliError::config(
            "E_SCHEMA",
            format!("Rust struct name {name:?} is a reserved keyword"),
        ));
    }
    Ok(())
}

fn rust_field_ident(name: &str) -> String {
    rust_field_base(name)
}

fn rust_const_ident(name: &str, suffix: &str) -> String {
    let suffix = sanitize_const_suffix(suffix);
    let base = rust_const_base(name);
    if suffix.is_empty() {
        base
    } else {
        format!("{base}_{suffix}")
    }
}

fn rust_field_base(name: &str) -> String {
    let mut ident = snake_base(name, false);
    if ident
        .as_bytes()
        .first()
        .is_some_and(|first| first.is_ascii_digit())
    {
        ident = format!("field_{ident}");
    }
    if is_rust_keyword(&ident) {
        ident.push_str("_field");
    }
    ident
}

fn rust_const_base(name: &str) -> String {
    let mut ident = snake_base(name, true);
    if ident
        .as_bytes()
        .first()
        .is_some_and(|first| first.is_ascii_digit())
    {
        ident = format!("FIELD_{ident}");
    }
    ident
}

fn snake_base(name: &str, uppercase: bool) -> String {
    let mut out = String::new();
    let mut last_was_separator = true;
    let mut previous_was_lower_or_digit = false;
    for byte in name.bytes() {
        let next = match byte {
            b'a'..=b'z' if uppercase => Some((byte as char).to_ascii_uppercase()),
            b'A'..=b'Z' if !uppercase => {
                if previous_was_lower_or_digit && !last_was_separator {
                    out.push('_');
                }
                Some((byte as char).to_ascii_lowercase())
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' => Some(byte as char),
            b'_' | b'-' | b'.' => {
                if last_was_separator {
                    None
                } else {
                    Some('_')
                }
            }
            _ => {
                if last_was_separator {
                    None
                } else {
                    Some('_')
                }
            }
        };
        if let Some(ch) = next {
            out.push(ch);
            last_was_separator = ch == '_';
            previous_was_lower_or_digit = byte.is_ascii_lowercase() || byte.is_ascii_digit();
        } else {
            previous_was_lower_or_digit = false;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    if out.is_empty() {
        if uppercase {
            "FIELD".to_string()
        } else {
            "field".to_string()
        }
    } else {
        out
    }
}

fn sanitize_const_suffix(suffix: &str) -> String {
    if suffix.is_empty() {
        String::new()
    } else {
        snake_base(suffix, true)
    }
}

fn rust_type(field: &FieldSpec) -> &'static str {
    match field.field_type {
        FieldType::DisplayText
        | FieldType::MixedDbcsText
        | FieldType::PackedDecimal
        | FieldType::ZonedDecimal => "String",
        FieldType::Binary if field.signed.unwrap_or(true) => "i64",
        FieldType::Binary => "u64",
        FieldType::RawBytes => "Vec<u8>",
    }
}

fn visibility(vis: RustVisibility) -> &'static str {
    match vis {
        RustVisibility::Pub => "pub ",
        RustVisibility::PubCrate => "pub(crate) ",
        RustVisibility::Private => "",
    }
}

fn derive_names(derives: &[RustDerive]) -> Vec<&'static str> {
    let mut out = Vec::new();
    if derives.contains(&RustDerive::Debug) {
        out.push("Debug");
    }
    if derives.contains(&RustDerive::Clone) {
        out.push("Clone");
    }
    if derives.contains(&RustDerive::Serde) {
        out.push("serde::Serialize");
        out.push("serde::Deserialize");
    }
    out
}

fn field_comment(field: &FieldSpec, length: usize) -> String {
    let mut parts = vec![format!("HostLens {}", field_type_label(field.field_type))];
    if let Some(encoding) = field.encoding {
        parts.push(format!("encoding={encoding}"));
    }
    if let Some(total_digits) = field.total_digits {
        parts.push(format!("total_digits={total_digits}"));
    }
    if let Some(scale) = field.scale {
        parts.push(format!("scale={scale}"));
    }
    if let Some(signed) = field.signed {
        parts.push(format!("signed={signed}"));
    }
    parts.push(format!("length={length}"));
    let mut comment = format!("{}.", parts.join("; "));
    if let Some(description) = &field.description {
        comment.push(' ');
        comment.push_str(&ascii_comment(description));
    }
    comment
}

fn field_type_label(field_type: FieldType) -> &'static str {
    match field_type {
        FieldType::PackedDecimal => "packed-decimal",
        FieldType::DisplayText => "display-text",
        FieldType::MixedDbcsText => "mixed-dbcs-text",
        FieldType::ZonedDecimal => "zoned-decimal",
        FieldType::Binary => "binary",
        FieldType::RawBytes => "raw-bytes",
    }
}

fn ascii_comment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_graphic() || ch == ' ' {
                ch
            } else {
                '?'
            }
        })
        .collect()
}

fn is_rust_keyword(value: &str) -> bool {
    matches!(
        value,
        "Self"
            | "abstract"
            | "as"
            | "async"
            | "await"
            | "become"
            | "box"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "do"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "final"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "macro"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "override"
            | "priv"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "try"
            | "type"
            | "typeof"
            | "union"
            | "unsafe"
            | "unsized"
            | "use"
            | "virtual"
            | "where"
            | "while"
            | "yield"
    )
}
