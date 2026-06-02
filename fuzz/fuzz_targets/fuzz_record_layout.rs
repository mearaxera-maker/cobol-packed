//! Shared record-layout fuzz harness.
//!
//! Exercises coverage maps, offset alignment, byte-length derivation, and
//! range summarization in `cobol-record`.
//!
//! Run:
//!   cargo fuzz run fuzz_record_layout -- -max_len=256 -timeout=5

#![no_main]

use cobol_record::{
    align_offset, coverage_summary, elementary_byte_len, sync_alignment, CoverageKind,
    CoverageRange, PicCategory, PlatformProfile, RecordPicture, RecordUsage,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 8 {
        return;
    }

    let record_length = usize::from(data[0]);
    let mut ranges = Vec::new();
    for (idx, chunk) in data[1..].chunks(4).take(32).enumerate() {
        if chunk.len() < 4 {
            break;
        }
        let offset = usize::from(chunk[0]) % record_length.saturating_add(1).max(1);
        let length = usize::from(chunk[1]) % 32;
        let kind = match chunk[2] % 5 {
            0 => CoverageKind::Field,
            1 => CoverageKind::Filler,
            2 => CoverageKind::SyncSlack,
            3 => CoverageKind::Occurs,
            _ => CoverageKind::RedefinesBase,
        };
        ranges.push(CoverageRange {
            kind,
            name: format!("range_{idx}"),
            offset,
            length,
        });
    }

    let summary = coverage_summary(record_length, &ranges);
    assert_eq!(
        summary.covered_bytes + summary.uncovered_bytes,
        record_length
    );
    assert!(summary.covered_bytes <= record_length);
    for range in summary.gaps.iter().chain(summary.overlaps.iter()) {
        assert!(range.offset <= record_length);
        assert!(range.offset.saturating_add(range.length) <= record_length);
    }

    let usage = match data[1] % 9 {
        0 => RecordUsage::Group,
        1 => RecordUsage::PackedDecimal,
        2 => RecordUsage::ZonedDecimal,
        3 => RecordUsage::Binary,
        4 => RecordUsage::NativeBinary,
        5 => RecordUsage::IbmFloat32,
        6 => RecordUsage::IbmFloat64,
        7 => RecordUsage::Alphanumeric,
        _ => RecordUsage::Display,
    };
    let picture = RecordPicture {
        raw: "FUZZ".to_string(),
        category: PicCategory::NumericDisplay,
        signed: data[2] & 1 == 1,
        digits: usize::from((data[3] % 18).saturating_add(1)),
        scale: 0,
        char_len: usize::from((data[4] % 32).saturating_add(1)),
    };
    let byte_len = elementary_byte_len(&usage, Some(&picture));
    let profile = match data[5] % 4 {
        0 => PlatformProfile::IbmZOs,
        1 => PlatformProfile::MicroFocus,
        2 => PlatformProfile::GnuCobol,
        _ => PlatformProfile::IbmI,
    };
    if let Some(alignment) = sync_alignment(&usage, byte_len, data[6] & 1 == 1, profile) {
        let aligned = align_offset(record_length, alignment);
        assert!(aligned >= record_length);
        if alignment > 0 {
            assert_eq!(aligned % alignment, 0);
        }
    }
});
