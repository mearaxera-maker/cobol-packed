#![cfg(feature = "converter")]

use assert_cmd::prelude::*;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;

struct OracleFixture {
    name: &'static str,
    source: &'static str,
    files: &'static [(&'static str, &'static [u8])],
    source_format: OracleSourceFormat,
}

#[derive(Clone, Copy)]
enum OracleSourceFormat {
    Fixed,
    Free,
}

impl OracleSourceFormat {
    fn cobc_arg(self) -> &'static str {
        match self {
            Self::Fixed => "-fixed",
            Self::Free => "-free",
        }
    }

    fn converter_arg(self) -> &'static str {
        match self {
            Self::Fixed => "fixed",
            Self::Free => "free",
        }
    }
}

fn cobc_available() -> bool {
    Command::new("cobc").arg("--version").output().is_ok()
}

fn exe_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    })
}

fn normalize_stdout(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim_end()
        .to_string()
}

fn run_gnucobol_fixture(fixture: &OracleFixture, dir: &Path) -> String {
    let input = dir.join(format!("{}.cbl", fixture.name));
    let cobol_exe = exe_path(dir, &format!("{}-cobol", fixture.name));
    fs::write(&input, fixture.source).expect("write COBOL oracle fixture");
    for (name, bytes) in fixture.files {
        fs::write(dir.join(name), bytes).expect("write COBOL oracle file input");
    }

    Command::new("cobc")
        .current_dir(dir)
        .args([
            "-x",
            fixture.source_format.cobc_arg(),
            "-o",
            cobol_exe.to_str().expect("utf8 exe path"),
            input.to_str().expect("utf8 input path"),
        ])
        .assert()
        .success();
    let output = Command::new(&cobol_exe)
        .current_dir(dir)
        .output()
        .expect("run GnuCOBOL fixture");
    assert!(
        output.status.success(),
        "GnuCOBOL fixture {} failed: {}",
        fixture.name,
        String::from_utf8_lossy(&output.stderr)
    );
    normalize_stdout(&output.stdout)
}

fn run_converter_fixture(fixture: &OracleFixture, dir: &Path) -> String {
    let input = dir.join(format!("{}.cbl", fixture.name));
    let out = dir.join(format!("{}-generated", fixture.name));
    fs::write(&input, fixture.source).expect("write converter oracle fixture");
    Command::cargo_bin("cobol2rust")
        .expect("cobol2rust binary")
        .args([
            "convert",
            "--input",
            input.to_str().expect("utf8 input path"),
            "--out",
            out.to_str().expect("utf8 out path"),
            "--dialect",
            "gnucobol",
            "--source-format",
            fixture.source_format.converter_arg(),
            "--copybook-dir",
            dir.to_str().expect("utf8 copybook dir"),
        ])
        .assert()
        .success();
    for (name, bytes) in fixture.files {
        fs::write(out.join(name), bytes).expect("write generated Rust file input");
    }
    let output = Command::new("cargo")
        .current_dir(&out)
        .args(["run", "--offline"])
        .output()
        .expect("run generated Rust oracle fixture");
    assert!(
        output.status.success(),
        "generated Rust fixture {} failed: {}",
        fixture.name,
        String::from_utf8_lossy(&output.stderr)
    );
    normalize_stdout(&output.stdout)
}

fn assert_oracle_equivalence(fixture: OracleFixture) {
    if !cobc_available() {
        eprintln!(
            "skipping GnuCOBOL oracle fixture {}: cobc is not available",
            fixture.name
        );
        return;
    }
    let dir = tempdir().expect("tempdir");
    let cobol_stdout = run_gnucobol_fixture(&fixture, dir.path());
    let rust_stdout = run_converter_fixture(&fixture, dir.path());
    assert_eq!(
        cobol_stdout, rust_stdout,
        "oracle stdout mismatch for fixture {}",
        fixture.name
    );
}

#[test]
fn gnucobol_oracle_if_and_88_when_cobc_is_available() {
    assert_oracle_equivalence(OracleFixture {
        name: "if_88",
        source: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. ORACLE.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FLAG PIC X VALUE "Y".
   88 WS-OK VALUE "Y".
PROCEDURE DIVISION.
MAIN.
    IF WS-OK
        DISPLAY "OK"
    ELSE
        DISPLAY "BAD"
    END-IF.
    STOP RUN.
"#,
        files: &[],
        source_format: OracleSourceFormat::Free,
    });
}

#[test]
fn gnucobol_oracle_dynamic_call_using_when_cobc_is_available() {
    assert_oracle_equivalence(OracleFixture {
        name: "dynamic_call_using",
        source: r#"
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
        files: &[],
        source_format: OracleSourceFormat::Free,
    });
}

#[test]
fn gnucobol_oracle_fixed_sequential_read_when_cobc_is_available() {
    assert_oracle_equivalence(OracleFixture {
        name: "fixed_seq_read",
        source: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FILEORACLE.
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
PROCEDURE DIVISION.
MAIN.
    OPEN INPUT INFILE.
    READ INFILE
        AT END DISPLAY "EOF"
        NOT AT END DISPLAY IN-REC
    END-READ.
    CLOSE INFILE.
    STOP RUN.
"#,
        files: &[("input.dat", b"ABC")],
        source_format: OracleSourceFormat::Free,
    });
}

#[test]
fn gnucobol_oracle_go_to_depending_on_when_cobc_is_available() {
    assert_oracle_equivalence(OracleFixture {
        name: "go_to_depending_on",
        source: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. GOTODEP.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-IDX PIC 9 VALUE 2.
PROCEDURE DIVISION.
MAIN.
    GO TO ONE TWO THREE DEPENDING ON WS-IDX.
    DISPLAY "FALL".
    STOP RUN.
ONE.
    DISPLAY "ONE".
    STOP RUN.
TWO.
    DISPLAY "TWO".
    STOP RUN.
THREE.
    DISPLAY "THREE".
    STOP RUN.
"#,
        files: &[],
        source_format: OracleSourceFormat::Free,
    });
}

#[test]
fn gnucobol_oracle_string_delimited_by_size_when_cobc_is_available() {
    assert_oracle_equivalence(OracleFixture {
        name: "string_size",
        source: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. STRSIZE.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TEXT PIC X(4).
PROCEDURE DIVISION.
MAIN.
    STRING "AB" DELIMITED BY SIZE
           "CD" DELIMITED BY SIZE
        INTO WS-TEXT
    END-STRING.
    DISPLAY WS-TEXT.
    STOP RUN.
"#,
        files: &[],
        source_format: OracleSourceFormat::Free,
    });
}

#[test]
fn gnucobol_oracle_numeric_display_codec_when_cobc_is_available() {
    assert_oracle_equivalence(OracleFixture {
        name: "numeric_display_codec",
        source: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. NUMCODEC.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-NUM PIC 9(3) VALUE 7.
PROCEDURE DIVISION.
MAIN.
    ADD 5 TO WS-NUM.
    DISPLAY WS-NUM.
    STOP RUN.
"#,
        files: &[],
        source_format: OracleSourceFormat::Free,
    });
}

#[test]
fn gnucobol_oracle_file_status_missing_input_when_cobc_is_available() {
    assert_oracle_equivalence(OracleFixture {
        name: "file_status_missing",
        source: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. FSORACLE.
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
01 IN-REC PIC X.
WORKING-STORAGE SECTION.
01 FS PIC XX.
PROCEDURE DIVISION.
MAIN.
    OPEN INPUT INFILE.
    DISPLAY FS.
    STOP RUN.
"#,
        files: &[],
        source_format: OracleSourceFormat::Free,
    });
}

#[test]
fn gnucobol_oracle_sort_procedure_when_cobc_is_available() {
    assert_oracle_equivalence(OracleFixture {
        name: "sort_procedure",
        source: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SORTORCL.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT SORT-FILE ASSIGN TO "sortwork.tmp".
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
        files: &[],
        source_format: OracleSourceFormat::Free,
    });
}

#[test]
fn gnucobol_oracle_search_serial_when_cobc_is_available() {
    assert_oracle_equivalence(OracleFixture {
        name: "search_serial",
        source: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SRCHORCL.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-IDX PIC 9.
01 WS-TABLE.
   05 WS-ITEM OCCURS 3 TIMES INDEXED BY T-IDX PIC X.
PROCEDURE DIVISION.
MAIN.
    MOVE "A" TO WS-ITEM(1).
    MOVE "B" TO WS-ITEM(2).
    MOVE "C" TO WS-ITEM(3).
    SET T-IDX TO 1.
    SEARCH WS-ITEM
        AT END DISPLAY "NONE"
        WHEN WS-ITEM(T-IDX) = "B" DISPLAY WS-ITEM(T-IDX)
    END-SEARCH.
    STOP RUN.
"#,
        files: &[],
        source_format: OracleSourceFormat::Free,
    });
}

#[test]
fn gnucobol_oracle_odo_table_access_when_cobc_is_available() {
    assert_oracle_equivalence(OracleFixture {
        name: "odo_table_access",
        source: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. ODOORCL.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-COUNT PIC 9 VALUE 2.
01 WS-TABLE.
   05 WS-ITEM OCCURS 0 TO 3 TIMES DEPENDING ON WS-COUNT PIC X.
PROCEDURE DIVISION.
MAIN.
    MOVE "A" TO WS-ITEM(1).
    MOVE "B" TO WS-ITEM(2).
    DISPLAY WS-COUNT.
    DISPLAY WS-ITEM(1).
    DISPLAY WS-ITEM(2).
    STOP RUN.
"#,
        files: &[],
        source_format: OracleSourceFormat::Free,
    });
}

#[test]
fn gnucobol_oracle_fixed_format_copy_when_cobc_is_available() {
    assert_oracle_equivalence(OracleFixture {
        name: "fixed_copy",
        source: "       IDENTIFICATION DIVISION.\n       PROGRAM-ID. FIXCOPY.\n       DATA DIVISION.\n       WORKING-STORAGE SECTION.\n       COPY FIELDDEF.\n       PROCEDURE DIVISION.\n       MAIN.\n           DISPLAY WS-FIELD.\n           STOP RUN.\n",
        files: &[("FIELDDEF.cpy", b"       01 WS-FIELD PIC X(4) VALUE \"COPY\".\n")],
        source_format: OracleSourceFormat::Fixed,
    });
}

#[test]
fn oracle_fixture_names_are_unique() {
    let names = [
        "if_88",
        "dynamic_call_using",
        "fixed_seq_read",
        "go_to_depending_on",
        "string_size",
    ];
    let mut seen = BTreeMap::new();
    for name in names {
        assert!(seen.insert(name, ()).is_none(), "duplicate fixture {name}");
    }
}
