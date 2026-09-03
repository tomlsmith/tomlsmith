#!/usr/bin/env bash
# Deterministic instruction-count comparison of two release CLIs.
#
# Wall time on shared CI runners drifts by tens of percent; the number of
# instructions a fixed workload executes does not. This script runs the head
# and comparison binaries under Valgrind's callgrind tool on generated TOML
# workloads, extracts the total instruction count ("Ir") of each run, and
# reports the head/comparison ratio per case. A ratio above the blocking
# budget fails the script; a ratio above the advisory budget is reported.
#
# usage: callgrind-compare.sh HEAD_BIN COMPARISON_BIN [SUMMARY_MARKDOWN]
#   TOMLSMITH_INSTRUCTIONS_ADVISE_ABOVE  advisory ratio (default 1.10)
#   TOMLSMITH_INSTRUCTIONS_BLOCK_ABOVE   blocking ratio (default 1.25)
#   VALGRIND                             valgrind executable (default: valgrind)
set -euo pipefail

if (($# < 2 || $# > 3)); then
  printf 'usage: %s HEAD_BIN COMPARISON_BIN [SUMMARY_MARKDOWN]\n' "$0" >&2
  exit 2
fi
head_bin=$1
comparison_bin=$2
summary=${3:-}
advise_above=${TOMLSMITH_INSTRUCTIONS_ADVISE_ABOVE:-1.10}
block_above=${TOMLSMITH_INSTRUCTIONS_BLOCK_ABOVE:-1.25}
valgrind=${VALGRIND:-valgrind}

for binary in "$head_bin" "$comparison_bin"; do
  if [[ ! -x "$binary" ]]; then
    printf 'not an executable: %s\n' "$binary" >&2
    exit 2
  fi
done
if ! command -v "$valgrind" > /dev/null 2>&1; then
  printf 'valgrind is not installed (set VALGRIND to its path)\n' >&2
  exit 2
fi

workdir=$(mktemp -d "${TMPDIR:-/tmp}/tomlsmith-callgrind.XXXXXX")
trap 'rm -rf -- "$workdir"' EXIT

# Workloads are generated with awk so the script has no dependency beyond a
# POSIX shell; sizes are chosen so each run executes well over 10^8
# instructions and the count is insensitive to start-up.
awk 'BEGIN {
  for (i = 0; i < 4096; i++) {
    printf "[section_%d]\nvalue = %d\nwhen = 1979-05-27 07:32:00Z\n", i, i
    printf "metadata = { left = %d, right = %d }\nitems = [1, 2, 3, 4]\n", i, i + 1
  }
}' > "$workdir/representative.toml"
awk 'BEGIN { for (i = 0; i < 16384; i++) printf "item_%d={left=%d,right=%d}\n", i, i, i + 1 }' \
  > "$workdir/inline-tables.toml"
awk 'BEGIN { for (i = 0; i < 16384; i++) printf "[[table_%d]]\nvalue = %d\n", i, i }' \
  > "$workdir/array-tables.toml"
awk 'BEGIN {
  for (i = 0; i < 128; i++) {
    printf "root_%d = ", i
    for (d = 0; d < 96; d++) printf "{ value = "
    printf "0"
    for (d = 0; d < 96; d++) printf " }"
    printf "\n"
  }
}' > "$workdir/nested.toml"

cases=(
  "representative check|--toml-version 1.1 check -|representative.toml"
  "representative format|--toml-version 1.1 fmt --line-width 65535 -|representative.toml"
  "inline tables expanded|--toml-version 1.1 fmt --line-width 24 -|inline-tables.toml"
  "array tables check|--toml-version 1.1 check -|array-tables.toml"
  "nested tables format|--toml-version 1.1 fmt --line-width 20 -|nested.toml"
)

instructions() {
  local binary=$1 input=$2
  shift 2
  local log="$workdir/callgrind.log"
  # callgrind prints "Collected : <Ir>" on stderr; the out-file is not needed.
  "$valgrind" --tool=callgrind --callgrind-out-file=/dev/null \
    --log-file="$log" "$binary" "$@" < "$input" > /dev/null
  awk '/Collected/ { gsub(",", "", $NF); print $NF; found = 1 } END { if (!found) exit 1 }' "$log"
}

status=0
rows=()
for case in "${cases[@]}"; do
  IFS='|' read -r name arguments input <<< "$case"
  # shellcheck disable=SC2206 # the argument string is script-controlled
  argument_array=($arguments)
  head_ir=$(instructions "$head_bin" "$workdir/$input" "${argument_array[@]}")
  comparison_ir=$(instructions "$comparison_bin" "$workdir/$input" "${argument_array[@]}")
  ratio=$(awk -v h="$head_ir" -v c="$comparison_ir" 'BEGIN { printf "%.4f", h / c }')
  verdict=ok
  if awk -v r="$ratio" -v b="$block_above" 'BEGIN { exit !(r > b) }'; then
    verdict=blocked
    status=1
  elif awk -v r="$ratio" -v a="$advise_above" 'BEGIN { exit !(r > a) }'; then
    verdict=advisory
  fi
  printf '[instructions] %s: comparison %s -> head %s (%sx, %s)\n' \
    "$name" "$comparison_ir" "$head_ir" "$ratio" "$verdict"
  rows+=("| $name | $comparison_ir | $head_ir | $ratio | $verdict |")
done

if [[ -n "$summary" ]]; then
  {
    echo "### Instruction counts (callgrind, deterministic)"
    echo
    echo "| case | comparison Ir | head Ir | ratio | verdict (advisory > ${advise_above}x, block > ${block_above}x) |"
    echo "| --- | --- | --- | --- | --- |"
    printf '%s\n' "${rows[@]}"
  } >> "$summary"
fi

exit "$status"
