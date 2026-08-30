# tomlsmith-lsp

`tomlsmith-lsp` exposes the TomlSmith language core over the Language Server Protocol. It provides diagnostics, document formatting, semantic tokens, hover information, document symbols, and folding ranges over stdio.

```bash
tomlsmith-lsp
```

Clients select TOML 1.0 or 1.1 and formatter options through `initializationOptions` and configuration updates. The absent-option default is TOML 1.0 for compatibility with existing ecosystem consumers; see the repository's [version policy](https://github.com/tomlsmith/tomlsmith/blob/main/docs/version-policy.md). Schema-backed completion and code actions are not available yet.

The protocol and configuration surface is pre-alpha. See the [TomlSmith repository](https://github.com/tomlsmith/tomlsmith) for editor setup and compatibility details.
