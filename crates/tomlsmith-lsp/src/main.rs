#![forbid(unsafe_code)]

use std::{error::Error, process::ExitCode};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("tomlsmith-lsp: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error + Send + Sync>> {
    let (connection, io_threads) = lsp_server::Connection::stdio();
    tomlsmith_lsp::serve(&connection)?;
    drop(connection);
    io_threads.join()?;
    Ok(())
}
