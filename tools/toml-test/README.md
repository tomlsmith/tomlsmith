# TOML conformance

This harness checks TomlSmith against the official [`toml-lang/toml-test`](https://github.com/toml-lang/toml-test) decoder suites for TOML 1.0 and TOML 1.1.

## Run

Requirements are Bash, Rust/Cargo, and Go 1.19 or newer.

Run the same complete gate used by CI:

```bash
bash tools/toml-test/run.sh
```

Run only the local decoder tests:

```bash
cargo test -p tomlsmith-test-decoder
```

## Pinned suite

The harness uses `toml-test` **v2.2.0** at commit [`ce08da1ddb075d1c7596d663c7fcba9a2ae02c5c`](https://github.com/toml-lang/toml-test/commit/ce08da1ddb075d1c7596d663c7fcba9a2ae02c5c), with Go module checksum `h1:q3ELZu7oPnpl9TClC6OOcAccXwj+jwAyFP8WvzBdK1M=`. Both TOML versions run without a skip list.

## Expected results

| Corpus | Valid passed | Valid failed | Invalid passed | Invalid failed |
| --- | ---: | ---: | ---: | ---: |
| TOML 1.0.0 | 205 | 0 | 474 | 0 |
| TOML 1.1.0 | 214 | 0 | 467 | 0 |

TomlSmith passes all 1,360 decoder cases in this pinned suite, with zero failures and zero skips.

## Scope and output

The claim covers TOML decoding only; TomlSmith does not currently expose an encoder. It is intentionally scoped to the pinned upstream suite rather than behaviors that suite does not test.

Reports are written to `target/toml-test/v2.2.0/reports/`. Set `TOMLSMITH_CONFORMANCE_DIR` to store downloaded tools, build artifacts, and reports elsewhere.
