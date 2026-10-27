#!/usr/bin/env bash
# Runs inside the builder VM. Tests the whole workspace with the Rust the
# tree's rust-toolchain names and reports, with report_tests.py, against the
# tree's support/vm/expected-failures: test binaries that built but did not
# run, loader errors, and failures that are new or gone. The logs stay in
# <log-dir>.
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: verify.sh <tree-dir> <log-dir>" >&2
  exit 2
fi
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
tree=$(realpath "$1")
mkdir -p "$2"
logs=$(realpath "$2")

. "${here}/env.sh"
cd "$tree"

toolchain=$(tr -d '[:space:]' <rust-toolchain)
run rustup toolchain install "$toolchain" --profile minimal
status=0
# --no-run names every test binary cargo builds, which the report checks ran.
run cargo "+$toolchain" test --locked --workspace --no-run >"${logs}/built.log" 2>&1 || status=1
# Failing tests are expected here; the report decides which ones matter.
run cargo "+$toolchain" test --locked --workspace --no-fail-fast -- --test-threads=4 \
  >"${logs}/test.log" 2>&1 || true
python=$(nix build --impure --no-link --print-out-paths "${np}#python3")/bin/python3
# The report is trusted only while its own tests pass.
"$python" -I -m unittest discover -s "$here" -p 'test_*.py' || status=1
"$python" -I "${here}/report_tests.py" "${logs}/built.log" "${logs}/test.log" \
  "${tree}/support/vm/expected-failures" || status=1
echo "verify: logs in ${logs}"
exit "$status"
