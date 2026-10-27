#!/usr/bin/env bash
# Runs inside the builder VM. Tests the named packages with the Rust the
# tree's rust-toolchain names and checks their formatting with the rustfmt the
# tree was formatted with.
set -euo pipefail

if [[ $# -lt 2 ]]; then
  echo "usage: test.sh <tree-dir> <package>..." >&2
  exit 2
fi
tree=$(realpath "$1")
shift

. "$(dirname "$0")/env.sh"
cd "$tree"
packages=()
for p in "$@"; do
  packages+=(-p "$p")
done

toolchain=$(tr -d '[:space:]' <rust-toolchain)
run rustup toolchain install "$toolchain" --profile minimal
status=0
run cargo "+$toolchain" test --locked "${packages[@]}" || status=1
run env PATH="${rustfmt_nightly}/bin:${PATH}" cargo-fmt fmt "${packages[@]}" -- --check || status=1
exit "$status"
