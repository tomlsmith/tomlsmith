# TomlSmith

**English** | [简体中文](README.zh-Hans.md)

<p align="center"><img src="assets/tomlsmith-icon.svg" width="144" alt="TomlSmith icon"></p>

[![CI](https://github.com/tomlsmith/tomlsmith/actions/workflows/ci.yml/badge.svg)](https://github.com/tomlsmith/tomlsmith/actions/workflows/ci.yml) [![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**Parse, check, and format TOML 1.0 and 1.1 from Rust, the command line, or an editor.**


https://github.com/user-attachments/assets/1a6e65f9-e0f2-43de-8428-6bea7a4078c6

## Features

In the terminal and CI:

- Validate TOML 1.0 or 1.1 and report diagnostics with source locations.
- Format documents while preserving comments and literal spellings in covered cases, or use `fmt --check` without changing files. TOML 1.1 inline tables stay on one line when they fit the line width (a trailing comma is kept but never forces expansion) and expand one entry per line when they do not, contain a comment, or contain a multi-line string.
- Emit machine-readable diagnostic JSON for tools and automation.

In editors:

- Error and warning diagnostics with stable codes, formatting, structural semantic highlighting, hover, document symbols, and folding through LSP.

In Rust applications:

- Parse once and query diagnostics, formatted output, highlights, and decoded values through the immutable `Document` API. Source-backed highlights distinguish regular tables, arrays of tables, scalar keys, array-valued keys, and inline-table-valued keys.

TomlSmith passes all 1,360 `toml-test` decoder cases for TOML 1.0 and 1.1. See [TOML conformance](tools/toml-test/README.md) for the scope and command.

## Quick start

Install the native CLI from crates.io:

```bash
cargo install tomlsmith-cli --locked
tomlsmith check Cargo.toml
tomlsmith fmt Cargo.toml
tomlsmith fmt --check Cargo.toml
tomlsmith parse Cargo.toml
```

TOML 1.1 is the default. Select TOML 1.0 explicitly when needed:

```bash
tomlsmith --toml-version 1.0 check Cargo.toml
```

The library, CLI, LSP, and VS Code extension all default to TOML 1.1. Select TOML 1.0 explicitly for files that must remain readable by 1.0-only consumers; see the [TOML version policy](docs/version-policy.md) for the complete matrix and integration rules.

Every command accepts a file path or `-` for standard input. Run `tomlsmith --help` to list all commands and options. An optional npm wrapper is maintained in the repository for Node.js consumers, but it is not required by the CLI or any TomlSmith repository.

The support matrix, Rust MSRV, and platform-change policy are documented in [Support policy](docs/support-policy.md).

## Use from Rust

```rust
use tomlsmith::{Document, TomlVersion};

let document = Document::parse_as("title = \"TomlSmith\"\n", TomlVersion::V1_1);
assert!(document.diagnostics().is_empty());
```

## Editor support

`tomlsmith-lsp` implements the Language Server Protocol over stdio. The [TomlSmith for VS Code](https://github.com/tomlsmith/vscode-plugin) extension launches it for VS Code; other editors can configure it as a generic LSP server.

The implemented language-server surface does not include Schema-backed completion or code actions.

## Related projects

- [TomlSmith Playground](https://github.com/tomlsmith/playground) — check and format TOML 1.0 and 1.1 in the browser.
- [TomlSmith for VS Code](https://github.com/tomlsmith/vscode-plugin) — use TomlSmith language features in VS Code.
- [TomlSmith Benchmark](https://github.com/tomlsmith/benchmark) — compare end-to-end TOML checker and formatter performance.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request. Participation is governed by the [Code of Conduct](CODE_OF_CONDUCT.md).

## License

TomlSmith is released under the [MIT License](LICENSE).
