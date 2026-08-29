#!/usr/bin/env bash
set -euo pipefail

readonly TOML_TEST_VERSION="v2.2.0"
readonly TOML_TEST_REVISION="ce08da1ddb075d1c7596d663c7fcba9a2ae02c5c"
readonly TOML_TEST_MODULE="github.com/toml-lang/toml-test/v2"
readonly TOML_TEST_MODULE_SUM="h1:q3ELZu7oPnpl9TClC6OOcAccXwj+jwAyFP8WvzBdK1M="

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd -- "$script_dir/../.." && pwd)
report_root=${TOMLSMITH_CONFORMANCE_DIR:-"$repo_root/target/toml-test/$TOML_TEST_VERSION"}
runner_bin="$report_root/bin/toml-test"
cargo_target="$report_root/cargo-target"

if (($# != 0)); then
  printf 'usage: %s\n' "$0" >&2
  exit 2
fi

mkdir -p "$report_root/bin" "$report_root/reports"

module_metadata=$(go mod download -json "$TOML_TEST_MODULE@$TOML_TEST_VERSION")
module_sum=$(sed -n 's/^[[:space:]]*"Sum": "\([^"]*\)",$/\1/p' <<<"$module_metadata")
if [[ "$module_sum" != "$TOML_TEST_MODULE_SUM" ]]; then
  printf 'unexpected %s module sum: %s\n' "$TOML_TEST_VERSION" "$module_sum" >&2
  exit 2
fi
GOBIN="$report_root/bin" go install "$TOML_TEST_MODULE/cmd/toml-test@$TOML_TEST_VERSION"

(
  cd -- "$repo_root"
  CARGO_TARGET_DIR="$cargo_target" cargo build --locked -p tomlsmith-test-decoder
)

# toml-test splits the decoder command on whitespace, so keep its executable in
# a guaranteed whitespace-free command path even when the checkout path has spaces.
command_dir=$(mktemp -d /tmp/tomlsmith-toml-test.XXXXXX)
trap 'rm -rf -- "$command_dir"' EXIT
cp -- "$cargo_target/debug/tomlsmith-test-decoder" "$command_dir/decoder"

run_version() {
  local toml_version=$1
  local report="$report_root/reports/toml-$toml_version.json"
  local decoder="$command_dir/decoder --toml-version $toml_version"
  local -a runner_args=(
    "test"
    "-toml=$toml_version"
    "-decoder=$decoder"
    "-parallel=1"
    "-timeout=5s"
    "-json"
  )

  printf 'running TOML %s conformance with toml-test %s (%s)\n' \
    "$toml_version" "$TOML_TEST_VERSION" "$TOML_TEST_REVISION"
  local runner_exit=0
  NO_COLOR=1 "$runner_bin" "${runner_args[@]}" > "$report" || runner_exit=$?
  printf 'report: %s\n' "$report"
  return "$runner_exit"
}

run_exit_code=0
run_version "1.0" || run_exit_code=1
run_version "1.1" || run_exit_code=1
"$cargo_target/debug/tomlsmith-test-report" \
  "$report_root/reports/toml-1.0.json" \
  "$report_root/reports/toml-1.1.json" || run_exit_code=1
exit "$run_exit_code"
