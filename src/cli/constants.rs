pub(super) const OUTPUT_VERSION: u8 = 1;
pub(super) const TOOL_NAME: &str = "hostlens";
pub(super) const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");
// IBM COMP-3 stores up to 18 supported digits in 10 bytes:
// ceil((18 digits + sign nibble) / 2).
pub(super) const MAX_SINGLE_FIELD_BYTES: usize = cobol_packed::expected_len(18);
pub(super) const MAX_SCHEMA_BYTES: u64 = 1024 * 1024;
pub(super) const MAX_RECORD_BYTES: usize = 16 * 1024 * 1024;
pub(super) const MAX_LINE_BYTES: usize = 1024 * 1024;
pub(super) const MAX_BUFFERED_ROWS: usize = 100_000;
pub(super) const MAX_SCHEMA_FIELDS: usize = 1_024;
pub(super) const MAX_FAILURE_SAMPLES: usize = 25;
pub(super) const MAX_FAILURE_SAMPLE_LIMIT: usize = 1_000;
pub(super) const MAX_ERROR_RAW_BYTES: usize = 64;
pub(super) const ERROR_DOCS_BASE_URL: &str =
    "https://github.com/mearaxera-maker/cobol-packed/blob/main/docs/cli.md#error-model";
