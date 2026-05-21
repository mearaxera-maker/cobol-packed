use super::*;

#[derive(Debug, Clone, Copy)]
pub(super) struct ProcessingLimits {
    pub(super) max_records: Option<usize>,
    pub(super) max_buffered_rows: Option<usize>,
    pub(super) parallelism: usize,
    pub(super) dry_run: bool,
    pub(super) quiet: bool,
}

pub(super) struct RecordOutcome {
    pub(super) events: Vec<RecordEvent>,
    pub(super) record_valid: bool,
    pub(super) failure: Option<(&'static str, String)>,
}

pub(super) struct RecordEvent {
    pub(super) row: DecodedField,
    pub(super) emit: bool,
}

pub(super) fn batch_decode(args: BatchInputArgs) -> Result<(), CliError> {
    let (schema, hashes) = load_schema(&args.schema)?;
    let output = args.output.or(schema.output).unwrap_or(OutputFormat::Jsonl);
    let limits = limits_from_args(&args)?;
    let verification_scope = verification_scope_from_args(&schema, &args)?;
    let mut audit = new_audit(
        &schema,
        AuditContext {
            command: "batch decode",
            hashes: &hashes,
            input: &args.input,
            limits,
            failure_sample_limit: args.sample_failures,
            include_input_hash: output == OutputFormat::Audit,
            verification_scope,
            evidence_mode: args.evidence_mode,
        },
    )?;
    let mut sink = RowSink::for_output(output, limits);
    process_records(
        &schema,
        &args.input,
        false,
        limits,
        args.one_based_index,
        &mut audit,
        &mut sink,
    )?;
    finalize_audit(&mut audit);
    finalize_audit_metrics(&mut audit);
    report_progress(args.progress, &audit);
    match output {
        OutputFormat::Audit => render_audit(&audit),
        _ => sink.finish(output),
    }?;
    fail_if_empty(args.fail_on_empty, &audit)
}

pub(super) fn batch_verify(args: BatchInputArgs) -> Result<(), CliError> {
    let (schema, hashes) = load_schema(&args.schema)?;
    let output = args.output.or(schema.output).unwrap_or(OutputFormat::Audit);
    let limits = limits_from_args(&args)?;
    let verification_scope = verification_scope_from_args(&schema, &args)?;
    let mut audit = new_audit(
        &schema,
        AuditContext {
            command: "batch verify",
            hashes: &hashes,
            input: &args.input,
            limits,
            failure_sample_limit: args.sample_failures,
            include_input_hash: true,
            verification_scope,
            evidence_mode: args.evidence_mode,
        },
    )?;
    let mut sink = RowSink::for_output(output, limits);
    process_records(
        &schema,
        &args.input,
        true,
        limits,
        args.one_based_index,
        &mut audit,
        &mut sink,
    )?;
    finalize_audit(&mut audit);
    finalize_verify_audit(&mut audit);
    finalize_audit_metrics(&mut audit);
    report_progress(args.progress, &audit);
    match output {
        OutputFormat::Audit => render_audit(&audit),
        _ => sink.finish(output),
    }?;
    fail_if_empty(args.fail_on_empty, &audit)?;
    if matches!(audit.status, AuditStatus::Failed) {
        return Err(CliError::data(
            "E_VERIFY",
            "batch verify failed; see output or audit report for failing records",
        ));
    }
    Ok(())
}

pub(super) fn profile(args: BatchInputArgs) -> Result<(), CliError> {
    let (schema, hashes) = load_schema(&args.schema)?;
    let output = args.output.or(schema.output).unwrap_or(OutputFormat::Audit);
    let limits = limits_from_args(&args)?;
    let verification_scope = verification_scope_from_args(&schema, &args)?;
    let mut audit = new_audit(
        &schema,
        AuditContext {
            command: "profile",
            hashes: &hashes,
            input: &args.input,
            limits,
            failure_sample_limit: args.sample_failures,
            include_input_hash: true,
            verification_scope,
            evidence_mode: args.evidence_mode,
        },
    )?;
    let mut sink = RowSink::None;
    process_records(
        &schema,
        &args.input,
        false,
        limits,
        args.one_based_index,
        &mut audit,
        &mut sink,
    )?;
    finalize_audit(&mut audit);
    finalize_audit_metrics(&mut audit);
    report_progress(args.progress, &audit);
    render_audit_with_format(&audit, output)?;
    fail_if_empty(args.fail_on_empty, &audit)
}

fn process_records(
    schema: &Schema,
    input: &Path,
    verify: bool,
    limits: ProcessingLimits,
    one_based_index: bool,
    audit: &mut AuditReport,
    sink: &mut RowSink,
) -> Result<(), CliError> {
    if limits.dry_run {
        return process_records_dry_run(schema, input, limits, audit);
    }
    if limits.parallelism > 1 && !matches!(schema.input_encoding, InputEncoding::Binary) {
        return Err(CliError::config(
            "E_CONFIG",
            "--parallel currently supports fixed-width binary input only",
        ));
    }
    match schema.input_encoding {
        InputEncoding::Binary => {
            process_binary_records(schema, input, verify, limits, one_based_index, audit, sink)?
        }
        InputEncoding::Hex => {
            process_hex_records(schema, input, verify, limits, one_based_index, audit, sink)?
        }
        InputEncoding::Csv => {
            process_csv_records(schema, input, verify, limits, one_based_index, audit, sink)?
        }
        InputEncoding::Jsonl => {
            process_jsonl_records(schema, input, verify, limits, one_based_index, audit, sink)?
        }
    }
    Ok(())
}

fn process_records_dry_run(
    schema: &Schema,
    input: &Path,
    limits: ProcessingLimits,
    audit: &mut AuditReport,
) -> Result<(), CliError> {
    match schema.input_encoding {
        InputEncoding::Binary => {
            let record_len = schema.record_length.ok_or_else(|| {
                CliError::config("E_SCHEMA", "binary schemas require record_length")
            })?;
            let mut reader = BufReader::new(open_input(input)?);
            let mut buf = vec![0u8; record_len];
            loop {
                if record_limit_reached(audit.records_seen, limits) {
                    break;
                }
                let mut read = 0usize;
                while read < record_len {
                    let n = reader.read(&mut buf[read..])?;
                    if n == 0 {
                        break;
                    }
                    read += n;
                }
                if read == 0 {
                    break;
                }
                if read != record_len {
                    let row = DecodedField::error(
                        Some(audit.records_seen),
                        "<record>",
                        Some(read),
                        &buf[..read],
                        "E_RECORD_LENGTH",
                        format!("truncated record: expected {record_len} bytes, got {read}"),
                    );
                    audit.records_seen += 1;
                    audit.records_invalid += 1;
                    record_audit(audit, &row);
                    break;
                }
                audit.records_seen += 1;
            }
        }
        InputEncoding::Hex | InputEncoding::Jsonl => {
            let mut reader = BufReader::new(open_input(input)?);
            let mut line = String::new();
            while read_bounded_line(&mut reader, &mut line, MAX_LINE_BYTES)? != 0 {
                if line.trim().is_empty() {
                    continue;
                }
                if record_limit_reached(audit.records_seen, limits) {
                    break;
                }
                audit.records_seen += 1;
            }
        }
        InputEncoding::Csv => {
            let mut reader = BufReader::new(open_input(input)?);
            let mut line = String::new();
            if read_bounded_line(&mut reader, &mut line, MAX_LINE_BYTES)? == 0 {
                return Ok(());
            }
            while read_bounded_line(&mut reader, &mut line, MAX_LINE_BYTES)? != 0 {
                if line.trim().is_empty() {
                    continue;
                }
                if record_limit_reached(audit.records_seen, limits) {
                    break;
                }
                audit.records_seen += 1;
            }
        }
    }
    Ok(())
}

fn process_binary_records(
    schema: &Schema,
    input: &Path,
    verify: bool,
    limits: ProcessingLimits,
    one_based_index: bool,
    audit: &mut AuditReport,
    sink: &mut RowSink,
) -> Result<(), CliError> {
    if limits.parallelism > 1 {
        return process_binary_records_parallel(
            schema,
            input,
            verify,
            limits,
            one_based_index,
            audit,
            sink,
        );
    }
    let record_len = schema
        .record_length
        .ok_or_else(|| CliError::config("E_SCHEMA", "binary schemas require record_length"))?;
    let mut reader = BufReader::new(open_input(input)?);
    let mut record = vec![0u8; record_len];
    let mut idx = 0usize;
    loop {
        if record_limit_reached(idx, limits) {
            break;
        }
        let mut read = 0usize;
        while read < record_len {
            let n = reader.read(&mut record[read..])?;
            if n == 0 {
                break;
            }
            read += n;
        }
        if read == 0 {
            break;
        }
        if read != record_len {
            let err = DecodedField::error(
                Some(idx),
                "<record>",
                Some(read),
                &record[..read],
                "E_RECORD_LENGTH",
                format!("truncated record: expected {record_len} bytes, got {read}"),
            );
            handle_record_error(schema, audit, sink, err)?;
            break;
        }
        let record_index = display_record_index(idx, one_based_index);
        process_record_bytes(schema, record_index, &record, verify, audit, sink)?;
        record.fill(0);
        idx += 1;
    }
    Ok(())
}

fn process_binary_records_parallel(
    schema: &Schema,
    input: &Path,
    verify: bool,
    limits: ProcessingLimits,
    one_based_index: bool,
    audit: &mut AuditReport,
    sink: &mut RowSink,
) -> Result<(), CliError> {
    let record_len = schema
        .record_length
        .ok_or_else(|| CliError::config("E_SCHEMA", "binary schemas require record_length"))?;
    let mut data = Vec::new();
    open_input(input)?.read_to_end(&mut data)?;
    let full_records = data.len() / record_len;
    let remainder = data.len() % record_len;
    let records_to_process = limits
        .max_records
        .map_or(full_records, |limit| limit.min(full_records));
    if records_to_process > 0 {
        let worker_count = limits.parallelism.min(records_to_process);
        let chunk_size = records_to_process.div_ceil(worker_count);
        let data_ref = data.as_slice();
        let mut results = Vec::with_capacity(records_to_process);
        thread::scope(|scope| -> Result<(), CliError> {
            let mut handles = Vec::new();
            for worker in 0..worker_count {
                let start = worker * chunk_size;
                let end = ((worker + 1) * chunk_size).min(records_to_process);
                if start >= end {
                    continue;
                }
                let chunk_data = data_ref;
                handles.push(scope.spawn(
                    move || -> Result<Vec<(usize, RecordOutcome)>, CliError> {
                        let mut chunk = Vec::with_capacity(end - start);
                        for idx in start..end {
                            let offset = idx * record_len;
                            let record = &chunk_data[offset..offset + record_len];
                            let display_idx = display_record_index(idx, one_based_index);
                            let outcome =
                                decode_fixed_record_outcome(schema, display_idx, record, verify)?;
                            chunk.push((idx, outcome));
                        }
                        Ok(chunk)
                    },
                ));
            }
            for handle in handles {
                let chunk = handle
                    .join()
                    .map_err(|_| CliError::internal("parallel record worker panicked"))??;
                results.extend(chunk);
            }
            Ok(())
        })?;
        results.sort_by_key(|(idx, _)| *idx);
        for (_, outcome) in results {
            apply_record_outcome(audit, sink, outcome)?;
        }
    }
    let limit_allows_remainder = match limits.max_records {
        Some(limit) => full_records < limit,
        None => true,
    };
    if remainder != 0 && limit_allows_remainder {
        let err = DecodedField::error(
            Some(display_record_index(full_records, one_based_index)),
            "<record>",
            Some(remainder),
            &data[full_records * record_len..],
            "E_RECORD_LENGTH",
            format!("truncated record: expected {record_len} bytes, got {remainder}"),
        );
        handle_record_error(schema, audit, sink, err)?;
    }
    Ok(())
}

fn process_hex_records(
    schema: &Schema,
    input: &Path,
    verify: bool,
    limits: ProcessingLimits,
    one_based_index: bool,
    audit: &mut AuditReport,
    sink: &mut RowSink,
) -> Result<(), CliError> {
    let mut reader = BufReader::new(open_input(input)?);
    let max_record_len = schema.record_length.unwrap_or(MAX_RECORD_BYTES);
    let mut line = String::new();
    let mut idx = 0usize;
    while read_bounded_line(&mut reader, &mut line, MAX_LINE_BYTES)? != 0 {
        if line.trim().is_empty() {
            continue;
        }
        if record_limit_reached(idx, limits) {
            break;
        }
        let record = match parse_hex_with_limit(&line, Some(max_record_len)) {
            Ok(record) => record,
            Err(err) => {
                let row = DecodedField::error(
                    Some(display_record_index(idx, one_based_index)),
                    "<record>",
                    None,
                    line.as_bytes(),
                    err.code,
                    err.message,
                );
                handle_record_error(schema, audit, sink, row)?;
                idx += 1;
                continue;
            }
        };
        let record_index = display_record_index(idx, one_based_index);
        process_record_bytes(schema, record_index, &record, verify, audit, sink)?;
        idx += 1;
    }
    Ok(())
}

fn process_csv_records(
    schema: &Schema,
    input: &Path,
    verify: bool,
    limits: ProcessingLimits,
    one_based_index: bool,
    audit: &mut AuditReport,
    sink: &mut RowSink,
) -> Result<(), CliError> {
    let mut reader = BufReader::new(open_input(input)?);
    let mut line = String::new();
    if read_bounded_line(&mut reader, &mut line, MAX_LINE_BYTES)? == 0 {
        return Ok(());
    }
    let headers = parse_csv_line(&line)?;
    let mut idx = 0usize;
    while read_bounded_line(&mut reader, &mut line, MAX_LINE_BYTES)? != 0 {
        if line.trim().is_empty() {
            continue;
        }
        if record_limit_reached(idx, limits) {
            break;
        }
        let record = match parse_csv_line(&line) {
            Ok(record) => record,
            Err(err) => {
                let row = DecodedField::error(
                    Some(display_record_index(idx, one_based_index)),
                    "<record>",
                    None,
                    line.as_bytes(),
                    err.code,
                    err.message,
                );
                handle_record_error(schema, audit, sink, row)?;
                idx += 1;
                continue;
            }
        };
        if record.len() != headers.len() {
            let row = DecodedField::error(
                Some(display_record_index(idx, one_based_index)),
                "<record>",
                None,
                line.as_bytes(),
                "E_CSV",
                format!(
                    "expected {} CSV fields, got {}",
                    headers.len(),
                    record.len()
                ),
            );
            handle_record_error(schema, audit, sink, row)?;
            idx += 1;
            continue;
        }
        let mut fields = BTreeMap::new();
        for (header, value) in headers.iter().zip(record.iter()) {
            fields.insert(header.to_string(), value.to_string());
        }
        let record_index = display_record_index(idx, one_based_index);
        process_named_hex_fields(
            schema,
            record_index,
            &fields,
            &BTreeMap::new(),
            verify,
            audit,
            sink,
        )?;
        idx += 1;
    }
    Ok(())
}

fn process_jsonl_records(
    schema: &Schema,
    input: &Path,
    verify: bool,
    limits: ProcessingLimits,
    one_based_index: bool,
    audit: &mut AuditReport,
    sink: &mut RowSink,
) -> Result<(), CliError> {
    let mut reader = BufReader::new(open_input(input)?);
    let mut line = String::new();
    let mut idx = 0usize;
    while read_bounded_line(&mut reader, &mut line, MAX_LINE_BYTES)? != 0 {
        if line.trim().is_empty() {
            continue;
        }
        if record_limit_reached(idx, limits) {
            break;
        }
        let value: serde_json::Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(err) => {
                let row = DecodedField::error(
                    Some(display_record_index(idx, one_based_index)),
                    "<record>",
                    None,
                    line.as_bytes(),
                    "E_JSON",
                    err.to_string(),
                );
                handle_record_error(schema, audit, sink, row)?;
                idx += 1;
                continue;
            }
        };
        let Some(obj) = value.as_object() else {
            let row = DecodedField::error(
                Some(display_record_index(idx, one_based_index)),
                "<record>",
                None,
                line.as_bytes(),
                "E_JSON",
                format!("record {idx} is not a JSON object"),
            );
            handle_record_error(schema, audit, sink, row)?;
            idx += 1;
            continue;
        };
        let mut fields = BTreeMap::new();
        let mut type_errors = BTreeMap::new();
        for (key, value) in obj {
            if let Some(s) = value.as_str() {
                fields.insert(key.clone(), s.to_string());
            } else if schema.fields.iter().any(|field| field.name == *key) {
                type_errors.insert(key.clone(), serde_json::to_string(value)?);
            }
        }
        let record_index = display_record_index(idx, one_based_index);
        process_named_hex_fields(
            schema,
            record_index,
            &fields,
            &type_errors,
            verify,
            audit,
            sink,
        )?;
        idx += 1;
    }
    Ok(())
}

fn process_record_bytes(
    schema: &Schema,
    idx: usize,
    record: &[u8],
    verify: bool,
    audit: &mut AuditReport,
    sink: &mut RowSink,
) -> Result<(), CliError> {
    if let Some(expected) = schema.record_length {
        if record.len() != expected {
            let err = DecodedField::error(
                Some(idx),
                "<record>",
                Some(record.len()),
                record,
                "E_RECORD_LENGTH",
                format!(
                    "record length mismatch: expected {expected} bytes, got {}",
                    record.len()
                ),
            );
            handle_record_error(schema, audit, sink, err)?;
            return Ok(());
        }
    }
    if schema.on_error != OnError::EmitErrorRow {
        return process_record_bytes_atomic(schema, idx, record, verify, audit, sink);
    }
    audit.records_seen += 1;
    let before_invalid = audit.fields_invalid;
    for spec in &schema.fields {
        let plan = plan_field(spec)?;
        let offset = plan.spec.offset.ok_or_else(|| {
            CliError::config(
                "E_SCHEMA",
                format!(
                    "field {} requires offset for fixed-width input",
                    plan.spec.name
                ),
            )
        })?;
        let length = plan.spec.length.unwrap_or(plan.expected_len);
        if offset
            .checked_add(length)
            .map_or(true, |end| end > record.len())
        {
            let err = DecodedField::error(
                Some(idx),
                &plan.spec.name,
                Some(offset),
                &[],
                "E_OFFSET",
                "field extends past record boundary",
            );
            if !handle_row_error(schema, audit, sink, err)? {
                break;
            }
            continue;
        }
        let bytes = &record[offset..offset + length];
        let row = decode_plan_field(Some(idx), &plan, Some(offset), bytes, verify);
        if !handle_decoded_row(schema, audit, sink, row)? {
            break;
        }
    }
    if audit.fields_invalid == before_invalid {
        audit.records_valid += 1;
    } else {
        audit.records_invalid += 1;
    }
    Ok(())
}

fn process_record_bytes_atomic(
    schema: &Schema,
    idx: usize,
    record: &[u8],
    verify: bool,
    audit: &mut AuditReport,
    sink: &mut RowSink,
) -> Result<(), CliError> {
    audit.records_seen += 1;
    let mut pending = Vec::new();
    for spec in &schema.fields {
        let plan = plan_field(spec)?;
        let offset = plan.spec.offset.ok_or_else(|| {
            CliError::config(
                "E_SCHEMA",
                format!(
                    "field {} requires offset for fixed-width input",
                    plan.spec.name
                ),
            )
        })?;
        let length = plan.spec.length.unwrap_or(plan.expected_len);
        if offset
            .checked_add(length)
            .map_or(true, |end| end > record.len())
        {
            let err = DecodedField::error(
                Some(idx),
                &plan.spec.name,
                Some(offset),
                &[],
                "E_OFFSET",
                "field extends past record boundary",
            );
            record_audit(audit, &err);
            audit.records_invalid += 1;
            return match schema.on_error {
                OnError::Fail => Err(CliError::data(
                    err.error_code.unwrap_or("E_DATA"),
                    err.message.unwrap_or_else(|| "data error".to_string()),
                )),
                OnError::SkipRecord => Ok(()),
                OnError::EmitErrorRow => Err(CliError::internal(
                    "emit-error-row reached atomic fixed-width processor",
                )),
            };
        }
        let bytes = &record[offset..offset + length];
        let row = decode_plan_field(Some(idx), &plan, Some(offset), bytes, verify);
        if !row.valid {
            let code = row.error_code.unwrap_or("E_DATA");
            let message = row
                .message
                .clone()
                .unwrap_or_else(|| "data error".to_string());
            record_audit(audit, &row);
            audit.records_invalid += 1;
            return match schema.on_error {
                OnError::Fail => Err(CliError::data(code, message)),
                OnError::SkipRecord => Ok(()),
                OnError::EmitErrorRow => Err(CliError::internal(
                    "emit-error-row reached atomic fixed-width processor",
                )),
            };
        }
        pending.push(row);
    }
    for row in pending {
        record_audit(audit, &row);
        sink.emit(row)?;
    }
    audit.records_valid += 1;
    Ok(())
}

fn decode_fixed_record_outcome(
    schema: &Schema,
    idx: usize,
    record: &[u8],
    verify: bool,
) -> Result<RecordOutcome, CliError> {
    match schema.on_error {
        OnError::EmitErrorRow => decode_fixed_record_emit_error(schema, idx, record, verify),
        OnError::Fail | OnError::SkipRecord => {
            decode_fixed_record_atomic(schema, idx, record, verify)
        }
    }
}

fn decode_fixed_record_emit_error(
    schema: &Schema,
    idx: usize,
    record: &[u8],
    verify: bool,
) -> Result<RecordOutcome, CliError> {
    let mut events = Vec::with_capacity(schema.fields.len());
    let mut record_valid = true;
    for spec in &schema.fields {
        let plan = plan_field(spec)?;
        let offset = plan.spec.offset.ok_or_else(|| {
            CliError::config(
                "E_SCHEMA",
                format!(
                    "field {} requires offset for fixed-width input",
                    plan.spec.name
                ),
            )
        })?;
        let length = plan.spec.length.unwrap_or(plan.expected_len);
        let row = if offset
            .checked_add(length)
            .map_or(true, |end| end > record.len())
        {
            DecodedField::error(
                Some(idx),
                &plan.spec.name,
                Some(offset),
                &[],
                "E_OFFSET",
                "field extends past record boundary",
            )
        } else {
            let bytes = &record[offset..offset + length];
            decode_plan_field(Some(idx), &plan, Some(offset), bytes, verify)
        };
        record_valid &= row.valid;
        events.push(RecordEvent { row, emit: true });
    }
    Ok(RecordOutcome {
        events,
        record_valid,
        failure: None,
    })
}

fn decode_fixed_record_atomic(
    schema: &Schema,
    idx: usize,
    record: &[u8],
    verify: bool,
) -> Result<RecordOutcome, CliError> {
    let mut events = Vec::with_capacity(schema.fields.len());
    for spec in &schema.fields {
        let plan = plan_field(spec)?;
        let offset = plan.spec.offset.ok_or_else(|| {
            CliError::config(
                "E_SCHEMA",
                format!(
                    "field {} requires offset for fixed-width input",
                    plan.spec.name
                ),
            )
        })?;
        let length = plan.spec.length.unwrap_or(plan.expected_len);
        let row = if offset
            .checked_add(length)
            .map_or(true, |end| end > record.len())
        {
            DecodedField::error(
                Some(idx),
                &plan.spec.name,
                Some(offset),
                &[],
                "E_OFFSET",
                "field extends past record boundary",
            )
        } else {
            let bytes = &record[offset..offset + length];
            decode_plan_field(Some(idx), &plan, Some(offset), bytes, verify)
        };
        if !row.valid {
            let failure = matches!(schema.on_error, OnError::Fail).then(|| failure_from_row(&row));
            return Ok(RecordOutcome {
                events: vec![RecordEvent { row, emit: false }],
                record_valid: false,
                failure,
            });
        }
        events.push(RecordEvent { row, emit: true });
    }
    Ok(RecordOutcome {
        events,
        record_valid: true,
        failure: None,
    })
}

fn apply_record_outcome(
    audit: &mut AuditReport,
    sink: &mut RowSink,
    outcome: RecordOutcome,
) -> Result<(), CliError> {
    audit.records_seen += 1;
    if outcome.record_valid {
        audit.records_valid += 1;
    } else {
        audit.records_invalid += 1;
    }
    for event in outcome.events {
        record_audit(audit, &event.row);
        if event.emit {
            sink.emit(event.row)?;
        }
    }
    if let Some((code, message)) = outcome.failure {
        return Err(CliError::data(code, message));
    }
    Ok(())
}

fn failure_from_row(row: &DecodedField) -> (&'static str, String) {
    (
        row.error_code.unwrap_or("E_DATA"),
        row.message
            .clone()
            .unwrap_or_else(|| "data error".to_string()),
    )
}

fn process_named_hex_fields(
    schema: &Schema,
    idx: usize,
    fields: &BTreeMap<String, String>,
    type_errors: &BTreeMap<String, String>,
    verify: bool,
    audit: &mut AuditReport,
    sink: &mut RowSink,
) -> Result<(), CliError> {
    if schema.on_error != OnError::EmitErrorRow {
        return process_named_hex_fields_atomic(
            schema,
            idx,
            fields,
            type_errors,
            verify,
            audit,
            sink,
        );
    }
    audit.records_seen += 1;
    let before_invalid = audit.fields_invalid;
    for spec in &schema.fields {
        let plan = plan_field(spec)?;
        if let Some(raw) = type_errors.get(&plan.spec.name) {
            let err = DecodedField::error(
                Some(idx),
                &plan.spec.name,
                None,
                raw.as_bytes(),
                "E_JSON_TYPE",
                "JSONL field must be a string containing hex bytes",
            );
            if !handle_row_error(schema, audit, sink, err)? {
                break;
            }
            continue;
        }
        let Some(raw) = fields.get(&plan.spec.name) else {
            if plan.spec.required {
                let err = DecodedField::error(
                    Some(idx),
                    &plan.spec.name,
                    None,
                    &[],
                    "E_REQUIRED",
                    "required field is missing",
                );
                if !handle_row_error(schema, audit, sink, err)? {
                    break;
                }
            }
            continue;
        };
        let bytes = match parse_hex_with_limit(raw, Some(plan.expected_len)) {
            Ok(bytes) => bytes,
            Err(err) => {
                let row = DecodedField::error(
                    Some(idx),
                    &plan.spec.name,
                    None,
                    &[],
                    err.code,
                    err.message,
                );
                if !handle_row_error(schema, audit, sink, row)? {
                    break;
                }
                continue;
            }
        };
        let row = decode_plan_field(Some(idx), &plan, None, &bytes, verify);
        if !handle_decoded_row(schema, audit, sink, row)? {
            break;
        }
    }
    if audit.fields_invalid == before_invalid {
        audit.records_valid += 1;
    } else {
        audit.records_invalid += 1;
    }
    Ok(())
}

fn process_named_hex_fields_atomic(
    schema: &Schema,
    idx: usize,
    fields: &BTreeMap<String, String>,
    type_errors: &BTreeMap<String, String>,
    verify: bool,
    audit: &mut AuditReport,
    sink: &mut RowSink,
) -> Result<(), CliError> {
    audit.records_seen += 1;
    let mut pending = Vec::new();
    for spec in &schema.fields {
        let plan = plan_field(spec)?;
        if let Some(raw) = type_errors.get(&plan.spec.name) {
            let err = DecodedField::error(
                Some(idx),
                &plan.spec.name,
                None,
                raw.as_bytes(),
                "E_JSON_TYPE",
                "JSONL field must be a string containing hex bytes",
            );
            let code = err.error_code.unwrap_or("E_DATA");
            let message = err
                .message
                .clone()
                .unwrap_or_else(|| "data error".to_string());
            record_audit(audit, &err);
            audit.records_invalid += 1;
            return match schema.on_error {
                OnError::Fail => Err(CliError::data(code, message)),
                OnError::SkipRecord => Ok(()),
                OnError::EmitErrorRow => Err(CliError::internal(
                    "emit-error-row reached atomic named-field processor",
                )),
            };
        }
        let Some(raw) = fields.get(&plan.spec.name) else {
            if plan.spec.required {
                let err = DecodedField::error(
                    Some(idx),
                    &plan.spec.name,
                    None,
                    &[],
                    "E_REQUIRED",
                    "required field is missing",
                );
                record_audit(audit, &err);
                audit.records_invalid += 1;
                return match schema.on_error {
                    OnError::Fail => Err(CliError::data(
                        err.error_code.unwrap_or("E_DATA"),
                        err.message.unwrap_or_else(|| "data error".to_string()),
                    )),
                    OnError::SkipRecord => Ok(()),
                    OnError::EmitErrorRow => Err(CliError::internal(
                        "emit-error-row reached atomic named-field processor",
                    )),
                };
            }
            continue;
        };
        let bytes = match parse_hex_with_limit(raw, Some(plan.expected_len)) {
            Ok(bytes) => bytes,
            Err(err) => {
                let row = DecodedField::error(
                    Some(idx),
                    &plan.spec.name,
                    None,
                    &[],
                    err.code,
                    err.message,
                );
                let code = row.error_code.unwrap_or("E_DATA");
                let message = row
                    .message
                    .clone()
                    .unwrap_or_else(|| "data error".to_string());
                record_audit(audit, &row);
                audit.records_invalid += 1;
                return match schema.on_error {
                    OnError::Fail => Err(CliError::data(code, message)),
                    OnError::SkipRecord => Ok(()),
                    OnError::EmitErrorRow => Err(CliError::internal(
                        "emit-error-row reached atomic named-field processor",
                    )),
                };
            }
        };
        let row = decode_plan_field(Some(idx), &plan, None, &bytes, verify);
        if !row.valid {
            let code = row.error_code.unwrap_or("E_DATA");
            let message = row
                .message
                .clone()
                .unwrap_or_else(|| "data error".to_string());
            record_audit(audit, &row);
            audit.records_invalid += 1;
            return match schema.on_error {
                OnError::Fail => Err(CliError::data(code, message)),
                OnError::SkipRecord => Ok(()),
                OnError::EmitErrorRow => Err(CliError::internal(
                    "emit-error-row reached atomic named-field processor",
                )),
            };
        }
        pending.push(row);
    }
    for row in pending {
        record_audit(audit, &row);
        sink.emit(row)?;
    }
    audit.records_valid += 1;
    Ok(())
}

fn handle_row_error(
    schema: &Schema,
    audit: &mut AuditReport,
    sink: &mut RowSink,
    err: DecodedField,
) -> Result<bool, CliError> {
    record_audit(audit, &err);
    match schema.on_error {
        OnError::Fail => Err(CliError::data(
            err.error_code.unwrap_or("E_DATA"),
            err.message.unwrap_or_else(|| "data error".to_string()),
        )),
        OnError::SkipRecord => Ok(false),
        OnError::EmitErrorRow => {
            sink.emit(err)?;
            Ok(true)
        }
    }
}

fn handle_record_error(
    schema: &Schema,
    audit: &mut AuditReport,
    sink: &mut RowSink,
    err: DecodedField,
) -> Result<bool, CliError> {
    audit.records_seen += 1;
    audit.records_invalid += 1;
    handle_row_error(schema, audit, sink, err)
}

fn handle_decoded_row(
    schema: &Schema,
    audit: &mut AuditReport,
    sink: &mut RowSink,
    row: DecodedField,
) -> Result<bool, CliError> {
    if row.valid {
        record_audit(audit, &row);
        sink.emit(row)?;
        return Ok(true);
    }
    match schema.on_error {
        OnError::Fail => {
            let code = row.error_code.unwrap_or("E_DATA");
            let message = row
                .message
                .clone()
                .unwrap_or_else(|| "data error".to_string());
            record_audit(audit, &row);
            Err(CliError::data(code, message))
        }
        OnError::SkipRecord => {
            record_audit(audit, &row);
            Ok(false)
        }
        OnError::EmitErrorRow => {
            record_audit(audit, &row);
            sink.emit(row)?;
            Ok(true)
        }
    }
}

fn limits_from_args(args: &BatchInputArgs) -> Result<ProcessingLimits, CliError> {
    if matches!(args.max_records, Some(0)) {
        return Err(CliError::config(
            "E_CONFIG",
            "--max-records must be greater than zero",
        ));
    }
    if args.sample_failures > MAX_FAILURE_SAMPLE_LIMIT {
        return Err(CliError::config(
            "E_CONFIG",
            format!("--sample-failures must be at most {MAX_FAILURE_SAMPLE_LIMIT}"),
        ));
    }
    let max_buffered_rows = match args.max_rows {
        Some(0) => None,
        Some(limit) => Some(limit),
        None => Some(MAX_BUFFERED_ROWS),
    };
    if matches!(args.parallel, Some(0)) {
        return Err(CliError::config(
            "E_CONFIG",
            "--parallel must be greater than zero",
        ));
    }
    Ok(ProcessingLimits {
        max_records: args.max_records,
        max_buffered_rows,
        parallelism: args.parallel.unwrap_or(1),
        dry_run: args.dry_run,
        quiet: args.quiet,
    })
}

fn verification_scope_from_args(
    schema: &Schema,
    args: &BatchInputArgs,
) -> Result<VerificationScope, CliError> {
    let scope = if args.strict_record {
        VerificationScope::Record
    } else {
        schema.verification_scope
    };
    if scope == VerificationScope::Record
        && !matches!(
            schema.input_encoding,
            InputEncoding::Binary | InputEncoding::Hex
        )
    {
        return Err(CliError::config(
            "E_SCHEMA",
            "record verification scope requires binary or hex fixed-width input",
        ));
    }
    Ok(scope)
}

fn record_limit_reached(idx: usize, limits: ProcessingLimits) -> bool {
    limits.max_records.is_some_and(|max| idx >= max)
}

fn display_record_index(idx: usize, one_based: bool) -> usize {
    if one_based {
        idx.saturating_add(1)
    } else {
        idx
    }
}

pub(super) fn is_stdin_path(path: &Path) -> bool {
    path == Path::new("-")
}

pub(super) fn open_input(path: &Path) -> Result<Box<dyn Read>, CliError> {
    if is_stdin_path(path) {
        Ok(Box::new(io::stdin()))
    } else {
        Ok(Box::new(fs::File::open(path)?))
    }
}

fn read_bounded_line<R: BufRead>(
    reader: &mut R,
    out: &mut String,
    max_bytes: usize,
) -> Result<usize, CliError> {
    out.clear();
    let mut total = 0usize;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(total);
        }
        let take = match available.iter().position(|b| *b == b'\n') {
            Some(pos) => pos + 1,
            None => available.len(),
        };
        total = total
            .checked_add(take)
            .ok_or_else(|| CliError::data("E_LENGTH", "line length overflow"))?;
        if total > max_bytes {
            return Err(CliError::data(
                "E_LENGTH",
                format!("input line exceeds {max_bytes} bytes"),
            ));
        }
        let chunk = std::str::from_utf8(&available[..take]).map_err(|err| {
            CliError::data("E_ENCODING", format!("input line is not UTF-8: {err}"))
        })?;
        let ended = chunk.ends_with('\n');
        out.push_str(chunk);
        reader.consume(take);
        if ended {
            return Ok(total);
        }
    }
}

pub(super) fn parse_hex_with_limit(
    input: &str,
    max_bytes: Option<usize>,
) -> Result<Vec<u8>, CliError> {
    if let Some(max) = max_bytes {
        let text_limit = max.saturating_mul(8).saturating_add(128);
        if input.len() > text_limit {
            return Err(CliError::data(
                "E_LENGTH",
                format!("hex text exceeds {text_limit} bytes for a {max}-byte field"),
            ));
        }
    }
    let mut digits = String::new();
    let mut chars = input.trim().chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '0' && matches!(chars.peek(), Some('x' | 'X')) {
            let _ = chars.next();
            continue;
        }
        if ch.is_ascii_hexdigit() {
            digits.push(ch);
            if let Some(max) = max_bytes {
                if digits.len() / 2 > max {
                    return Err(CliError::data(
                        "E_LENGTH",
                        format!("hex input exceeds {max} bytes"),
                    ));
                }
            }
        } else if ch.is_ascii_whitespace() || matches!(ch, ',' | '_' | '-' | ':') {
            continue;
        } else {
            return Err(CliError::data(
                "E_HEX",
                format!("invalid hex character {ch}"),
            ));
        }
    }
    if digits.len() % 2 != 0 {
        return Err(CliError::data(
            "E_HEX",
            "hex input must contain an even number of digits",
        ));
    }
    let mut out = Vec::with_capacity(digits.len() / 2);
    let bytes = digits.as_bytes();
    for i in (0..bytes.len()).step_by(2) {
        let hi = hex_value(bytes[i])?;
        let lo = hex_value(bytes[i + 1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn parse_csv_line(line: &str) -> Result<csv::StringRecord, CliError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .from_reader(line.as_bytes());
    let mut records = reader.records();
    let Some(record) = records.next() else {
        return Ok(csv::StringRecord::new());
    };
    let record = record.map_err(|err| CliError::data("E_CSV", err.to_string()))?;
    if records
        .next()
        .transpose()
        .map_err(|err| CliError::data("E_CSV", err.to_string()))?
        .is_some()
    {
        return Err(CliError::data(
            "E_CSV",
            "CSV records must fit on one physical line",
        ));
    }
    Ok(record)
}

pub(super) fn hex_value(ch: u8) -> Result<u8, CliError> {
    match ch {
        b'0'..=b'9' => Ok(ch - b'0'),
        b'a'..=b'f' => Ok(ch - b'a' + 10),
        b'A'..=b'F' => Ok(ch - b'A' + 10),
        _ => Err(CliError::data("E_HEX", "invalid hex digit")),
    }
}
