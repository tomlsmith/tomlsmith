# TomlSmith

**English** | [简体中文](README.zh-Hans.md)

<p align="center"><img src="assets/tomlsmith-icon.svg" width="144" alt="TomlSmith icon"></p>

[![CI](https://github.com/tomlsmith/tomlsmith/actions/workflows/ci.yml/badge.svg)](https://github.com/tomlsmith/tomlsmith/actions/workflows/ci.yml) [![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**Parse, check, and format TOML 1.0 and 1.1 from Rust, the command line, or an editor.**

> **Status:** Pre-alpha. The command-line interface, Rust API, and formatter behavior may still change.

## Features

In the terminal and CI:

- Validate TOML 1.0 or 1.1 and report diagnostics with source locations.
- Format documents while preserving comments and literal spellings in covered cases, or use `fmt --check` without changing files.
- Emit machine-readable diagnostic JSON for tools and automation.

In editors:

- Diagnostics, formatting, semantic highlighting, hover, document symbols, and folding through LSP.

In Rust applications:

- Parse once and query diagnostics, formatted output, highlights, and decoded values through the immutable `Document` API.

TomlSmith passes all 1,360 `toml-test` decoder cases for TOML 1.0 and 1.1. See [TOML conformance](tools/toml-test/README.md) for the scope and command.

## Quick start

The first public release will use npm as the primary CLI installation path:

```bash
pnpm add --save-dev @tomlsmith/cli
pnpm exec tomlsmith check Cargo.toml
pnpm exec tomlsmith fmt Cargo.toml
pnpm exec tomlsmith fmt --check Cargo.toml
pnpm exec tomlsmith parse Cargo.toml
```

TOML 1.1 is the default. Select TOML 1.0 explicitly when needed:

```bash
pnpm exec tomlsmith --toml-version 1.0 check Cargo.toml
```

Defaults intentionally differ for editor compatibility: the LSP and VS Code extension default to TOML 1.0. See the [TOML version policy](docs/version-policy.md) for the complete matrix and integration rules.

Every command accepts a file path or `-` for standard input. Run `pnpm exec tomlsmith --help` to list all commands and options. `@tomlsmith/cli` requires Node.js 22.12 or newer and installs a platform-specific native executable without running an install-time download script.

Until the first npm release, run the private Rust adapter from a source checkout with `cargo run -p tomlsmith-cli -- <arguments>`.

The preview support matrix, Rust MSRV, and platform-change policy are documented in [Support policy](docs/support-policy.md).

## Use from Rust

```rust
use tomlsmith::{Document, TomlVersion};

let document = Document::parse_as("title = \"TomlSmith\"\n", TomlVersion::V1_1);
assert!(document.diagnostics().is_empty());
```

## Editor support

`tomlsmith-lsp` implements the Language Server Protocol over stdio. The [TomlSmith for VS Code](https://github.com/tomlsmith/vscode-plugin) extension launches it for VS Code; other editors can configure it as a generic LSP server.

Schema-backed completion and code actions are not available yet.

## Related projects

- [TomlSmith Playground](https://github.com/tomlsmith/playground) — check and format TOML 1.0 and 1.1 in the browser.
- [TomlSmith for VS Code](https://github.com/tomlsmith/vscode-plugin) — use TomlSmith language features in VS Code.
- [TomlSmith Benchmark](https://github.com/tomlsmith/benchmark) — compare end-to-end TOML checker and formatter performance.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request. Participation is governed by the [Code of Conduct](CODE_OF_CONDUCT.md).

## License

TomlSmith is released under the [MIT License](LICENSE).
