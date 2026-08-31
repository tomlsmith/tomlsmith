# tomlsmith-lsp

`tomlsmith-lsp` exposes the TomlSmith language core over the Language Server Protocol. It provides Problems-compatible diagnostics with stable codes, document formatting, structural semantic tokens, hover information, document symbols, and folding ranges over stdio.

```bash
tomlsmith-lsp
```

Clients select TOML 1.0 or 1.1 and formatter options through `initializationOptions` and configuration updates. The absent-option default is TOML 1.0 for compatibility with existing ecosystem consumers; see the repository's [version policy](https://github.com/tomlsmith/tomlsmith/blob/main/docs/version-policy.md). The implemented protocol surface does not include Schema-backed completion or code actions.

See the [TomlSmith repository](https://github.com/tomlsmith/tomlsmith) for editor setup, protocol configuration, and compatibility details.
