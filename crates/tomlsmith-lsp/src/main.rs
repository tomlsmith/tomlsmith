#![forbid(unsafe_code)]

use std::{env, error::Error, ffi::OsString, process::ExitCode};

fn main() -> ExitCode {
    let mut arguments = env::args_os().skip(1);
    match (arguments.next(), arguments.next()) {
        (None, None) => run_server(),
        (Some(argument), None) if argument == "--version" || argument == "-V" => {
            println!("tomlsmith-lsp {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        (Some(argument), None) if argument == "--help" || argument == "-h" => {
            println!(
                "tomlsmith-lsp {}\n\nUsage: tomlsmith-lsp [--stdio]\n\nRuns the language server over standard input and output. The optional --stdio flag is accepted for editor-client compatibility.",
                env!("CARGO_PKG_VERSION")
            );
            ExitCode::SUCCESS
        }
        (Some(argument), None) if argument == "--stdio" => run_server(),
        (Some(argument), trailing) => invalid_arguments(&argument, trailing),
        (None, Some(_)) => unreachable!("a second argument requires a first argument"),
    }
}

fn invalid_arguments(argument: &OsString, trailing: Option<OsString>) -> ExitCode {
    let suffix = trailing.map_or_else(String::new, |trailing| {
        format!(" {}", trailing.to_string_lossy())
    });
    eprintln!(
        "tomlsmith-lsp: unexpected argument: {}{suffix}\nTry 'tomlsmith-lsp --help' for more information.",
        argument.to_string_lossy()
    );
    ExitCode::from(2)
}

fn run_server() -> ExitCode {
    match serve_stdio() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("tomlsmith-lsp: {error}");
            ExitCode::FAILURE
        }
    }
}

fn serve_stdio() -> Result<(), Box<dyn Error + Send + Sync>> {
    let (connection, io_threads) = lsp_server::Connection::stdio();
    tomlsmith_lsp::serve(&connection)?;
    drop(connection);
    io_threads.join()?;
    Ok(())
}
