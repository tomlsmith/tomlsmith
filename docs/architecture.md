# Architecture overview

TomlSmith is organized around one deep in-process language module and thin adapters. The current public seam is an immutable `Document`.

```text
                       CLI     LSP
                        \       /
                       protocol adapters
                              |
                    public Document facade
                              |
          +-------------------+-------------------+
          |                   |                   |
       syntax             semantics          formatting
          |                   |                   |
          +---------- shared snapshot ------------+
                              |
                 private Rowan green tree
```

The diagram describes the intended module boundary, not a claim that every box is complete today.

## Core contract

`Document` owns source text, a private lossless syntax snapshot, a selected TOML version, and diagnostics. It is immutable, cheap to clone, and safe to share between threads. Future edits produce another snapshot rather than mutating a tree observed by an in-flight request.

`SemanticDocument::root` materializes the decoded TOML value tree without discarding the declaration model used for ambiguity and conflict diagnostics. Tables and array-of-tables are nested semantic values; date/time values retain their original spelling while exposing a TOML type and canonical protocol value. The official conformance decoder consumes this root directly and only maps it to the upstream tagged-JSON wire format.

The core follows these contracts:

- syntax errors return a `Document` plus diagnostics rather than a top-level parsing error;
- every source byte, including trivia and invalid text, remains represented;
- public source ranges are UTF-8 byte offsets;
- TOML version selection is explicit and formatting never upgrades a version;
- semantic conflicts retain every declaration instead of choosing a winner;
- formatting preserves literal and key spelling and refuses unsafe rewrites;
- collection and key/table nesting are bounded at 256 levels while the CST remains lossless; the supported depth exceeds the specification's suggested minimum of 100;
- old snapshot results can be discarded using revision information; and
- a future incremental implementation must preserve observable full-reparse behavior.

## Module depth and locality

The public facade has high leverage because it keeps coordination knowledge in the module that owns it. Parser recovery sets live beside parser productions; declaration-conflict rules live in semantic analysis; comment attachment and line breaking live in the formatter. LSP and CLI adapters must not recreate any of that knowledge.

Rowan is deliberately hidden. Public signatures use TomlSmith-owned document, diagnostic, range, edit, semantic, and highlight types. The following remain implementation details:

- Rowan green/red nodes and language kinds;
- lexer tokens, parser events, and the tree sink;
- typed-AST generation machinery;
- semantic arenas and indexes;
- formatter layout IR and text-diff algorithm;
- line-index storage and incremental cache invalidation.

This boundary permits internal replacement without imposing Rowan's node lifetime, thread, or compatibility model on callers.

## Dependencies and seams

Lexer, parser, Rowan, semantic analysis, diagnostics, highlighting, and formatting are core in-process dependencies. They call one another directly and are not split behind public plugin traits merely for hypothetical flexibility.

Filesystem and configuration access are local-substitutable dependencies. Adapters read them and create or update a `Document`; the language core itself does not assume a current working directory, home directory, editor, or LSP runtime. Tests can therefore use in-memory text and configuration.

The core accepts Rust `str`, so byte encoding is an adapter responsibility. Byte-oriented CLI and conformance adapters reject malformed UTF-8 as invalid content before constructing a `Document`; filesystem failures remain operational errors. This keeps encoding policy outside the parser without silently replacing source bytes.

Network schema retrieval is a true external dependency. When schema support is introduced, resolution, timeout, cache, authentication, proxy, offline, and cancellation behavior will sit behind a separately versioned provider port. The downloaded immutable artifact is passed into semantic analysis; analysis does not perform ambient network access.

Lint rules and schema providers are planned extension seams. TOML grammar and formatter layout internals are intentionally not extension seams: allowing plugins to rewrite the core grammar or formatter would weaken conformance, determinism, and safe-edit guarantees.

Editor integrations live outside this repository and connect through the stdio LSP boundary. They may own editor activation, settings, client packaging, and server process lifecycle, but not TOML language rules.

## Adapter responsibilities

Adapters own environment-specific translation only:

| Adapter | Owns | Must not own |
| --- | --- | --- |
| CLI | arguments, streams, exit codes, filesystem orchestration | TOML parsing rules |
| LSP | JSON-RPC, UTF-8/UTF-16 conversion, position clamping, per-request panic isolation, revision checks | separate semantic interpretation |
| `toml-test` | process protocol and tagged-JSON serialization | an independent TOML decoder |
| Schema host | resource lookup, network/cache policy | hidden mutation of a document snapshot |

This separation keeps observable behavior consistent across every product surface and makes protocol layers testable using ordinary core results.

Request cancellation (`$/cancelRequest`) is not implemented yet; the current
server processes messages synchronously in arrival order. Cancellation moves
into the LSP adapter's responsibilities together with the planned
request-dispatch rework.

Internal path/key maps use a deterministic Fx-style hasher rather than
SipHash: they are never iterated, so ordering cannot become observable, and
the speedup on parse-heavy paths is substantial. The tradeoff — reduced
hash-flooding resistance on attacker-chosen keys, the same one rustc and
rust-analyzer accept — is intentional and should be revisited alongside the
fuzzing and resource-budget release gate.

## Known internal debt

The raw-text value splitter that once re-scanned every payload inside
semantic lowering has been retired: values now lower exclusively from the
green tree, so string-lexing knowledge lives in the lexer (plus the
byte-scanning validator) and literal parsing decodes complete tokens.
Degenerate payloads — parser-recovery leftovers, unterminated strings,
depth-limited collections — follow lexer token boundaries and surface as
`SemanticValue::Invalid` carrying the trimmed source slice of their own
span, with `INVALID_VALUE` pointing at the first offending element. The
`value_lowering_tests` invariants and the public
`value_lowering_edges` suite pin these semantics.
