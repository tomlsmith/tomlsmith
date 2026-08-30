# tomlsmith

`tomlsmith` is the lossless, error-tolerant language core behind the TomlSmith TOML toolchain. It supports strict TOML 1.0 and 1.1 parsing, diagnostics, decoded values, source-backed highlighting, and guarded formatting through an immutable `Document` snapshot.

```rust
use tomlsmith::{Document, TomlVersion};

let document = Document::parse_as("title = \"TomlSmith\"\n", TomlVersion::V1_1);
assert!(document.diagnostics().is_empty());
assert_eq!(document.text(), "title = \"TomlSmith\"\n");
```

The API is pre-alpha and may change before 1.0. See the [TomlSmith repository](https://github.com/tomlsmith/tomlsmith) for the CLI, language server, conformance scope, and project roadmap.
