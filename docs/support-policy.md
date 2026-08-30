# Support policy

TomlSmith is pre-alpha software. This page defines which environments are continuously checked and which binary artifacts the first public preview is designed to ship. It does not turn the 0.x API or formatter layout into a stability promise.

## Rust toolchain

The minimum supported Rust version (MSRV) is Rust 1.85.0. CI checks every workspace target and feature on that toolchain and separately runs formatting, Clippy, tests, documentation, packaging, conformance, and WebAssembly contract checks on the pinned development toolchain.

Raising the MSRV requires a changelog entry and a 0.x minor release. A dependency update that raises the effective MSRV is treated the same way.

## Runtime and binary targets

| Surface | Continuously checked environments | Preview distribution targets |
| --- | --- | --- |
| Rust library and native adapters | Linux x64, current macOS, current Windows; Rust 1.85.0 workspace check | crates.io source packages for `tomlsmith` and `tomlsmith-lsp` |
| `@tomlsmith/cli` | clean packed-tarball installation and execution on Linux, macOS, and Windows | macOS arm64/x64, Linux arm64/x64 musl, Windows x64 |
| GitHub Release binaries | archive build plus CLI/LSP process smoke on every release target | macOS arm64/x64, Linux arm64/x64 musl, Windows x64 |
| Browser core | `wasm32-unknown-unknown` build and real generated-WASM analysis/formatting tests | Playground static site |

The npm launcher requires Node.js 22.12 or newer. The native CLI and LSP binaries do not require Node.js. Linux npm and Release binaries target musl so they do not inherit a particular glibc baseline. The release workflows set macOS deployment targets explicitly for each architecture.

## Compatibility changes

- Removing a distribution target, raising Node.js requirements, or raising the MSRV requires release notes and a 0.x minor version.
- A platform is not listed as supported merely because the Rust crate can compile there; it must have a repeatable build and a product-level execution smoke test.
- Best-effort source builds on other Rust-supported targets are welcome, but they are not release blockers until added to this matrix.
- Security or correctness fixes may narrow a platform range when no safe compatible implementation exists; the release notes must state the reason and migration path.
