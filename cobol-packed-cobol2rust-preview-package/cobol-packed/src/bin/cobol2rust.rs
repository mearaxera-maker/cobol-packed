use cobol_codegen_rust::{convert, ConvertError, ConvertOptions, Dialect, SourceFormat};
use std::path::PathBuf;

fn main() {
    let result = std::panic::catch_unwind(run);
    let code = match result {
        Ok(Ok(())) => 0,
        Ok(Err(err)) => {
            eprintln!("{err}");
            match err {
                AppError::Convert(ConvertError::MigrationBlocked { .. }) => 1,
                AppError::Args(_)
                | AppError::Convert(ConvertError::Source(_) | ConvertError::Syntax(_)) => 2,
                AppError::Convert(ConvertError::Io { .. }) => 3,
                AppError::Convert(ConvertError::Report(_)) => 4,
            }
        }
        Err(_) => {
            eprintln!("internal converter panic");
            4
        }
    };
    std::process::exit(code);
}

#[derive(Debug)]
enum AppError {
    Args(String),
    Convert(ConvertError),
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Args(message) => write!(f, "{message}"),
            Self::Convert(err) => write!(f, "{err}"),
        }
    }
}

impl From<ConvertError> for AppError {
    fn from(err: ConvertError) -> Self {
        Self::Convert(err)
    }
}

fn run() -> Result<(), AppError> {
    let args = parse_args()?;
    if args.help {
        print_help();
        return Ok(());
    }
    if args.command.as_deref() != Some("convert") {
        print_help();
        return Err(AppError::Args("missing command: convert".to_string()));
    }

    let input = required_path(&args.input, "--input")?;
    let out_dir = required_path(&args.out, "--out")?;
    let dialect = Dialect::parse(&args.dialect)
        .ok_or_else(|| AppError::Args(format!("invalid --dialect {}", args.dialect)))?;
    let source_format = SourceFormat::parse(&args.source_format)
        .ok_or_else(|| AppError::Args(format!("invalid --source-format {}", args.source_format)))?;
    let project = convert(ConvertOptions {
        input,
        copybook_dirs: args.copybook_dirs,
        out_dir,
        dialect,
        source_format,
    })?;
    println!("generated Rust project: {}", project.out_dir.display());
    println!("migration report: {}", project.report_path.display());
    for file in project.files {
        println!("wrote {}", file.display());
    }
    Ok(())
}

#[derive(Debug, Default)]
struct Args {
    command: Option<String>,
    input: Option<PathBuf>,
    copybook_dirs: Vec<PathBuf>,
    out: Option<PathBuf>,
    dialect: String,
    source_format: String,
    help: bool,
}

fn parse_args() -> Result<Args, AppError> {
    let mut args = std::env::args().skip(1);
    let mut parsed = Args {
        dialect: "ibm".to_string(),
        source_format: "fixed".to_string(),
        ..Args::default()
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => parsed.help = true,
            "convert" if parsed.command.is_none() => parsed.command = Some(arg),
            "--input" => parsed.input = Some(next_path(&mut args, "--input")?),
            "--copybook-dir" => {
                parsed
                    .copybook_dirs
                    .push(next_path(&mut args, "--copybook-dir")?);
            }
            "--out" => parsed.out = Some(next_path(&mut args, "--out")?),
            "--dialect" => {
                parsed.dialect = next_value(&mut args, "--dialect")?;
            }
            "--source-format" => {
                parsed.source_format = next_value(&mut args, "--source-format")?;
            }
            value if value.starts_with('-') => {
                return Err(AppError::Args(format!("unknown option {value}")));
            }
            value => return Err(AppError::Args(format!("unexpected argument {value}"))),
        }
    }
    Ok(parsed)
}

fn next_path(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<PathBuf, AppError> {
    Ok(PathBuf::from(next_value(args, flag)?))
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, AppError> {
    let value = args
        .next()
        .ok_or_else(|| AppError::Args(format!("missing value for {flag}")))?;
    if value.starts_with('-') {
        return Err(AppError::Args(format!("missing value for {flag}")));
    }
    Ok(value)
}

fn required_path(value: &Option<PathBuf>, flag: &str) -> Result<PathBuf, AppError> {
    value
        .clone()
        .ok_or_else(|| AppError::Args(format!("missing required argument {flag}")))
}

fn print_help() {
    println!(
        "cobol2rust convert --input program.cbl --copybook-dir copybooks --out generated-rust --dialect ibm --source-format fixed"
    );
    println!();
    println!("Options:");
    println!("  --input <path>          COBOL source file");
    println!("  --copybook-dir <path>   Copybook directory; may be repeated");
    println!("  --out <path>            Generated Rust project directory");
    println!("  --dialect <ibm|gnucobol|microfocus>");
    println!("  --source-format <fixed|free|auto>");
}
