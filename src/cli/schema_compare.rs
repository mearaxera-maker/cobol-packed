use super::*;

#[derive(Debug, Serialize)]
struct SchemaCompareReport {
    changed: bool,
    breaking_count: usize,
    warning_count: usize,
    info_count: usize,
    diffs: Vec<SchemaDiff>,
}

#[derive(Debug, Serialize)]
struct SchemaDiff {
    severity: &'static str,
    path: String,
    old: Option<String>,
    new: Option<String>,
    message: String,
    #[serde(skip)]
    is_change: bool,
}

#[derive(Clone, Copy)]
enum DiffSeverity {
    Breaking,
    Warning,
    Info,
}

#[derive(Serialize)]
struct ComparableField<'a> {
    name: &'a str,
    field_type: FieldType,
    offset: Option<usize>,
    length: usize,
    total_digits: Option<u8>,
    scale: Option<u8>,
    signed: Option<bool>,
    sign_mode: CliSignMode,
    mode: FieldMode,
    encoding: Option<TextEncoding>,
    required: bool,
}

#[derive(Serialize)]
struct ComparableFiller<'a> {
    name: &'a str,
    offset: usize,
    length: usize,
}

pub(super) fn compare(args: CompareArgs) -> Result<(), CliError> {
    let (old_schema, _) = load_schema(&args.old)?;
    let (new_schema, _) = load_schema(&args.new)?;

    let mut diffs = Vec::new();
    let include_unchanged = args.show_unchanged
        && matches!(
            args.output,
            CompareOutputFormat::Json | CompareOutputFormat::Jsonl
        );

    compare_record_fields(&mut diffs, &old_schema, &new_schema)?;
    compare_fields(&mut diffs, &old_schema, &new_schema, include_unchanged)?;
    compare_fillers(&mut diffs, &old_schema, &new_schema, include_unchanged)?;
    if !args.ignore_order {
        compare_order(
            &mut diffs,
            "fields",
            "field",
            old_schema.fields.iter().map(|field| field.name.as_str()),
            new_schema.fields.iter().map(|field| field.name.as_str()),
        )?;
        compare_order(
            &mut diffs,
            "fillers",
            "filler",
            old_schema.fillers.iter().map(|filler| filler.name.as_str()),
            new_schema.fillers.iter().map(|filler| filler.name.as_str()),
        )?;
    }

    sort_diffs(&mut diffs);
    let report = SchemaCompareReport::new(diffs);
    render_compare_report(&report, args.output)?;

    if fail_on_matches(args.fail_on, &report) {
        Err(CliError::data(
            "E_SCHEMA_DIFF",
            format!(
                "schema diff detected: {} breaking, {} warning, {} info",
                report.breaking_count, report.warning_count, report.info_count
            ),
        ))
    } else {
        Ok(())
    }
}

impl SchemaCompareReport {
    fn new(diffs: Vec<SchemaDiff>) -> Self {
        let changed = diffs.iter().any(|diff| diff.is_change);
        let breaking_count = count_changes(&diffs, "breaking");
        let warning_count = count_changes(&diffs, "warning");
        let info_count = count_changes(&diffs, "info");
        Self {
            changed,
            breaking_count,
            warning_count,
            info_count,
            diffs,
        }
    }
}

impl SchemaDiff {
    fn change(
        severity: DiffSeverity,
        path: impl Into<String>,
        old: Option<String>,
        new: Option<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity: severity.as_str(),
            path: path.into(),
            old,
            new,
            message: message.into(),
            is_change: true,
        }
    }

    fn unchanged(path: impl Into<String>, value: String, message: impl Into<String>) -> Self {
        Self {
            severity: DiffSeverity::Info.as_str(),
            path: path.into(),
            old: Some(value.clone()),
            new: Some(value),
            message: message.into(),
            is_change: false,
        }
    }
}

impl DiffSeverity {
    fn as_str(self) -> &'static str {
        match self {
            DiffSeverity::Breaking => "breaking",
            DiffSeverity::Warning => "warning",
            DiffSeverity::Info => "info",
        }
    }
}

fn count_changes(diffs: &[SchemaDiff], severity: &'static str) -> usize {
    diffs
        .iter()
        .filter(|diff| diff.is_change && diff.severity == severity)
        .count()
}

fn compare_record_fields(
    diffs: &mut Vec<SchemaDiff>,
    old: &Schema,
    new: &Schema,
) -> Result<(), CliError> {
    compare_value(
        diffs,
        DiffSeverity::Breaking,
        "version",
        &old.version,
        &new.version,
        "schema version changed",
    )?;
    compare_optional_value(
        diffs,
        DiffSeverity::Breaking,
        "record_length",
        old.record_length,
        new.record_length,
        "record length changed",
    )?;
    compare_value(
        diffs,
        DiffSeverity::Breaking,
        "input_encoding",
        &old.input_encoding,
        &new.input_encoding,
        "input encoding changed",
    )?;
    compare_value(
        diffs,
        DiffSeverity::Warning,
        "on_error",
        &old.on_error,
        &new.on_error,
        "on_error changed",
    )?;
    compare_value(
        diffs,
        DiffSeverity::Warning,
        "verification_scope",
        &old.verification_scope,
        &new.verification_scope,
        "verification scope changed",
    )?;
    Ok(())
}

fn compare_fields(
    diffs: &mut Vec<SchemaDiff>,
    old_schema: &Schema,
    new_schema: &Schema,
    include_unchanged: bool,
) -> Result<(), CliError> {
    let old_fields = field_map(&old_schema.fields);
    let new_fields = field_map(&new_schema.fields);
    let mut names = BTreeSet::new();
    names.extend(old_fields.keys().copied());
    names.extend(new_fields.keys().copied());

    for name in names {
        match (old_fields.get(name), new_fields.get(name)) {
            (Some(old), Some(new)) => {
                let changed = compare_common_field(diffs, old, new)?;
                if include_unchanged && !changed {
                    let value = json_string(&comparable_field(old)?)?;
                    diffs.push(SchemaDiff::unchanged(
                        format!("fields.{name}"),
                        value,
                        "field unchanged",
                    ));
                }
            }
            (Some(old), None) => {
                diffs.push(SchemaDiff::change(
                    DiffSeverity::Breaking,
                    format!("fields.{name}"),
                    Some(json_string(&comparable_field(old)?)?),
                    None,
                    "field removed",
                ));
            }
            (None, Some(new)) => {
                diffs.push(SchemaDiff::change(
                    DiffSeverity::Warning,
                    format!("fields.{name}"),
                    None,
                    Some(json_string(&comparable_field(new)?)?),
                    "field added",
                ));
            }
            (None, None) => {}
        }
    }

    Ok(())
}

fn compare_common_field(
    diffs: &mut Vec<SchemaDiff>,
    old: &FieldSpec,
    new: &FieldSpec,
) -> Result<bool, CliError> {
    let mut changed = false;
    let base = format!("fields.{}", old.name);
    let old = comparable_field(old)?;
    let new = comparable_field(new)?;
    changed |= compare_value(
        diffs,
        DiffSeverity::Breaking,
        format!("{base}.field_type"),
        &old.field_type,
        &new.field_type,
        "field type changed",
    )?;
    changed |= compare_optional_value(
        diffs,
        DiffSeverity::Breaking,
        format!("{base}.offset"),
        old.offset,
        new.offset,
        "field offset changed",
    )?;
    changed |= compare_optional_value(
        diffs,
        DiffSeverity::Breaking,
        format!("{base}.length"),
        Some(old.length),
        Some(new.length),
        "field length changed",
    )?;
    changed |= compare_optional_value(
        diffs,
        DiffSeverity::Breaking,
        format!("{base}.encoding"),
        old.encoding,
        new.encoding,
        "field encoding changed",
    )?;
    changed |= compare_optional_value(
        diffs,
        DiffSeverity::Breaking,
        format!("{base}.total_digits"),
        old.total_digits,
        new.total_digits,
        "field digit count changed",
    )?;
    changed |= compare_optional_value(
        diffs,
        DiffSeverity::Breaking,
        format!("{base}.scale"),
        old.scale,
        new.scale,
        "field scale changed",
    )?;
    changed |= compare_optional_value(
        diffs,
        DiffSeverity::Breaking,
        format!("{base}.signed"),
        old.signed,
        new.signed,
        "field signedness changed",
    )?;
    changed |= compare_value(
        diffs,
        DiffSeverity::Breaking,
        format!("{base}.sign_mode"),
        &old.sign_mode,
        &new.sign_mode,
        "field sign mode changed",
    )?;
    changed |= compare_value(
        diffs,
        DiffSeverity::Warning,
        format!("{base}.mode"),
        &old.mode,
        &new.mode,
        "field mode changed",
    )?;
    let required_severity = if !old.required && new.required {
        DiffSeverity::Breaking
    } else {
        DiffSeverity::Warning
    };
    changed |= compare_value(
        diffs,
        required_severity,
        format!("{base}.required"),
        &old.required,
        &new.required,
        "field required flag changed",
    )?;
    Ok(changed)
}

fn compare_fillers(
    diffs: &mut Vec<SchemaDiff>,
    old_schema: &Schema,
    new_schema: &Schema,
    include_unchanged: bool,
) -> Result<(), CliError> {
    let old_fillers = filler_map(&old_schema.fillers);
    let new_fillers = filler_map(&new_schema.fillers);
    let mut names = BTreeSet::new();
    names.extend(old_fillers.keys().copied());
    names.extend(new_fillers.keys().copied());

    for name in names {
        match (old_fillers.get(name), new_fillers.get(name)) {
            (Some(old), Some(new)) => {
                let changed = compare_common_filler(diffs, old, new)?;
                if include_unchanged && !changed {
                    let value = json_string(&comparable_filler(old))?;
                    diffs.push(SchemaDiff::unchanged(
                        format!("fillers.{name}"),
                        value,
                        "filler unchanged",
                    ));
                }
            }
            (Some(old), None) => {
                diffs.push(SchemaDiff::change(
                    DiffSeverity::Warning,
                    format!("fillers.{name}"),
                    Some(json_string(&comparable_filler(old))?),
                    None,
                    "filler removed",
                ));
            }
            (None, Some(new)) => {
                diffs.push(SchemaDiff::change(
                    DiffSeverity::Warning,
                    format!("fillers.{name}"),
                    None,
                    Some(json_string(&comparable_filler(new))?),
                    "filler added",
                ));
            }
            (None, None) => {}
        }
    }

    Ok(())
}

fn compare_common_filler(
    diffs: &mut Vec<SchemaDiff>,
    old: &FillerSpec,
    new: &FillerSpec,
) -> Result<bool, CliError> {
    let mut changed = false;
    let base = format!("fillers.{}", old.name);
    changed |= compare_value(
        diffs,
        DiffSeverity::Breaking,
        format!("{base}.offset"),
        &old.offset,
        &new.offset,
        "filler offset changed",
    )?;
    changed |= compare_value(
        diffs,
        DiffSeverity::Breaking,
        format!("{base}.length"),
        &old.length,
        &new.length,
        "filler length changed",
    )?;
    Ok(changed)
}

fn compare_order<'a>(
    diffs: &mut Vec<SchemaDiff>,
    collection: &str,
    item_kind: &str,
    old_names: impl Iterator<Item = &'a str>,
    new_names: impl Iterator<Item = &'a str>,
) -> Result<(), CliError> {
    let old_names: Vec<&str> = old_names.collect();
    let new_names: Vec<&str> = new_names.collect();
    let old_set: BTreeSet<&str> = old_names.iter().copied().collect();
    let new_set: BTreeSet<&str> = new_names.iter().copied().collect();
    let old_common: Vec<&str> = old_names
        .iter()
        .copied()
        .filter(|name| new_set.contains(name))
        .collect();
    let new_common: Vec<&str> = new_names
        .iter()
        .copied()
        .filter(|name| old_set.contains(name))
        .collect();

    if old_common == new_common {
        return Ok(());
    }

    let old_positions = position_map(&old_common);
    let new_positions = position_map(&new_common);
    let mut names = BTreeSet::new();
    names.extend(old_common.iter().copied());
    names.extend(new_common.iter().copied());

    for name in names {
        let old_position = old_positions.get(name).copied();
        let new_position = new_positions.get(name).copied();
        if old_position != new_position {
            diffs.push(SchemaDiff::change(
                DiffSeverity::Info,
                format!("{collection}.{name}.order"),
                format_optional_json(old_position)?,
                format_optional_json(new_position)?,
                format!("{item_kind} order changed"),
            ));
        }
    }

    Ok(())
}

fn compare_value<T: Serialize + PartialEq>(
    diffs: &mut Vec<SchemaDiff>,
    severity: DiffSeverity,
    path: impl Into<String>,
    old: &T,
    new: &T,
    message: impl Into<String>,
) -> Result<bool, CliError> {
    if old == new {
        return Ok(false);
    }
    diffs.push(SchemaDiff::change(
        severity,
        path,
        Some(json_string(old)?),
        Some(json_string(new)?),
        message,
    ));
    Ok(true)
}

fn compare_optional_value<T: Copy + Serialize + PartialEq>(
    diffs: &mut Vec<SchemaDiff>,
    severity: DiffSeverity,
    path: impl Into<String>,
    old: Option<T>,
    new: Option<T>,
    message: impl Into<String>,
) -> Result<bool, CliError> {
    if old == new {
        return Ok(false);
    }
    diffs.push(SchemaDiff::change(
        severity,
        path,
        format_optional_json(old)?,
        format_optional_json(new)?,
        message,
    ));
    Ok(true)
}

fn field_map(fields: &[FieldSpec]) -> BTreeMap<&str, &FieldSpec> {
    fields
        .iter()
        .map(|field| (field.name.as_str(), field))
        .collect()
}

fn filler_map(fillers: &[FillerSpec]) -> BTreeMap<&str, &FillerSpec> {
    fillers
        .iter()
        .map(|filler| (filler.name.as_str(), filler))
        .collect()
}

fn position_map<'a>(names: &[&'a str]) -> BTreeMap<&'a str, usize> {
    names
        .iter()
        .enumerate()
        .map(|(idx, name)| (*name, idx))
        .collect()
}

fn comparable_field(field: &FieldSpec) -> Result<ComparableField<'_>, CliError> {
    let plan = plan_field(field)?;
    let (scale, signed) = match plan.kind {
        FieldPlanKind::PackedDecimal(_) | FieldPlanKind::ZonedDecimal(_) => {
            (Some(field.scale.unwrap_or(0)), field.signed)
        }
        FieldPlanKind::Binary { signed, scale } => (Some(scale), Some(signed)),
        FieldPlanKind::DisplayText(_)
        | FieldPlanKind::MixedDbcsText(_)
        | FieldPlanKind::RawBytes => (None, None),
    };

    Ok(ComparableField {
        name: &field.name,
        field_type: field.field_type,
        offset: field.offset,
        length: field.length.unwrap_or(plan.expected_len),
        total_digits: field.total_digits,
        scale,
        signed,
        sign_mode: field.sign_mode,
        mode: field.mode,
        encoding: field.encoding,
        required: field.required,
    })
}

fn comparable_filler(filler: &FillerSpec) -> ComparableFiller<'_> {
    ComparableFiller {
        name: &filler.name,
        offset: filler.offset,
        length: filler.length,
    }
}

fn json_string<T: Serialize + ?Sized>(value: &T) -> Result<String, CliError> {
    serde_json::to_string(value).map_err(|err| {
        CliError::internal(format!("failed to serialize schema compare value: {err}"))
    })
}

fn format_optional_json<T: Serialize>(value: Option<T>) -> Result<Option<String>, CliError> {
    value.map(|value| json_string(&value)).transpose()
}

fn sort_diffs(diffs: &mut [SchemaDiff]) {
    diffs.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| severity_rank(left.severity).cmp(&severity_rank(right.severity)))
            .then_with(|| left.message.cmp(&right.message))
    });
}

fn severity_rank(severity: &str) -> u8 {
    match severity {
        "breaking" => 0,
        "warning" => 1,
        "info" => 2,
        _ => 3,
    }
}

fn render_compare_report(
    report: &SchemaCompareReport,
    output: CompareOutputFormat,
) -> Result<(), CliError> {
    match output {
        CompareOutputFormat::Table => render_compare_table(report),
        CompareOutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(report)?);
            Ok(())
        }
        CompareOutputFormat::Jsonl => {
            for diff in &report.diffs {
                println!("{}", serde_json::to_string(diff)?);
            }
            Ok(())
        }
    }
}

fn render_compare_table(report: &SchemaCompareReport) -> Result<(), CliError> {
    let mut out = String::new();
    writeln!(&mut out, "severity\tpath\told\tnew\tmessage")
        .map_err(|err| CliError::internal(format!("failed to render compare table: {err}")))?;
    for diff in &report.diffs {
        writeln!(
            &mut out,
            "{}\t{}\t{}\t{}\t{}",
            diff.severity,
            table_cell(&diff.path),
            table_optional(diff.old.as_deref()),
            table_optional(diff.new.as_deref()),
            table_cell(&diff.message),
        )
        .map_err(|err| CliError::internal(format!("failed to render compare table: {err}")))?;
    }
    print!("{out}");
    Ok(())
}

fn table_optional(value: Option<&str>) -> String {
    value.map(table_cell).unwrap_or_else(|| "-".to_string())
}

fn table_cell(value: &str) -> String {
    if value
        .chars()
        .any(|ch| matches!(ch, '\t' | '\n' | '\r' | '"'))
    {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn fail_on_matches(fail_on: CompareFailOn, report: &SchemaCompareReport) -> bool {
    match fail_on {
        CompareFailOn::Never => false,
        CompareFailOn::Any => report.changed,
        CompareFailOn::Breaking => report.breaking_count > 0,
        CompareFailOn::Warning => report.breaking_count > 0 || report.warning_count > 0,
    }
}
