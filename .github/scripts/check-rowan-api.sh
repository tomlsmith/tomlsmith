#!/usr/bin/env bash
set -euo pipefail

cargo doc --package tomlsmith --lib --no-deps

doc_root="${CARGO_TARGET_DIR:-target}/doc/tomlsmith"
if [[ ! -d "$doc_root" ]]; then
  echo "Rustdoc output for the tomlsmith crate was not found at $doc_root" >&2
  exit 1
fi

if grep -RInE \
  --include='*.html' \
  --exclude-dir='src' \
  'rowan(::|/)' \
  "$doc_root"; then
  echo "Rowan appeared in rendered public API documentation." >&2
  echo "Expose a TomlSmith-owned wrapper instead of a Rowan type." >&2
  exit 1
fi

echo "No Rowan types found in rendered public API documentation."
