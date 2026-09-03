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

`Document` is a facade over a private immutable `AnalysisSnapshot` containing source text, a lossless syntax snapshot, a selected TOML version, diagnostics, and semantics. It is cheap to clone and safe to share between threads. Future edits produce another snapshot rather than mutating a tree observed by an in-flight request. Syntax, version, and semantic products needed by the diagnostic contract are eager; editor-only products (highlight spans and reformatting of an existing snapshot) share a compact token tape of six bytes per token, lexed on first use and cached, so `check` and one-shot `format` never retain one and an editor pays for at most one lex per snapshot.

`SemanticDocument::root` materializes the decoded TOML value tree without discarding the declaration model used for ambiguity and conflict diagnostics. Tables and array-of-tables are nested semantic values; date/time values retain their original spelling while exposing a TOML type and canonical protocol value. The official conformance decoder consumes this root directly and only maps it to the upstream tagged-JSON wire format.

The core follows these contracts:

- syntax errors return a `Document` plus diagnostics rather than a top-level parsing error;
- every source byte, including trivia and invalid text, remains represented;
- public source ranges are UTF-8 byte offsets;
- TOML version selection is explicit and formatting never upgrades a version;
- semantic conflicts retain every declaration instead of choosing a winner;
- formatting preserves literal and key spelling and refuses unsafe rewrites;
- a trailing comma inside a TOML 1.1 inline table is layout, not an expansion hint: the table is written on one line whenever it fits and the comma is kept in place (`{ a = 1, }`), so a formatter run never turns a fitting table into a multi-line one because of punctuation the author left behind. Tools that treat a trailing comma as a request for one entry per line (tombi's magic trailing comma) make a policy choice that TomlSmith deliberately does not make, because the only facts that expand a table are its width at the real output column, a comment, or a multi-line string;
- collection and key/table nesting are bounded at 256 levels while the CST remains lossless; the supported depth exceeds the specification's suggested minimum of 100;
- old snapshot results can be discarded using revision information; and
- a future incremental implementation must preserve observable full-reparse behavior.

## Performance contracts

Performance is an architectural invariant, not an after-the-fact benchmark target. A new rule must declare its natural locality and use facts owned by that module; it must not recover those facts by repeatedly scanning the document, all previously visited declarations, or text the formatter has already produced.

The current core follows these complexity contracts:

- check's base traversal is linear in tokens. Semantic paths materialize their bounded prefixes; active array-of-table lookup and activation are `O(path depth)` rather than `O(number of prior array tables)`; implicit-parent bookkeeping for a header takes one trie walk rather than one lookup per prefix; and final diagnostic ordering is `O(diagnostics log diagnostics)`;
- formatting is exactly three linear passes over one token tape, with no recursion and no re-lexing of produced text: a reverse pass records each token's next significant and content neighbours, a forward pass computes the canonical flat width and comment/multiline facts of every TOML 1.1 inline table with a stack (memory proportional to the number of tables, not tokens), and one forward render pass writes the output while an explicit delimiter-frame stack carries the layout mode chosen for each inline table at its opening brace;
- an inline table's mode is decided once, when its opening brace is written, from its precomputed flat width and the real output column; arrays wrap at real output columns; an array element that is an inline table starts a fresh line when it fits flat there but not on the current line. Nesting depth increases the produced indentation bytes but never the number of passes;
- formatter time is bounded by input tokens plus produced bytes; transient memory is a few bytes per token plus one entry per inline table plus the single output buffer;
- guarded formatting refuses before it renders, and the one-shot `parse_and_format_with` path skips its speculative render whenever a lexer or parser diagnostic would refuse, using the same predicate as the refusal itself, so invalid input is never laid out; and
- optional snapshot products are lazy and bounded: the token tape is columnar (kind plus start offset, six bytes per token rather than twenty-four), which also bounds the parser's own transient memory. Caches keyed by arbitrary formatter options or document history require a separate design and memory budget.

Forbidden patterns include per-token prefix/suffix scans, scanning every active semantic scope for each declaration, rebuilding complete ancestor strings while planning nested layout, document-wide fixed-point loops whose pass count depends on source nesting or feature count, re-lexing or re-parsing produced text to make a layout decision, and recursion over token-level structure (the parser's 256-level collection limit does not bound the token tape of a document that will be refused). A genuinely necessary fallback must be local, deterministic, structurally bounded, and visible in its module's tests or metrics.

These contracts are proved in Rust on every CI operating system: `tools/complexity-guards` counts allocator calls, bytes, and peak live heap under a counting global allocator and fails when an 8x larger input allocates more than 12x or when peak heap exceeds a fixed number of bytes per input byte (deterministic on every platform); `crates/tomlsmith/tests/complexity.rs` times each operation in-process on an input and on the same input scaled by a large factor and fails when the time grows faster than a waterline any quadratic implementation would cross; `crates/tomlsmith/tests/formatter_snapshots/` pins the formatter's layout byte-for-byte so every layout change is a reviewed diff; and the TOML conformance job runs `crates/tomlsmith/tests/conformance_corpus.rs`, which formats the whole pinned corpus in both versions and checks guarded refusal, idempotence, and semantic preservation. The release CLI's Vitest suite runs on a dedicated Ubuntu job as an end-to-end mirror: its growth waterlines subtract the measured process start-up floor and block, its resident-memory waterlines block at about twice the governed build's peak RSS, its head-versus-comparison-SHA wall-time ratios are advisory on shared runners (published in the step summary and an artifact) and block only on a gross regression, and `tools/performance/callgrind-compare.sh` compares Valgrind instruction counts of the head and comparison binaries, the one head-versus-base delta that is deterministic enough to block at a tight budget. Full-fidelity wall-time, throughput, and peak-RSS measurements against competitors remain in the benchmark repository, whose Criterion lanes also run on demand for a pull request's head and comparison binaries.

Native analysis uses at most one scoped side worker once a source reaches 8 KiB, overlapping independent validation and the optional speculative format with semantic lowering; smaller snapshots avoid thread-start overhead. The worker always joins before the immutable snapshot and guarded format result become observable, and WebAssembly keeps the same result contract with sequential execution.

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
| optional npm launcher | native package selection and process delegation | CLI argument parsing or TOML language behavior |
| LSP | JSON-RPC, UTF-8/UTF-16 conversion, position clamping, per-request panic isolation, revision checks | separate semantic interpretation |
| `toml-test` | process protocol and tagged-JSON serialization | an independent TOML decoder |
| Schema host | resource lookup, network/cache policy | hidden mutation of a document snapshot |

This separation keeps observable behavior consistent across every product surface and makes protocol layers testable using ordinary core results.

The published `tomlsmith-cli` crate is the canonical command-line distribution. The optional `@tomlsmith/cli` TypeScript launcher selects an exact-version platform package and delegates the complete process interface to the same native executable. The launcher does not parse, normalize, or reinterpret user arguments, so npm distribution cannot develop a second CLI interface.

Request cancellation (`$/cancelRequest`) is not implemented; the current
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
