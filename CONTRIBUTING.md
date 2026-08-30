# Contributing to TomlSmith

Thank you for helping build TomlSmith. The project is pre-alpha: interfaces and internals can still change, but correctness and compatibility claims must remain precise.

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

The public CLI is distributed as a pnpm workspace backed by the private Rust `tomlsmith-cli` adapter. Install the Node.js version from `.node-version` and the pinned pnpm version from `package.json`, then run the npm distribution checks:

```bash
pnpm install --frozen-lockfile
pnpm npm:typecheck
pnpm npm:check
pnpm npm:test
```

`pnpm npm:package` builds a release Rust binary, stages it in the current platform package, builds the TypeScript launcher, validates version invariants, and writes installable tarballs under `npm/dist`.

Pushing a matching `vX.Y.Z` tag runs `.github/workflows/npm-release.yml`, builds all five native packages, and publishes them before `@tomlsmith/cli`. For the first release, an npm owner must create the `@tomlsmith` scope, enable account 2FA, and temporarily configure a narrowly scoped granular `NPM_TOKEN` repository secret with publish permission and bypass-2FA enabled so the workflow can create the six packages. After they exist, configure `npm-release.yml` as the GitHub Actions trusted publisher for every package with `npm publish` permission, delete the bootstrap secret, and disallow token publishing in each package's settings. npm will then use short-lived OIDC credentials automatically, and workflow retries skip versions already present in the registry only when their archive integrity matches.

Markdown prose is not manually wrapped to a source-column width. Keep each ordinary paragraph, list item, and blockquote paragraph on one physical source line; retain separate lines only where Markdown structure requires them, such as headings, blank lines, tables, lists, and fenced code.

## Testing expectations

- Parser changes should add a focused regression test and preserve every input byte in the lossless tree.
- Changes to `syntax.ungram` must regenerate the checked-in typed syntax wrappers with `cargo run --package xtask -- codegen`.
- Error recovery tests should cover malformed and truncated input and must prove termination without panic.
- Semantic changes should retain all conflicting declarations; tests must not assume silent last-write-wins behavior.
- Formatter changes should demonstrate idempotence, semantic equivalence, and preservation of literal spelling and comments.
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
