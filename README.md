# TomlSmith

**English** | [简体中文](README.zh-Hans.md)

<p align="center"><img src="assets/tomlsmith-icon.svg" width="144" alt="TomlSmith icon"></p>

> **Why TomlSmith?** The name joins `TOML` with `smith`: just as a smith forges raw material into dependable tools, TomlSmith checks and refines TOML into configuration you can trust.

[![CI](https://github.com/tomlsmith/tomlsmith/actions/workflows/ci.yml/badge.svg)](https://github.com/tomlsmith/tomlsmith/actions/workflows/ci.yml) [![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**One TOML toolchain for code, CI, and editors.**

TomlSmith checks, formats, and understands TOML 1.0 and TOML 1.1 through a Rust library, a command-line tool, and a language server.

> **Status:** TomlSmith is pre-alpha and is not published to crates.io yet. The command-line interface, Rust API, and formatter behavior may still change.

## Features

- Validate TOML 1.0 or TOML 1.1 with clear diagnostics and exact source locations.
- Format documents safely, or use `fmt --check` in CI without changing files.
- Emit machine-readable diagnostic JSON for tools and automation.
- Preserve comments and literal spellings while formatting.
- Provide syntax highlighting data for editor integrations.
- Power editor diagnostics, formatting, semantic highlighting, hover, document symbols, and folding through LSP.
- Embed TOML parsing, diagnostics, formatting, highlighting, and semantic values in Rust applications.

TomlSmith passes all 1,360 decoder cases in the pinned official `toml-test` v2.2.0 suites for TOML 1.0 and 1.1, with zero failures and zero skips. See [TOML conformance](tools/toml-test/README.md) for the reproducible command and result scope.

## Quick start

Run the current CLI from a source checkout:

```bash
cargo run -p tomlsmith-cli -- check Cargo.toml
cargo run -p tomlsmith-cli -- fmt Cargo.toml
cargo run -p tomlsmith-cli -- fmt --check Cargo.toml
cargo run -p tomlsmith-cli -- parse Cargo.toml
```

TOML 1.1 is the default. Select TOML 1.0 explicitly when needed:

```bash
cargo run -p tomlsmith-cli -- --toml-version 1.0 check Cargo.toml
```

Every command accepts a file path or `-` for standard input. Run `cargo run -p tomlsmith-cli -- --help` for the complete CLI reference.

## Use from Rust

```rust
use tomlsmith::{Document, TomlVersion};

let document = Document::parse_as("title = \"TomlSmith\"\n", TomlVersion::V1_1);
assert!(document.diagnostics().is_empty());
```

The Rust API is currently available from a source checkout and is not yet a stable crates.io interface.

## Editor support

`tomlsmith-lsp` provides the shared language-server features. The development-preview VS Code client is maintained in [TomlSmith for VS Code](https://github.com/tomlsmith/tomlsmith-vscode).

Schema-backed completion and code actions are not available yet.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request. Participation is governed by the [Code of Conduct](CODE_OF_CONDUCT.md).

## License

TomlSmith is released under the [MIT License](LICENSE).
