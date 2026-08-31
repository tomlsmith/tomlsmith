# TomlSmith

[English](README.md) | **简体中文**

<p align="center"><img src="assets/tomlsmith-icon.svg" width="144" alt="TomlSmith 图标"></p>

[![CI](https://github.com/tomlsmith/tomlsmith/actions/workflows/ci.yml/badge.svg)](https://github.com/tomlsmith/tomlsmith/actions/workflows/ci.yml) [![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**在 Rust、命令行或编辑器中解析、检查和格式化 TOML 1.0 与 1.1。**

## 主要能力

在终端和 CI 中：

- 按 TOML 1.0 或 1.1 检查文档，并报告带源码位置的诊断。
- 格式化文档；已覆盖的场景会保留注释和字面量写法，也可以使用 `fmt --check` 只检查、不改文件。
- 为工具和自动化流程输出机器可读的诊断 JSON。

在编辑器中：

- 通过 LSP 提供带稳定代码的错误与警告诊断、格式化、结构化语义高亮、悬停提示、文档符号和折叠。

在 Rust 应用中：

- 解析一次文档，再通过不可变的 `Document` API 查询诊断、格式化结果、高亮和解码后的值。源码区间高亮会区分普通表、表数组、标量值键、数组值键和内联表值键。

TomlSmith 通过 TOML 1.0 与 1.1 的全部 1,360 个 `toml-test` 解码用例。测试范围与命令见 [TOML 一致性测试](tools/toml-test/README.md)。

## 快速开始

从 crates.io 安装原生 CLI：

```bash
cargo install tomlsmith-cli --version '=0.2.0' --locked
tomlsmith check Cargo.toml
tomlsmith fmt Cargo.toml
tomlsmith fmt --check Cargo.toml
tomlsmith parse Cargo.toml
```

默认使用 TOML 1.1；需要时可以显式选择 TOML 1.0：

```bash
tomlsmith --toml-version 1.0 check Cargo.toml
```

为兼容现有编辑器生态，各入口的默认值并不完全相同：LSP 与 VS Code 扩展默认使用 TOML 1.0。完整矩阵和接入规则见 [TOML 版本策略](docs/version-policy.md)。

所有命令都可以读取文件路径或使用 `-` 从标准输入读取。运行 `tomlsmith --help` 可列出全部命令与参数。仓库仍维护面向 Node.js 使用者的可选 npm wrapper，但 CLI 和其他 TomlSmith 仓库都不依赖它。

平台矩阵、Rust MSRV 与平台变更规则见[支持策略](docs/support-policy.md)。

## 在 Rust 中使用

```rust
use tomlsmith::{Document, TomlVersion};

let document = Document::parse_as("title = \"TomlSmith\"\n", TomlVersion::V1_1);
assert!(document.diagnostics().is_empty());
```

## 编辑器支持

`tomlsmith-lsp` 通过 stdio 实现 Language Server Protocol。[TomlSmith for VS Code](https://github.com/tomlsmith/vscode-plugin) 扩展会在 VS Code 中启动它；其他编辑器可以把它配置为通用 LSP 服务器。

已实现的语言服务器能力不包括基于 Schema 的补全和代码操作。

## 相关项目

- [TomlSmith Playground](https://github.com/tomlsmith/playground) — 在浏览器中检查和格式化 TOML 1.0 与 1.1。
- [TomlSmith for VS Code](https://github.com/tomlsmith/vscode-plugin) — 在 VS Code 中使用 TomlSmith 语言能力。
- [TomlSmith Benchmark](https://github.com/tomlsmith/benchmark) — 比较 TOML 检查器与格式化器的端到端性能。

## 参与贡献

提交 Pull Request 前请阅读 [CONTRIBUTING.md](CONTRIBUTING.md)。参与项目需要遵守 [行为准则](CODE_OF_CONDUCT.md)。

## 许可证

TomlSmith 依据 [MIT License](LICENSE) 发布。
