use super::constants::MAX_FAILURE_SAMPLES;
use super::{CliSignMode, EvidenceArgv, EvidenceMode, OutputFormat};
use clap::{Args, Parser, Subcommand};
use clap_complete::Shell;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "cobol-packed")]
#[command(version)]
#[command(about = "Professional COMP-3 packed decimal migration and forensic CLI")]
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
    EmitRust(EmitRustArgs),
    FromCopybook(CopybookArgs),
    Compare(SchemaCompareArgs),
}

#[derive(Args)]
pub(super) struct SchemaArgs {
    #[arg(long)]
    pub(super) schema: PathBuf,
    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    pub(super) output: OutputFormat,
}

#[derive(Args)]
pub(super) struct EmitRustArgs {
    #[arg(long)]
    pub(super) schema: PathBuf,
    #[arg(long)]
    pub(super) output: PathBuf,
}

#[derive(Args)]
pub(super) struct CopybookArgs {
    #[arg(long)]
    pub(super) input: PathBuf,
    #[arg(long)]
    pub(super) output: PathBuf,
    #[arg(long)]
    pub(super) record_length: Option<usize>,
    #[arg(long, default_value = "binary")]
    pub(super) input_encoding: String,
    #[arg(long, default_value = "cp037")]
    pub(super) codepage: String,
    #[arg(long, default_value = "big")]
    pub(super) endian: String,
}

#[derive(Args)]
pub(super) struct SchemaCompareArgs {
    #[arg(long)]
    pub(super) left: PathBuf,
    #[arg(long)]
    pub(super) right: PathBuf,
    #[arg(long, value_enum, default_value_t = OutputFormat::Table)]
    pub(super) output: OutputFormat,
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
    #[arg(long)]
    pub(super) max_records: Option<usize>,
    #[arg(long, default_value_t = MAX_FAILURE_SAMPLES)]
    pub(super) sample_failures: usize,
    #[arg(long)]
    pub(super) strict_record: bool,
    #[arg(long, value_enum, default_value_t = EvidenceMode::Minimal)]
    pub(super) evidence_mode: EvidenceMode,
    #[arg(long, value_enum, default_value_t = EvidenceArgv::Redacted)]
    pub(super) evidence_argv: EvidenceArgv,
    #[arg(long)]
    pub(super) coverage_report: bool,
}

#[derive(Args)]
pub(super) struct CompletionsArgs {
    #[arg(value_enum)]
    pub(super) shell: Shell,
}
