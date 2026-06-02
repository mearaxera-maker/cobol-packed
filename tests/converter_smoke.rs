#![cfg(feature = "converter")]

use assert_cmd::prelude::*;
use predicates::prelude::*;
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

fn json_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

fn assert_generated_stdout_exact(out: &Path, expected: &str) {
    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(
        !program_rs.contains("UnsupportedTrap"),
        "successful generated project contains an unsupported runtime trap"
    );
    let output = Command::new("cargo")
        .current_dir(out)
        .args(["run", "--offline"])
        .output()
        .expect("run generated project");
    assert!(
        output.status.success(),
        "generated project failed\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

fn read_migration_report(out: &Path) -> Value {
    let report = fs::read_to_string(out.join("migration-report.json")).expect("report");
    serde_json::from_str(&report).expect("parse migration-report.json")
}

fn json_contains_string(value: &Value, needle: &str) -> bool {
    let key_needle = needle.trim_matches('"');
    match value {
        Value::String(text) => text.contains(needle),
        Value::Array(values) => values
            .iter()
            .any(|value| json_contains_string(value, needle)),
        Value::Object(values) => {
            values.keys().any(|key| key.contains(key_needle))
                || values
                    .values()
                    .any(|value| json_contains_string(value, needle))
        }
        _ => false,
    }
}

fn report_diagnostic_codes(report: &Value) -> Vec<&str> {
    report
        .get("diagnostics")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|diagnostic| diagnostic.get("code").and_then(Value::as_str))
        .collect()
}

fn assert_report_status(report: &Value, expected: &str) {
    assert_eq!(
        report.get("status").and_then(Value::as_str),
        Some(expected),
        "unexpected migration report status: {report:#}"
    );
}

fn assert_report_has_top_level(report: &Value, key: &str) {
    assert!(
        report.get(key).is_some(),
        "migration report missing top-level key {key}: {report:#}"
    );
}

fn assert_report_has_diagnostic_code(report: &Value, code: &str) {
    let codes = report_diagnostic_codes(report);
    assert!(
        codes.contains(&code),
        "migration report missing diagnostic {code}; actual codes: {codes:?}\n{report:#}"
    );
}

fn assert_report_contains_json_string(report: &Value, needle: &str) {
    let serialized = serde_json::to_string(report).expect("serialize parsed report");
    let pretty = serde_json::to_string_pretty(report).expect("serialize parsed report");
    assert!(
        json_contains_string(report, needle)
            || serialized.contains(needle)
            || pretty.contains(needle),
        "migration report JSON does not contain string fragment {needle:?}: {report:#}"
    );
}

#[test]
fn cobol2rust_help_lists_operational_options() {
    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("cobol2rust convert"))
        .stdout(predicate::str::contains("--input <path>"))
        .stdout(predicate::str::contains(
            "--source-format <fixed|free|auto>",
        ));
}

#[test]
fn cobol2rust_bad_args_are_configuration_errors() {
    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args(["convert", "--unknown"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("unknown option --unknown"));

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args(["convert", "--input"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("missing value for --input"));

    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("hello.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\nPROCEDURE DIVISION.\nMAIN.\nSTOP RUN.\n",
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "unknown",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("invalid --dialect unknown"));
}

#[test]
fn cobol2rust_generates_runtime_backed_project() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("hello.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\nPROCEDURE DIVISION.\nMAIN.\nDISPLAY \"HELLO\".\nSTOP RUN.\n",
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("generated Rust project"));

    assert!(out.join("Cargo.toml").is_file());
    assert!(out.join("src/program.rs").is_file());
    assert!(out.join("vendor/cobol-runtime/Cargo.toml").is_file());
    assert!(out.join("vendor/cobol-record/Cargo.toml").is_file());
    let manifest = fs::read_to_string(out.join("Cargo.toml")).expect("manifest");
    assert!(manifest.contains("vendor/cobol-runtime"));
    assert!(manifest.contains("vendor/cobol-record"));
    let report = read_migration_report(&out);
    assert_report_status(&report, "generated");
    assert!(
        report.pointer("/storage/record_plan").is_some(),
        "migration report missing storage record plan: {report:#}"
    );
    assert_report_has_top_level(&report, "dialect_profile");
    assert_report_has_top_level(&report, "semantic");
    assert_report_has_top_level(&report, "diagnostic_sections");
}

#[test]
fn cobol2rust_auto_source_format_accepts_indented_free_format() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("indented-free.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        "    IDENTIFICATION DIVISION.\n    PROGRAM-ID. INDENTED.\n    PROCEDURE DIVISION.\n    MAIN.\n    DISPLAY \"OK\".\n    STOP RUN.\n",
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "auto",
        ])
        .assert()
        .success();

    let report = read_migration_report(&out);
    assert_eq!(
        report.get("source_format").and_then(Value::as_str),
        Some("free")
    );
    assert!(
        !json_contains_string(&report, "NTIFICATION"),
        "auto source normalization corrupted the report JSON: {report:#}"
    );
    assert_generated_stdout_exact(&out, "OK\n");
}

#[test]
fn cobol2rust_generated_vm_uses_requested_dialect() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("dialect.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        "IDENTIFICATION DIVISION.\nPROGRAM-ID. DIALECT.\nPROCEDURE DIVISION.\nMAIN.\nDISPLAY \"OK\".\nSTOP RUN.\n",
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "gnucobol",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("cobol_dialect::DialectProfile::gnucobol()"));
    assert!(!program_rs.contains("cobol_dialect::DialectProfile::ibm_zos()"));
}

#[test]
fn cobol2rust_generates_perform_times_through_vm_procedure() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("perform-times.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. PERFTIME.
PROCEDURE DIVISION.
MAIN.
    PERFORM SUBPARA 3 TIMES.
    STOP RUN.
SUBPARA.
    DISPLAY "X".
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let report = read_migration_report(&out);
    assert_report_has_top_level(&report, "procedure_cfg");
    assert_report_contains_json_string(&report, "cfg");
    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("cobol_vm::VmProcedure"));
    assert!(program_rs.contains("execute_procedure"));
    assert!(!program_rs.contains("enum ParagraphId"));
    assert!(!program_rs.contains("fn dispatch"));
    assert!(!program_rs.contains("fn perform_range"));
    assert!(!program_rs.contains("ControlFlow"));

    assert_generated_stdout_exact(&out, "X\nX\nX\n");
}

#[test]
fn cobol2rust_unwinds_goto_out_of_perform_scope_from_nested_if() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("perform-goto.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. PERFGOTO.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 FLAG PIC X VALUE "Y".
PROCEDURE DIVISION.
MAIN.
    PERFORM SUB THRU END-SUB.
    DISPLAY "AFTER".
    STOP RUN.
SUB.
    IF FLAG = "Y" GO TO OUT.
END-SUB.
    DISPLAY "END".
OUT.
    DISPLAY "OUT".
    STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("VmProcedureOp::GoTo"));

    assert_generated_stdout_exact(&out, "OUT\n");
}

#[test]
fn cobol2rust_blocks_if_next_sentence_then_branch_until_sentence_cfg() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("next-sentence-then.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. NEXTSENT.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FLAG PIC X VALUE "Y".
PROCEDURE DIVISION.
MAIN.
IF WS-FLAG = "Y" NEXT SENTENCE ELSE DISPLAY "ELSE".
DISPLAY "AFTER".
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migration blocked"));

    let report = read_migration_report(&out);
    assert_report_has_diagnostic_code(&report, "E_UNSUPPORTED_CONTROL_FLOW");
    assert_report_contains_json_string(&report, "NEXT SENTENCE");
}

#[test]
fn cobol2rust_blocks_if_next_sentence_else_branch_until_sentence_cfg() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("next-sentence-else.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. NEXTSENF.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FLAG PIC X VALUE "N".
PROCEDURE DIVISION.
MAIN.
IF WS-FLAG = "Y" NEXT SENTENCE ELSE DISPLAY "ELSE".
DISPLAY "AFTER".
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migration blocked"));

    let report = read_migration_report(&out);
    assert_report_has_diagnostic_code(&report, "E_UNSUPPORTED_CONTROL_FLOW");
    assert_report_contains_json_string(&report, "NEXT SENTENCE");
}

#[test]
fn cobol2rust_runs_nested_if_with_nearest_else_binding() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("nested-if.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. NESTIF.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-A PIC X VALUE "Y".
01 WS-B PIC X VALUE "N".
PROCEDURE DIVISION.
MAIN.
IF WS-A = "Y" IF WS-B = "Y" DISPLAY "B" ELSE DISPLAY "A" END-IF ELSE DISPLAY "N" END-IF.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    assert_generated_stdout_exact(&out, "A\n");
}

#[test]
fn cobol2rust_blocks_complex_next_sentence_shape() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("next-sentence-complex.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. NEXTSENX.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FLAG PIC X VALUE "Y".
PROCEDURE DIVISION.
MAIN.
IF WS-FLAG = "Y" NEXT SENTENCE DISPLAY "BAD".
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migration blocked"));

    let report = read_migration_report(&out);
    assert_report_has_diagnostic_code(&report, "E_UNSUPPORTED_CONTROL_FLOW");
    assert_report_contains_json_string(&report, "NEXT SENTENCE");
}

#[test]
fn cobol2rust_blocks_unsupported_inline_statement_inside_size_error_branch() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("nested-unsupported.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. NESTUNSUP.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 N PIC 9 VALUE 9.
PROCEDURE DIVISION.
MAIN.
COMPUTE N = N + 1 ON SIZE ERROR MERGE SORT-FILE.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migration blocked"));

    let report = read_migration_report(&out);
    assert_report_has_diagnostic_code(&report, "E_UNSUPPORTED_STATEMENT");
    assert_report_contains_json_string(&report, "MERGE");
    assert!(!out.join("src/program.rs").exists());
}

#[test]
fn cobol2rust_blocked_conversion_removes_stale_generated_project() {
    let dir = tempdir().expect("tempdir");
    let good = dir.path().join("good.cbl");
    let bad = dir.path().join("bad.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &good,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. GOOD.
PROCEDURE DIVISION.
MAIN.
DISPLAY "OK".
STOP RUN.
"#,
    )
    .expect("write good fixture");
    fs::write(
        &bad,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. BAD.
PROCEDURE DIVISION.
MAIN.
ACCEPT WS-NOT-SUPPORTED.
STOP RUN.
"#,
    )
    .expect("write bad fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            good.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();
    assert!(out.join("src/program.rs").exists());

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            bad.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migration blocked"));

    let report = read_migration_report(&out);
    assert_report_has_diagnostic_code(&report, "E_UNSUPPORTED_VERB");
    assert!(!out.join("src/program.rs").exists());
    assert!(!out.join("Cargo.toml").exists());
    assert!(!out.join("vendor/cobol-vm/src/lib.rs").exists());
}

#[test]
fn cobol2rust_generates_codec_backed_accessors_for_packed_and_binary_fields() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("typed-accessors.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. TYPED.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-AMOUNT PIC S9(7)V99 COMP-3.
   05 WS-COUNT PIC 9(4) COMP.
   05 WS-RATIO COMP-1.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let data_rs = fs::read_to_string(out.join("src/data.rs")).expect("data.rs");
    assert!(data_rs.contains("pub fn ws_rec_ws_amount"));
    assert!(data_rs.contains("decode_packed_decimal"));
    assert!(data_rs.contains("pub fn ws_rec_ws_count"));
    assert!(data_rs.contains("decode_binary_integer"));
    assert!(data_rs.contains("pub fn ws_rec_ws_ratio"));
    assert!(data_rs.contains("decode_ibm_float32"));

    Command::new("cargo")
        .current_dir(&out)
        .args(["check", "--offline"])
        .assert()
        .success();
}

#[test]
fn cobol2rust_generated_byte_storage_project_compiles() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("data-display.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        "IDENTIFICATION DIVISION.\nPROGRAM-ID. DATADISP.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 WS-NAME PIC X(10).\nPROCEDURE DIVISION.\nMAIN.\nMOVE \"ALPHA\" TO WS-NAME.\nDISPLAY WS-NAME.\nSTOP RUN.\n",
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["check", "--offline"])
        .assert()
        .success();

    let data_rs = fs::read_to_string(out.join("src/data.rs")).expect("data.rs");
    assert!(data_rs.contains("pub struct DataView"));
    assert!(data_rs.contains("pub fn ws_name"));
}

#[test]
fn cobol2rust_display_uses_codec_backed_accessors_for_numeric_fields() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("display-codecs.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. DISPCODEC.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-AMOUNT PIC S9(5)V99 COMP-3.
   05 WS-COUNT PIC 9(4) COMP.
   05 WS-RATIO COMP-1.
PROCEDURE DIVISION.
MAIN.
DISPLAY WS-AMOUNT WS-COUNT WS-RATIO.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("cobol_vm::VmProcedureOp::Display"));
    assert!(program_rs.contains("cobol_vm::VmExpr::Access"));
    assert!(program_rs.contains("execute_procedure"));
    let data_rs = fs::read_to_string(out.join("src/data.rs")).expect("data.rs");
    assert!(data_rs.contains("pub fn ws_rec_ws_amount"));
    assert!(data_rs.contains("pub fn ws_rec_ws_count"));
    assert!(data_rs.contains("pub fn ws_rec_ws_ratio"));

    Command::new("cargo")
        .current_dir(&out)
        .args(["check", "--offline"])
        .assert()
        .success();
}

#[test]
fn cobol2rust_generates_executable_if_through_vm() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("if-vm.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. IFVM.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FLAG PIC X VALUE "Y".
PROCEDURE DIVISION.
MAIN.
IF WS-FLAG = "Y" DISPLAY "YES".
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("fn eval_condition"));
    assert!(program_rs.contains("cobol_vm::VmCondition"));
    assert_generated_stdout_exact(&out, "YES\n");
}

#[test]
fn cobol2rust_generates_condition_name_set_and_if() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("set-cond.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SETCOND.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FLAG PIC X.
   88 WS-OK VALUE "Y".
PROCEDURE DIVISION.
MAIN.
SET WS-OK TO TRUE.
IF WS-OK DISPLAY "OK".
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    assert_generated_stdout_exact(&out, "OK\n");
}

#[test]
fn cobol2rust_emits_declared_view_for_group_condition_name() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("group-88-view.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. G88VIEW.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-PAIR.
   88 PAIR-OK VALUE "YZ".
   05 WS-A PIC X.
   05 WS-B PIC X.
PROCEDURE DIVISION.
MAIN.
SET PAIR-OK TO TRUE.
IF PAIR-OK DISPLAY "OK".
DISPLAY WS-A.
DISPLAY WS-B.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("VmDeclaredView"));
    assert!(program_rs.contains("\"WS_PAIR.WS_A\""));
    assert!(program_rs.contains("\"WS_PAIR.WS_B\""));

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("OK\nY\nZ"));
}

#[test]
fn cobol2rust_generates_simple_evaluate_also_snapshot() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("evaluate-vm.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. EVALVM.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FLAG PIC X VALUE "A".
PROCEDURE DIVISION.
MAIN.
EVALUATE WS-FLAG ALSO TRUE
    WHEN "A" ALSO TRUE DISPLAY "A"
    WHEN ANY ALSO ANY DISPLAY "OTHER"
END-EVALUATE.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "\"evaluates\"");
    assert_generated_stdout_exact(&out, "A\n");
}

#[test]
fn cobol2rust_blocks_unsupported_exec_sql_with_report() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("exec.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        "IDENTIFICATION DIVISION.\nPROGRAM-ID. EXECBAD.\nPROCEDURE DIVISION.\nMAIN.\nEXEC SQL SELECT 1 END-EXEC.\nSTOP RUN.\n",
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migration blocked"));

    let report = read_migration_report(&out);
    assert_report_status(&report, "blocked");
    assert_report_has_diagnostic_code(&report, "E_UNSUPPORTED_STATEMENT");
}

#[test]
fn cobol2rust_blocks_unsupported_perform_control_forms_precisely() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("perform-control.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. BADPERF.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 I PIC 9.
PROCEDURE DIVISION.
MAIN.
PERFORM VARYING I FROM 1 BY 1 UNTIL I > 3 DISPLAY "X" END-PERFORM.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migration blocked"));

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "E_UNSUPPORTED_PERFORM_VARYING");
    assert!(!out.join("src/program.rs").exists());
}

#[test]
fn cobol2rust_runs_out_of_line_perform_until_through_vm_loop() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("perform-until.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. PERFUNTIL.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 DONE-FLAG PIC X VALUE "N".
PROCEDURE DIVISION.
MAIN.
PERFORM SUBPARA UNTIL DONE-FLAG = "Y".
DISPLAY "AFTER".
STOP RUN.
SUBPARA.
DISPLAY "LOOP".
MOVE "Y" TO DONE-FLAG.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("VmProcedureOp::PerformLoop"));
    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("LOOP\nAFTER"));
}

#[test]
fn cobol2rust_runs_out_of_line_perform_varying_until_through_vm_loop() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("perform-varying.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. PERFVARY.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 I PIC 9 VALUE 0.
PROCEDURE DIVISION.
MAIN.
PERFORM SUBPARA VARYING I FROM 1 BY 1 UNTIL I > 3.
DISPLAY "DONE".
STOP RUN.
SUBPARA.
DISPLAY I.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("VmProcedureOp::PerformLoop"));
    assert_generated_stdout_exact(&out, "1\n2\n3\nDONE\n");
}

#[test]
fn cobol2rust_runs_alterable_go_to_dot_through_vm() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("alter.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. ALTEROK.
PROCEDURE DIVISION.
MAIN.
ALTER DIVE-IN TO PROCEED TO END-WORLD.
GO TO DIVE-IN.
DIVE-IN.
GO TO .
DISPLAY "BAD".
END-WORLD.
DISPLAY "END".
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("VmProcedureOp::Alter"));
    assert!(program_rs.contains("VmControlTransfer::AlteredGoTo"));
    assert_generated_stdout_exact(&out, "END\n");
}

#[test]
fn cobol2rust_runs_go_to_depending_on_through_vm() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("go-depending.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. GODEPOK.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 LOOP-INDEX PIC 9 VALUE 2.
PROCEDURE DIVISION.
MAIN.
GO TO PATH1 PATH2 PATH3 DEPENDING ON LOOP-INDEX.
DISPLAY "FALL".
STOP RUN.
PATH1.
DISPLAY "ONE".
STOP RUN.
PATH2.
DISPLAY "TWO".
STOP RUN.
PATH3.
DISPLAY "THREE".
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("VmProcedureOp::ComputedGoTo"));
    assert_generated_stdout_exact(&out, "TWO\n");
}

#[test]
fn cobol2rust_runs_dynamic_perform_through_vm() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("dynamic-perform.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. DYNPERFOK.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TARGET PIC X(6) VALUE "TARGET".
PROCEDURE DIVISION.
MAIN.
PERFORM WS-TARGET.
DISPLAY "AFTER".
STOP RUN.
TARGET.
DISPLAY "DYN".
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("VmProcedureOp::DynamicPerform"));
    assert_generated_stdout_exact(&out, "DYN\nAFTER\n");
}

#[test]
fn cobol2rust_runs_read_on_exception_branch_through_vm() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("read-exception.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. READEXCOK.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT IN-FILE ASSIGN TO "in.dat"
        FILE STATUS IS WS-FS.
DATA DIVISION.
FILE SECTION.
FD IN-FILE.
01 IN-REC PIC X.
WORKING-STORAGE SECTION.
01 WS-FS PIC XX.
PROCEDURE DIVISION.
MAIN.
READ IN-FILE ON EXCEPTION DISPLAY "ERR" WS-FS.
DISPLAY "AFTER".
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("on_exception_ops"));
    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ERR90\nAFTER"));
}

#[test]
fn cobol2rust_runs_use_after_error_declarative_without_file_status() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("decl-no-status.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. DECLNOST.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT IN-FILE ASSIGN TO "missing.dat".
DATA DIVISION.
FILE SECTION.
FD IN-FILE.
01 IN-REC PIC X.
PROCEDURE DIVISION.
DECLARATIVES.
ERR-HOOK SECTION.
    USE AFTER ERROR ON IN-FILE.
    DISPLAY "HOOK".
END DECLARATIVES.
MAIN.
OPEN INPUT IN-FILE.
DISPLAY "AFTER".
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("register_file_error_declarative"));
    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("HOOK\nAFTER"));
}

#[test]
fn cobol2rust_runs_use_for_debugging_declarative_on_paragraph_entry() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("debug-decl-entry.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. DBGDECL.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FLAG PIC X VALUE "N".
PROCEDURE DIVISION.
DECLARATIVES.
DBG-HOOK SECTION.
    USE FOR DEBUGGING ON MAIN.
    MOVE "Y" TO WS-FLAG.
END DECLARATIVES.
MAIN.
DISPLAY WS-FLAG.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("register_debugging_declarative"));
    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Y\n"));
}

#[test]
fn cobol2rust_runs_use_for_debugging_declarative_on_perform_entry() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("debug-decl-perform.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. DBGDECP.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 HIT-COUNT PIC 9 VALUE 0.
PROCEDURE DIVISION.
DECLARATIVES.
DBG-HOOK SECTION.
    USE FOR DEBUGGING ON SUB-PARA.
    ADD 1 TO HIT-COUNT.
END DECLARATIVES.
MAIN.
PERFORM SUB-PARA.
PERFORM SUB-PARA.
DISPLAY HIT-COUNT.
STOP RUN.
SUB-PARA.
DISPLAY "S".
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("S\nS\n2\n"));
}

#[test]
fn cobol2rust_runs_ready_reset_trace_for_paragraph_entries() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("ready-reset-trace.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. TRACEOK.
PROCEDURE DIVISION.
MAIN.
READY TRACE.
PERFORM SUB-PARA.
RESET TRACE.
PERFORM SUB-PARA.
STOP RUN.
SUB-PARA.
DISPLAY "S".
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("VmProcedureOp::TraceOn"));
    assert!(program_rs.contains("VmProcedureOp::TraceOff"));
    let run = Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success();
    let stdout = String::from_utf8(run.get_output().stdout.clone()).expect("stdout utf8");
    assert_eq!(stdout.matches("TRACE SUB_PARA\n").count(), 1);
    assert!(stdout.contains("TRACE SUB_PARA\nS\nS\n"));
}

#[test]
fn cobol2rust_sets_debug_special_registers_for_debugging_declarative() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("debug-special-registers.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. DBGSPEC.
PROCEDURE DIVISION.
DECLARATIVES.
DBG-HOOK SECTION.
    USE FOR DEBUGGING ON SUB-PARA.
    DISPLAY DEBUG-ITEM.
    DISPLAY DEBUG-CONTENTS.
END DECLARATIVES.
MAIN.
PERFORM SUB-PARA.
STOP RUN.
SUB-PARA.
DISPLAY "BODY".
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let run = Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success();
    let stdout = String::from_utf8(run.get_output().stdout.clone()).expect("stdout utf8");
    assert!(stdout.contains("SUB_PARA"));
    assert!(stdout.contains("ENTER"));
    assert!(stdout.contains("BODY\n"));
}

#[test]
fn cobol2rust_use_after_error_declarative_can_mutate_file_status() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("decl-status.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. DECLSTAT.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT IN-FILE ASSIGN TO "missing.dat"
        FILE STATUS IS WS-FS.
DATA DIVISION.
FILE SECTION.
FD IN-FILE.
01 IN-REC PIC X.
WORKING-STORAGE SECTION.
01 WS-FS PIC XX.
PROCEDURE DIVISION.
DECLARATIVES.
ERR-HOOK SECTION.
    USE AFTER ERROR ON IN-FILE.
    DISPLAY "HOOK" WS-FS.
    MOVE "00" TO WS-FS.
END DECLARATIVES.
MAIN.
OPEN INPUT IN-FILE.
DISPLAY WS-FS.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("HOOK35\n00"));
}

#[test]
fn cobol2rust_use_after_error_runs_before_read_on_exception_branch() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("decl-before-exception.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. DECLORD.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT IN-FILE ASSIGN TO "in.dat"
        FILE STATUS IS WS-FS.
DATA DIVISION.
FILE SECTION.
FD IN-FILE.
01 IN-REC PIC X.
WORKING-STORAGE SECTION.
01 WS-FS PIC XX.
01 WS-FLAG PIC X VALUE "N".
PROCEDURE DIVISION.
DECLARATIVES.
ERR-HOOK SECTION.
    USE AFTER ERROR ON IN-FILE.
    MOVE "Y" TO WS-FLAG.
END DECLARATIVES.
MAIN.
READ IN-FILE ON EXCEPTION DISPLAY "BRANCH" WS-FLAG WS-FS.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("BRANCHY90"));
}

#[test]
fn cobol2rust_use_after_error_reentrancy_guard_propagates_same_file_recursion() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("decl-reentrant.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. DECLREENT.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT IN-FILE ASSIGN TO "missing.dat".
DATA DIVISION.
FILE SECTION.
FD IN-FILE.
01 IN-REC PIC X.
PROCEDURE DIVISION.
DECLARATIVES.
ERR-HOOK SECTION.
    USE AFTER ERROR ON IN-FILE.
    DISPLAY "HOOK".
    OPEN INPUT IN-FILE.
END DECLARATIVES.
MAIN.
OPEN INPUT IN-FILE.
DISPLAY "AFTER".
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("FileRuntime"))
        .stderr(predicate::str::contains("IN_FILE"));
}

#[test]
fn cobol2rust_runs_rewrite_last_read_record_through_vm() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("rewrite.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. REWRITEOK.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT IN-FILE ASSIGN TO "records.dat"
        FILE STATUS IS WS-FS.
DATA DIVISION.
FILE SECTION.
FD IN-FILE.
01 IN-REC PIC XX.
WORKING-STORAGE SECTION.
01 WS-FS PIC XX.
PROCEDURE DIVISION.
MAIN.
OPEN I-O IN-FILE.
READ IN-FILE.
MOVE "ZZ" TO IN-REC.
REWRITE IN-REC INVALID KEY DISPLAY "BAD" NOT INVALID KEY DISPLAY "OK" END-REWRITE.
CLOSE IN-FILE.
DISPLAY WS-FS.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("VmProcedureOp::RewriteFile"));
    fs::write(out.join("records.dat"), b"AABB").expect("assigned input bytes");

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("OK\n00"))
        .stdout(predicate::str::contains("BAD").not());
    assert_eq!(
        fs::read(out.join("records.dat")).expect("rewritten bytes"),
        b"ZZBB"
    );
}

#[test]
fn cobol2rust_runs_delete_last_read_record_through_vm() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("delete.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. DELETEOK.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT IN-FILE ASSIGN TO "records.dat"
        FILE STATUS IS WS-FS.
DATA DIVISION.
FILE SECTION.
FD IN-FILE.
01 IN-REC PIC XX.
WORKING-STORAGE SECTION.
01 WS-FS PIC XX.
PROCEDURE DIVISION.
MAIN.
OPEN I-O IN-FILE.
READ IN-FILE.
READ IN-FILE.
DELETE IN-FILE INVALID KEY DISPLAY "BAD" NOT INVALID KEY DISPLAY "DELETED" END-DELETE.
CLOSE IN-FILE.
DISPLAY WS-FS.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("VmProcedureOp::DeleteFile"));
    fs::write(out.join("records.dat"), b"AABBCC").expect("assigned input bytes");

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("DELETED\n00"))
        .stdout(predicate::str::contains("BAD").not());
    assert_eq!(
        fs::read(out.join("records.dat")).expect("deleted bytes"),
        b"AACC"
    );
}

#[test]
fn cobol2rust_runs_sort_procedure_ascending_through_vm() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("sort-ascending.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SORTASC.
DATA DIVISION.
FILE SECTION.
SD SORT-FILE.
01 SORT-REC PIC XX.
PROCEDURE DIVISION.
MAIN.
SORT SORT-FILE ASCENDING KEY SORT-REC
    INPUT PROCEDURE IS LOAD-SORT
    OUTPUT PROCEDURE IS DRAIN-SORT THRU SORT-DONE.
STOP RUN.
LOAD-SORT.
MOVE "C3" TO SORT-REC.
RELEASE SORT-REC.
MOVE "A1" TO SORT-REC.
RELEASE SORT-REC.
MOVE "B2" TO SORT-REC.
RELEASE SORT-REC.
DRAIN-SORT.
RETURN SORT-FILE AT END GO TO SORT-DONE.
DISPLAY SORT-REC.
GO TO DRAIN-SORT.
SORT-DONE.
DISPLAY "DONE".
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("VmProcedureOp::SortProcedure"));
    assert!(program_rs.contains("VmProcedureOp::ReleaseSortRecord"));
    assert!(program_rs.contains("VmProcedureOp::ReturnSortRecord"));
    assert_generated_stdout_exact(&out, "A1\nB2\nC3\nDONE\n");
}

#[test]
fn cobol2rust_runs_sort_procedure_descending_through_vm() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("sort-descending.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SORTDESC.
DATA DIVISION.
FILE SECTION.
SD SORT-FILE.
01 SORT-REC PIC XX.
PROCEDURE DIVISION.
MAIN.
SORT SORT-FILE DESCENDING KEY SORT-REC
    INPUT PROCEDURE IS LOAD-SORT
    OUTPUT PROCEDURE IS DRAIN-SORT THRU SORT-DONE.
STOP RUN.
LOAD-SORT.
MOVE "C3" TO SORT-REC.
RELEASE SORT-REC.
MOVE "A1" TO SORT-REC.
RELEASE SORT-REC.
MOVE "B2" TO SORT-REC.
RELEASE SORT-REC.
DRAIN-SORT.
RETURN SORT-FILE AT END GO TO SORT-DONE.
DISPLAY SORT-REC.
GO TO DRAIN-SORT.
SORT-DONE.
DISPLAY "DONE".
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    assert_generated_stdout_exact(&out, "C3\nB2\nA1\nDONE\n");
}

#[test]
fn cobol2rust_sort_release_from_and_return_into_copy_bytes() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("sort-copy.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SORTCOPY.
DATA DIVISION.
FILE SECTION.
SD SORT-FILE.
01 SORT-REC PIC XX.
WORKING-STORAGE SECTION.
01 WS-REC PIC XX.
PROCEDURE DIVISION.
MAIN.
SORT SORT-FILE ASCENDING KEY SORT-REC
    INPUT PROCEDURE IS LOAD-SORT
    OUTPUT PROCEDURE IS DRAIN-SORT THRU SORT-DONE.
STOP RUN.
LOAD-SORT.
MOVE "B2" TO WS-REC.
RELEASE SORT-REC FROM WS-REC.
DISPLAY SORT-REC.
MOVE "A1" TO WS-REC.
RELEASE SORT-REC FROM WS-REC.
DISPLAY SORT-REC.
DRAIN-SORT.
RETURN SORT-FILE INTO WS-REC AT END GO TO SORT-DONE NOT AT END DISPLAY WS-REC SORT-REC.
GO TO DRAIN-SORT.
SORT-DONE.
DISPLAY "DONE".
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    assert_generated_stdout_exact(&out, "B2\nA1\nA1A1\nB2B2\nDONE\n");
}

#[test]
fn cobol2rust_sort_self_release_from_sd_record_is_stable() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("sort-self-release.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SORTSELF.
DATA DIVISION.
FILE SECTION.
SD SORT-FILE.
01 SORT-REC PIC XX.
PROCEDURE DIVISION.
MAIN.
SORT SORT-FILE ASCENDING KEY SORT-REC
    INPUT PROCEDURE IS LOAD-SORT
    OUTPUT PROCEDURE IS DRAIN-SORT THRU SORT-DONE.
STOP RUN.
LOAD-SORT.
MOVE "B2" TO SORT-REC.
RELEASE SORT-REC FROM SORT-REC.
MOVE "A1" TO SORT-REC.
RELEASE SORT-REC FROM SORT-REC.
DRAIN-SORT.
RETURN SORT-FILE AT END GO TO SORT-DONE.
DISPLAY SORT-REC.
GO TO DRAIN-SORT.
SORT-DONE.
DISPLAY "DONE".
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    assert_generated_stdout_exact(&out, "A1\nB2\nDONE\n");
}

#[test]
fn cobol2rust_sort_release_outside_input_phase_abends() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("sort-release-phase.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SORTRELPH.
DATA DIVISION.
FILE SECTION.
SD SORT-FILE.
01 SORT-REC PIC XX.
PROCEDURE DIVISION.
MAIN.
SORT SORT-FILE ASCENDING KEY SORT-REC
    INPUT PROCEDURE IS LOAD-SORT
    OUTPUT PROCEDURE IS DRAIN-SORT.
STOP RUN.
LOAD-SORT.
MOVE "A1" TO SORT-REC.
RELEASE SORT-REC.
DRAIN-SORT.
MOVE "B2" TO SORT-REC.
RELEASE SORT-REC.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "RELEASE executed during SORT Output phase",
        ));
}

#[test]
fn cobol2rust_sort_return_outside_output_phase_abends() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("sort-return-phase.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SORTRETPH.
DATA DIVISION.
FILE SECTION.
SD SORT-FILE.
01 SORT-REC PIC XX.
PROCEDURE DIVISION.
MAIN.
SORT SORT-FILE ASCENDING KEY SORT-REC
    INPUT PROCEDURE IS LOAD-SORT
    OUTPUT PROCEDURE IS DRAIN-SORT.
STOP RUN.
LOAD-SORT.
RETURN SORT-FILE AT END DISPLAY "BAD".
DRAIN-SORT.
DISPLAY "DONE".
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "RETURN executed during SORT Input phase",
        ));
}

#[test]
fn cobol2rust_nested_sort_uses_innermost_sort_state() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("sort-nested.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SORTNEST.
DATA DIVISION.
FILE SECTION.
SD OUTER-SORT.
01 OUTER-REC PIC XX.
SD INNER-SORT.
01 INNER-REC PIC XX.
PROCEDURE DIVISION.
MAIN.
SORT OUTER-SORT ASCENDING KEY OUTER-REC
    INPUT PROCEDURE IS LOAD-OUTER
    OUTPUT PROCEDURE IS DRAIN-OUTER THRU OUTER-DONE.
STOP RUN.
LOAD-OUTER.
MOVE "B2" TO OUTER-REC.
RELEASE OUTER-REC.
MOVE "A1" TO OUTER-REC.
RELEASE OUTER-REC.
DRAIN-OUTER.
RETURN OUTER-SORT AT END GO TO OUTER-DONE.
SORT INNER-SORT ASCENDING KEY INNER-REC
    INPUT PROCEDURE IS LOAD-INNER
    OUTPUT PROCEDURE IS DRAIN-INNER THRU INNER-DONE.
DISPLAY OUTER-REC.
GO TO DRAIN-OUTER.
OUTER-DONE.
DISPLAY "DONE".
LOAD-INNER.
MOVE "Y2" TO INNER-REC.
RELEASE INNER-REC.
MOVE "X1" TO INNER-REC.
RELEASE INNER-REC.
DRAIN-INNER.
RETURN INNER-SORT AT END GO TO INNER-DONE.
DISPLAY INNER-REC.
GO TO DRAIN-INNER.
INNER-DONE.
DISPLAY "INNER-DONE".
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    assert_generated_stdout_exact(
        &out,
        "X1\nY2\nINNER-DONE\nA1\nX1\nY2\nINNER-DONE\nB2\nDONE\n",
    );
}

#[test]
fn cobol2rust_sort_output_procedure_alter_persists_after_sort() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("sort-alter.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SORTALTER.
DATA DIVISION.
FILE SECTION.
SD SORT-FILE.
01 SORT-REC PIC X.
PROCEDURE DIVISION.
MAIN.
SORT SORT-FILE ASCENDING KEY SORT-REC
    INPUT PROCEDURE IS LOAD-SORT
    OUTPUT PROCEDURE IS DRAIN-SORT THRU SORT-DONE.
GO TO HANDLER.
STOP RUN.
LOAD-SORT.
MOVE "A" TO SORT-REC.
RELEASE SORT-REC.
DRAIN-SORT.
RETURN SORT-FILE AT END GO TO SORT-DONE.
ALTER HANDLER TO PROCEED TO NEW-HANDLER.
GO TO DRAIN-SORT.
SORT-DONE.
DISPLAY "DONE".
HANDLER.
DISPLAY "OLD".
STOP RUN.
NEW-HANDLER.
DISPLAY "NEW".
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    assert_generated_stdout_exact(&out, "DONE\nNEW\n");
}

#[test]
fn cobol2rust_sort_return_stays_at_end_after_exhaustion() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("sort-exhausted.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SORTEND.
DATA DIVISION.
FILE SECTION.
SD SORT-FILE.
01 SORT-REC PIC XX.
PROCEDURE DIVISION.
MAIN.
SORT SORT-FILE ASCENDING KEY SORT-REC
    INPUT PROCEDURE IS LOAD-SORT
    OUTPUT PROCEDURE IS DRAIN-SORT.
STOP RUN.
LOAD-SORT.
MOVE "A1" TO SORT-REC.
RELEASE SORT-REC.
DRAIN-SORT.
RETURN SORT-FILE AT END DISPLAY "BAD" NOT AT END DISPLAY SORT-REC.
RETURN SORT-FILE AT END DISPLAY "END1".
RETURN SORT-FILE AT END DISPLAY "END2".
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    assert_generated_stdout_exact(&out, "A1\nEND1\nEND2\n");
}

#[test]
fn cobol2rust_sort_can_run_inside_use_after_error_declarative() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("sort-declarative.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SORTDECL.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT IN-FILE ASSIGN TO "missing.dat".
DATA DIVISION.
FILE SECTION.
FD IN-FILE.
01 IN-REC PIC X.
SD SORT-FILE.
01 SORT-REC PIC XX.
WORKING-STORAGE SECTION.
01 WS-FLAG PIC X VALUE "N".
PROCEDURE DIVISION.
DECLARATIVES.
ERR-HOOK SECTION.
    USE AFTER ERROR ON IN-FILE.
    SORT SORT-FILE ASCENDING KEY SORT-REC
        INPUT PROCEDURE IS LOAD-SORT
        OUTPUT PROCEDURE IS DRAIN-SORT THRU SORT-DONE.
    MOVE "Y" TO WS-FLAG.
END DECLARATIVES.
MAIN.
OPEN INPUT IN-FILE.
DISPLAY WS-FLAG.
STOP RUN.
LOAD-SORT.
MOVE "B2" TO SORT-REC.
RELEASE SORT-REC.
MOVE "A1" TO SORT-REC.
RELEASE SORT-REC.
DRAIN-SORT.
RETURN SORT-FILE AT END GO TO SORT-DONE.
DISPLAY SORT-REC.
GO TO DRAIN-SORT.
SORT-DONE.
DISPLAY "SORTDONE".
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    assert_generated_stdout_exact(&out, "A1\nB2\nSORTDONE\nY\n");
}

#[test]
fn cobol2rust_sort_comp3_key_orders_by_numeric_value_not_packed_bytes() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("sort-packed-key.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SORTPACK.
DATA DIVISION.
FILE SECTION.
SD SORT-FILE.
01 SORT-REC.
   05 SORT-KEY PIC S9(3) COMP-3.
   05 SORT-TEXT PIC X.
PROCEDURE DIVISION.
MAIN.
SORT SORT-FILE ASCENDING KEY SORT-KEY
    INPUT PROCEDURE IS LOAD-SORT
    OUTPUT PROCEDURE IS DRAIN-SORT.
STOP RUN.
LOAD-SORT.
MOVE 1 TO SORT-KEY.
MOVE "P" TO SORT-TEXT.
RELEASE SORT-REC.
MOVE -1 TO SORT-KEY.
MOVE "N" TO SORT-TEXT.
RELEASE SORT-REC.
MOVE 2 TO SORT-KEY.
MOVE "T" TO SORT-TEXT.
RELEASE SORT-REC.
DRAIN-SORT.
RETURN SORT-FILE
    AT END GO TO SORT-DONE
    NOT AT END DISPLAY SORT-TEXT
END-RETURN.
GO TO DRAIN-SORT.
SORT-DONE.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    assert_generated_stdout_exact(&out, "N\nP\nT\n");
}

#[test]
fn cobol2rust_abandoned_sort_input_state_is_cleaned_before_later_return() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("sort-abandoned.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SORTABND.
DATA DIVISION.
FILE SECTION.
SD SORT-FILE.
01 SORT-REC PIC XX.
PROCEDURE DIVISION.
MAIN.
SORT SORT-FILE ASCENDING KEY SORT-REC
    INPUT PROCEDURE IS LOAD-SORT
    OUTPUT PROCEDURE IS DRAIN-SORT.
DISPLAY "BAD".
STOP RUN.
LOAD-SORT.
MOVE "B2" TO SORT-REC.
RELEASE SORT-REC.
GO TO AFTER-ABANDON.
DRAIN-SORT.
DISPLAY "OUTPUT-BAD".
AFTER-ABANDON.
RETURN SORT-FILE AT END DISPLAY "END".
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "RETURN executed without an active SORT",
        ));
}

#[test]
fn cobol2rust_abandoned_sort_output_state_is_cleaned_before_later_return() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("sort-output-abandoned.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SORTOABN.
DATA DIVISION.
FILE SECTION.
SD SORT-FILE.
01 SORT-REC PIC XX.
PROCEDURE DIVISION.
MAIN.
SORT SORT-FILE ASCENDING KEY SORT-REC
    INPUT PROCEDURE IS LOAD-SORT
    OUTPUT PROCEDURE IS DRAIN-SORT.
DISPLAY "BAD".
STOP RUN.
LOAD-SORT.
MOVE "B2" TO SORT-REC.
RELEASE SORT-REC.
DRAIN-SORT.
GO TO AFTER-ABANDON.
AFTER-ABANDON.
RETURN SORT-FILE AT END DISPLAY "END".
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "RETURN executed without an active SORT",
        ));
}

#[test]
fn cobol2rust_blocks_unsupported_sort_shapes_fail_closed() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("sort-blockers.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SORTBLOCK.
DATA DIVISION.
FILE SECTION.
SD SORT-FILE.
01 SORT-REC PIC X.
SD SORT-TWO.
01 SORT-TWO-REC.
   05 SORT-KEY-A PIC X.
   05 SORT-KEY-B PIC X.
SD ODO-SORT.
01 ODO-REC.
   05 ODO-COUNT PIC 9.
   05 ODO-ITEM OCCURS 0 TO 2 DEPENDING ON ODO-COUNT PIC X.
SD PACK-SORT.
01 PACK-REC.
   05 PACK-KEY PIC S9(3) COMP-3.
SD BIN-SORT.
01 BIN-REC.
   05 BIN-KEY PIC S9(4) COMP.
PROCEDURE DIVISION.
MAIN.
SORT SORT-FILE USING IN-FILE GIVING OUT-FILE.
SORT SORT-TWO ASCENDING KEY SORT-KEY-A DESCENDING KEY SORT-KEY-B
    INPUT PROCEDURE IS IN-PROC
    OUTPUT PROCEDURE IS OUT-PROC.
SORT SORT-FILE INPUT PROCEDURE IS IN-PROC.
SORT ODO-SORT ASCENDING KEY ODO-REC
    INPUT PROCEDURE IS IN-PROC
    OUTPUT PROCEDURE IS OUT-PROC.
SORT PACK-SORT ASCENDING KEY PACK-KEY
    INPUT PROCEDURE IS IN-PROC
    OUTPUT PROCEDURE IS OUT-PROC.
SORT BIN-SORT ASCENDING KEY BIN-KEY
    INPUT PROCEDURE IS IN-PROC
    OUTPUT PROCEDURE IS OUT-PROC.
STOP RUN.
IN-PROC.
DISPLAY "IN".
OUT-PROC.
DISPLAY "OUT".
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migration blocked"));

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "E_UNSUPPORTED_SORT");
    assert_report_contains_json_string(&report, "E_UNSUPPORTED_SORT_RECORD_ODO");
    assert_report_contains_json_string(&report, "E_UNSUPPORTED_SORT_KEY");
    assert!(!out.join("src/program.rs").exists());
}

#[test]
fn cobol2rust_runs_inspect_tallying_all_literal() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("inspect-tally.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. INSPTAL.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TEXT PIC X(6) VALUE "ABACAD".
01 WS-COUNT PIC 99 VALUE 5.
PROCEDURE DIVISION.
MAIN.
INSPECT WS-TEXT TALLYING WS-COUNT FOR ALL "A".
DISPLAY WS-COUNT.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("VmProcedureOp::InspectLike"));
    assert_generated_stdout_exact(&out, "08\n");
}

#[test]
fn cobol2rust_runs_inspect_replacing_all_literal() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("inspect-replace.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. INSPREP.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TEXT PIC X(6) VALUE "ABACAD".
PROCEDURE DIVISION.
MAIN.
INSPECT WS-TEXT REPLACING ALL "A" BY "Z".
DISPLAY WS-TEXT.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    assert_generated_stdout_exact(&out, "ZBZCZD\n");
}

#[test]
fn cobol2rust_runs_inspect_converting_literal_bytes() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("inspect-convert.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. INSPCNV.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TEXT PIC X(6) VALUE "ABC123".
PROCEDURE DIVISION.
MAIN.
INSPECT WS-TEXT CONVERTING "ABC" TO "XYZ".
DISPLAY WS-TEXT.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    assert_generated_stdout_exact(&out, "XYZ123\n");
}

#[test]
fn cobol2rust_runs_examine_tallying_and_replacing() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("examine-tally-replace.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. EXAMINE1.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TEXT PIC X(6) VALUE "ABACAD".
PROCEDURE DIVISION.
MAIN.
EXAMINE WS-TEXT TALLYING ALL "A" REPLACING BY "Z".
DISPLAY WS-TEXT.
DISPLAY TALLY.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    assert_generated_stdout_exact(&out, "ZBZCZD\n000000003\n");
}

#[test]
fn cobol2rust_blocks_unsupported_complex_inspect_shapes_fail_closed() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("inspect-blockers.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. INSPBLK.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TEXT PIC X(6) VALUE "ABACAD".
01 WS-COUNT PIC 99.
PROCEDURE DIVISION.
MAIN.
INSPECT WS-TEXT TALLYING WS-COUNT FOR LEADING "A".
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migration blocked"));

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "E_UNSUPPORTED_VERB");
    assert_report_contains_json_string(&report, "INSPECT");
    assert!(!out.join("src/program.rs").exists());
}

#[test]
fn cobol2rust_runs_string_core_delimited_by_size() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("string-size.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. STRSIZE.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-A PIC XX VALUE "AB".
01 WS-B PIC XX VALUE "CD".
01 WS-TEXT PIC X(4).
PROCEDURE DIVISION.
MAIN.
STRING WS-A DELIMITED BY SIZE WS-B DELIMITED BY SIZE INTO WS-TEXT.
DISPLAY WS-TEXT.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("VmProcedureOp::StringOp"));
    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ABCD\n"));
}

#[test]
fn cobol2rust_runs_string_core_delimited_by_literal() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("string-literal.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. STRLIT.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TEXT PIC X(4).
PROCEDURE DIVISION.
MAIN.
STRING "ABC" DELIMITED BY "B" INTO WS-TEXT.
DISPLAY WS-TEXT.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("A\n"));
}

#[test]
fn cobol2rust_runs_unstring_core_delimited_by_space() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("unstring-space.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. UNSTRSP.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-SRC PIC X(5) VALUE "A B C".
01 WS-A PIC X.
01 WS-B PIC X.
01 WS-C PIC X.
PROCEDURE DIVISION.
MAIN.
UNSTRING WS-SRC DELIMITED BY SPACE INTO WS-A WS-B WS-C.
DISPLAY WS-A WS-B WS-C.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("VmProcedureOp::UnstringOp"));
    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ABC\n"));
}

#[test]
fn cobol2rust_runs_unstring_core_delimited_by_literal() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("unstring-literal.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. UNSTRLT.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-SRC PIC X(5) VALUE "A,B,C".
01 WS-A PIC X.
01 WS-B PIC X.
01 WS-C PIC X.
PROCEDURE DIVISION.
MAIN.
UNSTRING WS-SRC DELIMITED BY "," INTO WS-A WS-B WS-C.
DISPLAY WS-A WS-B WS-C.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("ABC\n"));
}

#[test]
fn cobol2rust_runs_move_corresponding_basic_through_vm() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("move-corr-basic.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. MVCORR.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 SRC-GROUP.
   05 A PIC X VALUE "Q".
   05 B PIC 99 VALUE 42.
   05 ONLY-SRC PIC X VALUE "Z".
01 DST-GROUP.
   05 B PIC 99.
   05 A PIC X.
   05 ONLY-DST PIC X VALUE "D".
PROCEDURE DIVISION.
MAIN.
MOVE CORRESPONDING SRC-GROUP TO DST-GROUP.
DISPLAY A OF DST-GROUP B OF DST-GROUP ONLY-DST OF DST-GROUP.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Q42D\n"));
}

#[test]
fn cobol2rust_runs_move_corr_nested_and_padding_through_vm() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("move-corr-nested.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. MVCORRN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 SRC-GROUP.
   05 SRC-NEST.
      10 CODE PIC X VALUE "Z".
   05 NAME PIC X(3) VALUE "ABC".
01 DST-GROUP.
   05 DST-NEST.
      10 CODE PIC X VALUE "A".
   05 NAME PIC X(5) VALUE "-----".
PROCEDURE DIVISION.
MAIN.
MOVE CORR SRC-GROUP TO DST-GROUP.
DISPLAY CODE OF DST-NEST OF DST-GROUP.
IF NAME OF DST-GROUP = "ABC  " DISPLAY "PADDED" ELSE DISPLAY "BAD".
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Z\nPADDED\n"));
}

#[test]
fn cobol2rust_runs_renames_alias_through_vm_storage() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("renames-alias.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. RENALIAS.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 BLOB.
   05 HEAD PIC XX VALUE "AB".
   05 TAIL PIC XX VALUE "CD".
66 BLOB-TAIL RENAMES TAIL.
PROCEDURE DIVISION.
MAIN.
DISPLAY BLOB-TAIL.
MOVE "XY" TO BLOB-TAIL.
DISPLAY BLOB.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("CD\nABXY"));
}

#[test]
fn cobol2rust_runs_same_record_area_for_fd_records() {
    let dir = tempdir().expect("tempdir");
    let input_file = dir.path().join("shared.dat");
    fs::write(&input_file, "QZ\n").expect("write data");
    let input = dir.path().join("same-record-area.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        format!(
            r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SAMEAREA.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT IN-FILE ASSIGN TO "{}".
    SELECT OUT-FILE ASSIGN TO "unused.dat".
I-O-CONTROL.
    SAME RECORD AREA FOR IN-FILE OUT-FILE.
DATA DIVISION.
FILE SECTION.
FD IN-FILE.
01 IN-REC PIC XX.
FD OUT-FILE.
01 OUT-REC PIC XX.
PROCEDURE DIVISION.
MAIN.
OPEN INPUT IN-FILE.
READ IN-FILE AT END DISPLAY "BAD".
DISPLAY OUT-REC.
STOP RUN.
"#,
            input_file.display()
        ),
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("QZ"));
}

#[test]
fn cobol2rust_blocks_ambiguous_move_corresponding_names() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("move-corr-ambiguous.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. MVCORRBLK.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 SRC-GROUP.
   05 LEFT-SIDE.
      10 ID PIC X VALUE "A".
   05 RIGHT-SIDE.
      10 ID PIC X VALUE "B".
01 DST-GROUP.
   05 ID PIC X.
PROCEDURE DIVISION.
MAIN.
MOVE CORRESPONDING SRC-GROUP TO DST-GROUP.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migration blocked"));

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "E_UNSUPPORTED_MOVE_CORRESPONDING");
}

#[test]
fn cobol2rust_runs_compute_expression_through_vm() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("compute-basic.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. COMPBASIC.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-A PIC 99 VALUE 4.
01 WS-B PIC 99 VALUE 2.
01 WS-N PIC 99.
PROCEDURE DIVISION.
MAIN.
COMPUTE WS-N = WS-A + WS-B * 3.
DISPLAY WS-N.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    assert_generated_stdout_exact(&out, "10\n");
}

#[test]
fn cobol2rust_runs_compute_into_packed_decimal_target() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("compute-packed.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. COMPPACK.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-N PIC S9(3) COMP-3.
PROCEDURE DIVISION.
MAIN.
COMPUTE WS-N = 120 + 3
    ON SIZE ERROR DISPLAY "BAD"
    NOT ON SIZE ERROR DISPLAY "OK"
END-COMPUTE.
DISPLAY WS-N.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    assert_generated_stdout_exact(&out, "OK\n123\n");
}

#[test]
fn cobol2rust_runs_packed_compute_size_error_without_clobbering_target() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("compute-packed-size.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. COMPPKSZ.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-N PIC S9(3) COMP-3.
PROCEDURE DIVISION.
MAIN.
MOVE 123 TO WS-N.
COMPUTE WS-N = 1234
    ON SIZE ERROR DISPLAY "SIZE"
    NOT ON SIZE ERROR DISPLAY "BAD"
END-COMPUTE.
DISPLAY WS-N.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    assert_generated_stdout_exact(&out, "SIZE\n123\n");
}

#[test]
fn cobol2rust_runs_compute_not_on_size_error_branch() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("compute-not-size.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. COMPNOT.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-N PIC 99.
PROCEDURE DIVISION.
MAIN.
COMPUTE WS-N = 2 + 3
    ON SIZE ERROR DISPLAY "BAD"
    NOT ON SIZE ERROR DISPLAY "OK"
END-COMPUTE.
DISPLAY WS-N.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    assert_generated_stdout_exact(&out, "OK\n05\n");
}

#[test]
fn cobol2rust_runs_compute_on_size_error_branch_without_clobbering_target() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("compute-size.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. COMPSIZE.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-N PIC 99 VALUE 7.
PROCEDURE DIVISION.
MAIN.
COMPUTE WS-N = 99 + 1
    ON SIZE ERROR DISPLAY "SIZE"
    NOT ON SIZE ERROR DISPLAY "BAD"
END-COMPUTE.
DISPLAY WS-N.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    assert_generated_stdout_exact(&out, "SIZE\n07\n");
}

#[test]
fn cobol2rust_runs_multiple_statements_in_compute_size_error_branch() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("compute-size-multi.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. COMPSIZM.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-N PIC 99 VALUE 7.
PROCEDURE DIVISION.
MAIN.
COMPUTE WS-N = 99 + 1
    ON SIZE ERROR DISPLAY "SIZE" DISPLAY "SECOND"
    NOT ON SIZE ERROR DISPLAY "BAD"
END-COMPUTE.
DISPLAY WS-N.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    assert_generated_stdout_exact(&out, "SIZE\nSECOND\n07\n");
}

#[test]
fn cobol2rust_runs_compute_divide_by_zero_as_size_error() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("compute-div0.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. COMPDIV0.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-N PIC 99 VALUE 8.
01 WS-Z PIC 9 VALUE 0.
PROCEDURE DIVISION.
MAIN.
COMPUTE WS-N = 1 / WS-Z
    ON SIZE ERROR DISPLAY "DIV0"
END-COMPUTE.
DISPLAY WS-N.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    assert_generated_stdout_exact(&out, "DIV0\n08\n");
}

#[test]
fn cobol2rust_runs_string_with_pointer_and_not_on_overflow() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("string-pointer.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. STRPTR.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TEXT PIC X(5) VALUE "-----".
01 WS-PTR PIC 99.
PROCEDURE DIVISION.
MAIN.
MOVE 3 TO WS-PTR.
STRING "AB" DELIMITED BY SIZE INTO WS-TEXT WITH POINTER WS-PTR
    ON OVERFLOW DISPLAY "BAD"
    NOT ON OVERFLOW DISPLAY "OK"
END-STRING.
DISPLAY WS-TEXT.
DISPLAY WS-PTR.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    assert_generated_stdout_exact(&out, "OK\n--AB-\n05\n");
}

#[test]
fn cobol2rust_runs_string_on_overflow_branch() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("string-overflow.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. STROVF.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TEXT PIC X(2) VALUE "--".
01 WS-PTR PIC 99 VALUE 2.
PROCEDURE DIVISION.
MAIN.
STRING "ABC" DELIMITED BY SIZE INTO WS-TEXT WITH POINTER WS-PTR
    ON OVERFLOW DISPLAY "OV"
    NOT ON OVERFLOW DISPLAY "BAD"
END-STRING.
DISPLAY WS-TEXT.
DISPLAY WS-PTR.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    assert_generated_stdout_exact(&out, "OV\n-A\n03\n");
}

#[test]
fn cobol2rust_runs_unstring_all_count_tallying_and_pointer() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("unstring-all.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. UNSTRALL.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-SRC PIC X(7) VALUE "A,,B,,C".
01 WS-A PIC X.
01 WS-B PIC X.
01 WS-C PIC X.
01 WS-C1 PIC 9.
01 WS-C2 PIC 9.
01 WS-C3 PIC 9.
01 WS-PTR PIC 99 VALUE 1.
01 WS-TALLY PIC 9.
PROCEDURE DIVISION.
MAIN.
UNSTRING WS-SRC DELIMITED BY ALL "," INTO
    WS-A COUNT IN WS-C1
    WS-B COUNT IN WS-C2
    WS-C COUNT IN WS-C3
    WITH POINTER WS-PTR
    TALLYING IN WS-TALLY
END-UNSTRING.
DISPLAY WS-A WS-B WS-C.
DISPLAY WS-C1 WS-C2 WS-C3.
DISPLAY WS-PTR.
DISPLAY WS-TALLY.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    assert_generated_stdout_exact(&out, "ABC\n111\n08\n3\n");
}

#[test]
fn cobol2rust_runs_unstring_on_overflow_branch() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("unstring-overflow.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. UNSTROVF.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-SRC PIC X(5) VALUE "A,B,C".
01 WS-A PIC X.
01 WS-B PIC X.
PROCEDURE DIVISION.
MAIN.
UNSTRING WS-SRC DELIMITED BY "," INTO WS-A WS-B
    ON OVERFLOW DISPLAY "OV"
    NOT ON OVERFLOW DISPLAY "BAD"
END-UNSTRING.
DISPLAY WS-A WS-B.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("OV\nAB\n"))
        .stdout(predicate::str::contains("BAD").not());
}

#[test]
fn cobol2rust_reports_precise_call_blockers_without_generic_unsupported_call() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("call-blockers.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLMAIN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-PROG PIC X(7) VALUE "SUBPROG".
01 WS-ARG PIC X VALUE "A".
PROCEDURE DIVISION.
MAIN.
CALL WS-PROG.
CALL "SUBPROG" USING WS-ARG.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migration blocked"));

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "W_DYNAMIC_CALL_RUNTIME_CHECK");
    assert_report_contains_json_string(&report, "E_UNSUPPORTED_CALL_USING");
    assert_report_contains_json_string(&report, "E_UNRESOLVED_CALL_TARGET");
    assert!(!json_contains_string(
        &report,
        "unsupported COBOL statement: CALL"
    ));
}

#[test]
fn cobol2rust_compiles_dynamic_call_to_linked_program() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("dynamic-call.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLMAIN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-PROG PIC X(7) VALUE "SUBPROG".
PROCEDURE DIVISION.
MAIN.
    CALL WS-PROG.
    STOP RUN.
END PROGRAM CALLMAIN.

IDENTIFICATION DIVISION.
PROGRAM-ID. SUBPROG.
PROCEDURE DIVISION.
SUBMAIN.
    DISPLAY "DYN".
    GOBACK.
END PROGRAM SUBPROG.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("VmCallTarget::Dynamic"));

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--quiet", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("DYN"));
}

#[test]
fn cobol2rust_dynamic_call_using_binds_linkage_by_reference() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("dynamic-call-using.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLMAIN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-PROG PIC X(7) VALUE "SUBPROG".
01 WS-FLAG PIC X VALUE "N".
PROCEDURE DIVISION.
MAIN.
    CALL WS-PROG USING WS-FLAG.
    DISPLAY WS-FLAG.
    STOP RUN.
END PROGRAM CALLMAIN.

IDENTIFICATION DIVISION.
PROGRAM-ID. SUBPROG.
DATA DIVISION.
LINKAGE SECTION.
01 LK-FLAG PIC X.
PROCEDURE DIVISION USING LK-FLAG.
SUBMAIN.
    MOVE "Y" TO LK-FLAG.
    GOBACK.
END PROGRAM SUBPROG.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--quiet", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Y"));
}

#[test]
fn cobol2rust_dynamic_call_respects_initial_lifecycle() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("dynamic-call-initial.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLMAIN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-PROG PIC X(7) VALUE "SUBPROG".
PROCEDURE DIVISION.
MAIN.
    CALL WS-PROG.
    CALL WS-PROG.
    STOP RUN.
END PROGRAM CALLMAIN.

IDENTIFICATION DIVISION.
PROGRAM-ID. SUBPROG IS INITIAL.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FLAG PIC X VALUE "A".
PROCEDURE DIVISION.
SUBMAIN.
    DISPLAY WS-FLAG.
    MOVE "B" TO WS-FLAG.
    GOBACK.
END PROGRAM SUBPROG.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--quiet", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("A\nA"));
}

#[test]
fn cobol2rust_multi_program_duplicate_working_storage_names_are_isolated() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("duplicate-ws-names.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLMAIN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-PROG PIC X(7) VALUE "SUBPROG".
01 WS-FLAG PIC X VALUE "M".
PROCEDURE DIVISION.
MAIN.
    DISPLAY WS-FLAG.
    CALL WS-PROG.
    DISPLAY WS-FLAG.
    STOP RUN.
END PROGRAM CALLMAIN.

IDENTIFICATION DIVISION.
PROGRAM-ID. SUBPROG.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FLAG PIC X VALUE "S".
PROCEDURE DIVISION.
SUBMAIN.
    DISPLAY WS-FLAG.
    MOVE "Z" TO WS-FLAG.
    DISPLAY WS-FLAG.
    GOBACK.
END PROGRAM SUBPROG.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--quiet", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("M\nS\nZ\nM"));
}

#[test]
fn cobol2rust_multi_program_duplicate_odo_and_index_names_are_isolated() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("duplicate-odo-index.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLMAIN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-PROG PIC X(7) VALUE "SUBPROG".
01 ODO-COUNT PIC 9 VALUE 1.
01 WS-ITEM OCCURS 0 TO 3 DEPENDING ON ODO-COUNT INDEXED BY WS-IDX PIC X.
PROCEDURE DIVISION.
MAIN.
    SET WS-IDX TO 1.
    MOVE "M" TO WS-ITEM(WS-IDX).
    CALL WS-PROG.
    DISPLAY ODO-COUNT.
    SET WS-IDX TO 1.
    DISPLAY WS-ITEM(WS-IDX).
    STOP RUN.
END PROGRAM CALLMAIN.

IDENTIFICATION DIVISION.
PROGRAM-ID. SUBPROG.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 ODO-COUNT PIC 9 VALUE 1.
01 WS-ITEM OCCURS 0 TO 3 DEPENDING ON ODO-COUNT INDEXED BY WS-IDX PIC X.
PROCEDURE DIVISION.
SUBMAIN.
    SET WS-IDX TO 1.
    MOVE "S" TO WS-ITEM(WS-IDX).
    MOVE 2 TO ODO-COUNT.
    SET WS-IDX TO 2.
    MOVE "Z" TO WS-ITEM(WS-IDX).
    DISPLAY ODO-COUNT.
    DISPLAY WS-ITEM(WS-IDX).
    GOBACK.
END PROGRAM SUBPROG.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--quiet", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("2\nZ\n1\nM"));
}

#[test]
fn cobol2rust_missing_dynamic_call_sets_program_status_and_continues() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("dynamic-call-missing.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLMAIN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-PROG PIC X(7) VALUE "MISSING".
PROCEDURE DIVISION.
MAIN.
    CALL WS-PROG.
    DISPLAY PROGRAM-STATUS.
    DISPLAY "AFTER".
    STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--quiet", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("01\nAFTER"));
}

#[test]
fn cobol2rust_dynamic_call_linkage_mismatch_sets_program_status() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("dynamic-call-mismatch.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLMAIN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-PROG PIC X(7) VALUE "SUBPROG".
PROCEDURE DIVISION.
MAIN.
    CALL WS-PROG.
    DISPLAY PROGRAM-STATUS.
    DISPLAY "AFTER".
    STOP RUN.
END PROGRAM CALLMAIN.

IDENTIFICATION DIVISION.
PROGRAM-ID. SUBPROG.
DATA DIVISION.
LINKAGE SECTION.
01 LK-FLAG PIC X.
PROCEDURE DIVISION USING LK-FLAG.
SUBMAIN.
    DISPLAY "SHOULD-NOT-RUN".
    GOBACK.
END PROGRAM SUBPROG.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--quiet", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("02\nAFTER"))
        .stdout(predicate::str::contains("SHOULD-NOT-RUN").not());
}

#[test]
fn cobol2rust_compiles_static_call_using_linkage_between_programs() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("call-using-multi.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLMAIN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FLAG PIC X VALUE "N".
PROCEDURE DIVISION.
MAIN.
    CALL "SUBPROG" USING WS-FLAG.
    DISPLAY WS-FLAG.
    STOP RUN.
END PROGRAM CALLMAIN.

IDENTIFICATION DIVISION.
PROGRAM-ID. SUBPROG.
DATA DIVISION.
LINKAGE SECTION.
01 LK-FLAG PIC X.
PROCEDURE DIVISION USING LK-FLAG.
SUBMAIN.
    MOVE "Y" TO LK-FLAG.
    DISPLAY LK-FLAG.
    GOBACK.
END PROGRAM SUBPROG.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("insert_with_lifecycle_descriptors(\"SUBPROG\""));
    assert!(program_rs.contains("VmProcedureOp::Call"));
    assert!(!program_rs.contains("VmStorage"));

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Y\nY"));
}

#[test]
fn cobol2rust_compiles_group_call_using_linkage_by_reference() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("call-using-group.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLMAIN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-GRP.
   05 WS-A PIC X VALUE "A".
   05 WS-B PIC X VALUE "B".
PROCEDURE DIVISION.
MAIN.
    CALL "SUBPROG" USING WS-GRP.
    DISPLAY WS-A.
    DISPLAY WS-B.
    STOP RUN.
END PROGRAM CALLMAIN.

IDENTIFICATION DIVISION.
PROGRAM-ID. SUBPROG.
DATA DIVISION.
LINKAGE SECTION.
01 LK-GRP.
   05 LK-A PIC X.
   05 LK-B PIC X.
PROCEDURE DIVISION USING LK-GRP.
SUBMAIN.
    MOVE "Z" TO LK-A.
    MOVE "Q" TO LK-B.
    DISPLAY LK-A.
    DISPLAY LK-B.
    GOBACK.
END PROGRAM SUBPROG.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("VmLinkageChild"));

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Z\nQ\nZ\nQ"));
}

#[test]
fn cobol2rust_compiles_odo_scalar_call_using_linkage_by_reference() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("call-using-odo.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLMAIN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 TAB-ITEM PIC X OCCURS 2.
PROCEDURE DIVISION.
MAIN.
    CALL "SUBPROG" USING TAB-ITEM(2).
    IF TAB-ITEM(2) = "Z" DISPLAY "OK".
    STOP RUN.
END PROGRAM CALLMAIN.

IDENTIFICATION DIVISION.
PROGRAM-ID. SUBPROG.
DATA DIVISION.
LINKAGE SECTION.
01 LK-ITEM PIC X.
PROCEDURE DIVISION USING LK-ITEM.
SUBMAIN.
    MOVE "Z" TO LK-ITEM.
    DISPLAY LK-ITEM.
    GOBACK.
END PROGRAM SUBPROG.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Z\nOK"));
}

#[test]
fn cobol2rust_compiles_call_using_conversion_via_temp_cell() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("call-using-conversion.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLMAIN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-NUM PIC 9 VALUE 1.
PROCEDURE DIVISION.
MAIN.
    CALL "SUBPROG" USING WS-NUM.
    DISPLAY WS-NUM.
    STOP RUN.
END PROGRAM CALLMAIN.

IDENTIFICATION DIVISION.
PROGRAM-ID. SUBPROG.
DATA DIVISION.
LINKAGE SECTION.
01 LK-TEXT PIC X.
PROCEDURE DIVISION USING LK-TEXT.
SUBMAIN.
    DISPLAY LK-TEXT.
    MOVE "9" TO LK-TEXT.
    DISPLAY LK-TEXT.
    GOBACK.
END PROGRAM SUBPROG.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("__CALL_TMP_CALLMAIN_SUBPROG"));

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1\n9\n1"));
}

#[test]
fn cobol2rust_blocks_unsupported_call_using_conversion_shapes() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("call-using-packed-conversion.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLMAIN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TEXT PIC X VALUE "1".
PROCEDURE DIVISION.
MAIN.
    CALL "SUBPROG" USING WS-TEXT.
    STOP RUN.
END PROGRAM CALLMAIN.

IDENTIFICATION DIVISION.
PROGRAM-ID. SUBPROG.
DATA DIVISION.
LINKAGE SECTION.
01 LK-NUM PIC 9 COMP-3.
PROCEDURE DIVISION USING LK-NUM.
SUBMAIN.
    DISPLAY LK-NUM.
    GOBACK.
END PROGRAM SUBPROG.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migration blocked"));

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "E_UNSUPPORTED_CALL_USING_CONVERSION");
}

#[test]
fn cobol2rust_compiles_external_scalar_shared_between_programs() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("external-scalar.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLMAIN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 EXT-FLAG PIC X EXTERNAL VALUE "A".
PROCEDURE DIVISION.
MAIN.
    CALL "SUBPROG".
    DISPLAY EXT-FLAG.
    STOP RUN.
END PROGRAM CALLMAIN.

IDENTIFICATION DIVISION.
PROGRAM-ID. SUBPROG.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 EXT-FLAG PIC X EXTERNAL.
PROCEDURE DIVISION.
SUBMAIN.
    MOVE "Z" TO EXT-FLAG.
    DISPLAY EXT-FLAG.
    GOBACK.
END PROGRAM SUBPROG.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("StorageKey::external(\"EXT_FLAG\")"));

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "\"external_items\"");
    assert_report_contains_json_string(&report, "EXT_FLAG");

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Z\nZ"));
}

#[test]
fn cobol2rust_blocks_external_type_mismatch_between_programs() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("external-mismatch.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLMAIN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 EXT-FLAG PIC X EXTERNAL.
PROCEDURE DIVISION.
MAIN.
    CALL "SUBPROG".
    STOP RUN.
END PROGRAM CALLMAIN.

IDENTIFICATION DIVISION.
PROGRAM-ID. SUBPROG.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 EXT-FLAG PIC 99 EXTERNAL.
PROCEDURE DIVISION.
SUBMAIN.
    GOBACK.
END PROGRAM SUBPROG.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migration blocked"));

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "E_EXTERNAL_TYPE_MISMATCH");
}

#[test]
fn cobol2rust_common_subprogram_working_storage_persists_between_calls() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("common-lifecycle.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLMAIN.
PROCEDURE DIVISION.
MAIN.
    CALL "SUBPROG".
    CALL "SUBPROG".
    STOP RUN.
END PROGRAM CALLMAIN.

IDENTIFICATION DIVISION.
PROGRAM-ID. SUBPROG IS COMMON.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-COUNT PIC 9 VALUE 0.
PROCEDURE DIVISION.
SUBMAIN.
    ADD 1 TO WS-COUNT.
    DISPLAY WS-COUNT.
    GOBACK.
END PROGRAM SUBPROG.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "\"is_common\": true");

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1\n2"));
}

#[test]
fn cobol2rust_initial_subprogram_working_storage_resets_between_calls() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("initial-lifecycle.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLMAIN.
PROCEDURE DIVISION.
MAIN.
    CALL "SUBPROG".
    CALL "SUBPROG".
    STOP RUN.
END PROGRAM CALLMAIN.

IDENTIFICATION DIVISION.
PROGRAM-ID. SUBPROG IS INITIAL.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-COUNT PIC 9 VALUE 0.
PROCEDURE DIVISION.
SUBMAIN.
    ADD 1 TO WS-COUNT.
    DISPLAY WS-COUNT.
    GOBACK.
END PROGRAM SUBPROG.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("insert_with_lifecycle_descriptors"));

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "\"is_initial\": true");

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1\n1"));
}

#[test]
fn cobol2rust_initial_subprogram_does_not_reset_external_storage() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("initial-external.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLMAIN.
PROCEDURE DIVISION.
MAIN.
    CALL "SUBPROG".
    CALL "SUBPROG".
    STOP RUN.
END PROGRAM CALLMAIN.

IDENTIFICATION DIVISION.
PROGRAM-ID. SUBPROG IS INITIAL.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-LOCAL PIC X VALUE "L".
01 EXT-FLAG PIC X EXTERNAL VALUE "A".
PROCEDURE DIVISION.
SUBMAIN.
    DISPLAY WS-LOCAL.
    DISPLAY EXT-FLAG.
    MOVE "X" TO WS-LOCAL.
    MOVE "Z" TO EXT-FLAG.
    GOBACK.
END PROGRAM SUBPROG.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("L\nA\nL\nZ"));
}

#[test]
fn cobol2rust_initial_subprogram_resets_file_record_lifecycle_storage() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("initial-file-record.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLMAIN.
PROCEDURE DIVISION.
MAIN.
    CALL "SUBPROG".
    CALL "SUBPROG".
    STOP RUN.
END PROGRAM CALLMAIN.

IDENTIFICATION DIVISION.
PROGRAM-ID. SUBPROG IS INITIAL.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT INFILE ASSIGN TO "INFILE".
DATA DIVISION.
FILE SECTION.
FD INFILE.
01 IN-REC PIC X(5).
PROCEDURE DIVISION.
SUBMAIN.
    IF IN-REC = "     " DISPLAY "RESET".
    MOVE "HELLO" TO IN-REC.
    DISPLAY IN-REC.
    GOBACK.
END PROGRAM SUBPROG.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("__initial_files_"));
    assert!(program_rs.contains("insert_with_lifecycle_descriptors"));

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("RESET\nHELLO\nRESET\nHELLO"));
}

#[test]
fn cobol2rust_initial_subprogram_does_not_reset_external_file_status() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("initial-external-file-status.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLMAIN.
PROCEDURE DIVISION.
MAIN.
    CALL "SUBPROG".
    CALL "SUBPROG".
    STOP RUN.
END PROGRAM CALLMAIN.

IDENTIFICATION DIVISION.
PROGRAM-ID. SUBPROG IS INITIAL.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT INFILE ASSIGN TO "INFILE"
        ORGANIZATION IS SEQUENTIAL
        ACCESS MODE IS SEQUENTIAL
        FILE STATUS IS EXT-FS.
DATA DIVISION.
FILE SECTION.
FD INFILE.
01 IN-REC PIC X(5).
WORKING-STORAGE SECTION.
01 EXT-FS PIC X(2) EXTERNAL VALUE "AA".
PROCEDURE DIVISION.
SUBMAIN.
    DISPLAY EXT-FS.
    OPEN INPUT INFILE.
    READ INFILE AT END DISPLAY "EOF".
    DISPLAY EXT-FS.
    GOBACK.
END PROGRAM SUBPROG.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("bind_file_status"));
    assert!(program_rs.contains("StorageKey::external(\"EXT_FS\")"));
    assert!(program_rs.contains("__initial_files_"));

    fs::write(out.join("INFILE"), b"").expect("empty assigned input file");

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("AA\nEOF\n10\n10\nEOF\n10"));
}

#[test]
fn cobol2rust_runs_multiple_statements_in_read_at_end_branch() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("read-at-end-multi.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. READMULTI.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT INFILE ASSIGN TO "INFILE"
        ORGANIZATION IS SEQUENTIAL
        ACCESS MODE IS SEQUENTIAL.
DATA DIVISION.
FILE SECTION.
FD INFILE.
01 IN-REC PIC X.
PROCEDURE DIVISION.
MAIN.
    OPEN INPUT INFILE.
    READ INFILE AT END DISPLAY "A" DISPLAY "B" END-READ.
    DISPLAY "AFTER".
    CLOSE INFILE.
    STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    fs::write(out.join("INFILE"), b"").expect("empty assigned input file");
    assert_generated_stdout_exact(&out, "A\nB\nAFTER\n");
}

#[test]
fn cobol2rust_initial_subprogram_resets_local_odo_count_and_cells() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("initial-odo.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLMAIN.
PROCEDURE DIVISION.
MAIN.
    CALL "SUBPROG".
    CALL "SUBPROG".
    STOP RUN.
END PROGRAM CALLMAIN.

IDENTIFICATION DIVISION.
PROGRAM-ID. SUBPROG IS INITIAL.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 ODO-COUNT PIC 9 VALUE 1.
01 TAB-ITEM PIC X OCCURS 0 TO 3 DEPENDING ON ODO-COUNT VALUE "A".
PROCEDURE DIVISION.
SUBMAIN.
    DISPLAY ODO-COUNT.
    IF TAB-ITEM(1) = " " DISPLAY "ONE".
    MOVE 2 TO ODO-COUNT.
    IF TAB-ITEM(2) = " " DISPLAY "TWO".
    IF TAB-ITEM(2) = "A" DISPLAY "TEMPLATE".
    GOBACK.
END PROGRAM SUBPROG.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1\nTWO\n1\nTEMPLATE"));
}

#[test]
fn cobol2rust_common_subprogram_retains_local_odo_count_and_cells() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("common-odo.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLMAIN.
PROCEDURE DIVISION.
MAIN.
    CALL "SUBPROG".
    CALL "SUBPROG".
    STOP RUN.
END PROGRAM CALLMAIN.

IDENTIFICATION DIVISION.
PROGRAM-ID. SUBPROG IS COMMON.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 ODO-COUNT PIC 9 VALUE 1.
01 TAB-ITEM PIC X OCCURS 0 TO 3 DEPENDING ON ODO-COUNT VALUE "A".
PROCEDURE DIVISION.
SUBMAIN.
    DISPLAY ODO-COUNT.
    IF ODO-COUNT = 2 DISPLAY "KEPT".
    MOVE 2 TO ODO-COUNT.
    IF TAB-ITEM(2) = " " DISPLAY "CELL".
    GOBACK.
END PROGRAM SUBPROG.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1\nCELL\n2\nKEPT\nCELL"));
}

#[test]
fn cobol2rust_blocks_common_initial_conflict() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("common-initial-conflict.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLMAIN.
PROCEDURE DIVISION.
MAIN.
    CALL "SUBPROG".
    STOP RUN.
END PROGRAM CALLMAIN.

IDENTIFICATION DIVISION.
PROGRAM-ID. SUBPROG IS COMMON INITIAL.
PROCEDURE DIVISION.
SUBMAIN.
    GOBACK.
END PROGRAM SUBPROG.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migration blocked"));

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "E_COMMON_INITIAL_CONFLICT");
}

#[test]
fn cobol2rust_rejects_invalid_dialect_instead_of_defaulting() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("hello.cbl");
    fs::write(
        &input,
        "IDENTIFICATION DIVISION.\nPROGRAM-ID. HELLO.\nPROCEDURE DIVISION.\nMAIN.\nSTOP RUN.\n",
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            dir.path().join("generated").to_str().expect("utf8 path"),
            "--dialect",
            "whatever",
            "--source-format",
            "free",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid --dialect"));
}

#[test]
fn cobol2rust_blocks_unresolved_data_references() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("bad-ref.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        "IDENTIFICATION DIVISION.\nPROGRAM-ID. BADREF.\nPROCEDURE DIVISION.\nMAIN.\nDISPLAY MISSING-FIELD.\nSTOP RUN.\n",
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migration blocked"));

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "E_UNRESOLVED_DATA");
    assert_report_contains_json_string(&report, "\"semantic\"");
    assert_report_contains_json_string(&report, "\"status\": \"Missing\"");
}

#[test]
fn cobol2rust_reports_condition_ir_and_type_errors() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("condition-mismatch.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CONDMISMATCH.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-NUM PIC 9(3).
01 WS-TEXT PIC X(3).
PROCEDURE DIVISION.
MAIN.
IF WS-NUM = WS-TEXT.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migration blocked"));

    let report = read_migration_report(&out);
    assert!(
        report
            .pointer("/semantic/conditions")
            .and_then(Value::as_array)
            .is_some_and(|conditions| !conditions.is_empty()),
        "migration report missing semantic conditions: {report:#}"
    );
    assert_report_contains_json_string(&report, "\"Relation\"");
    assert_report_contains_json_string(&report, "E_CONDITION_TYPE_MISMATCH");
    assert!(!out.join("src/program.rs").exists());
}

#[test]
fn cobol2rust_expands_copy_replacing() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("copy.cbl");
    let copybook_dir = dir.path().join("copybooks");
    fs::create_dir_all(&copybook_dir).expect("copybook dir");
    fs::write(copybook_dir.join("REC.cpy"), "01 WS-NAME PIC X(10).\n").expect("copybook");
    fs::write(
        &input,
        "IDENTIFICATION DIVISION.\nPROGRAM-ID. COPYBAD.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\nCOPY REC REPLACING ==WS-NAME== BY ==WS-OTHER==.\nPROCEDURE DIVISION.\nMAIN.\nSTOP RUN.\n",
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--copybook-dir",
            copybook_dir.to_str().expect("utf8 path"),
            "--out",
            dir.path().join("generated").to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("generated Rust project"));

    let data_rs = fs::read_to_string(dir.path().join("generated/src/data.rs")).expect("data.rs");
    assert!(data_rs.contains("WS_OTHER"));
    assert!(!data_rs.contains("WS_NAME"));
    let report = read_migration_report(&dir.path().join("generated"));
    assert_report_contains_json_string(&report, "\"storage_bytes\": 10");
}

#[test]
fn cobol2rust_reports_layout_model_for_redefines_without_guessing_procedure_codecs() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("layout.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. LAYOUT.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-AMOUNT PIC S9(7)V99 COMP-3.
   05 WS-ALT REDEFINES WS-AMOUNT PIC X(5).
PROCEDURE DIVISION.
MAIN.
DISPLAY WS-AMOUNT.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migration blocked"));

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "\"storage_bytes\": 5");
    assert_report_contains_json_string(&report, "E_CODEGEN_REDEFINES_REFERENCE");
    assert!(!out.join("src/program.rs").exists());
}

#[test]
fn cobol2rust_blocks_occurs_procedure_reference_until_subscripts_exist() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("occurs-ref.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. OCCREF.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-TABLE OCCURS 3 TIMES.
      10 WS-ITEM PIC X(2).
PROCEDURE DIVISION.
MAIN.
MOVE "A" TO WS-ITEM OF WS-TABLE.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migration blocked"));

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "\"storage_bytes\": 6");
    assert_report_contains_json_string(&report, "WS_TABLE.WS_ITEM");
    assert_report_contains_json_string(&report, "E_CODEGEN_OCCURS_REFERENCE");
    assert!(!out.join("src/program.rs").exists());
}

#[test]
fn cobol2rust_runs_subscripted_occurs_move_through_vm() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("subscript-ref.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SUBREF.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-TABLE OCCURS 3 TIMES.
      10 WS-ITEM PIC X(2).
PROCEDURE DIVISION.
MAIN.
MOVE "A" TO WS-ITEM(1).
DISPLAY WS-ITEM(1).
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "WS-ITEM(1)");
    assert!(!json_contains_string(&report, "E_CODEGEN_SUBSCRIPT"));

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--quiet", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("A"));
}

#[test]
fn cobol2rust_runs_qualified_condition_name_and_set() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("qualified-88.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. Q88.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 V1-STAT PIC X.
   88 OK VALUE "Y".
01 V2-STAT PIC X.
   88 OK VALUE "Z".
PROCEDURE DIVISION.
MAIN.
SET OK OF V1-STAT TO TRUE.
IF OK OF V1-STAT DISPLAY "V1".
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("V1"));
}

#[test]
fn cobol2rust_runs_reference_modification_function_length_and_fixed_occurs_conditions() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("deep-conditions.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. DEEPCOND.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 CHUNK-A PIC X(8).
01 WS-TABLE.
   05 WS-ITEM OCCURS 3 TIMES PIC X.
PROCEDURE DIVISION.
MAIN.
MOVE "HELLO   " TO CHUNK-A.
MOVE "ABC" TO WS-TABLE.
IF CHUNK-A(2:4) = "ELLO" DISPLAY "RM".
IF FUNCTION LENGTH (CHUNK-A) = 8 DISPLAY "LEN".
IF WS-ITEM(2) = "B" DISPLAY "OCC".
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("RM"))
        .stdout(predicate::str::contains("LEN"))
        .stdout(predicate::str::contains("OCC"));
}

#[test]
fn cobol2rust_runs_basic_sequential_file_write_through_vm() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("file-write.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FILEWRITE.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT OUTFILE ASSIGN TO "OUTFILE".
DATA DIVISION.
FILE SECTION.
FD OUTFILE.
01 OUT-REC PIC X(5).
PROCEDURE DIVISION.
MAIN.
OPEN OUTPUT OUTFILE.
MOVE "HELLO" TO OUT-REC.
WRITE OUT-REC.
CLOSE OUTFILE.
DISPLAY "DONE".
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "\"name\": \"OUTFILE\"");
    assert_report_contains_json_string(&report, "\"record_name\": \"OUT_REC\"");
    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("VmProcedureOp::OpenFile"));
    assert!(program_rs.contains("VmProcedureOp::WriteFile"));
    assert!(program_rs.contains("define_os_sequential_file"));

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("DONE"));
    assert_eq!(
        fs::read(out.join("OUTFILE")).expect("outfile bytes"),
        b"HELLO"
    );
}

#[test]
fn cobol2rust_runs_write_after_advancing_page_through_vm() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("file-write-page.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FILEPAGE.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT OUTFILE ASSIGN TO "OUTFILE".
DATA DIVISION.
FILE SECTION.
FD OUTFILE.
01 OUT-REC PIC X(5).
PROCEDURE DIVISION.
MAIN.
OPEN OUTPUT OUTFILE.
MOVE "HELLO" TO OUT-REC.
WRITE OUT-REC AFTER ADVANCING PAGE.
CLOSE OUTFILE.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success();
    assert_eq!(
        fs::read(out.join("OUTFILE")).expect("outfile bytes"),
        b"HELLO\x0C"
    );
}

#[test]
fn cobol2rust_runs_write_after_advancing_top_of_page_through_vm() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("file-write-top.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FILETOP.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT OUTFILE ASSIGN TO "OUTFILE".
DATA DIVISION.
FILE SECTION.
FD OUTFILE.
01 OUT-REC PIC X.
PROCEDURE DIVISION.
MAIN.
OPEN OUTPUT OUTFILE.
MOVE "A" TO OUT-REC.
WRITE OUT-REC AFTER ADVANCING TOP-OF-PAGE.
CLOSE OUTFILE.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success();
    assert_eq!(
        fs::read(out.join("OUTFILE")).expect("outfile bytes"),
        b"A\x0C"
    );
}

#[test]
fn cobol2rust_runs_fd_linage_with_line_advancing_through_vm() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("file-write-linage.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FILELIN.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT OUTFILE ASSIGN TO "OUTFILE".
DATA DIVISION.
FILE SECTION.
FD OUTFILE LINAGE IS 2 LINES.
01 OUT-REC PIC X.
PROCEDURE DIVISION.
MAIN.
OPEN OUTPUT OUTFILE.
MOVE "A" TO OUT-REC.
WRITE OUT-REC AFTER ADVANCING 1 LINE.
MOVE "B" TO OUT-REC.
WRITE OUT-REC AFTER ADVANCING 1 LINE.
MOVE "C" TO OUT-REC.
WRITE OUT-REC AFTER ADVANCING 1 LINE.
CLOSE OUTFILE.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success();
    assert_eq!(
        fs::read(out.join("OUTFILE")).expect("outfile bytes"),
        b"A\nB\x0CC\n"
    );
}

#[test]
fn cobol2rust_runs_sequential_read_at_end_and_file_status_through_vm() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("file-read-status.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FILEREAD.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT INFILE ASSIGN TO "INFILE"
        ORGANIZATION IS SEQUENTIAL
        ACCESS MODE IS SEQUENTIAL
        FILE STATUS IS FS.
DATA DIVISION.
FILE SECTION.
FD INFILE.
01 IN-REC PIC X(5).
WORKING-STORAGE SECTION.
01 FS PIC X(2).
PROCEDURE DIVISION.
MAIN.
OPEN OUTPUT INFILE.
MOVE "HELLO" TO IN-REC.
WRITE IN-REC.
CLOSE INFILE.
OPEN INPUT INFILE.
READ INFILE AT END DISPLAY "EOF".
DISPLAY IN-REC.
DISPLAY FS.
READ INFILE AT END DISPLAY "EOF".
DISPLAY FS.
CLOSE INFILE.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("bind_file_status"));
    assert!(program_rs.contains("at_end_ops: vec!"));
    assert!(program_rs.contains("VmProcedureOp::Display"));

    assert_generated_stdout_exact(&out, "HELLO\n00\nEOF\n10\n");
}

#[test]
fn cobol2rust_runs_rerun_checkpoint_every_n_reads() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("rerun-checkpoint.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. RERUNCK.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT MASTER-FILE ASSIGN TO "MASTER".
    SELECT JUNK-FILE ASSIGN TO "JUNK".
I-O-CONTROL.
    RERUN ON JUNK-FILE EVERY 2 RECORDS OF MASTER-FILE.
DATA DIVISION.
FILE SECTION.
FD MASTER-FILE.
01 M-REC PIC XX.
FD JUNK-FILE.
01 J-REC PIC X(80).
PROCEDURE DIVISION.
MAIN.
OPEN INPUT MASTER-FILE.
OPEN EXTEND JUNK-FILE.
READ MASTER-FILE AT END DISPLAY "BAD".
READ MASTER-FILE AT END DISPLAY "BAD".
READ MASTER-FILE AT END DISPLAY "BAD".
CLOSE MASTER-FILE.
CLOSE JUNK-FILE.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("define_tape_file"));
    assert!(program_rs.contains("restore_last_rerun_checkpoint"));

    fs::write(out.join("MASTER"), b"A1B2C3").expect("master input");

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success();

    let checkpoint = fs::read(out.join("JUNK")).expect("junk output");
    assert!(checkpoint
        .windows(b"COBOLVM-TAPE-1".len())
        .any(|window| window == b"COBOLVM-TAPE-1"));
    assert!(checkpoint
        .windows(b"COBOLVMCKPT1".len())
        .any(|window| window == b"COBOLVMCKPT1"));
    assert!(checkpoint
        .windows(b"4D41535445525F46494C45".len())
        .any(|window| window == b"4D41535445525F46494C45"));
}

#[test]
fn cobol2rust_blocks_unsupported_rerun_shapes() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("rerun-unsupported.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. RERUNBAD.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
I-O-CONTROL.
    RERUN ON JUNK-FILE.
PROCEDURE DIVISION.
MAIN.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .failure();

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "E_UNSUPPORTED_ENVIRONMENT");
    assert_report_contains_json_string(&report, "RERUN");
}

#[test]
fn cobol2rust_reads_fixed_records_from_assigned_os_sequential_file() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("os-file-read.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. OSREAD.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT INFILE ASSIGN TO "input.dat"
        ORGANIZATION IS SEQUENTIAL
        ACCESS MODE IS SEQUENTIAL
        FILE STATUS IS FS.
DATA DIVISION.
FILE SECTION.
FD INFILE.
01 IN-REC.
   05 IN-TEXT PIC X(3).
WORKING-STORAGE SECTION.
01 FS PIC X(2).
PROCEDURE DIVISION.
MAIN.
OPEN INPUT INFILE.
READ INFILE AT END DISPLAY "EOF".
DISPLAY IN-TEXT.
READ INFILE AT END DISPLAY "EOF".
DISPLAY IN-TEXT.
READ INFILE AT END DISPLAY "EOF".
DISPLAY FS.
CLOSE INFILE.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("define_os_sequential_file"));
    assert!(program_rs
        .contains("define_os_sequential_file_with_record_len(\"INFILE\", \"input.dat\", 3)"));
    fs::write(out.join("input.dat"), b"AAABBB").expect("assigned input bytes");

    assert_generated_stdout_exact(&out, "AAA\nBBB\nEOF\n10\n");
}

#[test]
fn cobol2rust_writes_fixed_record_to_assigned_os_sequential_file() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("os-file-write.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. OSWRITE.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT OUTFILE ASSIGN TO "output.dat"
        ORGANIZATION IS SEQUENTIAL
        ACCESS MODE IS SEQUENTIAL.
DATA DIVISION.
FILE SECTION.
FD OUTFILE.
01 OUT-REC PIC X(3).
PROCEDURE DIVISION.
MAIN.
OPEN OUTPUT OUTFILE.
MOVE "XYZ" TO OUT-REC.
WRITE OUT-REC.
CLOSE OUTFILE.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success();
    assert_eq!(
        fs::read(out.join("output.dat")).expect("assigned output bytes"),
        b"XYZ"
    );
}

#[test]
fn cobol2rust_file_map_overrides_assign_external_name() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("file-map-read.cbl");
    let out = dir.path().join("generated");
    let mapped = dir.path().join("actual-input.dat");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. MAPREAD.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT INFILE ASSIGN TO "SYSIN"
        ORGANIZATION IS SEQUENTIAL
        ACCESS MODE IS SEQUENTIAL.
DATA DIVISION.
FILE SECTION.
FD INFILE.
01 IN-REC PIC X(3).
PROCEDURE DIVISION.
MAIN.
OPEN INPUT INFILE.
READ INFILE AT END DISPLAY "EOF".
DISPLAY IN-REC.
CLOSE INFILE.
STOP RUN.
"#,
    )
    .expect("write fixture");
    fs::write(&mapped, b"MAP").expect("mapped input bytes");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    fs::write(
        out.join("file-map.json"),
        format!("{{\"SYSIN\":\"{}\"}}", json_path(&mapped)),
    )
    .expect("file map");

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline", "--", "--file-map", "file-map.json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("MAP"));
}

#[test]
fn cobol2rust_runtime_config_overrides_assign_external_name() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("runtime-config-read.cbl");
    let out = dir.path().join("generated");
    let mapped = dir.path().join("actual-input.dat");
    let combined_mapped = dir.path().join("combined-map-input.dat");
    let default_mapped = dir.path().join("default-input.dat");
    let legacy_mapped = dir.path().join("legacy-default-input.dat");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. RUNTIMEREAD.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT INFILE ASSIGN TO "SYSIN"
        ORGANIZATION IS SEQUENTIAL
        ACCESS MODE IS SEQUENTIAL.
DATA DIVISION.
FILE SECTION.
FD INFILE.
01 IN-REC PIC X(3).
PROCEDURE DIVISION.
MAIN.
OPEN INPUT INFILE.
READ INFILE AT END DISPLAY "EOF".
DISPLAY IN-REC.
CLOSE INFILE.
STOP RUN.
"#,
    )
    .expect("write fixture");
    fs::write(&mapped, b"RTC").expect("mapped input bytes");
    fs::write(&combined_mapped, b"OVR").expect("combined mapped input bytes");
    fs::write(&default_mapped, b"DFT").expect("default mapped input bytes");
    fs::write(&legacy_mapped, b"LEG").expect("legacy default mapped input bytes");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    fs::write(
        out.join("runtime.json"),
        format!(
            "{{\"files\":[{{\"name\":\"INFILE\",\"path\":\"{}\",\"organization\":\"sequential\",\"record_format\":{{\"kind\":\"fixed\",\"record_len\":3}},\"disposition\":\"old\",\"encoding\":\"ascii\"}}]}}",
            json_path(&mapped)
        ),
    )
    .expect("runtime config");

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline", "--", "--runtime-config", "runtime.json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("RTC"));

    fs::write(
        out.join("override-file-map.json"),
        format!("{{\"SYSIN\":\"{}\"}}", json_path(&combined_mapped)),
    )
    .expect("override file map");

    Command::new("cargo")
        .current_dir(&out)
        .args([
            "run",
            "--offline",
            "--",
            "--runtime-config",
            "runtime.json",
            "--file-map",
            "override-file-map.json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("OVR"));

    fs::write(
        out.join("indexed-runtime.json"),
        format!(
            "{{\"files\":[{{\"name\":\"INFILE\",\"path\":\"{}\",\"organization\":\"indexed\",\"record_format\":{{\"kind\":\"fixed\",\"record_len\":3}},\"disposition\":\"old\",\"encoding\":\"ascii\"}}]}}",
            json_path(&mapped)
        ),
    )
    .expect("unsupported runtime config");

    Command::new("cargo")
        .current_dir(&out)
        .args([
            "run",
            "--offline",
            "--",
            "--runtime-config",
            "indexed-runtime.json",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unsupported organization"));

    fs::write(
        out.join("cobol-runtime.json"),
        format!(
            "{{\"files\":[{{\"name\":\"INFILE\",\"path\":\"{}\",\"organization\":\"sequential\",\"record_format\":{{\"kind\":\"fixed\",\"record_len\":3}},\"disposition\":\"old\",\"encoding\":\"ascii\"}}]}}",
            json_path(&default_mapped)
        ),
    )
    .expect("default runtime config");
    fs::write(
        out.join("cobol-file-map.json"),
        format!("{{\"SYSIN\":\"{}\"}}", json_path(&legacy_mapped)),
    )
    .expect("legacy default file map");

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("DFT"));
}

#[test]
fn cobol2rust_open_missing_assigned_file_sets_file_status() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("missing-file-status.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. MISSFS.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT INFILE ASSIGN TO "missing.dat"
        ORGANIZATION IS SEQUENTIAL
        ACCESS MODE IS SEQUENTIAL
        FILE STATUS IS FS.
DATA DIVISION.
FILE SECTION.
FD INFILE.
01 IN-REC PIC X(3).
WORKING-STORAGE SECTION.
01 FS PIC X(2).
PROCEDURE DIVISION.
MAIN.
OPEN INPUT INFILE.
DISPLAY FS.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("35"));
}

#[test]
fn cobol2rust_read_into_moves_os_file_record_to_working_storage() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("read-into.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. READINTO.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT INFILE ASSIGN TO "input.dat"
        ORGANIZATION IS SEQUENTIAL
        ACCESS MODE IS SEQUENTIAL.
DATA DIVISION.
FILE SECTION.
FD INFILE.
01 IN-REC.
   05 IN-TEXT PIC X(3).
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-TEXT PIC X(3).
PROCEDURE DIVISION.
MAIN.
OPEN INPUT INFILE.
READ INFILE INTO WS-REC AT END DISPLAY "EOF".
DISPLAY WS-TEXT.
CLOSE INFILE.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    fs::write(out.join("input.dat"), b"RIO").expect("assigned input bytes");

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("RIO"));
}

#[test]
fn cobol2rust_read_into_resizes_odo_target_from_fixed_record_length() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("read-into-odo.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. READODO.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT INFILE ASSIGN TO "input.dat"
        ORGANIZATION IS SEQUENTIAL
        ACCESS MODE IS SEQUENTIAL.
DATA DIVISION.
FILE SECTION.
FD INFILE.
01 IN-REC PIC X(3).
WORKING-STORAGE SECTION.
01 ODO-COUNT PIC 9 VALUE 1.
01 WS-REC.
   05 WS-ITEM PIC X OCCURS 0 TO 3 DEPENDING ON ODO-COUNT.
PROCEDURE DIVISION.
MAIN.
OPEN INPUT INFILE.
READ INFILE INTO WS-REC AT END DISPLAY "EOF".
DISPLAY ODO-COUNT.
DISPLAY WS-ITEM(1).
DISPLAY WS-ITEM(2).
DISPLAY WS-ITEM(3).
CLOSE INFILE.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    fs::write(out.join("input.dat"), b"ABC").expect("assigned input bytes");

    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("3\nA\nB\nC"));
}

#[test]
fn cobol2rust_blocks_read_into_odo_target_with_incompatible_length() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("read-into-odo-bad.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. READODOBAD.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT INFILE ASSIGN TO "input.dat"
        ORGANIZATION IS SEQUENTIAL
        ACCESS MODE IS SEQUENTIAL.
DATA DIVISION.
FILE SECTION.
FD INFILE.
01 IN-REC PIC X(4).
WORKING-STORAGE SECTION.
01 ODO-COUNT PIC 9 VALUE 1.
01 WS-REC.
   05 WS-ITEM PIC X OCCURS 0 TO 3 DEPENDING ON ODO-COUNT.
PROCEDURE DIVISION.
MAIN.
OPEN INPUT INFILE.
READ INFILE INTO WS-REC AT END DISPLAY "EOF".
CLOSE INFILE.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migration blocked"));

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "E_CODEGEN_FILE_IO");
    assert_report_contains_json_string(&report, "E_ODO_TARGET_INCOMPATIBLE_LENGTH");
    assert!(!out.join("src/program.rs").exists());
}

#[test]
fn cobol2rust_blocks_line_sequential_file_metadata_for_vm_slice() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("line-sequential-file.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. LINEFILE.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT INFILE ASSIGN TO "input.txt"
        ORGANIZATION IS LINE SEQUENTIAL
        ACCESS MODE IS SEQUENTIAL.
DATA DIVISION.
FILE SECTION.
FD INFILE.
01 IN-REC PIC X(5).
PROCEDURE DIVISION.
MAIN.
OPEN INPUT INFILE.
READ INFILE AT END DISPLAY "EOF".
CLOSE INFILE.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migration blocked"));

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "E_CODEGEN_FILE_IO");
    assert_report_contains_json_string(
        &report,
        "organization LINE SEQUENTIAL is not executable yet",
    );
    assert!(!out.join("src/program.rs").exists());
}

#[test]
fn cobol2rust_blocks_odo_file_records_for_os_sequential_slice() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("odo-file-record.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. ODOFILE.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT INFILE ASSIGN TO "input.dat"
        ORGANIZATION IS SEQUENTIAL
        ACCESS MODE IS SEQUENTIAL.
DATA DIVISION.
FILE SECTION.
FD INFILE.
01 IN-REC.
   05 REC-COUNT PIC 9.
   05 REC-ITEM PIC X OCCURS 0 TO 3 DEPENDING ON REC-COUNT.
PROCEDURE DIVISION.
MAIN.
OPEN INPUT INFILE.
READ INFILE AT END DISPLAY "EOF".
CLOSE INFILE.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migration blocked"));

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "E_CODEGEN_FILE_IO");
    assert_report_contains_json_string(&report, "contains OCCURS DEPENDING ON");
    assert!(!out.join("src/program.rs").exists());
}

#[test]
fn cobol2rust_runs_open_io_for_fixed_os_sequential_file_slice() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("open-io-file.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. OPENIO.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT INFILE ASSIGN TO "input.dat"
        ORGANIZATION IS SEQUENTIAL
        ACCESS MODE IS SEQUENTIAL.
DATA DIVISION.
FILE SECTION.
FD INFILE.
01 IN-REC PIC X(5).
PROCEDURE DIVISION.
MAIN.
OPEN I-O INFILE.
READ INFILE AT END DISPLAY "EOF".
DISPLAY IN-REC.
CLOSE INFILE.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    fs::write(out.join("input.dat"), b"HELLO").expect("assigned input bytes");
    Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .assert()
        .success()
        .stdout(predicate::str::contains("HELLO"))
        .stdout(predicate::str::contains("EOF").not());
}

#[test]
fn cobol2rust_blocks_dynamic_assign_for_os_sequential_slice() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("dynamic-assign-file.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. DYNASSIGN.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT INFILE ASSIGN TO WS-PATH
        ORGANIZATION IS SEQUENTIAL
        ACCESS MODE IS SEQUENTIAL.
DATA DIVISION.
FILE SECTION.
FD INFILE.
01 IN-REC PIC X(5).
WORKING-STORAGE SECTION.
01 WS-PATH PIC X(20) VALUE "input.dat".
PROCEDURE DIVISION.
MAIN.
OPEN INPUT INFILE.
READ INFILE AT END DISPLAY "EOF".
CLOSE INFILE.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migration blocked"));

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "E_CODEGEN_FILE_IO");
    assert_report_contains_json_string(&report, "dynamic ASSIGN is not executable yet");
    assert!(!out.join("src/program.rs").exists());
}

#[test]
fn cobol2rust_blocks_nonsequential_file_metadata_for_vm_slice() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("indexed-file.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. IDXFILE.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT IDXFILE ASSIGN TO "IDX"
        ORGANIZATION IS INDEXED
        ACCESS MODE IS RANDOM.
DATA DIVISION.
FILE SECTION.
FD IDXFILE.
01 IDX-REC PIC X(5).
PROCEDURE DIVISION.
MAIN.
OPEN INPUT IDXFILE.
READ IDXFILE AT END DISPLAY "EOF".
CLOSE IDXFILE.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migration blocked"));

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "E_CODEGEN_FILE_IO");
    assert_report_contains_json_string(&report, "organization INDEXED is not executable yet");
    assert_report_contains_json_string(&report, "access mode RANDOM is not executable yet");
    assert!(!out.join("src/program.rs").exists());
}

#[test]
fn cobol2rust_runs_odo_active_count_mutation_for_checked_condition_reads() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("odo-run.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. ODORUN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 ODO-COUNT PIC 9 VALUE 1.
01 TAB.
   05 ITEM OCCURS 0 TO 3 DEPENDING ON ODO-COUNT PIC X.
PROCEDURE DIVISION.
MAIN.
MOVE 2 TO ODO-COUNT.
IF ITEM(2) = " " DISPLAY "ODO".
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "\"odo\"");
    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("define_odo"));
    assert!(program_rs.contains("define_odo_table_with_templates"));
    assert!(program_rs.contains("bind_occurs_storage_cell"));
    assert!(!program_rs.contains("VmStorage"));
    assert!(!program_rs.contains("storage.bytes()"));

    assert_generated_stdout_exact(&out, "ODO\n");
}

#[test]
fn cobol2rust_runs_indexed_by_and_dynamic_reference_modifier_through_vm() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("index_refmod.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. INDEXREF.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-COUNT PIC 9.
01 FLAT PIC X(10) VALUE "ABCDEFGHIJ".
01 WS-TABLE.
   05 WS-ITEM OCCURS 3 TIMES INDEXED BY WS-IDX PIC X VALUE "A".
PROCEDURE DIVISION.
MAIN.
MOVE 2 TO WS-COUNT.
SET WS-IDX TO WS-COUNT.
IF WS-ITEM(WS-IDX) = " " DISPLAY "INDEX".
IF FLAT(WS-COUNT * 2 : 3) = "DEF" DISPLAY "REFMOD".
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "\"indexes\"");
    assert_report_contains_json_string(&report, "WS_IDX");
    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("runtime.define_index"));
    assert!(program_rs.contains("VmProcedureOp::SetIndex"));
    assert!(program_rs.contains("VmExpr::Multiply"));
    assert!(program_rs.contains("index_name: Some(\"WS_IDX\".to_string())"));

    assert_generated_stdout_exact(&out, "INDEX\nREFMOD\n");
}

#[test]
fn cobol2rust_runs_serial_search_through_vm_index_loop() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("search.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SEARCHRUN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TABLE.
   05 WS-ITEM OCCURS 3 TIMES INDEXED BY WS-IDX PIC X.
PROCEDURE DIVISION.
MAIN.
SET WS-IDX TO 1.
SEARCH WS-ITEM VARYING WS-IDX
    AT END DISPLAY "NONE"
    WHEN WS-ITEM(WS-IDX) = " " DISPLAY "FOUND"
END-SEARCH.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("VmProcedureOp::SearchSerial"));
    assert!(program_rs.contains("VmSearchWhen"));

    assert_generated_stdout_exact(&out, "FOUND\n");
}

#[test]
fn cobol2rust_blocks_search_all_without_declared_key() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("search-all.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SEARCHALL.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TABLE.
   05 WS-ITEM OCCURS 3 TIMES INDEXED BY WS-IDX PIC X.
PROCEDURE DIVISION.
MAIN.
SET WS-IDX TO 1.
SEARCH ALL WS-ITEM
    WHEN WS-ITEM(WS-IDX) = " " DISPLAY "FOUND"
END-SEARCH.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migration blocked"));

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "E_SEARCH_ALL_REQUIRES_KEY");
    assert!(!out.join("src/program.rs").exists());
}

#[test]
fn cobol2rust_runs_search_all_binary_search_through_vm() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("search-all-run.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SEARCHALLRUN.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TABLE.
   05 WS-ITEM OCCURS 3 TIMES ASCENDING KEY IS WS-KEY INDEXED BY WS-IDX.
      10 WS-KEY PIC 9.
      10 WS-TEXT PIC X.
PROCEDURE DIVISION.
MAIN.
MOVE 1 TO WS-KEY(1).
MOVE "A" TO WS-TEXT(1).
MOVE 2 TO WS-KEY(2).
MOVE "B" TO WS-TEXT(2).
MOVE 3 TO WS-KEY(3).
MOVE "C" TO WS-TEXT(3).
SEARCH ALL WS-ITEM
    AT END DISPLAY "NONE"
    WHEN WS-KEY(WS-IDX) = 2 DISPLAY WS-TEXT(WS-IDX)
END-SEARCH.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    let program_rs = fs::read_to_string(out.join("src/program.rs")).expect("program.rs");
    assert!(program_rs.contains("VmProcedureOp::SearchAll"));
    assert!(program_rs.contains("VmSearchDirection::Ascending"));

    assert_generated_stdout_exact(&out, "B\n");
}

#[test]
fn cobol2rust_reads_os_file_into_odo_table_then_searches_all() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("read-odo-search-all.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. READODOSEARCH.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT INFILE ASSIGN TO "input.dat"
        ORGANIZATION IS SEQUENTIAL
        ACCESS MODE IS SEQUENTIAL.
DATA DIVISION.
FILE SECTION.
FD INFILE.
01 IN-REC PIC X(3).
WORKING-STORAGE SECTION.
01 ODO-COUNT PIC 9 VALUE 0.
01 WS-REC.
   05 WS-ITEM OCCURS 0 TO 3 DEPENDING ON ODO-COUNT
        ASCENDING KEY IS WS-ITEM
        INDEXED BY WS-IDX
        PIC X.
01 WS-KEY PIC X VALUE "B".
PROCEDURE DIVISION.
MAIN.
OPEN INPUT INFILE.
READ INFILE INTO WS-REC AT END DISPLAY "EOF".
SEARCH ALL WS-ITEM
    AT END DISPLAY "MISS"
    WHEN WS-ITEM(WS-IDX) = WS-KEY DISPLAY WS-ITEM(WS-IDX)
END-SEARCH.
CLOSE INFILE.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .success();

    fs::write(out.join("input.dat"), b"ABC").expect("assigned input bytes");

    assert_generated_stdout_exact(&out, "B\n");
}

#[test]
fn cobol2rust_torture_fixture_resolves_and_blocks_precisely() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("torture.cbl");
    let out = dir.path().join("generated");
    let copybook_dir = dir.path().join("copybooks");
    fs::create_dir_all(&copybook_dir).expect("copybook dir");
    fs::write(
        copybook_dir.join("TORTURE-CB1.cpy"),
        "05 :TAG:-STAT PIC X.\n   88 :COND: VALUE \"Y\".\n",
    )
    .expect("copybook 1");
    fs::write(
        copybook_dir.join("TORTURE-CB2.cpy"),
        "05 :TAG:-STAT PIC 9.\n   88 :COND: VALUE 1 2 3.\n",
    )
    .expect("copybook 2");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. TORTURE-TEST.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 COPY-AREA.
   COPY TORTURE-CB1 REPLACING ==:TAG:== BY ==V1== ==:COND:== BY ==OK==.
   COPY TORTURE-CB2 REPLACING ==:TAG:== BY ==V2== ==:COND:== BY ==OK==.
01 DYNAMIC-AREA.
   05 ODO-COUNT PIC 99.
   05 ODO-TABLE OCCURS 0 TO 50 DEPENDING ON ODO-COUNT.
      10 TAB-ELEM PIC X(10).
      10 TAB-VAL PIC 9(5).
         88 VAL-OK VALUE 1 THRU 100.
01 REDEF-HELL.
   05 CHUNK-A PIC X(8).
   05 CHUNK-B REDEFINES CHUNK-A.
      10 PART-NUM PIC 9(7) COMP.
         88 PART-BAD VALUE 9999999.
      10 FILLER PIC X(1).
   05 CHUNK-C REDEFINES CHUNK-A.
      10 PART-STR PIC X(8).
         88 PART-HELLO VALUE "HELLO".
01 NUM-REC.
   05 A PIC 9(3)V99.
   05 B PIC S9(5) COMP-3.
   05 C PIC X(4) JUSTIFIED RIGHT.
01 WS-SWITCHES.
   05 SW-1 PIC X VALUE "N".
      88 SW-ON VALUE "Y".
   05 SW-2 PIC X VALUE "N".
      88 SW-ON VALUE "Y".
PROCEDURE DIVISION.
MAIN.
EVALUATE TRUE ALSO CHUNK-A(2:4) ALSO TAB-VAL(1) ALSO FUNCTION LENGTH (CHUNK-A)
   WHEN OK OF V1-STAT ALSO "ELLO" ALSO 50 THRU 75 ALSO ANY
      SET SW-ON OF SW-1 TO TRUE
      DISPLAY "Branch 1"
   WHEN (OK OF V2-STAT AND (A > B OR NOT < C)) ALSO "HELL" ALSO VAL-OK(1) ALSO 8
      MOVE "HELLO" TO PART-STR
      DISPLAY "Branch 2"
   WHEN OTHER
      CONTINUE
END-EVALUATE.
STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--copybook-dir",
            copybook_dir.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migration blocked"));

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "TORTURE-CB1");
    assert_report_contains_json_string(&report, "V1_STAT.OK");
    assert_report_contains_json_string(&report, "V2_STAT.OK");
    assert_report_contains_json_string(&report, "\"evaluate\"");
    assert_report_contains_json_string(&report, "E_CONDITION_TYPE_MISMATCH");
    assert!(!out.join("src/program.rs").exists());
}

#[test]
fn cobol2rust_blocks_hostile_cobol_instead_of_guessing() {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join("hostile.cbl");
    let out = dir.path().join("generated");
    fs::write(
        &input,
        r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. HOSTILE.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT INFILE ASSIGN TO "INFILE".
DATA DIVISION.
FILE SECTION.
FD INFILE.
01 IN-REC.
   05 IN-NAME PIC X(10).
WORKING-STORAGE SECTION.
01 WS-REC.
   05 WS-AMOUNT REDEFINES IN-NAME PIC S9(7)V99 COMP-3 VALUE 1.
   05 WS-TABLE OCCURS 10 TIMES INDEXED BY WS-IDX PIC X.
PROCEDURE DIVISION.
MAIN.
    PERFORM VARYING WS-IDX FROM 1 BY 1 UNTIL WS-IDX > 10.
    CALL "SUBPROG".
    EXEC SQL SELECT 1 END-EXEC.
    SORT SORT-FILE.
    STOP RUN.
"#,
    )
    .expect("write fixture");

    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
            "--dialect",
            "ibm",
            "--source-format",
            "free",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("migration blocked"));

    let report = read_migration_report(&out);
    assert_report_contains_json_string(&report, "\"procedure_cfg\"");
    assert_report_contains_json_string(&report, "\"indexes\"");
    assert_report_contains_json_string(&report, "WS_IDX");
    assert_report_contains_json_string(&report, "E_UNSUPPORTED_VERB");
    assert_report_contains_json_string(&report, "E_UNSUPPORTED_PERFORM_VARYING");
    assert!(!out.join("src/program.rs").exists());
}
