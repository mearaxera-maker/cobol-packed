#![cfg(feature = "cli")]

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use serde_json::Value;
use std::fs;

fn hostlens() -> Command {
    Command::cargo_bin("hostlens").unwrap()
}

#[test]
fn hostlens_is_primary_cli_name() {
    let mut cmd = hostlens();
    cmd.arg("--help");
    cmd.assert()
        .success()
        .stdout(contains("HostLens"))
        .stdout(contains("mainframe record"));
}

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
fn schema_workflow_commands_are_listed_in_help() {
    let mut cmd = hostlens();
    cmd.args(["schema", "--help"]);
    cmd.assert()
        .success()
        .stdout(contains("check"))
        .stdout(contains("from-copybook"))
        .stdout(contains("emit-rust"))
        .stdout(contains("compare"));
}

#[test]
fn schema_from_copybook_generates_schema_v2_for_mixed_record() {
    let dir = tempfile::tempdir().unwrap();
    let copybook = dir.path().join("customer.cpy");
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

    let mut cmd = hostlens();
    cmd.args([
        "schema",
        "from-copybook",
        "--copybook",
        copybook.to_str().unwrap(),
        "--encoding",
        "cp037",
    ]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["version"], 2);
    assert_eq!(json["record_length"], 19);
    assert_eq!(json["fields"][0]["name"], "account_id");
    assert_eq!(json["fields"][0]["field_type"], "display-text");
    assert_eq!(json["fields"][0]["offset"], 0);
    assert_eq!(json["fields"][1]["name"], "amount");
    assert_eq!(json["fields"][1]["field_type"], "packed-decimal");
    assert_eq!(json["fields"][1]["length"], 4);
    assert_eq!(json["fields"][2]["field_type"], "zoned-decimal");
    assert_eq!(json["fields"][2]["offset"], 8);
    assert_eq!(json["fields"][3]["field_type"], "binary");
    assert_eq!(json["fields"][3]["length"], 4);
    assert_eq!(json["fillers"][0]["offset"], 17);
    assert_eq!(json["fillers"][0]["length"], 2);
}

#[test]
fn schema_from_copybook_respects_usage_before_pic() {
    let dir = tempfile::tempdir().unwrap();
    let copybook = dir.path().join("usage-first.cpy");
    fs::write(
        &copybook,
        r#"
       01 USAGE-FIRST-REC.
          05 AMOUNT USAGE COMP-3 PIC S9(5)V99.
        "#,
    )
    .unwrap();

    let mut cmd = hostlens();
    cmd.args([
        "schema",
        "from-copybook",
        "--copybook",
        copybook.to_str().unwrap(),
        "--encoding",
        "cp037",
    ]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["record_length"], 4);
    assert_eq!(json["fields"][0]["field_type"], "packed-decimal");
    assert_eq!(json["fields"][0]["length"], 4);
}

#[test]
fn schema_from_copybook_handles_inline_comments_and_literal_periods() {
    let dir = tempfile::tempdir().unwrap();
    let copybook = dir.path().join("comments.cpy");
    fs::write(
        &copybook,
        r#"
       01 COMMENT-REC.
          05 STATUS-CODE PIC X(3) VALUE 'A.B'. *> keep literal period
          05 NEXT-FIELD  PIC X(2).
        "#,
    )
    .unwrap();

    let mut cmd = hostlens();
    cmd.args([
        "schema",
        "from-copybook",
        "--copybook",
        copybook.to_str().unwrap(),
        "--encoding",
        "cp037",
    ]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["record_length"], 5);
    assert_eq!(json["fields"][0]["name"], "status_code");
    assert_eq!(json["fields"][1]["name"], "next_field");
}

#[test]
fn schema_from_copybook_rejects_name_based_input_encodings() {
    let dir = tempfile::tempdir().unwrap();
    let copybook = dir.path().join("record.cpy");
    fs::write(&copybook, "       01 REC.\n          05 A PIC X(1).\n").unwrap();

    let mut cmd = hostlens();
    cmd.args([
        "schema",
        "from-copybook",
        "--copybook",
        copybook.to_str().unwrap(),
        "--encoding",
        "cp037",
        "--input-encoding",
        "csv",
    ]);
    cmd.assert()
        .code(2)
        .stderr(contains("fixed-width"))
        .stderr(contains("binary"))
        .stderr(contains("hex"));
}

#[test]
fn schema_from_copybook_suffixes_duplicate_normalized_names() {
    let dir = tempfile::tempdir().unwrap();
    let copybook = dir.path().join("dupes.cpy");
    fs::write(
        &copybook,
        r#"
       01 DUP-REC.
          05 A-B PIC X(1).
          05 A_B PIC X(1).
        "#,
    )
    .unwrap();

    let mut cmd = hostlens();
    cmd.args([
        "schema",
        "from-copybook",
        "--copybook",
        copybook.to_str().unwrap(),
        "--encoding",
        "cp037",
    ]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let json: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["fields"][0]["name"], "a_b");
    assert_eq!(json["fields"][1]["name"], "a_b_2");
}

#[test]
fn schema_from_copybook_rejects_unsupported_constructs_with_line_numbers() {
    let cases = [
        (
            "REDEFINES",
            "          05 ALT-AMOUNT REDEFINES AMOUNT PIC X(4).",
        ),
        ("OCCURS", "          05 ITEM PIC X(2) OCCURS 3 TIMES."),
        ("SYNC", "          05 BIN-FIELD PIC 9(4) COMP SYNC."),
        ("level 88", "          88 ACTIVE VALUE 'Y'."),
    ];

    for (expected, line) in cases {
        let dir = tempfile::tempdir().unwrap();
        let copybook = dir.path().join("bad.cpy");
        fs::write(
            &copybook,
            format!("       01 BAD-REC.\n          05 AMOUNT PIC 9(4).\n{line}\n"),
        )
        .unwrap();
        let mut cmd = hostlens();
        cmd.args([
            "schema",
            "from-copybook",
            "--copybook",
            copybook.to_str().unwrap(),
            "--encoding",
            "cp037",
        ]);
        cmd.assert()
            .code(2)
            .stderr(contains(expected))
            .stderr(contains("line"));
    }
}

#[test]
fn schema_from_copybook_requires_record_name_for_multiple_01_records() {
    let dir = tempfile::tempdir().unwrap();
    let copybook = dir.path().join("multi.cpy");
    fs::write(
        &copybook,
        "       01 FIRST-REC.\n          05 A PIC X(1).\n       01 SECOND-REC.\n          05 B PIC X(1).\n",
    )
    .unwrap();

    let mut cmd = hostlens();
    cmd.args([
        "schema",
        "from-copybook",
        "--copybook",
        copybook.to_str().unwrap(),
        "--encoding",
        "cp037",
    ]);
    cmd.assert().code(2).stderr(contains("--record-name"));
}

#[test]
fn schema_emit_rust_generates_struct_and_offsets() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "record_length": 7,
          "input_encoding": "binary",
          "fields": [{
            "name": "account-id",
            "field_type": "display-text",
            "offset": 0,
            "length": 4,
            "encoding": "cp037"
          }, {
            "name": "amount",
            "field_type": "packed-decimal",
            "offset": 4,
            "length": 3,
            "total_digits": 4,
            "scale": 2,
            "signed": true
          }]
        }"#,
    )
    .unwrap();

    let mut cmd = hostlens();
    cmd.args([
        "schema",
        "emit-rust",
        "--schema",
        schema.to_str().unwrap(),
        "--struct-name",
        "CustomerRecord",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("pub const RECORD_LEN: usize = 7;"))
        .stdout(contains("pub const ACCOUNT_ID_OFFSET: usize = 0;"))
        .stdout(contains("pub struct CustomerRecord"))
        .stdout(contains("pub account_id: String"))
        .stdout(contains("pub amount: String"))
        .stdout(predicates::str::contains("repr(C)").not())
        .stdout(predicates::str::contains("unsafe").not());
}

#[test]
fn schema_emit_rust_uses_effective_binary_signed_default_and_description() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "record_length": 4,
          "input_encoding": "binary",
          "fields": [{
            "name": "sequence",
            "field_type": "binary",
            "offset": 0,
            "length": 4,
            "description": "copybook line 2: PIC 9(9) COMP uses IBM big-endian binary width"
          }]
        }"#,
    )
    .unwrap();

    let mut cmd = hostlens();
    cmd.args([
        "schema",
        "emit-rust",
        "--schema",
        schema.to_str().unwrap(),
        "--struct-name",
        "BinaryRecord",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("pub sequence: i64"))
        .stdout(contains("IBM big-endian binary width"));
}

#[test]
fn schema_compare_ignores_description_and_output_preferences() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("old.json");
    let new = dir.path().join("new.json");
    fs::write(
        &old,
        r#"{"version":2,"record_length":3,"input_encoding":"binary","output":"jsonl","fields":[{"name":"amount","field_type":"packed-decimal","offset":0,"length":3,"total_digits":4,"scale":2,"signed":true,"description":"old"}]}"#,
    )
    .unwrap();
    fs::write(
        &new,
        r#"{"version":2,"record_length":3,"input_encoding":"binary","output":"csv","fields":[{"name":"amount","field_type":"packed-decimal","offset":0,"length":3,"total_digits":4,"scale":2,"signed":true,"description":"new"}]}"#,
    )
    .unwrap();

    let mut cmd = hostlens();
    cmd.args([
        "schema",
        "compare",
        "--old",
        old.to_str().unwrap(),
        "--new",
        new.to_str().unwrap(),
        "--output",
        "json",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"changed\": false"));
}

#[test]
fn schema_compare_uses_effective_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("old.json");
    let new = dir.path().join("new.json");
    fs::write(
        &old,
        r#"{"version":2,"record_length":7,"input_encoding":"binary","fields":[{"name":"amount","field_type":"packed-decimal","offset":0,"total_digits":4,"signed":true},{"name":"sequence","field_type":"binary","offset":3,"length":4}]}"#,
    )
    .unwrap();
    fs::write(
        &new,
        r#"{"version":2,"record_length":7,"input_encoding":"binary","fields":[{"name":"amount","field_type":"packed-decimal","offset":0,"length":3,"total_digits":4,"scale":0,"signed":true},{"name":"sequence","field_type":"binary","offset":3,"length":4,"scale":0,"signed":true}]}"#,
    )
    .unwrap();

    let mut cmd = hostlens();
    cmd.args([
        "schema",
        "compare",
        "--old",
        old.to_str().unwrap(),
        "--new",
        new.to_str().unwrap(),
        "--output",
        "json",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"changed\": false"));
}

#[test]
fn schema_compare_reports_breaking_field_layout_changes() {
    let dir = tempfile::tempdir().unwrap();
    let old = dir.path().join("old.json");
    let new = dir.path().join("new.json");
    fs::write(
        &old,
        r#"{"version":2,"record_length":3,"input_encoding":"binary","fields":[{"name":"amount","field_type":"packed-decimal","offset":0,"length":3,"total_digits":4,"scale":2,"signed":true}]}"#,
    )
    .unwrap();
    fs::write(
        &new,
        r#"{"version":2,"record_length":4,"input_encoding":"binary","fields":[{"name":"amount","field_type":"packed-decimal","offset":1,"length":3,"total_digits":4,"scale":2,"signed":true}]}"#,
    )
    .unwrap();

    let mut cmd = hostlens();
    cmd.args([
        "schema",
        "compare",
        "--old",
        old.to_str().unwrap(),
        "--new",
        new.to_str().unwrap(),
        "--output",
        "json",
    ]);
    cmd.assert()
        .code(1)
        .stdout(contains("\"changed\": true"))
        .stdout(contains("\"severity\": \"breaking\""))
        .stdout(contains("offset"));
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
        .stdout(contains("\"tool\": \"hostlens\""))
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

    assert_eq!(first_json["schema_hash"], second_json["schema_hash"]);
    assert_ne!(
        first_json["schema_file_sha256"],
        second_json["schema_file_sha256"]
    );
}

#[test]
fn semantic_schema_hash_ignores_descriptions_and_output_preferences() {
    let dir = tempfile::tempdir().unwrap();
    let schema_a = dir.path().join("a.json");
    let schema_b = dir.path().join("b.json");
    fs::write(
        &schema_a,
        r#"{
          "version": 2,
          "record_length": 7,
          "input_encoding": "binary",
          "output": "jsonl",
          "fields": [{
            "name": "customer",
            "field_type": "display-text",
            "offset": 0,
            "length": 4,
            "encoding": "cp037",
            "description": "customer mnemonic"
          }, {
            "name": "amount",
            "field_type": "packed-decimal",
            "offset": 4,
            "length": 3,
            "total_digits": 4,
            "scale": 2,
            "signed": true,
            "sign_mode": "pfd",
            "mode": "lossless",
            "description": "payment amount"
          }]
        }"#,
    )
    .unwrap();
    fs::write(
        &schema_b,
        r#"{
          "version": 2,
          "record_length": 7,
          "input_encoding": "binary",
          "output": "csv",
          "fields": [{
            "name": "customer",
            "field_type": "display-text",
            "offset": 0,
            "length": 4,
            "encoding": "cp037",
            "description": "changed documentation"
          }, {
            "name": "amount",
            "field_type": "packed-decimal",
            "offset": 4,
            "length": 3,
            "total_digits": 4,
            "scale": 2,
            "signed": true,
            "sign_mode": "pfd",
            "mode": "lossless",
            "description": "changed documentation"
          }]
        }"#,
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

    assert_eq!(first_json["schema_hash"], second_json["schema_hash"]);
    assert_ne!(
        first_json["schema_file_sha256"],
        second_json["schema_file_sha256"]
    );
}

#[test]
fn schema_v2_decodes_mixed_packed_text_and_raw_fields() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "record_length": 8,
          "input_encoding": "binary",
          "on_error": "emit-error-row",
          "verification_scope": "record",
          "fields": [{
            "name": "account",
            "field_type": "display-text",
            "offset": 0,
            "length": 4,
            "encoding": "cp037"
          }, {
            "name": "amount",
            "field_type": "packed-decimal",
            "offset": 4,
            "length": 3,
            "total_digits": 4,
            "scale": 2,
            "signed": true,
            "sign_mode": "pfd",
            "mode": "lossless"
          }, {
            "name": "flags",
            "field_type": "raw-bytes",
            "offset": 7,
            "length": 1
          }]
        }"#,
    )
    .unwrap();
    fs::write(&data, [0xC1, 0xC2, 0xC3, 0xF1, 0x01, 0x23, 0x4C, 0xAA]).unwrap();

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
        .stdout(contains("\"field\":\"account\""))
        .stdout(contains("\"value\":\"ABC1\""))
        .stdout(contains("\"field\":\"amount\""))
        .stdout(contains("\"value\":\"12.34\""))
        .stdout(contains("\"field\":\"flags\""))
        .stdout(contains("\"value\":\"AA\""));
}

#[test]
fn schema_v2_decodes_zoned_decimal_and_binary_fields() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "record_length": 8,
          "input_encoding": "binary",
          "on_error": "emit-error-row",
          "fields": [{
            "name": "zoned",
            "field_type": "zoned-decimal",
            "offset": 0,
            "length": 4,
            "total_digits": 4,
            "scale": 2,
            "signed": true,
            "sign_mode": "pfd"
          }, {
            "name": "counter",
            "field_type": "binary",
            "offset": 4,
            "length": 4,
            "signed": false
          }]
        }"#,
    )
    .unwrap();
    fs::write(&data, [0xF1, 0xF2, 0xF3, 0xD4, 0x00, 0x00, 0x03, 0xE8]).unwrap();

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
        .stdout(contains("\"field\":\"zoned\""))
        .stdout(contains("\"value\":\"-12.34\""))
        .stdout(contains("\"field\":\"counter\""))
        .stdout(contains("\"value\":\"1000\""));
}

#[test]
fn batch_verify_round_trips_all_schema_v2_field_types() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "record_length": 16,
          "input_encoding": "binary",
          "on_error": "emit-error-row",
          "verification_scope": "record",
          "fields": [{
            "name": "account",
            "field_type": "display-text",
            "offset": 0,
            "length": 4,
            "encoding": "cp037"
          }, {
            "name": "amount",
            "field_type": "packed-decimal",
            "offset": 4,
            "length": 3,
            "total_digits": 4,
            "scale": 2,
            "signed": true,
            "sign_mode": "pfd",
            "mode": "lossless"
          }, {
            "name": "tax",
            "field_type": "zoned-decimal",
            "offset": 7,
            "length": 4,
            "total_digits": 4,
            "scale": 2,
            "signed": true,
            "sign_mode": "pfd"
          }, {
            "name": "sequence",
            "field_type": "binary",
            "offset": 11,
            "length": 4,
            "signed": false
          }, {
            "name": "flags",
            "field_type": "raw-bytes",
            "offset": 15,
            "length": 1
          }]
        }"#,
    )
    .unwrap();
    fs::write(
        &data,
        [
            0xC1, 0xC3, 0xC3, 0xE3, 0x01, 0x23, 0x4C, 0xF1, 0xF2, 0xF3, 0xD4, 0x00, 0x00, 0x03,
            0xE8, 0xAA,
        ],
    )
    .unwrap();

    let mut cmd = hostlens();
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
        .stdout(contains("\"field_byte_for_byte_verified\": true"))
        .stdout(contains("\"record_byte_for_byte_verified\": true"));
}

#[test]
fn display_text_rejects_missing_or_unsupported_encoding() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.json");
    let unsupported = dir.path().join("unsupported.json");
    fs::write(
        &missing,
        r#"{
          "version": 2,
          "record_length": 4,
          "input_encoding": "binary",
          "fields": [{
            "name": "name",
            "field_type": "display-text",
            "offset": 0,
            "length": 4
          }]
        }"#,
    )
    .unwrap();
    fs::write(
        &unsupported,
        r#"{
          "version": 2,
          "record_length": 4,
          "input_encoding": "binary",
          "fields": [{
            "name": "name",
            "field_type": "display-text",
            "offset": 0,
            "length": 4,
            "encoding": "cp9999"
          }]
        }"#,
    )
    .unwrap();

    let mut missing_cmd = Command::cargo_bin("cobol-packed").unwrap();
    missing_cmd.args([
        "schema",
        "check",
        "--schema",
        missing.to_str().unwrap(),
        "--output",
        "json",
    ]);
    missing_cmd.assert().code(2).stderr(contains("encoding"));

    let mut unsupported_cmd = Command::cargo_bin("cobol-packed").unwrap();
    unsupported_cmd.args([
        "schema",
        "check",
        "--schema",
        unsupported.to_str().unwrap(),
        "--output",
        "json",
    ]);
    unsupported_cmd
        .assert()
        .code(2)
        .stderr(contains("unsupported EBCDIC encoding"));
}

#[test]
fn display_text_decodes_expanded_ebcdic_codepages() {
    let cases = [
        ("cp273", [0x4A, 0x5A, 0xBA, 0xC1, 0xF1], "ÄÜ¬A1"),
        ("cp297", [0x4A, 0x5A, 0xBA, 0xC1, 0xF1], "°§¬A1"),
        ("cp1026", [0x4A, 0x5A, 0xBA, 0xC1, 0xF1], "ÇĞ¬A1"),
        ("cp1025", [0x4A, 0x5A, 0xBA, 0xC1, 0xF1], "[]БA1"),
    ];
    for (encoding, bytes, expected) in cases {
        let dir = tempfile::tempdir().unwrap();
        let schema = dir.path().join("schema.json");
        let data = dir.path().join("records.bin");
        fs::write(
            &schema,
            format!(
                r#"{{
                  "version": 2,
                  "record_length": 5,
                  "input_encoding": "binary",
                  "fields": [{{
                    "name": "name",
                    "field_type": "display-text",
                    "offset": 0,
                    "length": 5,
                    "encoding": "{encoding}"
                  }}]
                }}"#
            ),
        )
        .unwrap();
        fs::write(&data, bytes).unwrap();

        let mut cmd = hostlens();
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
        let output = cmd.assert().success().get_output().stdout.clone();
        let row: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(row["value"], expected);
    }
}

#[test]
fn display_text_rejects_mixed_dbcs_encodings() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "record_length": 4,
          "input_encoding": "binary",
          "fields": [{
            "name": "name",
            "field_type": "display-text",
            "offset": 0,
            "length": 4,
            "encoding": "cp939"
          }]
        }"#,
    )
    .unwrap();

    let mut cmd = hostlens();
    cmd.args([
        "schema",
        "check",
        "--schema",
        schema.to_str().unwrap(),
        "--output",
        "json",
    ]);
    cmd.assert().code(2).stderr(contains("mixed-dbcs-text"));
}

#[test]
fn mixed_dbcs_text_decodes_stateful_sbcs_and_dbcs_blank() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "record_length": 6,
          "input_encoding": "binary",
          "fields": [{
            "name": "name",
            "field_type": "mixed-dbcs-text",
            "offset": 0,
            "length": 6,
            "encoding": "cp939"
          }]
        }"#,
    )
    .unwrap();
    fs::write(&data, [0xC1, 0x0E, 0x40, 0x40, 0x0F, 0xF1]).unwrap();

    let mut cmd = hostlens();
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
    let output = cmd.assert().success().get_output().stdout.clone();
    let row: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(row["value"], "A\u{3000}1");
}

#[test]
fn mixed_dbcs_text_decodes_cjk_glyph_tables() {
    let cases = [
        ("cp930", [0x44, 0x81], "あ"),
        ("cp933", [0x88, 0x61], "가"),
        ("cp935", [0x59, 0xBA], "一"),
        ("cp937", [0x4C, 0x41], "一"),
        ("cp939", [0x44, 0x81], "あ"),
    ];
    for (encoding, dbcs_pair, expected) in cases {
        let dir = tempfile::tempdir().unwrap();
        let schema = dir.path().join("schema.json");
        let data = dir.path().join("records.bin");
        fs::write(
            &schema,
            format!(
                r#"{{
                  "version": 2,
                  "record_length": 4,
                  "input_encoding": "binary",
                  "fields": [{{
                    "name": "text",
                    "field_type": "mixed-dbcs-text",
                    "offset": 0,
                    "length": 4,
                    "encoding": "{encoding}"
                  }}]
                }}"#
            ),
        )
        .unwrap();
        fs::write(&data, [0x0E, dbcs_pair[0], dbcs_pair[1], 0x0F]).unwrap();

        let mut cmd = hostlens();
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
        let output = cmd.assert().success().get_output().stdout.clone();
        let row: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(row["value"], expected);
    }
}

#[test]
fn batch_verify_rejects_noncanonical_mixed_dbcs_shift_sequences() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "record_length": 8,
          "input_encoding": "binary",
          "on_error": "emit-error-row",
          "fields": [{
            "name": "text",
            "field_type": "mixed-dbcs-text",
            "offset": 0,
            "length": 8,
            "encoding": "cp939"
          }]
        }"#,
    )
    .unwrap();
    fs::write(&data, [0x0E, 0x44, 0x81, 0x0F, 0x0E, 0x44, 0x81, 0x0F]).unwrap();

    let mut cmd = hostlens();
    cmd.args([
        "batch",
        "verify",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "jsonl",
    ]);
    cmd.assert()
        .code(1)
        .stdout(contains("\"error_code\":\"E_VERIFY\""))
        .stderr(contains("E_VERIFY"));
}

#[test]
fn mixed_dbcs_text_rejects_bad_shift_state() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "record_length": 3,
          "input_encoding": "binary",
          "on_error": "emit-error-row",
          "fields": [{
            "name": "name",
            "field_type": "mixed-dbcs-text",
            "offset": 0,
            "length": 3,
            "encoding": "cp939"
          }]
        }"#,
    )
    .unwrap();
    fs::write(&data, [0xC1, 0x0E, 0x40]).unwrap();

    let mut cmd = hostlens();
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
        .stdout(contains("unterminated DBCS"));
}

#[test]
fn unsupported_dbcs_like_codepages_are_not_accepted_as_text_encodings() {
    let unsupported_cases = [
        ("display-text", "cp942", "unsupported EBCDIC encoding cp942"),
        (
            "mixed-dbcs-text",
            "cp942",
            "unsupported EBCDIC encoding cp942",
        ),
        (
            "display-text",
            "cp5026",
            "unsupported EBCDIC encoding cp5026",
        ),
        (
            "mixed-dbcs-text",
            "cp5026",
            "unsupported EBCDIC encoding cp5026",
        ),
    ];

    for (field_type, encoding, expected_error) in unsupported_cases {
        let dir = tempfile::tempdir().unwrap();
        let schema = dir.path().join("schema.json");
        fs::write(
            &schema,
            format!(
                r#"{{
                  "version": 2,
                  "record_length": 4,
                  "input_encoding": "binary",
                  "fields": [{{
                    "name": "text",
                    "field_type": "{field_type}",
                    "offset": 0,
                    "length": 4,
                    "encoding": "{encoding}"
                  }}]
                }}"#
            ),
        )
        .unwrap();

        let mut cmd = hostlens();
        cmd.args([
            "schema",
            "check",
            "--schema",
            schema.to_str().unwrap(),
            "--output",
            "json",
        ]);
        cmd.assert().code(2).stderr(contains(expected_error));
    }
}

#[test]
fn mixed_dbcs_text_rejects_single_byte_codepages() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    fs::write(
        &schema,
        r#"{
          "version": 2,
          "record_length": 4,
          "input_encoding": "binary",
          "fields": [{
            "name": "text",
            "field_type": "mixed-dbcs-text",
            "offset": 0,
            "length": 4,
            "encoding": "cp037"
          }]
        }"#,
    )
    .unwrap();

    let mut cmd = hostlens();
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
        .stderr(contains("requires a mixed DBCS encoding"));
}

#[test]
fn encodings_list_json_separates_single_byte_and_mixed_dbcs() {
    let mut cmd = hostlens();
    cmd.args(["encodings", "list", "--output", "json"]);
    let output = cmd.assert().success().get_output().stdout.clone();
    let rows: Vec<Value> = serde_json::from_slice(&output).unwrap();

    let cp037 = rows
        .iter()
        .find(|row| row["encoding"] == "cp037")
        .expect("cp037 should be listed");
    assert_eq!(cp037["field_type"], "display-text");
    assert_eq!(cp037["byte_model"], "single-byte");
    assert!(cp037["aliases"]
        .as_array()
        .unwrap()
        .iter()
        .any(|alias| alias == "ibm037"));

    let cp939 = rows
        .iter()
        .find(|row| row["encoding"] == "cp939")
        .expect("cp939 should be listed");
    assert_eq!(cp939["field_type"], "mixed-dbcs-text");
    assert_eq!(cp939["byte_model"], "stateful-mixed-dbcs");
    assert!(cp939["notes"].as_str().unwrap().contains("SO/SI"));
    assert!(!rows
        .iter()
        .any(|row| row["encoding"] == "cp939" && row["field_type"] == "display-text"));
}

#[test]
fn encodings_list_table_is_human_navigable() {
    let mut cmd = hostlens();
    cmd.args(["encodings", "list"]);
    cmd.assert()
        .success()
        .stdout(contains("encoding\tfield_type\tbyte_model"))
        .stdout(contains("cp037\tdisplay-text\tsingle-byte"))
        .stdout(contains("cp939\tmixed-dbcs-text\tstateful-mixed-dbcs"));
}

#[test]
fn schema_check_accepts_common_ebcdic_codepage_identifiers() {
    let encodings = [
        "cp037", "ibm037", "ccsid037", "cp273", "ibm273", "ccsid273", "cp277", "ibm277", "cp278",
        "ibm278", "cp280", "ibm280", "cp284", "ibm284", "cp285", "ibm285", "cp290", "ibm290",
        "cp297", "ibm297", "cp420", "ibm420", "cp423", "ibm423", "cp424", "ibm424", "cp500",
        "ibm500", "ccsid500", "cp833", "ibm833", "cp838", "ibm838", "cp870", "ibm870", "cp871",
        "ibm871", "cp875", "ibm875", "cp880", "ibm880", "cp905", "ibm905", "cp924", "ibm924",
        "cp1025", "ibm1025", "cp1026", "ibm1026", "cp1047", "ibm1047", "cp1140", "ibm1140",
        "cp1141", "ibm1141", "cp1142", "ibm1142", "cp1143", "ibm1143", "cp1144", "ibm1144",
        "cp1145", "ibm1145", "cp1146", "ibm1146", "cp1147", "ibm1147", "cp1148", "ibm1148",
        "cp1149", "ibm1149",
    ];
    assert!(encodings.len() > 50);
    for encoding in encodings {
        let dir = tempfile::tempdir().unwrap();
        let schema = dir.path().join("schema.json");
        fs::write(
            &schema,
            format!(
                r#"{{
                  "version": 2,
                  "record_length": 4,
                  "input_encoding": "binary",
                  "fields": [{{
                    "name": "name",
                    "field_type": "display-text",
                    "offset": 0,
                    "length": 4,
                    "encoding": "{encoding}"
                  }}]
                }}"#
            ),
        )
        .unwrap();

        let mut cmd = hostlens();
        cmd.args([
            "schema",
            "check",
            "--schema",
            schema.to_str().unwrap(),
            "--output",
            "json",
        ]);
        cmd.assert().success();
    }
}

#[test]
fn batch_decode_reads_hex_records_from_stdin_and_can_use_one_based_indices() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
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

    let mut cmd = Command::cargo_bin("cobol-packed").unwrap();
    cmd.args([
        "batch",
        "decode",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        "-",
        "--output",
        "jsonl",
        "--one-based-index",
    ])
    .write_stdin("01234C\n");
    cmd.assert()
        .success()
        .stdout(contains("\"record_index\":1"))
        .stdout(contains("\"value\":\"12.34\""));
}

#[test]
fn fail_on_empty_turns_empty_audit_into_data_error() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
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
    fs::write(&data, []).unwrap();

    let mut allowed = Command::cargo_bin("cobol-packed").unwrap();
    allowed.args([
        "batch",
        "decode",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "audit",
    ]);
    allowed
        .assert()
        .success()
        .stdout(contains("\"status\": \"empty\""));

    let mut strict = Command::cargo_bin("cobol-packed").unwrap();
    strict.args([
        "batch",
        "decode",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "audit",
        "--fail-on-empty",
    ]);
    strict
        .assert()
        .code(1)
        .stdout(contains("\"status\": \"empty\""))
        .stderr(contains("E_EMPTY"));
}

#[test]
fn dry_run_counts_records_without_emitting_rows_and_quiet_suppresses_table_header() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
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
    fs::write(&data, [0x01, 0x23, 0x4C, 0x02, 0x34, 0x51]).unwrap();

    let mut dry = Command::cargo_bin("cobol-packed").unwrap();
    dry.args([
        "batch",
        "decode",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "audit",
        "--dry-run",
    ]);
    dry.assert()
        .success()
        .stdout(contains("\"records_seen\": 2"))
        .stdout(contains("\"fields_seen\": 0"))
        .stdout(contains("\"status\": \"empty\""));

    let mut quiet = Command::cargo_bin("cobol-packed").unwrap();
    quiet.args([
        "batch",
        "decode",
        "--schema",
        schema.to_str().unwrap(),
        "--input",
        data.to_str().unwrap(),
        "--output",
        "table",
        "--quiet",
        "--max-records",
        "1",
    ]);
    quiet
        .assert()
        .success()
        .stdout(contains("12.34"))
        .stdout(contains("record\tfield").not());
}

#[test]
fn parallel_binary_decode_preserves_record_order() {
    let dir = tempfile::tempdir().unwrap();
    let schema = dir.path().join("schema.json");
    let data = dir.path().join("records.bin");
    fs::write(
        &schema,
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
        "jsonl",
        "--parallel",
        "2",
    ]);
    cmd.assert()
        .success()
        .stdout(contains("\"record_index\":0"))
        .stdout(contains("\"value\":\"12.34\""))
        .stdout(contains("\"record_index\":1"))
        .stdout(contains("\"value\":\"23.45\""));
}

#[test]
fn audit_output_includes_throughput_metrics() {
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
        .stdout(contains("\"elapsed_ms\""))
        .stdout(contains("\"records_per_sec\""))
        .stdout(contains("\"fields_per_sec\""))
        .stdout(contains("\"bytes_per_sec\""));
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
        .stdout(contains("\"argv\""));
}

#[test]
fn production_cli_paths_have_no_unwrap_or_expect() {
    let cli = include_str!("../src/cli/mod.rs");
    let legacy_bin = include_str!("../src/bin/cobol-packed.rs");
    let hostlens_bin = include_str!("../src/bin/hostlens.rs");
    assert!(!cli.contains(".unwrap()"));
    assert!(!cli.contains(".expect("));
    assert!(!legacy_bin.contains(".unwrap()"));
    assert!(!legacy_bin.contains(".expect("));
    assert!(!hostlens_bin.contains(".unwrap()"));
    assert!(!hostlens_bin.contains(".expect("));
}

#[test]
fn completions_and_man_page_are_generated() {
    let mut completions = Command::cargo_bin("cobol-packed").unwrap();
    completions.args(["completions", "bash"]);
    completions.assert().success().stdout(contains("hostlens"));

    let mut man = Command::cargo_bin("cobol-packed").unwrap();
    man.arg("man");
    man.assert().success().stdout(contains("hostlens"));
}
