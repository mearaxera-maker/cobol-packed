//! Fuzz generated COBOL source through normalization and parser entrypoints.
//!
//! Run:
//!   cargo fuzz run fuzz_source_parser -- -max_len=512 -timeout=5

#![no_main]

use cobol_source::{normalize_source, SourceFormat};
use cobol_syntax::parse_programs;
use libfuzzer_sys::fuzz_target;

fn word(data: &[u8], idx: usize, fallback: &str) -> String {
    let Some(seed) = data.get(idx) else {
        return fallback.to_string();
    };
    format!("{fallback}{seed}")
}

fn generated_program(data: &[u8]) -> String {
    let program = word(data, 0, "P");
    let mut source = format!(
        "IDENTIFICATION DIVISION.\nPROGRAM-ID. {program}.\nDATA DIVISION.\nWORKING-STORAGE SECTION.\n01 WS-N PIC 9(4) VALUE 0.\n01 WS-TEXT PIC X(16) VALUE SPACES.\nPROCEDURE DIVISION.\nMAIN.\n"
    );

    for (idx, chunk) in data.chunks(3).take(48).enumerate() {
        let op = chunk.first().copied().unwrap_or_default() % 10;
        let literal = chunk.get(1).copied().unwrap_or_default();
        let number = chunk.get(2).copied().unwrap_or_default();
        match op {
            0 => source.push_str(&format!("DISPLAY \"L{idx}-{literal}\".\n")),
            1 => source.push_str(&format!("MOVE {number} TO WS-N.\n")),
            2 => source.push_str("ADD 1 TO WS-N.\n"),
            3 => source.push_str("SUBTRACT 1 FROM WS-N.\n"),
            4 => source.push_str("IF WS-N = 0 DISPLAY \"ZERO\" ELSE DISPLAY \"NZ\" END-IF.\n"),
            5 => source.push_str("PERFORM BODY.\n"),
            6 => source.push_str("CONTINUE.\n"),
            7 => source.push_str("MOVE SPACES TO WS-TEXT.\n"),
            8 => source.push_str(&format!("DISPLAY \"QUOTE \"\" {literal} \"\"\".\n")),
            _ => source.push_str("NEXT SENTENCE.\n"),
        }
    }

    source.push_str("STOP RUN.\nBODY.\nDISPLAY \"BODY\".\n");
    source
}

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    let arbitrary = String::from_utf8_lossy(data);
    let _ = normalize_source(&arbitrary, SourceFormat::Free);
    let _ = normalize_source(&arbitrary, SourceFormat::Fixed);

    let generated = generated_program(data);
    let normalized = normalize_source(&generated, SourceFormat::Free);
    let _ = parse_programs("fuzz.cbl", &normalized);
});
