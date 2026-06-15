#[path = "../cli/mod.rs"]
mod cli;

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let payload = serde_json::json!({
            "version": 1,
            "error_code": "E_INTERNAL",
            "message": format!("internal panic: {info}"),
        });
        eprintln!("{payload}");
    }));

    let result = std::panic::catch_unwind(cli::run);
    let code = match result {
        Ok(Ok(())) => cli::ExitCode::Success as i32,
        Ok(Err(err)) => {
            eprintln!("{}", err.render());
            err.exit_code() as i32
        }
        Err(_) => cli::ExitCode::Internal as i32,
    };
    std::process::exit(code);
}
