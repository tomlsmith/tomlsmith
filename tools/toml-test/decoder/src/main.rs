use std::io::Read;

use tomlsmith::TomlVersion;

fn main() {
    let mut arguments = std::env::args().skip(1);
    let version = match (arguments.next().as_deref(), arguments.next().as_deref()) {
        (Some("--toml-version"), Some("1.0")) => TomlVersion::V1_0,
        (Some("--toml-version"), Some("1.1")) => TomlVersion::V1_1,
        _ => {
            eprintln!("usage: tomlsmith-test-decoder --toml-version <1.0|1.1>");
            std::process::exit(2);
        }
    };
    if arguments.next().is_some() {
        eprintln!("usage: tomlsmith-test-decoder --toml-version <1.0|1.1>");
        std::process::exit(2);
    }

    let mut source = String::new();
    if let Err(error) = std::io::stdin().read_to_string(&mut source) {
        eprintln!("failed to read TOML from stdin: {error}");
        std::process::exit(1);
    }
    match tomlsmith_test_decoder::decode(&source, version) {
        Ok(value) => {
            if let Err(error) = serde_json::to_writer(std::io::stdout(), &value) {
                eprintln!("failed to write tagged JSON: {error}");
                std::process::exit(1);
            }
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
