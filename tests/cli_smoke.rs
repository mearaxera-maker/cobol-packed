#![cfg(feature = "cli")]

use assert_cmd::Command;
use predicates::str::contains;
use serde_json::Value;
use std::fs;

#[test]
fn decode_single_field_hex() {
    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "decode", "--digits", "4", "--scale", "2", "--signed", "--hex", "01234C",
    ]);
    cmd.assert().success().stdout(contains("12.34"));
}

#[test]
fn inspect_reports_bad_sign() {
    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "inspect", "--digits", "4", "--scale", "2", "--signed", "--hex", "012341", "--output",
        "json",
    ]);
    cmd.assert().success().stdout(contains("E_SIGN"));
}

#[test]
fn decode_bad_sign_exits_with_data_error() {
    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "decode", "--digits", "4", "--scale", "2", "--signed", "--hex", "012341", "--output",
        "json",
    ]);
    cmd.assert().code(1).stderr(contains("E_SIGN"));
}

#[test]
fn decode_requires_exactly_one_input_source() {
    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "decode", "--digits", "4", "--scale", "2", "--signed", "--hex", "01234C", "--stdin",
    ]);
    cmd.assert().code(2).stderr(contains("cannot be used with"));
}

#[test]
fn schema_check_accepts_fixed_width_schema() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    fs::write(
        &schema,
        r#"{
          "version": 1,
          "record_length": 3,
          "input_encoding": "binary",
          "on_error": "emit-error-row",
          "fields": [{
            "name": "amount",
            "description": "operator-facing description must not affect semantic hash",
            "offset": 0,
            "length": 3,
            "total_digits": 4,
            "scale": 2,
            "signed": true,
            "sign_mode": "pfd",
            "mode": "lossless"
          }]
        }"#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "schema",
        "check",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        "json",
    ]);
    cmd.assert().success().stdout(contains("\"valid\": true"));
}

#[test]
fn batch_decode_binary_records_as_jsonl() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
        r#"{
          "version": 1,
          "record_length": 3,
          "input_encoding": "binary",
          "on_error": "emit-error-row",
          "fields": [{
            "name": "amount",
            "offset": 0,
            "length": 3,
            "total_digits": 4,
            "scale": 2,
            "signed": true,
            "sign_mode": "pfd",
            "mode": "lossless"
          }]
        }"#,
    )
    .unwrap();
    fs::write(&data, [0x01, 0x23, 0x4C]).unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "batch",
        "decode",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "jsonl",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"field\":\"amount\""))
        .stdout(contains("\"value\":\"12.34\""));
}

#[test]
fn batch_verify_reports_negative_zero_in_audit() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
        r#"{
          "version": 1,
          "record_length": 2,
          "input_encoding": "binary",
          "on_error": "emit-error-row",
          "fields": [{
            "name": "zero",
            "offset": 0,
            "length": 2,
            "total_digits": 3,
            "scale": 0,
            "signed": true,
            "sign_mode": "nopfd",
            "mode": "lossless"
          }]
        }"#,
    )
    .unwrap();
    fs::write(&data, [0x00, 0x0D]).unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "batch",
        "verify",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "audit",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"negative_zero_count\": 1"));
}

#[test]
fn batch_verify_exits_data_error_when_emit_error_row_finds_failure() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
        r#"{
          "version": 1,
          "record_length": 3,
          "input_encoding": "binary",
          "on_error": "emit-error-row",
          "fields": [{
            "name": "amount",
            "offset": 0,
            "length": 3,
            "total_digits": 4,
            "scale": 2,
            "signed": true,
            "sign_mode": "pfd",
            "mode": "lossless"
          }]
        }"#,
    )
    .unwrap();
    fs::write(&data, [0x01, 0x23, 0x41]).unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "batch",
        "verify",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "audit",
    ]);
    cmd.assert()
        .code(1)
        .stdout(contains("\"status\": \"failed\""))
        .stderr(contains("E_VERIFY"));
}

#[test]
fn batch_decode_emit_error_row_for_bad_hex_record() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.hex");
    fs::write(
        &schema,
        r#"{
          "version": 1,
          "record_length": 3,
          "input_encoding": "hex",
          "on_error": "emit-error-row",
          "fields": [{
            "name": "amount",
            "offset": 0,
            "length": 3,
            "total_digits": 4,
            "scale": 2,
            "signed": true,
            "sign_mode": "pfd",
            "mode": "lossless"
          }]
        }"#,
    )
    .unwrap();
    fs::write(&data, "not-hex\n").unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "batch",
        "decode",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "jsonl",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"field\":\"<record>\""))
        .stdout(contains("E_HEX"));
}

#[test]
fn skip_record_does_not_emit_partial_rows() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
        r#"{
          "version": 1,
          "record_length": 6,
          "input_encoding": "binary",
          "on_error": "skip-record",
          "fields": [
            {
              "name": "ok_amount",
              "offset": 0,
              "length": 3,
              "total_digits": 4,
              "scale": 2,
              "signed": true,
              "sign_mode": "pfd",
              "mode": "lossless"
            },
            {
              "name": "bad_amount",
              "offset": 3,
              "length": 3,
              "total_digits": 4,
              "scale": 2,
              "signed": true,
              "sign_mode": "pfd",
              "mode": "lossless"
            }
          ]
        }"#,
    )
    .unwrap();
    fs::write(&data, [0x01, 0x23, 0x4C, 0x01, 0x23, 0x41]).unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "batch",
        "decode",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "jsonl",
    ]);
    cmd.assert().success().stdout("");
}

#[test]
fn hex_records_must_match_schema_record_length() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.hex");
    fs::write(
        &schema,
        r#"{
          "version": 1,
          "record_length": 4,
          "input_encoding": "hex",
          "on_error": "emit-error-row",
          "fields": [{
            "name": "amount",
            "offset": 0,
            "length": 3,
            "total_digits": 4,
            "scale": 2,
            "signed": true,
            "sign_mode": "pfd",
            "mode": "lossless"
          }]
        }"#,
    )
    .unwrap();
    fs::write(&data, "01234C\n").unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "batch",
        "decode",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "jsonl",
    ]);
    cmd.assert().success().stdout(contains("E_RECORD_LENGTH"));
}

#[test]
fn name_based_schema_rejects_offsets() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    fs::write(
        &schema,
        r#"{
          "version": 1,
          "input_encoding": "csv",
          "fields": [{
            "name": "amount",
            "offset": 0,
            "length": 3,
            "total_digits": 4,
            "scale": 2,
            "signed": true,
            "sign_mode": "pfd",
            "mode": "lossless"
          }]
        }"#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "schema",
        "check",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        "json",
    ]);
    cmd.assert().code(2).stderr(contains("name-based"));
}

#[test]
fn audit_output_reports_status_and_tool_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
        r#"{
          "version": 1,
          "record_length": 3,
          "input_encoding": "binary",
          "on_error": "emit-error-row",
          "fields": [{
            "name": "amount",
            "offset": 0,
            "length": 3,
            "total_digits": 4,
            "scale": 2,
            "signed": true,
            "sign_mode": "pfd",
            "mode": "lossless"
          }]
        }"#,
    )
    .unwrap();
    fs::write(&data, [0x01, 0x23, 0x4C]).unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "batch",
        "decode",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "audit",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"tool\": \"cobol-packed\""))
        .stdout(contains("\"status\": \"passed\""))
        .stdout(contains("\"field_profiles\""))
        .stdout(contains("\"min_value\": \"12.34\""));
}

#[test]
fn max_records_limits_streaming_decode() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
        r#"{
          "version": 1,
          "record_length": 3,
          "input_encoding": "binary",
          "on_error": "emit-error-row",
          "fields": [{
            "name": "amount",
            "offset": 0,
            "length": 3,
            "total_digits": 4,
            "scale": 2,
            "signed": true,
            "sign_mode": "pfd",
            "mode": "lossless"
          }]
        }"#,
    )
    .unwrap();
    fs::write(&data, [0x01, 0x23, 0x4C, 0x02, 0x34, 0x5C]).unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "batch",
        "decode",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "audit",
        "--max-records",
        "1",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"record_limit\": 1"))
        .stdout(contains("\"records_seen\": 1"));
}

#[test]
fn strict_record_verify_passes_with_filler_coverage() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
        r#"{
          "version": 1,
          "record_length": 4,
          "input_encoding": "binary",
          "on_error": "emit-error-row",
          "fields": [{
            "name": "amount",
            "offset": 0,
            "length": 3,
            "total_digits": 4,
            "scale": 2,
            "signed": true,
            "sign_mode": "pfd",
            "mode": "lossless"
          }],
          "fillers": [{
            "name": "tail",
            "offset": 3,
            "length": 1
          }]
        }"#,
    )
    .unwrap();
    fs::write(&data, [0x01, 0x23, 0x4C, 0xAA]).unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "batch",
        "verify",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "audit",
        "--strict-record",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"record_byte_for_byte_verified\": true"))
        .stdout(contains("\"full_coverage\": true"));
}

#[test]
fn strict_record_verify_fails_when_layout_has_gap() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
        r#"{
          "version": 1,
          "record_length": 4,
          "input_encoding": "binary",
          "on_error": "emit-error-row",
          "fields": [{
            "name": "amount",
            "offset": 0,
            "length": 3,
            "total_digits": 4,
            "scale": 2,
            "signed": true,
            "sign_mode": "pfd",
            "mode": "lossless"
          }]
        }"#,
    )
    .unwrap();
    fs::write(&data, [0x01, 0x23, 0x4C, 0xAA]).unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "batch",
        "verify",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "audit",
        "--strict-record",
    ]);
    cmd.assert()
        .code(1)
        .stdout(contains("\"record_byte_for_byte_verified\": false"))
        .stdout(contains("\"full_coverage\": false"))
        .stderr(contains("E_VERIFY"));
}

#[test]
fn jsonl_wrong_field_type_is_explicit_error() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.jsonl");
    fs::write(
        &schema,
        r#"{
          "version": 1,
          "input_encoding": "jsonl",
          "on_error": "emit-error-row",
          "fields": [{
            "name": "amount",
            "total_digits": 4,
            "scale": 2,
            "signed": true,
            "sign_mode": "pfd",
            "mode": "lossless"
          }]
        }"#,
    )
    .unwrap();
    fs::write(&data, "{\"amount\":123}\n").unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "batch",
        "decode",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "jsonl",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"error_code\":\"E_JSON_TYPE\""))
        .stdout(contains("\"raw_hex\":\"313233\""));
}

#[test]
fn malformed_hex_record_keeps_raw_preview() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.hex");
    fs::write(
        &schema,
        r#"{
          "version": 1,
          "record_length": 3,
          "input_encoding": "hex",
          "on_error": "emit-error-row",
          "fields": [{
            "name": "amount",
            "offset": 0,
            "length": 3,
            "total_digits": 4,
            "scale": 2,
            "signed": true,
            "sign_mode": "pfd",
            "mode": "lossless"
          }]
        }"#,
    )
    .unwrap();
    fs::write(&data, "not-hex\n").unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "batch",
        "decode",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "jsonl",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"error_code\":\"E_HEX\""))
        .stdout(contains("\"raw_hex\":\"6E6F742D6865780A\""));
}

#[test]
fn semantic_schema_hash_is_stable_across_json_formatting() {
    let dir = tempfile::tempdir().unwrap();
    let schema_a = dir.path().join("a.json");
    let schema_b = dir.path().join("b.json");
    let schema_c = dir.path().join("c.json");
    fs::write(
        &schema_a,
        r#"{
          "version": 1,
          "record_length": 3,
          "input_encoding": "binary",
          "fields": [{
            "name": "amount",
            "offset": 0,
            "length": 3,
            "total_digits": 4,
            "scale": 2,
            "signed": true,
            "sign_mode": "pfd",
            "mode": "lossless"
          }]
        }"#,
    )
    .unwrap();
    fs::write(
        &schema_b,
        r#"{"fields":[{"mode":"lossless","sign_mode":"pfd","signed":true,"scale":2,"total_digits":4,"length":3,"offset":0,"name":"amount"}],"input_encoding":"binary","record_length":3,"version":1}"#,
    )
    .unwrap();
    fs::write(
        &schema_c,
        r#"{"fields":[{"mode":"lossless","sign_mode":"pfd","signed":true,"scale":1,"total_digits":4,"length":3,"offset":0,"name":"amount"}],"input_encoding":"binary","record_length":3,"version":1}"#,
    )
    .unwrap();

    let mut first = Command::cargo_bin("cobol-packed").unwrap();
    first.args([
        "schema",
        "check",
        "--schema",
        schema_a.to_str().unwrap(),
        "--output",
        "json",
    ]);
    let first_output = first.assert().success().get_output().stdout.clone();
    let first_json: Value = serde_json::from_slice(&first_output).unwrap();

    let mut second = Command::cargo_bin("cobol-packed").unwrap();
    second.args([
        "schema",
        "check",
        "--schema",
        schema_b.to_str().unwrap(),
        "--output",
        "json",
    ]);
    let second_output = second.assert().success().get_output().stdout.clone();
    let second_json: Value = serde_json::from_slice(&second_output).unwrap();

    let mut third = Command::cargo_bin("cobol-packed").unwrap();
    third.args([
        "schema",
        "check",
        "--schema",
        schema_c.to_str().unwrap(),
        "--output",
        "json",
    ]);
    let third_output = third.assert().success().get_output().stdout.clone();
    let third_json: Value = serde_json::from_slice(&third_output).unwrap();

    assert_eq!(first_json["schema_hash"], second_json["schema_hash"]);
    assert_ne!(first_json["schema_hash"], third_json["schema_hash"]);
    assert_ne!(
        first_json["schema_file_sha256"],
        second_json["schema_file_sha256"]
    );
}

#[test]
fn full_evidence_mode_includes_runtime_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
        r#"{
          "version": 1,
          "record_length": 3,
          "input_encoding": "binary",
          "on_error": "emit-error-row",
          "fields": [{
            "name": "amount",
            "offset": 0,
            "length": 3,
            "total_digits": 4,
            "scale": 2,
            "signed": true,
            "sign_mode": "pfd",
            "mode": "lossless"
          }]
        }"#,
    )
    .unwrap();
    fs::write(&data, [0x01, 0x23, 0x4C]).unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "batch",
        "decode",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "audit",
        "--evidence-mode",
        "full",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"evidence_mode\": \"full\""))
        .stdout(contains("\"runtime\""))
        .stdout(contains("\"argv\""))
        .stdout(contains("\"argv_redacted\": true"))
        .stdout(contains("<redacted>"));
}

#[test]
fn full_evidence_mode_can_emit_raw_argv_when_explicitly_requested() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
        r#"{
          "version": 1,
          "record_length": 3,
          "input_encoding": "binary",
          "on_error": "emit-error-row",
          "fields": [{
            "name": "amount",
            "offset": 0,
            "length": 3,
            "total_digits": 4,
            "scale": 2,
            "signed": true,
            "sign_mode": "pfd",
            "mode": "lossless"
          }]
        }"#,
    )
    .unwrap();
    fs::write(&data, [0x01, 0x23, 0x4C]).unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "batch",
        "decode",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "audit",
        "--evidence-mode",
        "full",
        "--evidence-argv",
        "raw",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"argv_redacted\": false"))
        .stdout(contains("schema.json"));
}

#[test]
fn malformed_json_schema_reports_expected_format_and_path() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    fs::write(&schema, "{not json").unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "schema",
        "check",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        "json",
    ]);
    cmd.assert()
        .failure()
        .stderr(contains("expected JSON schema at"))
        .stderr(contains("schema.json"));
}

#[test]
fn schema_v2_decodes_mixed_record_codecs() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "layout_mode": "declared",
          "record_length": 14,
          "input_encoding": "binary",
          "on_error": "emit-error-row",
          "layout": [
            {
              "kind": "field",
              "name": "packed_amount",
              "offset": 0,
              "field_type": "packed-decimal",
              "total_digits": 4,
              "scale": 2,
              "signed": true,
              "sign_mode": "pfd",
              "mode": "lossless"
            },
            {
              "kind": "field",
              "name": "zoned_amount",
              "offset": 3,
              "field_type": "zoned-decimal",
              "total_digits": 4,
              "scale": 2,
              "signed": true,
              "encoding": "ebcdic",
              "codepage": "cp037",
              "sign_policy": "preferred"
            },
            {
              "kind": "field",
              "name": "binary_count",
              "offset": 7,
              "field_type": "binary",
              "total_digits": 5,
              "signed": false,
              "endian": "big"
            },
            {
              "kind": "field",
              "name": "name",
              "offset": 11,
              "length": 3,
              "field_type": "alphanumeric",
              "encoding": "ebcdic",
              "codepage": "cp037"
            }
          ]
        }"#,
    )
    .unwrap();
    fs::write(
        &data,
        [
            0x01, 0x23, 0x4C, 0xF1, 0xF2, 0xF3, 0xD4, 0x00, 0x01, 0x86, 0xA0, 0xC1, 0xD1, 0xF0,
        ],
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "batch",
        "decode",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "jsonl",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"field\":\"packed_amount\""))
        .stdout(contains("\"value\":\"12.34\""))
        .stdout(contains("\"field\":\"zoned_amount\""))
        .stdout(contains("\"value\":\"-12.34\""))
        .stdout(contains("\"field\":\"binary_count\""))
        .stdout(contains("\"value\":\"100000\""))
        .stdout(contains("\"field\":\"name\""))
        .stdout(contains("\"value\":\"AJ0\""));
}

#[test]
fn schema_v2_scaled_binary_verifies_without_false_failure() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "layout_mode": "declared",
          "record_length": 2,
          "input_encoding": "binary",
          "on_error": "emit-error-row",
          "layout": [
            {
              "kind": "field",
              "name": "scaled_count",
              "offset": 0,
              "field_type": "binary",
              "total_digits": 4,
              "scale": 2,
              "signed": false,
              "endian": "big"
            }
          ]
        }"#,
    )
    .unwrap();
    fs::write(&data, [0x04, 0xD2]).unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "batch",
        "verify",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "audit",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"byte_for_byte_verified\": true"));
}

#[test]
fn schema_v2_zoned_decimal_verifies_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "layout_mode": "declared",
          "record_length": 4,
          "input_encoding": "binary",
          "on_error": "emit-error-row",
          "layout": [
            {
              "kind": "field",
              "name": "zoned_amount",
              "offset": 0,
              "field_type": "zoned-decimal",
              "total_digits": 4,
              "scale": 2,
              "signed": true,
              "encoding": "ebcdic",
              "codepage": "cp037",
              "sign_policy": "preferred"
            }
          ]
        }"#,
    )
    .unwrap();
    fs::write(&data, [0xF1, 0xF2, 0xF3, 0xD4]).unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "batch",
        "verify",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "audit",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"byte_for_byte_verified\": true"));
}

#[test]
fn schema_v2_ibm_float_verifies_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "layout_mode": "declared",
          "record_length": 4,
          "input_encoding": "binary",
          "on_error": "emit-error-row",
          "layout": [
            {
              "kind": "field",
              "name": "float_amount",
              "offset": 0,
              "field_type": "ibm-float32",
              "endian": "big"
            }
          ]
        }"#,
    )
    .unwrap();
    fs::write(&data, [0x42, 0x64, 0x00, 0x00]).unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "batch",
        "verify",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "audit",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"byte_for_byte_verified\": true"));
}

#[test]
fn schema_v2_rejects_zoned_digits_outside_decimal_bound() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "layout_mode": "declared",
          "input_encoding": "binary",
          "layout": [
            {
              "kind": "field",
              "name": "bad_zoned",
              "offset": 0,
              "field_type": "zoned-decimal",
              "total_digits": 19,
              "scale": 0,
              "signed": true,
              "encoding": "ebcdic",
              "codepage": "cp037"
            }
          ]
        }"#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "schema",
        "check",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        "json",
    ]);
    cmd.assert()
        .code(2)
        .stderr(contains("total_digits must be in 1..=18"));
}

#[test]
fn schema_v2_rejects_binary_scale_larger_than_digits() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "layout_mode": "declared",
          "input_encoding": "binary",
          "layout": [
            {
              "kind": "field",
              "name": "bad_binary",
              "offset": 0,
              "field_type": "binary",
              "total_digits": 4,
              "scale": 5,
              "signed": false,
              "endian": "big"
            }
          ]
        }"#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "schema",
        "check",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        "json",
    ]);
    cmd.assert()
        .code(2)
        .stderr(contains("scale exceeds total_digits"));
}

#[test]
fn schema_v2_rejects_ignored_codec_options() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "layout_mode": "declared",
          "input_encoding": "binary",
          "layout": [
            {
              "kind": "field",
              "name": "bad_binary",
              "offset": 0,
              "field_type": "binary",
              "total_digits": 4,
              "signed": false,
              "endian": "big",
              "sign_mode": "pfd"
            }
          ]
        }"#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "schema",
        "check",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        "json",
    ]);
    cmd.assert()
        .code(2)
        .stderr(contains("must not set sign_mode"));
}

#[test]
fn schema_v2_rejects_zero_length_text_field() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "layout_mode": "declared",
          "input_encoding": "binary",
          "layout": [
            {
              "kind": "field",
              "name": "empty_text",
              "offset": 0,
              "length": 0,
              "field_type": "alphanumeric",
              "encoding": "ascii"
            }
          ]
        }"#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "schema",
        "check",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        "json",
    ]);
    cmd.assert()
        .code(2)
        .stderr(contains("length must be greater than zero"));
}

#[test]
fn schema_v2_ascii_text_rejects_non_ascii_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "layout_mode": "declared",
          "record_length": 2,
          "input_encoding": "binary",
          "on_error": "emit-error-row",
          "layout": [
            {
              "kind": "field",
              "name": "text",
              "offset": 0,
              "length": 2,
              "field_type": "alphanumeric",
              "encoding": "ascii"
            }
          ]
        }"#,
    )
    .unwrap();
    fs::write(&data, [0xC3, 0xA9]).unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "batch",
        "decode",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "jsonl",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"error_code\":\"E_ENCODING\""))
        .stdout(contains("non-ASCII"));
}

#[test]
fn schema_v2_sequential_sync_inserts_slack() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "layout_mode": "sequential",
          "record_length": 8,
          "input_encoding": "binary",
          "verification_scope": "record",
          "layout": [
            {
              "kind": "field",
              "name": "halfword",
              "field_type": "binary",
              "total_digits": 4,
              "signed": false,
              "endian": "big"
            },
            {
              "kind": "field",
              "name": "fullword",
              "field_type": "binary",
              "total_digits": 5,
              "signed": false,
              "endian": "big",
              "sync": true
            }
          ]
        }"#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "schema",
        "check",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        "json",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"kind\": \"sync-slack\""))
        .stdout(contains("\"offset\": 2"))
        .stdout(contains("\"full_coverage\": true"));
}

#[test]
fn schema_v2_occurs_count_out_of_range_is_data_error() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "layout_mode": "declared",
          "input_encoding": "binary",
          "on_error": "emit-error-row",
          "layout": [
            {
              "kind": "field",
              "name": "count",
              "offset": 0,
              "field_type": "packed-decimal",
              "total_digits": 1,
              "scale": 0,
              "signed": false,
              "sign_mode": "pfd",
              "mode": "lossless"
            },
            {
              "kind": "occurs",
              "name": "items",
              "offset": 1,
              "counter_field": "count",
              "min_occurs": 0,
              "max_occurs": 2,
              "element_layout": [
                {
                  "kind": "field",
                  "name": "code",
                  "offset": 0,
                  "field_type": "binary",
                  "total_digits": 4,
                  "signed": false,
                  "endian": "big"
                }
              ]
            }
          ]
        }"#,
    )
    .unwrap();
    fs::write(&data, [0x3F]).unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "batch",
        "decode",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "jsonl",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"error_code\":\"E_OCCURS_COUNT\""));
}

#[test]
fn schema_v2_rejects_non_terminal_occurs_until_dynamic_offsets_exist() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "layout_mode": "sequential",
          "input_encoding": "binary",
          "layout": [
            {
              "kind": "field",
              "name": "count",
              "field_type": "packed-decimal",
              "total_digits": 1,
              "signed": false,
              "sign_mode": "pfd",
              "mode": "lossless"
            },
            {
              "kind": "occurs",
              "name": "items",
              "counter_field": "count",
              "min_occurs": 0,
              "max_occurs": 2,
              "element_layout": [
                {
                  "kind": "field",
                  "name": "code",
                  "field_type": "binary",
                  "total_digits": 4,
                  "signed": false,
                  "endian": "big"
                }
              ]
            },
            {
              "kind": "field",
              "name": "tail",
              "length": 1,
              "field_type": "alphanumeric",
              "encoding": "ascii"
            }
          ]
        }"#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "schema",
        "check",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        "json",
    ]);
    cmd.assert().code(2).stderr(contains("must be terminal"));
}

#[test]
fn schema_v2_declared_non_terminal_occurs_with_fixed_suffix_decodes() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "layout_mode": "declared",
          "input_encoding": "binary",
          "on_error": "emit-error-row",
          "layout": [
            {
              "kind": "field",
              "name": "count",
              "offset": 0,
              "field_type": "packed-decimal",
              "total_digits": 1,
              "signed": false,
              "sign_mode": "pfd",
              "mode": "lossless"
            },
            {
              "kind": "occurs",
              "name": "items",
              "offset": 1,
              "counter_field": "count",
              "min_occurs": 0,
              "max_occurs": 2,
              "element_layout": [
                {
                  "kind": "field",
                  "name": "code",
                  "offset": 0,
                  "field_type": "binary",
                  "total_digits": 4,
                  "signed": false,
                  "endian": "big"
                }
              ]
            },
            {
              "kind": "field",
              "name": "tail",
              "offset": 5,
              "length": 1,
              "field_type": "alphanumeric",
              "encoding": "ascii"
            }
          ]
        }"#,
    )
    .unwrap();
    fs::write(&data, [0x1F, 0x00, 0x2A, 0xAA, 0xBB, b'Z']).unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "batch",
        "decode",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "jsonl",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"field\":\"items[0].code\""))
        .stdout(contains("\"value\":\"42\""))
        .stdout(contains("\"field\":\"tail\""))
        .stdout(contains("\"value\":\"Z\""));
}

#[test]
fn schema_v2_redefines_decodes_all_safe_variants() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "layout_mode": "declared",
          "record_length": 3,
          "input_encoding": "binary",
          "on_error": "emit-error-row",
          "layout": [
            {
              "kind": "redefines",
              "name": "view",
              "offset": 0,
              "variants": [
                {
                  "name": "as_packed",
                  "layout": [
                    {
                      "kind": "field",
                      "name": "amount",
                      "offset": 0,
                      "field_type": "packed-decimal",
                      "total_digits": 4,
                      "scale": 0,
                      "signed": false,
                      "sign_mode": "pfd",
                      "mode": "lossless"
                    }
                  ]
                },
                {
                  "name": "as_text",
                  "layout": [
                    {
                      "kind": "field",
                      "name": "raw",
                      "offset": 0,
                      "length": 3,
                      "field_type": "alphanumeric",
                      "encoding": "ascii"
                    }
                  ]
                }
              ]
            }
          ]
        }"#,
    )
    .unwrap();
    fs::write(&data, b"ABC").unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "batch",
        "decode",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "jsonl",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"field\":\"view.as_packed.amount\""))
        .stdout(contains("\"error_code\":\"E_PADDING\""))
        .stdout(contains("\"field\":\"view.as_text.raw\""))
        .stdout(contains("\"value\":\"ABC\""));
}

#[test]
fn schema_v2_redefines_selector_marks_active_variant() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "layout_mode": "declared",
          "record_length": 4,
          "input_encoding": "binary",
          "on_error": "emit-error-row",
          "layout": [
            {
              "kind": "field",
              "name": "record_type",
              "offset": 0,
              "length": 1,
              "field_type": "alphanumeric",
              "encoding": "ascii"
            },
            {
              "kind": "redefines",
              "name": "view",
              "offset": 1,
              "variants": [
                {
                  "name": "as_a",
                  "selector": { "field": "record_type", "equals": "A" },
                  "layout": [
                    {
                      "kind": "field",
                      "name": "raw",
                      "offset": 0,
                      "length": 3,
                      "field_type": "alphanumeric",
                      "encoding": "ascii"
                    }
                  ]
                },
                {
                  "name": "as_b",
                  "selector": { "field": "record_type", "equals": "B" },
                  "layout": [
                    {
                      "kind": "field",
                      "name": "raw",
                      "offset": 0,
                      "length": 3,
                      "field_type": "alphanumeric",
                      "encoding": "ascii"
                    }
                  ]
                }
              ]
            }
          ]
        }"#,
    )
    .unwrap();
    fs::write(&data, b"AABC").unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "batch",
        "decode",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "jsonl",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"field\":\"view.as_a.raw:selector-active\""))
        .stdout(contains("\"field\":\"view.as_b.raw:selector-inactive\""));
}

#[test]
fn schema_v2_redefines_selector_requires_preceding_field() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "layout_mode": "declared",
          "record_length": 3,
          "input_encoding": "binary",
          "layout": [
            {
              "kind": "redefines",
              "name": "view",
              "offset": 0,
              "variants": [
                {
                  "name": "as_a",
                  "selector": { "field": "missing_type", "equals": "A" },
                  "layout": [
                    {
                      "kind": "field",
                      "name": "raw",
                      "offset": 0,
                      "length": 3,
                      "field_type": "alphanumeric",
                      "encoding": "ascii"
                    }
                  ]
                }
              ]
            }
          ]
        }"#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "schema",
        "check",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        "json",
    ]);
    cmd.assert().code(2).stderr(contains(
        "selector field missing_type must refer to a preceding field",
    ));
}

#[test]
fn schema_emit_rust_generates_safe_accessors() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let output = dir.path().join("record.rs");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "layout_mode": "declared",
          "record_length": 3,
          "input_encoding": "binary",
          "layout": [
            {
              "kind": "field",
              "name": "amount",
              "offset": 0,
              "field_type": "packed-decimal",
              "total_digits": 4,
              "scale": 2,
              "signed": true,
              "sign_mode": "pfd",
              "mode": "lossless"
            }
          ]
        }"#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "schema",
        "emit-rust",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
    ]);
    cmd.assert().success();
    let generated = fs::read_to_string(output).unwrap();
    assert!(!generated.contains("unsafe"));
    assert!(generated.contains("pub fn amount_raw"));
    assert!(generated.contains("pub fn amount_hex"));
    assert!(generated.contains("pub fn amount(&self) -> Result<rust_decimal::Decimal, String>"));
    assert!(generated.contains("decode_packed_decimal_generated"));
}

#[test]
fn schema_emit_rust_generates_redefines_variant_enum() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let output = dir.path().join("record.rs");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "layout_mode": "declared",
          "record_length": 3,
          "input_encoding": "binary",
          "layout": [
            {
              "kind": "redefines",
              "name": "view",
              "offset": 0,
              "variants": [
                {
                  "name": "as_packed",
                  "layout": [
                    {
                      "kind": "field",
                      "name": "amount",
                      "offset": 0,
                      "field_type": "packed-decimal",
                      "total_digits": 4,
                      "scale": 2,
                      "signed": true,
                      "sign_mode": "pfd",
                      "mode": "lossless"
                    }
                  ]
                },
                {
                  "name": "as_text",
                  "layout": [
                    {
                      "kind": "field",
                      "name": "raw",
                      "offset": 0,
                      "length": 3,
                      "field_type": "alphanumeric",
                      "encoding": "ascii"
                    }
                  ]
                }
              ]
            }
          ]
        }"#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "schema",
        "emit-rust",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
    ]);
    cmd.assert().success();

    let generated = fs::read_to_string(output).unwrap();
    assert!(!generated.contains("unsafe"));
    assert!(generated.contains("pub enum ViewVariant"));
    assert!(generated.contains("pub fn select(&self, variant: &str)"));
    assert!(generated.contains("pub fn amount(&self) -> Result<rust_decimal::Decimal, String>"));
    assert!(generated.contains("pub fn raw(&self) -> Result<String, String>"));
}

#[test]
fn schema_emit_rust_generates_selector_logic_for_redefines() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let output = dir.path().join("record.rs");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "layout_mode": "declared",
          "record_length": 4,
          "input_encoding": "binary",
          "layout": [
            {
              "kind": "field",
              "name": "record_type",
              "offset": 0,
              "length": 1,
              "field_type": "alphanumeric",
              "encoding": "ascii"
            },
            {
              "kind": "redefines",
              "name": "view",
              "offset": 1,
              "variants": [
                {
                  "name": "as_text",
                  "selector": { "field": "record_type", "equals": "A" },
                  "layout": [
                    {
                      "kind": "field",
                      "name": "raw",
                      "offset": 0,
                      "length": 3,
                      "field_type": "alphanumeric",
                      "encoding": "ascii"
                    }
                  ]
                }
              ]
            }
          ]
        }"#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "schema",
        "emit-rust",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
    ]);
    cmd.assert().success();

    let generated = fs::read_to_string(output).unwrap();
    assert!(!generated.contains("unsafe"));
    assert!(generated.contains("record_bytes: &'a [u8]"));
    assert!(generated.contains("pub fn selected(&self) -> Result<ViewVariant<'a>, String>"));
    assert!(generated.contains("selector field record_type"));
    assert!(generated.contains("decode_text_generated(bytes, \"ascii\", 0)"));
    assert!(!generated.contains("self.select(\"\")"));
}

#[test]
fn schema_emit_rust_rejects_nested_groups_inside_redefines() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let output = dir.path().join("record.rs");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "layout_mode": "declared",
          "record_length": 4,
          "input_encoding": "binary",
          "layout": [
            {
              "kind": "field",
              "name": "item_count",
              "offset": 0,
              "field_type": "packed-decimal",
              "total_digits": 1,
              "scale": 0,
              "signed": false,
              "sign_mode": "pfd",
              "mode": "lossless"
            },
            {
              "kind": "redefines",
              "name": "view",
              "offset": 1,
              "variants": [
                {
                  "name": "as_array",
                  "layout": [
                    {
                      "kind": "occurs",
                      "name": "items",
                      "offset": 1,
                      "counter_field": "item_count",
                      "min_occurs": 0,
                      "max_occurs": 2,
                      "element_layout": [
                        {
                          "kind": "field",
                          "name": "code",
                          "offset": 0,
                          "length": 1,
                          "field_type": "alphanumeric",
                          "encoding": "ascii"
                        }
                      ]
                    }
                  ]
                }
              ]
            }
          ]
        }"#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "schema",
        "emit-rust",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
    ]);
    cmd.assert()
        .failure()
        .stderr(predicates::str::contains("scalar REDEFINES variants only"));
}

#[test]
fn schema_emit_rust_generates_occurs_vec_accessor() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let output = dir.path().join("record.rs");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "layout_mode": "declared",
          "record_length": 3,
          "input_encoding": "binary",
          "layout": [
            {
              "kind": "field",
              "name": "item_count",
              "offset": 0,
              "field_type": "packed-decimal",
              "total_digits": 1,
              "scale": 0,
              "signed": false,
              "sign_mode": "pfd",
              "mode": "lossless"
            },
            {
              "kind": "occurs",
              "name": "items",
              "offset": 1,
              "counter_field": "item_count",
              "min_occurs": 0,
              "max_occurs": 2,
              "element_layout": [
                {
                  "kind": "field",
                  "name": "code",
                  "offset": 0,
                  "length": 1,
                  "field_type": "alphanumeric",
                  "encoding": "ascii"
                }
              ]
            }
          ]
        }"#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "schema",
        "emit-rust",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
    ]);
    cmd.assert().success();

    let generated = fs::read_to_string(output).unwrap();
    assert!(generated.contains("fn expected_record_len(&self) -> Result<usize, String>"));
    assert!(generated.contains("fn __occurs_count_items(&self) -> Result<usize, String>"));
    assert!(generated.contains("pub fn items(&self) -> Result<Vec<ItemsElement<'a>>, String>"));
    assert!(generated.contains("pub struct ItemsElement<'a>"));
    assert!(!generated.contains("pub fn items_code"));
}

#[test]
fn schema_v2_rejects_scaled_occurs_counter() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "layout_mode": "declared",
          "record_length": 3,
          "input_encoding": "binary",
          "layout": [
            {
              "kind": "field",
              "name": "item_count",
              "offset": 0,
              "field_type": "packed-decimal",
              "total_digits": 2,
              "scale": 1,
              "signed": false,
              "sign_mode": "pfd",
              "mode": "lossless"
            },
            {
              "kind": "occurs",
              "name": "items",
              "offset": 2,
              "counter_field": "item_count",
              "min_occurs": 0,
              "max_occurs": 2,
              "element_layout": [
                {
                  "kind": "field",
                  "name": "code",
                  "offset": 0,
                  "length": 1,
                  "field_type": "alphanumeric",
                  "encoding": "ascii"
                }
              ]
            }
          ]
        }"#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args(["schema", "check", "--schema", schema.to_str().unwrap()]);
    cmd.assert()
        .failure()
        .stderr(predicates::str::contains("scale-zero numeric field"));
}

#[test]
fn schema_from_copybook_generates_schema_v2() {
    let dir = tempfile::tempdir().unwrap();
    let copybook = dir.path().join("record.cpy");
    let schema = dir.path().join("schema.json");
    fs::write(
        &copybook,
        r#"
       01  ACCOUNT-RECORD.
           05  ACCOUNT-NAME PIC X(03).
           05  BALANCE      PIC S9(5)V99 COMP-3.
           05  TXN-COUNT    PIC 9(04) COMP.
        "#,
    )
    .unwrap();

    let mut import = Command::cargo_bin("cobol-packed").unwrap();
    import.args([
        "schema",
        "from-copybook",
        "--input",
        copybook.to_str().unwrap(),
        "--output",
        schema.to_str().unwrap(),
        "--record-length",
        "9",
    ]);
    import.assert().success();

    let generated = fs::read_to_string(&schema).unwrap();
    assert!(generated.contains("\"version\": 2"));
    assert!(generated.contains("\"field_type\": \"alphanumeric\""));
    assert!(generated.contains("\"field_type\": \"packed-decimal\""));
    assert!(generated.contains("\"field_type\": \"binary\""));

    let mut check = Command::cargo_bin("cobol-packed").unwrap();
    check.args([
        "schema",
        "check",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        "json",
    ]);
    check.assert().success();
}

#[test]
fn schema_compare_reports_changed_fields() {
    let dir = tempfile::tempdir().unwrap();
    let left = dir.path().join("left.json");
    let right = dir.path().join("right.json");
    fs::write(
        &left,
        r#"{
          "version": 2,
          "layout_mode": "declared",
          "record_length": 3,
          "input_encoding": "binary",
          "layout": [
            {
              "kind": "field",
              "name": "amount",
              "offset": 0,
              "field_type": "packed-decimal",
              "total_digits": 4,
              "scale": 2,
              "signed": true,
              "sign_mode": "pfd",
              "mode": "lossless"
            }
          ]
        }"#,
    )
    .unwrap();
    fs::write(
        &right,
        r#"{
          "version": 2,
          "layout_mode": "declared",
          "record_length": 3,
          "input_encoding": "binary",
          "layout": [
            {
              "kind": "field",
              "name": "amount",
              "offset": 0,
              "field_type": "packed-decimal",
              "total_digits": 4,
              "scale": 1,
              "signed": true,
              "sign_mode": "pfd",
              "mode": "lossless"
            }
          ]
        }"#,
    )
    .unwrap();

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "schema",
        "compare",
        "--left",
        left.to_str().unwrap(),
        "--right",
        right.to_str().unwrap(),
        "--output",
        "json",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"same_semantics\": false"))
        .stdout(contains("\"changed\""))
        .stdout(contains("\"scale\": 1"));
}

#[test]
fn production_cli_paths_have_no_unwrap_or_expect() {
    let cli = include_str!("../src/cli/mod.rs");
    let record = include_str!("../src/cli/record.rs");
    let bin = include_str!("../src/bin/cobol-packed.rs");
    assert!(!cli.contains(".unwrap()"));
    assert!(!cli.contains(".expect("));
    assert!(!record.contains(".unwrap()"));
    assert!(!record.contains(".expect("));
    assert!(!bin.contains(".unwrap()"));
    assert!(!bin.contains(".expect("));
}

#[test]
fn completions_and_man_page_are_generated() {
    let mut completions = Command::cargo_bin("cobol-packed").unwrap();
    completions.args(["completions", "bash"]);
    completions
        .assert()
        .success()
        .stdout(contains("cobol-packed"));

    let mut man = Command::cargo_bin("cobol-packed").unwrap();
    man.arg("man");
    man.assert().success().stdout(contains("cobol\\-packed"));
}
