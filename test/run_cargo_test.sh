#!/bin/bash

set -eou pipefail

while [[ $# -gt 1 ]]; do
  case $1 in
    -f | --features )       shift
                            features=$1
                            ;;
    -t | --test-options )   shift
                            test_options=$1
                            ;;
    * )                     echo "FAIL SCHOONER"
                            exit 1
  esac
  shift
done

# set the features string if needed
[ -z "${features:-}" ] && features_string="" || features_string="--features ${features}"

component=${1?component argument required}
cargo_test_command="cargo test ${features_string} -- --nocapture ${test_options:-}"

# TODO: fix this upstream, it looks like it's not saving correctly.
sudo chown -R buildkite-agent /home/buildkite-agent

# TODO: these should be in a shared script?
sudo doj pkg install karate/bzip2
sudo doj pkg install karate/libarchive
sudo doj pkg install karate/libsodium
sudo doj pkg install karate/openssl
sudo doj pkg install karate/xz
sudo doj pkg install karate/zeromq
sudo doj pkg install karate/protobuf --binlink
sudo doj pkg install karate/rust --binlink
export SODIUM_STATIC=true # so the libarchive crate links to sodium statically
export LIBARCHIVE_STATIC=true # so the libarchive crate *builds* statically
export OPENSSL_DIR # so the openssl crate knows what to build against
OPENSSL_DIR="$(doj pkg path karate/openssl)"
export OPENSSL_STATIC=true # so the openssl crate builds statically
export LIBZMQ_PREFIX
LIBZMQ_PREFIX=$(doj pkg path karate/zeromq)
# now include openssl and zeromq so thney exists in the runtime library path when cargo test is run
export LD_LIBRARY_PATH
LD_LIBRARY_PATH="$(doj pkg path karate/libsodium)/lib:$(doj pkg path karate/zeromq)/lib"
# include these so that the cargo tests can bind to libarchive (which dynamically binds to xz, bzip, etc), openssl, and sodium at *runtime*
export LIBRARY_PATH
LIBRARY_PATH="$(doj pkg path karate/bzip2)/lib:$(doj pkg path karate/libsodium)/lib:$(doj pkg path karate/openssl)/lib:$(doj pkg path karate/xz)/lib"
# setup pkgconfig so the libarchive crate can use pkg-config to fine bzip2 and xz at *build* time
export PKG_CONFIG_PATH
PKG_CONFIG_PATH="$(doj pkg path karate/libarchive)/lib/pkgconfig:$(doj pkg path karate/libsodium)/lib/pkgconfig:$(doj pkg path karate/openssl)/lib/pkgconfig"

# Set testing filesystem root
export TESTING_FS_ROOT
TESTING_FS_ROOT=$(mktemp -d /tmp/testing-fs-root-XXXXXX)
echo "--- Running cargo test on $component with command: '$cargo_test_command'"
cd "components/$component"
$cargo_test_command
