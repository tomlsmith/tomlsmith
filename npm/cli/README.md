# `@tomlsmith/cli`

The npm distribution of the native TomlSmith command-line interface.

```bash
pnpm add --save-dev @tomlsmith/cli
pnpm exec tomlsmith check Cargo.toml
pnpm exec tomlsmith fmt --check Cargo.toml
```

The package installs a prebuilt binary for the current operating system and CPU architecture. It does not download executables from an install script and does not reimplement TomlSmith in JavaScript.

See the [TomlSmith repository](https://github.com/tomlsmith/tomlsmith) for command documentation and source code.
