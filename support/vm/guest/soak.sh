#!/usr/bin/env bash
# Runs inside the builder VM: <test binary> <label> <runs>. Runs a test binary
# repeatedly under gdb to catch a hang, interrupting any run that outlives 240
# seconds so gdb prints every thread's stack. gdb runs the binary as its own
# child, since ptrace_scope 1 lets it trace nothing else. Logs land in
# ~/soak-<label>-<run>.log. A builder command that syncs the tree during a soak
# moves the tree away under it, so soak a copy of the binary.
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: soak.sh <test binary> <label> <runs>" >&2
  exit 2
fi
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
bin=$(realpath "$1")
label=$2
runs=$3

# shellcheck source=env.sh
. "${here}/env.sh"
gdb=$(nix build --impure --no-link --print-out-paths "${np}#gdb")/bin/gdb

for i in $(seq "$runs"); do
  log="${HOME}/soak-${label}-${i}.log"
  "$gdb" -q -batch -ex 'handle SIGPIPE nostop noprint pass' -ex run -ex 'thread apply all bt' \
    --args "$bin" --test-threads=4 >"$log" 2>&1 &
  gdb_pid=$!
  for _ in $(seq 240); do
    kill -0 "$gdb_pid" 2>/dev/null || break
    sleep 1
  done
  if kill -0 "$gdb_pid" 2>/dev/null; then
    # Matches the binary's own command line, not this script's or ssh's.
    pkill -INT -f "^${bin} --test-threads=4" || true
    wait "$gdb_pid" || true
    echo "${label} run ${i}: HUNG, $(grep -c '^Thread ' "$log") threads dumped to ${log}"
  else
    wait "$gdb_pid" || true
    echo "${label} run ${i}: $(grep '^test result' "$log" || echo "no test result; see ${log}")"
  fi
done
