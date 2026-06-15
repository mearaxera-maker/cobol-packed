#![cfg(feature = "converter")]

use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::fs;
use std::process::Command;
use tempfile::tempdir;

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
    let manifest = fs::read_to_string(out.join("Cargo.toml")).expect("manifest");
    assert!(manifest.contains("vendor/cobol-runtime"));
    let report = fs::read_to_string(out.join("migration-report.json")).expect("report");
    assert!(report.contains("\"status\": \"generated\""));
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

    let report = fs::read_to_string(out.join("migration-report.json")).expect("report");
    assert!(report.contains("\"status\": \"blocked\""));
    assert!(report.contains("E_UNSUPPORTED_STATEMENT"));
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

    let report = fs::read_to_string(out.join("migration-report.json")).expect("report");
    assert!(report.contains("E_UNRESOLVED_DATA"));
}

#[test]
fn cobol2rust_blocks_copy_replacing() {
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
        .failure()
        .stderr(predicate::str::contains("COPY REPLACING"));
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

    let report = fs::read_to_string(out.join("migration-report.json")).expect("report");
    assert!(report.contains("E_UNSUPPORTED_ENVIRONMENT"));
    assert!(report.contains("E_UNSUPPORTED_SECTION"));
    assert!(report.contains("E_UNSUPPORTED_DATA_CLAUSE"));
    assert!(report.contains("E_UNSUPPORTED_PERFORM_FORM"));
    assert!(report.contains("E_UNSUPPORTED_VERB"));
    assert!(!out.join("src/program.rs").exists());
}
