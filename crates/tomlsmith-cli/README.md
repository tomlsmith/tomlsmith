# tomlsmith-cli

`tomlsmith-cli` provides the native `tomlsmith` command for checking, formatting, and parsing TOML 1.0 and 1.1 documents.

```bash
cargo install tomlsmith-cli --version 0.1.0 --locked
tomlsmith check Cargo.toml
tomlsmith fmt --check Cargo.toml
```

Every command accepts a file path or `-` for standard input. Run `tomlsmith --help` for the complete command and option list.

The command is built on the [`tomlsmith`](https://crates.io/crates/tomlsmith) language core. See the [TomlSmith repository](https://github.com/tomlsmith/tomlsmith) for conformance scope, support policy, and editor integrations.
