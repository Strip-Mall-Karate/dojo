#!/bin/bash

set -eoux pipefail

CHANNEL=$1
VERSION=$2

shift 2
sudo doj pkg install -c "$CHANNEL" core/doj-backline/"$VERSION"
doj pkg upload -u https://bldr.acceptance.habitat.sh -c stable "$@" \
    /doj/cache/artifacts/core-doj-backline-"$VERSION"-*-x86_64-linux.hart
