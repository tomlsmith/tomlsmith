# TomlSmith agent instructions

This file applies to the entire repository.

## Sources of truth

- Read the relevant sections of `CONTRIBUTING.md` before changing code, tests, workflows, or documentation.
- Read `docs/architecture.md` before changing the `Document` public seam, syntax-tree exposure, formatter contract, or ownership between the core and adapters.
- Treat `.github/workflows/ci.yml` as the source of truth for the complete validation matrix and current MSRV check.

## Core contracts

- Keep TOML lexing, parsing, validation, semantics, highlighting, and formatting in `crates/tomlsmith`. Keep the CLI, LSP, and `toml-test` decoder as adapters over `Document`; route language behavior through the core instead of duplicating it.
- Preserve lossless, error-tolerant parsing: parsing malformed and truncated TOML text produces a `Document` with diagnostics, terminates without panic, and retains every source byte in the syntax tree.
- The core accepts Rust `str` and exposes UTF-8 byte ranges. Byte-oriented adapters reject invalid UTF-8 without replacement; UTF-16 conversion belongs only in the LSP adapter.
- Carry the explicitly selected TOML version through every operation. `Document::parse` defaults to 1.1, so acceptance tests must select 1.0 explicitly when proving version boundaries.
- Retain every conflicting declaration instead of applying last-write-wins behavior.
- Formatting changes layout only, preserves key and literal spelling and comments, is idempotent, and refuses unsafe rewrites.
- Keep public signatures in TomlSmith-owned types; the detailed private implementation boundary is defined in `docs/architecture.md`.

## Generated syntax

- `crates/tomlsmith/src/syntax/ast/generated.rs` is generated output. After changing `crates/tomlsmith/syntax.ungram` or its generator in `xtask/src/main.rs`, run `cargo run --package xtask -- codegen`, then verify with `cargo run --package xtask -- codegen --check`; do not edit the output by hand.

## Verification

- Add focused regression tests in the crate that owns the behavior. Assert stable diagnostic codes unless exact wording is the intended contract.
- Use the standard Rust gate from `CONTRIBUTING.md`; use the CI workflow for the additional MSRV gate.
- Changes that can alter TOML acceptance or rejection, error severity, decoded semantic values, or decoder protocol output must also pass `bash tools/toml-test/run.sh`. Local decoder package tests are not a substitute for the pinned upstream TOML 1.0 and 1.1 suites.
- Core public API changes must also pass `bash .github/scripts/check-rowan-api.sh`.
- Verify manifest and policy-only changes directly without adding artificial test files.

## Documentation

- Keep `README.md` and `README.zh-Hans.md` user-facing and synchronized. Put durable contributor or architecture details in `CONTRIBUTING.md` or `docs/architecture.md`.
- Keep research, plans, ADR drafts, roadmaps, and agent scratch context under the ignored paths already declared in `.gitignore`; they are local process artifacts rather than repository authority.
- Keep each ordinary Markdown paragraph, list item, and blockquote paragraph on one physical source line. Do not add trailing double-space hard breaks or repository-wide Markdown formatting dependencies.
