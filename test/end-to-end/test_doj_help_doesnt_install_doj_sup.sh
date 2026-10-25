#!/bin/bash

# A simple test assertion that running `doj sup --help` will not
# attempt to install `karate/doj-sup` if that pkg is not present.

set -ou pipefail

export TESTING_FS_ROOT
TESTING_FS_ROOT=$(mktemp -d)
export DOJ_SUP_BINARY
DOJ_SUP_BINARY=''

print_help() {
  program=$(basename "$0")
  echo "$program

Test \`doj sup --help\` will not install karate/doj-sup.

USAGE:
  $program <path-to-doj-binary>

ARGS:
  <path-to-doj-binary>  The name or path of the doj binary under test (eg: './target/debug/doj' or
simply 'doj'). In the latter case when just passing the binary name, the shell will search in \$PATH.
"
}

if [[ $# -eq 0 ]] ; then
  print_help
  echo
  echo "<path-to-doj-binary> must be specified!"
  exit 1
else
  DOJ_BINARY="$1"
fi

echo "Running \`$DOJ_BINARY sup --help\` - which should NOT attempt an install of karate/doj-sup"

if [ -z "$($DOJ_BINARY sup --help)" ]; then
  echo
  echo "ERROR: $DOJ_BINARY was not the proper executable doj binary!"
  exit 1
elif [ -d "$TESTING_FS_ROOT/doj/pkgs/karate/doj-sup" ]; then
  echo
  echo "ERROR: detected an installation of karate/doj-sup"
  exit 1
fi

echo "Success!"
