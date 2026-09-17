//! Schema-aware fixed-width batch fuzz harness.
//!
//! This target derives a small record layout from fuzzer bytes, including a
//! possible filler range, then exercises decode and lossless verify across the
//! selected field. The goal is to stress offset/length/record-size relationships
//! without shelling out through the CLI.
//!
//! Run:
//!   cargo fuzz run fuzz_schema_batch -- -max_len=256 -timeout=5

#![no_main]

use cobol_packed::{from_packed_lossless, to_packed_lossless, PackedConfig, SignMode};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 6 {
        return;
    }

    let total_digits = (data[0] % 18).saturating_add(1);
    let scale = data[1] % (total_digits + 1);
    let signed = data[2] & 1 == 1;
    let sign_mode = if data[3] & 1 == 1 {
        SignMode::Pfd
    } else {
        SignMode::Nopfd
    };
    let cfg = match PackedConfig::new(total_digits, scale, signed) {
        Ok(cfg) => cfg,
        Err(_) => return,
    };
    let field_len = cfg.byte_len();
    let filler_len = (data[4] as usize) % 8;
    let prefix_len = (data[5] as usize) % 8;
    let record_len = prefix_len
        .saturating_add(field_len)
        .saturating_add(filler_len);
    if data.len() < 6 + record_len {
        return;
    }

    let record = &data[6..6 + record_len];
    let field = &record[prefix_len..prefix_len + field_len];
    if let Ok(lossless) = from_packed_lossless(field, &cfg, sign_mode) {
        let repacked = match to_packed_lossless(&lossless, &cfg) {
            Ok(repacked) => repacked,
            Err(_) => return,
        };
        assert_eq!(repacked, field);
    }
});
