# Contributing to TomlSmith

Thank you for helping build TomlSmith. Keep correctness and compatibility claims precise whenever interfaces or internals change.

## Before starting

- Search existing issues and pull requests before proposing duplicate work.
- Open an issue or discussion before a large public-API, syntax-tree, formatter, or protocol change.
- Keep pull requests focused. Refactors unrelated to the stated change make correctness review harder.
- Do not present partial TOML support as full conformance.

Report security-sensitive issues privately to `1357711537@qq.com` rather than in a public issue.

## Development setup

Install Git and [rustup](https://rustup.rs/). The pinned toolchain, formatter, and Clippy components are declared in `rust-toolchain.toml` and are selected automatically inside the repository.

Run the Rust checks used by CI:

```bash
cargo fmt --all --check
cargo run --package xtask -- codegen --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
bash .github/scripts/check-rowan-api.sh
```

The native CLI is published as the `tomlsmith-cli` crate. The pnpm workspace is an optional Node.js distribution wrapper around the same executable. Install the Node.js version from `.node-version` and the pinned pnpm version from `package.json`, then run its distribution checks:

```bash
pnpm install --frozen-lockfile
pnpm npm:typecheck
pnpm npm:check
pnpm npm:test
pnpm npm:performance
```

`pnpm npm:package` builds a release Rust binary, stages it in the current platform package, builds the TypeScript launcher, validates version invariants, and writes installable tarballs under `npm/dist`.

`pnpm npm:performance` builds and stages the release CLI, then runs only the Vitest performance suite. Keep it separate from `npm:test`: correctness runs across all supported CI operating systems, while timing-sensitive checks run in their own Ubuntu job without competing with those jobs on the same runner. The suite's growth waterlines subtract the measured process start-up floor and block; its head-versus-comparison-SHA ratios (the merged base for pull requests) are advisory above 1.35x on shared runners and block only above 3x unless `TOMLSMITH_PERFORMANCE_BUDGET_MODE=strict`. Output content is asserted only for the head binary, so a deliberate layout change never fails the performance job. The same job installs Valgrind and runs `tools/performance/callgrind-compare.sh` to compare instruction counts of the head and comparison binaries on generated workloads, the one head-versus-base delta that is deterministic on shared runners (advisory above 1.10x, blocking above 1.25x), and it measures peak resident memory of the release CLI against fixed budgets. The job publishes a step summary and retains the samples, the JSON summary, and toolchain context as a diagnostic artifact; adding the `performance` label to a pull request additionally runs the benchmark repository's Criterion lanes for the head and comparison binaries on four parallel runners.

The optional `.github/workflows/npm-release.yml` workflow is started manually with a version after the `@tomlsmith` scope and publisher configuration are ready. For an initial npm publication, an npm owner must create the scope, enable account 2FA, and temporarily configure a narrowly scoped granular `NPM_TOKEN` repository secret with publish permission and bypass-2FA enabled so the workflow can create the six packages. After they exist, configure `npm-release.yml` as the GitHub Actions trusted publisher for every package with `npm publish` permission, delete the bootstrap secret, and disallow token publishing in each package's settings. npm then uses short-lived OIDC credentials automatically, and workflow retries skip versions already present in the registry only when their archive integrity matches.

Markdown prose is not manually wrapped to a source-column width. Keep each ordinary paragraph, list item, and blockquote paragraph on one physical source line; retain separate lines only where Markdown structure requires them, such as headings, blank lines, tables, lists, and fenced code.

## Testing expectations

- Parser changes should add a focused regression test and preserve every input byte in the lossless tree.
- Changes to `syntax.ungram` must regenerate the checked-in typed syntax wrappers with `cargo run --package xtask -- codegen`.
- Error recovery tests should cover malformed and truncated input and must prove termination without panic.
- Semantic changes should retain all conflicting declarations; tests must not assume silent last-write-wins behavior.
- Formatter changes should demonstrate idempotence, semantic equivalence, and preservation of literal spelling and comments.
- Check and formatter rules must preserve the complexity contracts in `docs/architecture.md`; a change touching whole-document traversal, scope lookup, or layout planning must add or update an allocation-growth case in `tools/complexity-guards/tests/guards.rs` and an in-process growth case in `crates/tomlsmith/tests/complexity.rs`, and may mirror it in the release-CLI suite `npm/cli/tests/performance.test.ts`.
- Formatter layout changes must be visible as a reviewed diff of `crates/tomlsmith/tests/formatter_snapshots/`: add a `<case>.toml` input (plus an optional `<case>.options` file), regenerate the expected files with `TOMLSMITH_UPDATE_SNAPSHOTS=1 cargo test -p tomlsmith --test formatter_snapshots`, and keep only the changes you intended. `bash tools/toml-test/run.sh` additionally formats the whole pinned conformance corpus in both TOML versions and checks guarded refusal, idempotence, and semantic preservation.
- Public diagnostic behavior should assert stable codes, not exact prose unless the wording itself is relevant.
- npm CLI tests exercise the installed process interface with Vitest and a real native executable; do not duplicate Rust command parsing in TypeScript or add standalone JavaScript test scripts.
- Changes to manifests and policy files can be verified directly; they do not need artificial test files.

Official TOML conformance fixtures and third-party parser behavior are different things. The TOML specification and pinned conformance suite are the authority; differential tests are discovery tools, not the correctness oracle.

## Architecture boundaries

The public core is centered on an immutable `Document`. Rowan nodes, lexer tokens, parser events, tree-sink details, semantic arenas, formatter IR, and diff algorithms stay private. CLI and LSP code are adapters and must not duplicate TOML parsing or semantic rules.

Read [docs/architecture.md](docs/architecture.md) before changing these boundaries. Explain user-visible architectural changes in the pull request and update the public architecture document when its contract changes.

## Pull requests

Before requesting review:

1. Rebase or merge the current default branch as appropriate.
2. Run all checks relevant to the changed packages.
3. Explain observable behavior, compatibility impact, and remaining limitations.
4. Add or update documentation for public-facing changes.
5. Keep commits attributable; do not include generated secrets or private data.

By contributing, you agree that your contribution is licensed under the MIT License used by this repository.
