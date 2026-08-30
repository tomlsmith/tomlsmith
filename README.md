# TomlSmith

**English** | [简体中文](README.zh-Hans.md)

<p align="center"><img src="assets/tomlsmith-icon.svg" width="144" alt="TomlSmith icon"></p>

> **Why TomlSmith?** The name joins `TOML` with `smith`: just as a smith forges raw material into dependable tools, TomlSmith checks and refines TOML into configuration you can trust.

[![CI](https://github.com/tomlsmith/tomlsmith/actions/workflows/ci.yml/badge.svg)](https://github.com/tomlsmith/tomlsmith/actions/workflows/ci.yml) [![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**One TOML toolchain for code, CI, and editors.**

TomlSmith checks, formats, and understands TOML 1.0 and TOML 1.1 through a Rust library, a command-line tool, and a language server.

> **Status:** Pre-alpha. The command-line interface, Rust API, and formatter behavior may still change.

## Features

In the terminal and CI:

- Validate TOML 1.0 or TOML 1.1 with clear diagnostics and exact source locations.
- Format documents safely — comments and literal spellings are preserved in covered formatter cases — or use `fmt --check` without changing files.
- Emit machine-readable diagnostic JSON for tools and automation.

In editors:

- Diagnostics, formatting, semantic highlighting, hover, document symbols, and folding through LSP.
- Syntax highlighting driven by the same engine that parses the file, so colors always match what the parser sees.

In Rust applications:

- Embed TOML parsing, diagnostics, formatting, highlighting, and semantic values behind one immutable `Document` API.

TomlSmith passes all 1,360 decoder cases in the pinned official `toml-test` v2.2.0 suites for TOML 1.0 and 1.1, with zero failures and zero skips. See [TOML conformance](tools/toml-test/README.md) for the reproducible command and result scope.

## Quick start

Run the CLI:

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

## Editor support

`tomlsmith-lsp` speaks the Language Server Protocol over stdio, so any LSP-capable editor can connect to it. The VS Code client is maintained in [TomlSmith for VS Code](https://github.com/tomlsmith/vscode-plugin).

Schema-backed completion and code actions are not available yet.

## Related projects

- [TomlSmith Playground](https://github.com/tomlsmith/playground) — try TOML 1.0 and 1.1 analysis and formatting in the browser.
- [TomlSmith for VS Code](https://github.com/tomlsmith/vscode-plugin) — use TomlSmith language features in VS Code.
- [TomlSmith Benchmark](https://github.com/tomlsmith/benchmark) — compare end-to-end TOML checker and formatter performance.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request. Participation is governed by the [Code of Conduct](CODE_OF_CONDUCT.md).

## License

TomlSmith is released under the [MIT License](LICENSE).
