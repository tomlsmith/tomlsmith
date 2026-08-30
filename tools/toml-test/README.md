# TOML conformance

This harness runs the [`toml-lang/toml-test`](https://github.com/toml-lang/toml-test) decoder cases for TOML 1.0 and 1.1 against TomlSmith.

## Run

Requirements are Bash, Rust/Cargo, and Go 1.19 or newer.

Run the same check used by CI:

```bash
bash tools/toml-test/run.sh
```

Run only the local decoder tests:

```bash
cargo test -p tomlsmith-test-decoder
```

## Test dependency

The runner currently uses `toml-test` v2.2.0 at commit [`ce08da1ddb075d1c7596d663c7fcba9a2ae02c5c`](https://github.com/toml-lang/toml-test/commit/ce08da1ddb075d1c7596d663c7fcba9a2ae02c5c), identified by the Go module checksum `h1:q3ELZu7oPnpl9TClC6OOcAccXwj+jwAyFP8WvzBdK1M=`. CI and local runs therefore use the same cases. Neither TOML version uses a skip list.

## Expected results

| Corpus | Valid passed | Valid failed | Invalid passed | Invalid failed |
| --- | ---: | ---: | ---: | ---: |
| TOML 1.0.0 | 205 | 0 | 474 | 0 |
| TOML 1.1.0 | 214 | 0 | 467 | 0 |

## Scope and output

The harness covers TOML decoding only. TomlSmith does not currently expose an encoder, and the result does not cover behavior outside these cases.

Reports are written to `target/toml-test/v2.2.0/reports/`. Set `TOMLSMITH_CONFORMANCE_DIR` to store downloaded tools, build artifacts, and reports elsewhere.
