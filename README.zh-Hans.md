# TomlSmith

[English](README.md) | **简体中文**

<p align="center"><img src="assets/tomlsmith-icon.svg" width="144" alt="TomlSmith 图标"></p>

> **为什么叫 TomlSmith？** 这个名字由 `TOML` 与 `Smith`（铁匠、工匠）组合而来：正如工匠把原材料锻造成可靠器物，TomlSmith 也通过检查和打磨 TOML，让配置值得信赖。

[![CI](https://github.com/tomlsmith/tomlsmith/actions/workflows/ci.yml/badge.svg)](https://github.com/tomlsmith/tomlsmith/actions/workflows/ci.yml) [![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**从代码和 CI 到编辑器，一套 TOML 工具链。**

TomlSmith 通过 Rust 库、命令行工具和语言服务器，为 TOML 1.0 与 TOML 1.1 提供检查、格式化和语言理解能力。

> **状态：** pre-alpha。命令行接口、Rust API 和格式化行为仍可能调整。

## 主要能力

在终端和 CI 中：

- 按 TOML 1.0 或 TOML 1.1 检查文档，并提供清晰诊断和精确源码位置。
- 安全地格式化文档——在已覆盖的格式化场景中保留注释和字面量拼写——也可以使用 `fmt --check` 而不修改文件。
- 为工具和自动化流程输出机器可读的诊断 JSON。

在编辑器中：

- 通过 LSP 提供诊断、格式化、语义高亮、悬停提示、文档符号和折叠。
- 语法高亮由解析文件的同一引擎驱动，着色始终与解析器的理解一致。

在 Rust 应用中：

- 通过统一的不可变 `Document` API 使用 TOML 解析、诊断、格式化、高亮和语义值。

TomlSmith 已通过固定版本的官方 `toml-test` v2.2.0 中全部 1,360 个 TOML 1.0 与 1.1 解码测试，零失败、零跳过。可复现命令和结果范围请参阅 [TOML 一致性测试](tools/toml-test/README.md)。

## 快速开始

运行 CLI：

```bash
cargo run -p tomlsmith-cli -- check Cargo.toml
cargo run -p tomlsmith-cli -- fmt Cargo.toml
cargo run -p tomlsmith-cli -- fmt --check Cargo.toml
cargo run -p tomlsmith-cli -- parse Cargo.toml
```

默认使用 TOML 1.1；需要时可以显式选择 TOML 1.0：

```bash
cargo run -p tomlsmith-cli -- --toml-version 1.0 check Cargo.toml
```

所有命令都可以读取文件路径或使用 `-` 从标准输入读取。完整命令说明可运行 `cargo run -p tomlsmith-cli -- --help` 查看。

## 在 Rust 中使用

```rust
use tomlsmith::{Document, TomlVersion};

let document = Document::parse_as("title = \"TomlSmith\"\n", TomlVersion::V1_1);
assert!(document.diagnostics().is_empty());
```

## 编辑器支持

`tomlsmith-lsp` 通过 stdio 提供标准的 Language Server Protocol，任何支持 LSP 的编辑器都可以接入。VS Code 客户端由 [TomlSmith for VS Code](https://github.com/tomlsmith/vscode-plugin) 独立维护。

当前尚未提供基于 Schema 的补全和代码操作。

## 相关项目

- [TomlSmith Playground](https://github.com/tomlsmith/playground) — 在浏览器中体验 TOML 1.0 与 1.1 的分析和格式化。
- [TomlSmith for VS Code](https://github.com/tomlsmith/vscode-plugin) — 在 VS Code 中使用 TomlSmith 语言能力。
- [TomlSmith Benchmark](https://github.com/tomlsmith/benchmark) — 比较 TOML 检查器与格式化器的端到端性能。

## 参与贡献

提交 Pull Request 前请阅读 [CONTRIBUTING.md](CONTRIBUTING.md)。参与项目需要遵守 [行为准则](CODE_OF_CONDUCT.md)。

## 许可证

TomlSmith 依据 [MIT License](LICENSE) 发布。
