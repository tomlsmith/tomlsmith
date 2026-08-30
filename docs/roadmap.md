# Roadmap

TomlSmith is being built in vertical slices. Checkboxes describe repository implementation status, not a promise of release dates.

This file covers the language core: the `tomlsmith` library, `tomlsmith-cli`, and `tomlsmith-lsp`. Sibling repositories own their own milestones:

- [TomlSmith for VS Code roadmap](https://github.com/tomlsmith/vscode-plugin/blob/main/docs/roadmap.md) — extension packaging, marketplace listing, client features
- [TomlSmith Playground roadmap](https://github.com/tomlsmith/playground/blob/main/docs/roadmap.md) — hosted deployment and browser workbench features
- [TomlSmith Benchmark roadmap](https://github.com/tomlsmith/benchmark/blob/main/docs/roadmap.md) — corpus coverage, measurement rigor, published results

## Phase 0: repository and core seam

- [x] Rust workspace, pinned toolchain, formatting and lint policy
- [x] MIT licensing and project contribution policy
- [x] Immutable `Document` public facade
- [x] Private Rowan-backed green tree
- [x] Source-preserving lexer/parser skeleton
- [x] Stable diagnostic codes with UTF-8 byte ranges
- [ ] Complete adversarial, truncation, and lossless property coverage

## Phase 1: language correctness

- [x] Complete TOML 1.0 lexical, grammar, and decoder behavior
- [x] Complete TOML 1.1 lexical, grammar, and decoder behavior
- [x] Version-specific diagnostics without auto-detection
- [x] Generated internal typed AST over the private syntax representation
- [x] Exact string, integer, float, date, time, and datetime validation
- [x] `toml-test` decoder coverage for TOML 1.0 and 1.1
- [ ] Fuzzing for termination, panic freedom, and losslessness

The decoder gate runs every configured `toml-test` case: TOML 1.0.0 passes 205 valid and 474 invalid cases; TOML 1.1.0 passes 214 valid and 467 invalid cases. No allowlist or skip list is used. Project regression tests also cover RFC 3339 leap-second placement, invalid UTF-8 adapters, BOM preservation, lone carriage returns in value position, and bounded nesting. This milestone covers parsing and decoding, not editor features.

## Phase 2: semantic model and safe formatting

- [x] Initial declaration model with ambiguity-preserving duplicate-key lookup
- [x] Complete key/table/array-of-tables conflict behavior for both official corpora
- [x] Snapshot-preserving text changes with validated UTF-8 byte ranges
- [x] Initial layout formatter with idempotence regression coverage
- [x] Preserve literal spelling and comment text in covered formatter cases
- [x] Refuse full-document formatting around parse, version, and semantic errors
- [x] Path-indexed namespace checks and trie-based inline-table conflict detection (linear-time semantic lowering)
- [x] Profile-driven lowering optimizations: green-tree traversal, `Arc<str>` key segments end to end, borrow-based value splitting, and byte-level validation scans
- [x] Cold-start optimizations: fat-LTO release profile, lazily materialized semantic root and resolve index, native-target validate/highlight overlap, a sequential WebAssembly path, byte-advancing lexer, and a fused `Document::parse_and_format_with` path
- [ ] Complete formatter behavior for all TOML structures and comment positions
- [ ] Minimal edits for editor integrations

## Phase 3: reusable tooling surfaces

- [x] Initial source-backed syntax highlighting classifications
- [x] CLI for checking, JSON diagnostics, formatting, and format checking
- [x] CLI formatter options (`--indent-width`, `--line-width`, `--line-ending`) with atomic in-place writes
- [x] LSP incremental synchronization and diagnostics
- [x] LSP formatting, semantic tokens, symbols, folding, and hover
- [x] LSP line-index position conversion, specification-conforming position clamping, and per-request panic isolation
- [x] Warning-free public Rust API documentation enforced by `missing_docs` and rustdoc release gates
- [ ] Schema-aware diagnostics, completion, and code actions
- [ ] LSP server documentation for non-VS Code clients (capabilities, `initializationOptions`)

Completion, schema-aware hover, and schema-aware diagnostics follow after the core parser and semantic contracts are stable enough to avoid duplicating work. Editor-client milestones (TextMate grammar, extension lifecycle, packaging) live in the [VS Code repository roadmap](https://github.com/tomlsmith/vscode-plugin/blob/main/docs/roadmap.md).

## Phase 4: extensibility and performance

- [ ] Versioned schema-provider port with offline/cache behavior, compatible with the SchemaStore.org catalog
- [ ] Versioned in-process lint-rule interface
- [ ] Incremental reparse only where profiling proves it beneficial
- [x] CST-driven semantic lowering that retires the raw-text value splitter (guarded by a dual-channel equivalence property test) and gives nested values precise source ranges — value lowering now walks the green tree exclusively, the splitter and its byte-level fallback are deleted, degenerate payloads follow lexer token boundaries, and `INVALID_VALUE` points at the first offending element
- [ ] Statement-level recovery inside unclosed arrays and inline tables so later lines keep their diagnostics
- [ ] Version-aware TOML encoder and the toml-test encoder lanes in the conformance gate
- [ ] Decision record on serde `Deserialize` support: bridge it or explicitly delegate data binding to `toml`/`toml_edit`

Incremental parsing is an internal optimization, not a user-visible semantic mode. Clean full parsing and incremental updates must produce equivalent observable syntax, semantic, diagnostic, highlight, and formatting results. Performance regression budgets against real-world corpora are tracked in the [benchmark repository roadmap](https://github.com/tomlsmith/benchmark/blob/main/docs/roadmap.md).

## Phase 5: distribution

Correctness that cannot be installed converts no users; distribution is its own track rather than an afterthought of 1.0.

- [x] Build and test an npm workspace with a TypeScript launcher, exact-version native platform packages, and installed-tarball smoke coverage
- [ ] Publish `@tomlsmith/cli` and its native platform packages to npm as the primary CLI distribution
- [ ] Publish the public 0.x Rust library and LSP crates to crates.io; keep the Rust CLI adapter private
- [x] Build, process-smoke, archive, and checksum CLI/LSP binaries for every release target before a GitHub Release is created
- [ ] Prebuilt CLI/LSP binaries on GitHub Releases as the no-Node installation path
- [x] Document the MSRV, tested targets, distribution targets, and support-change policy

Extension packaging and marketplace listing are tracked in the [VS Code repository roadmap](https://github.com/tomlsmith/vscode-plugin/blob/main/docs/roadmap.md); the hosted playground is tracked in the [playground repository roadmap](https://github.com/tomlsmith/playground/blob/main/docs/roadmap.md).

## Release gates

Before a `1.0` release, the project must have:

- documented and passing TOML 1.0/1.1 conformance behavior;
- a stable public Rust API and diagnostic-code policy;
- panic-free fuzzing and resource budgets for editor-facing input;
- formatter idempotence and safe-edit regression suites;
- supported-platform and MSRV policy with automated CI; and
- a migration policy for formatter changes that can create repository-wide diffs.
