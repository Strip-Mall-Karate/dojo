#!/usr/bin/env bash

# Builds a "cleanroom" Docker container to run BATS tests in, and then
# executes the tests in that container, mounting the tests and Dojo
# binaries as needed.

if [ $# -eq 0 ] ; then
    TESTS="."
else
    TESTS="$*"
fi

docker build -t doj-bats-cleanroom "$(pwd)"/test/integration

docker run -it --rm \
       --mount type=bind,source="$(pwd)/test/integration",target=/test \
       --mount type=bind,source="$(pwd)/target/debug/doj-launch",target=/bin/doj-launch \
       --mount type=bind,source="$(pwd)/target/debug/doj-sup",target=/bin/doj-sup \
       --mount type=bind,source="$(pwd)/target/debug/doj",target=/bin/doj \
       --env DOJ_BIN_DIR=/bin \
       --workdir=/test \
       --name doj-bats \
       doj-bats-cleanroom \
       bats "${TESTS}"