#![cfg(feature = "converter")]

use assert_cmd::prelude::*;
use predicates::prelude::*;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::process::Command;
use tempfile::tempdir;

struct ExpectedDiagnostic {
    code: &'static str,
    message: &'static str,
}

struct BlockedFixture {
    name: &'static str,
    source: &'static str,
    expected: &'static [ExpectedDiagnostic],
}

fn assert_fixture_blocks_exactly(fixture: &BlockedFixture) {
    let dir = tempdir().expect("tempdir");
    let input = dir.path().join(format!("{}.cbl", fixture.name));
    let out = dir.path().join("generated");
    fs::write(&input, fixture.source).expect("write fixture");

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
    let report_json: Value = serde_json::from_str(&report).expect("parse migration report");
    assert_eq!(
        report_json.get("status").and_then(Value::as_str),
        Some("blocked"),
        "fixture {} did not produce blocked report:\n{report}",
        fixture.name
    );

    let actual = report_json
        .get("diagnostics")
        .and_then(Value::as_array)
        .expect("diagnostics array")
        .iter()
        .map(|diagnostic| {
            (
                diagnostic
                    .get("code")
                    .and_then(Value::as_str)
                    .expect("diagnostic code")
                    .to_string(),
                diagnostic
                    .get("message")
                    .and_then(Value::as_str)
                    .expect("diagnostic message")
                    .to_string(),
            )
        })
        .collect::<BTreeSet<_>>();
    let expected = fixture
        .expected
        .iter()
        .map(|diagnostic| (diagnostic.code.to_string(), diagnostic.message.to_string()))
        .collect::<BTreeSet<_>>();

    assert_eq!(
        actual, expected,
        "fixture {} produced unexpected diagnostics:\n{report}",
        fixture.name
    );
    assert!(
        !out.join("src/program.rs").exists(),
        "fixture {} emitted Rust despite ABYSS blocker",
        fixture.name
    );
}

fn run_matrix(fixtures: &[BlockedFixture]) {
    for fixture in fixtures {
        assert_fixture_blocks_exactly(fixture);
    }
}

#[test]
fn abyss_control_flow_hazards_fail_closed_exactly() {
    run_matrix(&[
        BlockedFixture {
            name: "next-sentence",
            source: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. NEXTBAD.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-FLAG PIC X VALUE "Y".
PROCEDURE DIVISION.
MAIN.
    IF WS-FLAG = "Y" NEXT SENTENCE DISPLAY "BAD".
    DISPLAY "BAD".
    STOP RUN.
"#,
            expected: &[
                ExpectedDiagnostic {
                    code: "E_UNSUPPORTED_CONTROL_FLOW",
                    message: "NEXT SENTENCE has sentence-level CFG targets but executable period-scope lowering is not enabled yet",
                },
            ],
        },
        BlockedFixture {
            name: "perform-varying-after",
            source: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. PERFVAFT.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 I PIC 9 VALUE 1.
01 J PIC 9 VALUE 1.
PROCEDURE DIVISION.
MAIN.
    PERFORM BODY VARYING I FROM 1 BY 1 AFTER J FROM 1 BY 1 UNTIL I > 2.
    STOP RUN.
BODY.
    DISPLAY I J.
"#,
            expected: &[ExpectedDiagnostic {
                code: "E_UNSUPPORTED_CONTROL_FLOW",
                message: "PERFORM VARYING AFTER requires nested loop control-flow modeling and is not lowered yet",
            }],
        },
    ]);
}

#[test]
fn abyss_environment_hazards_fail_closed_exactly() {
    run_matrix(&[
        BlockedFixture {
            name: "segment-limit",
            source: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SEGBAD.
ENVIRONMENT DIVISION.
CONFIGURATION SECTION.
OBJECT-COMPUTER. IBM-370 SEGMENT-LIMIT IS 50.
PROCEDURE DIVISION.
MAIN.
    STOP RUN.
"#,
            expected: &[ExpectedDiagnostic {
                code: "E_UNSUPPORTED_ENVIRONMENT",
                message: "Environment Division feature SEGMENT-LIMIT requires platform/runtime emulation and is not lowered yet",
            }],
        },
        BlockedFixture {
            name: "rerun-unsupported-shape",
            source: r#"
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
            expected: &[ExpectedDiagnostic {
                code: "E_UNSUPPORTED_ENVIRONMENT",
                message: "RERUN is only lowered for `RERUN ON file EVERY n RECORDS OF file` checkpoint snapshots",
            }],
        },
        BlockedFixture {
            name: "decimal-point-comma",
            source: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. DECBAD.
ENVIRONMENT DIVISION.
CONFIGURATION SECTION.
SPECIAL-NAMES.
    DECIMAL-POINT IS COMMA.
PROCEDURE DIVISION.
MAIN.
    STOP RUN.
"#,
            expected: &[ExpectedDiagnostic {
                code: "E_UNSUPPORTED_ENVIRONMENT",
                message: "Environment Division feature DECIMAL-POINT requires platform/runtime emulation and is not lowered yet",
            }],
        },
        BlockedFixture {
            name: "currency-sign",
            source: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CURRBAD.
ENVIRONMENT DIVISION.
CONFIGURATION SECTION.
SPECIAL-NAMES.
    CURRENCY SIGN IS "$".
PROCEDURE DIVISION.
MAIN.
    STOP RUN.
"#,
            expected: &[ExpectedDiagnostic {
                code: "E_UNSUPPORTED_ENVIRONMENT",
                message: "Environment Division feature CURRENCY requires platform/runtime emulation and is not lowered yet",
            }],
        },
        BlockedFixture {
            name: "label-records",
            source: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. LABELBAD.
ENVIRONMENT DIVISION.
INPUT-OUTPUT SECTION.
FILE-CONTROL.
    SELECT MASTER-FILE ASSIGN TO "master.dat".
DATA DIVISION.
FILE SECTION.
FD MASTER-FILE
    LABEL RECORDS ARE STANDARD.
01 M-REC PIC X.
PROCEDURE DIVISION.
MAIN.
    STOP RUN.
"#,
            expected: &[ExpectedDiagnostic {
                code: "E_UNSUPPORTED_ENVIRONMENT",
                message: "File metadata feature LABEL RECORDS requires platform/runtime emulation and is not lowered yet",
            }],
        },
    ]);
}

#[test]
fn abyss_data_clause_hazards_fail_closed_exactly() {
    run_matrix(&[
        BlockedFixture {
            name: "global-clause",
            source: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. GLOBALBAD.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 GLOBAL-DOOM PIC X GLOBAL.
PROCEDURE DIVISION.
MAIN.
    STOP RUN.
"#,
            expected: &[ExpectedDiagnostic {
                code: "E_UNSUPPORTED_DATA_CLAUSE",
                message: "Data Division clause GLOBAL requires real layout/runtime semantics and is not lowered yet",
            },
            ExpectedDiagnostic {
                code: "E_UNSUPPORTED_DATA_CLAUSE",
                message: "Data Division clause GLOBAL requires exact storage/runtime semantics and is not lowered yet",
            }],
        },
        BlockedFixture {
            name: "pointer-usage",
            source: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. PTRBAD.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 PTR USAGE POINTER.
PROCEDURE DIVISION.
MAIN.
    STOP RUN.
"#,
            expected: &[ExpectedDiagnostic {
                code: "E_UNSUPPORTED_DATA_CLAUSE",
                message: "Data Division clause POINTER requires real layout/runtime semantics and is not lowered yet",
            },
            ExpectedDiagnostic {
                code: "E_UNSUPPORTED_DATA_CLAUSE",
                message: "Data Division clause POINTER requires exact storage/runtime semantics and is not lowered yet",
            }],
        },
        BlockedFixture {
            name: "sign-separate",
            source: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. SIGNBAD.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-NUM PIC 9(3) SIGN IS SEPARATE.
PROCEDURE DIVISION.
MAIN.
    STOP RUN.
"#,
            expected: &[ExpectedDiagnostic {
                code: "E_UNSUPPORTED_DATA_CLAUSE",
                message: "Data Division clause SIGN requires real layout/runtime semantics and is not lowered yet",
            },
            ExpectedDiagnostic {
                code: "E_UNSUPPORTED_DATA_CLAUSE",
                message: "Data Division clause SIGN requires exact storage/runtime semantics and is not lowered yet",
            }],
        },
        BlockedFixture {
            name: "justified-right",
            source: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. JUSTBAD.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-RIGHT PIC X(4) JUSTIFIED RIGHT.
PROCEDURE DIVISION.
MAIN.
    STOP RUN.
"#,
            expected: &[ExpectedDiagnostic {
                code: "E_UNSUPPORTED_DATA_CLAUSE",
                message: "Data Division clause JUSTIFIED requires real layout/runtime semantics and is not lowered yet",
            },
            ExpectedDiagnostic {
                code: "E_UNSUPPORTED_DATA_CLAUSE",
                message: "Data Division clause JUSTIFIED requires exact storage/runtime semantics and is not lowered yet",
            }],
        },
        BlockedFixture {
            name: "level-78",
            source: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CONSTBAD.
DATA DIVISION.
WORKING-STORAGE SECTION.
78 MAX-SIZE VALUE 10.
PROCEDURE DIVISION.
MAIN.
    STOP RUN.
"#,
            expected: &[ExpectedDiagnostic {
                code: "E_UNSUPPORTED_DATA_CLAUSE",
                message: "78-level constant MAX_SIZE is represented as a named constant and does not allocate storage yet",
            }],
        },
    ]);
}

#[test]
fn abyss_procedure_hazards_fail_closed_exactly() {
    run_matrix(&[
        BlockedFixture {
            name: "accept",
            source: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. ACCEPTBAD.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 WS-TEXT PIC X.
PROCEDURE DIVISION.
MAIN.
    ACCEPT WS-TEXT.
    STOP RUN.
"#,
            expected: &[ExpectedDiagnostic {
                code: "E_UNSUPPORTED_STATEMENT",
                message: "unsupported COBOL statement: ACCEPT",
            },
            ExpectedDiagnostic {
                code: "E_UNSUPPORTED_STATEMENT",
                message: "unsupported or not-yet-lowered COBOL statement: ACCEPT",
            },
            ExpectedDiagnostic {
                code: "E_UNSUPPORTED_VERB",
                message: "Procedure Division verb ACCEPT is not lowered by the converter preview",
            }],
        },
        BlockedFixture {
            name: "enter-assembler",
            source: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. ENTERBAD.
PROCEDURE DIVISION.
MAIN.
    ENTER LANGUAGE ASSEMBLER.
    STOP RUN.
"#,
            expected: &[ExpectedDiagnostic {
                code: "E_UNSUPPORTED_STATEMENT",
                message: "unsupported COBOL statement: ENTER",
            },
            ExpectedDiagnostic {
                code: "E_UNSUPPORTED_STATEMENT",
                message: "unsupported or not-yet-lowered COBOL statement: ENTER",
            },
            ExpectedDiagnostic {
                code: "E_UNSUPPORTED_VERB",
                message: "Procedure Division verb ENTER is not lowered by the converter preview",
            }],
        },
        BlockedFixture {
            name: "merge",
            source: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. MERGEBAD.
PROCEDURE DIVISION.
MAIN.
    MERGE SORT-FILE.
    STOP RUN.
"#,
            expected: &[ExpectedDiagnostic {
                code: "E_UNSUPPORTED_STATEMENT",
                message: "unsupported COBOL statement: MERGE",
            },
            ExpectedDiagnostic {
                code: "E_UNSUPPORTED_STATEMENT",
                message: "unsupported or not-yet-lowered COBOL statement: MERGE",
            },
            ExpectedDiagnostic {
                code: "E_UNSUPPORTED_VERB",
                message: "Procedure Division verb MERGE is not lowered by the converter preview",
            }],
        },
        BlockedFixture {
            name: "return-invalid-sort-file",
            source: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. RETBAD.
PROCEDURE DIVISION.
MAIN.
    RETURN SORT-FILE AT END DISPLAY "END".
    STOP RUN.
"#,
            expected: &[ExpectedDiagnostic {
                code: "E_INVALID_SORT_FILE",
                message: "RETURN file SORT_FILE is not declared as an SD file",
            }],
        },
        BlockedFixture {
            name: "move-corresponding-ambiguous",
            source: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. MOVECORR.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 SRC-GROUP.
   05 LEFT-SIDE.
      10 FIELD-A PIC X VALUE "A".
   05 RIGHT-SIDE.
      10 FIELD-A PIC X VALUE "B".
01 DST-GROUP.
   05 FIELD-A PIC X VALUE SPACE.
PROCEDURE DIVISION.
MAIN.
    MOVE CORRESPONDING SRC-GROUP TO DST-GROUP.
    STOP RUN.
"#,
            expected: &[ExpectedDiagnostic {
                code: "E_UNSUPPORTED_MOVE_CORRESPONDING",
                message: "MOVE CORRESPONDING name FIELD_A is ambiguous between source and target groups",
            }],
        },
        BlockedFixture {
            name: "compute-rounded",
            source: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. COMPROUND.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 N PIC 9 VALUE 9.
PROCEDURE DIVISION.
MAIN.
    COMPUTE N ROUNDED = N + 1.
    STOP RUN.
"#,
            expected: &[ExpectedDiagnostic {
                code: "E_UNSUPPORTED_ARITHMETIC",
                message: "COMPUTE with ROUNDED, exponentiation, or function operands is not lowered yet",
            },
            ExpectedDiagnostic {
                code: "E_UNSUPPORTED_STATEMENT",
                message: "unsupported COBOL statement: COMPUTE",
            },
            ExpectedDiagnostic {
                code: "E_UNSUPPORTED_STATEMENT",
                message: "unsupported or not-yet-lowered COBOL statement: COMPUTE",
            }],
        },
        BlockedFixture {
            name: "call-by-content",
            source: r#"
IDENTIFICATION DIVISION.
PROGRAM-ID. CALLMODE.
DATA DIVISION.
WORKING-STORAGE SECTION.
01 ARG PIC X VALUE "A".
PROCEDURE DIVISION.
MAIN.
    CALL "SUB" USING BY CONTENT ARG.
    STOP RUN.
"#,
            expected: &[ExpectedDiagnostic {
                code: "E_UNRESOLVED_CALL_TARGET",
                message: "literal CALL target SUB is not registered in this compilation unit",
            },
            ExpectedDiagnostic {
                code: "E_UNRESOLVED_DATA",
                message: "data reference BY CONTENT ARG does not resolve to a Data Division item or condition name",
            },
            ExpectedDiagnostic {
                code: "E_UNSUPPORTED_CALL_USING",
                message: "CALL USING cannot be lowered without a resolved callee LINKAGE signature",
            },
            ExpectedDiagnostic {
                code: "E_UNSUPPORTED_CALL_MODE",
                message: "CALL BY CONTENT requires explicit parameter passing mode semantics and is not lowered yet",
            }],
        },
    ]);
}
