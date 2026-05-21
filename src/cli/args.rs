use super::constants::MAX_FAILURE_SAMPLES;
use super::{
    CliSignMode, CompareFailOn, CompareOutputFormat, DiagnosticFormat, EncodingListFormat,
    EvidenceMode, InputEncoding, OnError, OutputFormat, RustDerive, RustVisibility,
    VerificationScope,
};
use clap::{Args, Parser, Subcommand};
use clap_complete::Shell;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "hostlens")]
#[command(version)]
#[command(about = "HostLens mainframe record decoding and forensic audit CLI")]
pub(super) struct Cli {
    #[command(subcommand)]
    pub(super) command: Command,
}

#[derive(Subcommand)]
pub(super) enum Command {
    Decode(FieldDecodeArgs),
    Encode(EncodeArgs),
    Inspect(FieldDecodeArgs),
    Batch {
        #[command(subcommand)]
        command: BatchCommand,
    },
    Schema {
        #[command(subcommand)]
        command: SchemaCommand,
    },
    Encodings {
        #[command(subcommand)]
        command: EncodingsCommand,
    },
    Profile(BatchInputArgs),
    Completions(CompletionsArgs),
    Man,
}

#[derive(Subcommand)]
pub(super) enum BatchCommand {
    Decode(BatchInputArgs),
    Verify(BatchInputArgs),
}

#[derive(Subcommand)]
pub(super) enum SchemaCommand {
    Check(SchemaArgs),
    FromCopybook(CopybookArgs),
    EmitRust(EmitRustArgs),
    Compare(CompareArgs),
}

#[derive(Subcommand)]
pub(super) enum EncodingsCommand {
    List(EncodingListArgs),
}

#[derive(Args)]
pub(super) struct SchemaArgs {
    #[arg(long)]
    pub(super) schema: PathBuf,
    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    pub(super) output: OutputFormat,
}

#[derive(Args)]
pub(super) struct CopybookArgs {
    #[arg(long)]
    pub(super) copybook: PathBuf,
    #[arg(long)]
    pub(super) record_name: Option<String>,
    #[arg(long)]
    pub(super) encoding: String,
    #[arg(long, value_enum, default_value_t = InputEncoding::Binary)]
    pub(super) input_encoding: InputEncoding,
    #[arg(long, value_enum, default_value_t = OnError::EmitErrorRow)]
    pub(super) on_error: OnError,
    #[arg(long, value_enum, default_value_t = VerificationScope::Field)]
    pub(super) verification_scope: VerificationScope,
    #[arg(long)]
    pub(super) output: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = DiagnosticFormat::Text)]
    pub(super) diagnostics: DiagnosticFormat,
    #[arg(long)]
    pub(super) strict: bool,
    #[arg(long, conflicts_with = "drop_fillers")]
    pub(super) include_fillers: bool,
    #[arg(long, conflicts_with = "include_fillers")]
    pub(super) drop_fillers: bool,
}

#[derive(Args)]
pub(super) struct EmitRustArgs {
    #[arg(long)]
    pub(super) schema: PathBuf,
    #[arg(long)]
    pub(super) struct_name: String,
    #[arg(long)]
    pub(super) module_name: Option<String>,
    #[arg(long)]
    pub(super) output: Option<PathBuf>,
    #[arg(long, value_delimiter = ',')]
    pub(super) derive: Vec<RustDerive>,
    #[arg(long)]
    pub(super) raw_slices: bool,
    #[arg(long, value_enum, default_value_t = RustVisibility::Pub)]
    pub(super) visibility: RustVisibility,
}

#[derive(Args)]
pub(super) struct CompareArgs {
    #[arg(long)]
    pub(super) old: PathBuf,
    #[arg(long)]
    pub(super) new: PathBuf,
    #[arg(long, value_enum, default_value_t = CompareOutputFormat::Table)]
    pub(super) output: CompareOutputFormat,
    #[arg(long, value_enum, default_value_t = CompareFailOn::Any)]
    pub(super) fail_on: CompareFailOn,
    #[arg(long)]
    pub(super) ignore_order: bool,
    #[arg(long)]
    pub(super) show_unchanged: bool,
}

#[derive(Args)]
pub(super) struct EncodingListArgs {
    #[arg(long, value_enum, default_value_t = EncodingListFormat::Table)]
    pub(super) output: EncodingListFormat,
}

#[derive(Args, Clone)]
pub(super) struct FieldShapeArgs {
    #[arg(long)]
    pub(super) digits: u8,
    #[arg(long, default_value_t = 0)]
    pub(super) scale: u8,
    #[arg(long, conflicts_with = "unsigned")]
    pub(super) signed: bool,
    #[arg(long, conflicts_with = "signed")]
    pub(super) unsigned: bool,
    #[arg(long, value_enum, default_value_t = CliSignMode::Pfd)]
    pub(super) sign_mode: CliSignMode,
}

#[derive(Args)]
pub(super) struct FieldDecodeArgs {
    #[command(flatten)]
    pub(super) shape: FieldShapeArgs,
    #[arg(long, conflicts_with_all = ["file", "stdin"])]
    pub(super) hex: Option<String>,
    #[arg(long, conflicts_with_all = ["hex", "stdin"])]
    pub(super) file: Option<PathBuf>,
    #[arg(long, conflicts_with_all = ["hex", "file"])]
    pub(super) stdin: bool,
    #[arg(long, default_value_t = 0)]
    pub(super) offset: u64,
    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    pub(super) output: OutputFormat,
}

#[derive(Args)]
pub(super) struct EncodeArgs {
    #[command(flatten)]
    pub(super) shape: FieldShapeArgs,
    #[arg(long)]
    pub(super) value: String,
    #[arg(long)]
    pub(super) sign_nibble: Option<String>,
    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    pub(super) output: OutputFormat,
}

#[derive(Args)]
pub(super) struct BatchInputArgs {
    #[arg(long)]
    pub(super) schema: PathBuf,
    #[arg(long)]
    pub(super) input: PathBuf,
    #[arg(long, value_enum)]
    pub(super) output: Option<OutputFormat>,
    #[arg(long, help = "Maximum records to process before stopping")]
    pub(super) max_records: Option<usize>,
    #[arg(
        long,
        help = "Maximum buffered rows for --output json; 0 disables the default 100,000-row cap"
    )]
    pub(super) max_rows: Option<usize>,
    #[arg(long, help = "Decode fixed-width binary records with N worker chunks")]
    pub(super) parallel: Option<usize>,
    #[arg(
        long,
        help = "Validate record framing and count records without field decode"
    )]
    pub(super) dry_run: bool,
    #[arg(short, long, help = "Suppress table headers and non-data output")]
    pub(super) quiet: bool,
    #[arg(long, default_value_t = MAX_FAILURE_SAMPLES)]
    pub(super) sample_failures: usize,
    #[arg(long)]
    pub(super) strict_record: bool,
    #[arg(
        long,
        help = "Return exit code 1 when no records or fields are processed"
    )]
    pub(super) fail_on_empty: bool,
    #[arg(long, help = "Emit record_index values starting at 1 instead of 0")]
    pub(super) one_based_index: bool,
    #[arg(long, help = "Write progress summaries to stderr only")]
    pub(super) progress: bool,
    #[arg(long, value_enum, default_value_t = EvidenceMode::Minimal)]
    pub(super) evidence_mode: EvidenceMode,
}

#[derive(Args)]
pub(super) struct CompletionsArgs {
    #[arg(value_enum)]
    pub(super) shell: Shell,
}
