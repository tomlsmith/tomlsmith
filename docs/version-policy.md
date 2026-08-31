# TOML version policy

TomlSmith implements two explicit language modes: the published TOML 1.0.0 and TOML 1.1.0 specifications. Version selection affects parsing, validation, semantic results, and formatter safety checks; results are comparable across surfaces only when the source, TOML version, and relevant options are the same.

## Defaults by surface

| Surface | Default | How to select another version |
| --- | --- | --- |
| Rust `Document::parse` and `FormatOptions::default` | TOML 1.1 | Prefer `Document::parse_as` and set `FormatOptions::target_version` explicitly in reusable integrations |
| Native CLI and `@tomlsmith/cli` | TOML 1.1 | Pass `--toml-version 1.0` or `--toml-version 1.1` |
| Browser playground | TOML 1.1 for a new or reset session | Use the version selector; the choice is stored with the local session |
| `tomlsmith-lsp` without client configuration | TOML 1.1 | Send `tomlVersion` in initialization options or configuration updates |
| TomlSmith for VS Code | TOML 1.1 | Set `tomlsmith.toml.version` |
| Conformance and benchmark harnesses | No implicit policy | Every run selects the version explicitly |

Every product surface defaults to TOML 1.1, the latest published language, so a valid TOML 1.0 document continues to work while new documents can use the full implemented language. TomlSmith does not infer a language version from a filename; integrations that must preserve compatibility with a 1.0-only consumer select TOML 1.0 explicitly.

## Integration rules

- Select TOML 1.0 for files that must be read by a 1.0-only consumer, even when TomlSmith itself can parse them as 1.1.
- Reusable Rust and LSP integrations should select the version explicitly rather than inheriting a product default.
- Parse and format with the same version. Formatting against a different target version performs an additional safety validation and can be refused.
- A change to any product default is release-note material. A change to the Rust default is also treated as an API compatibility change.
