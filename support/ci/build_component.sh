#!/bin/bash

set -eou pipefail

component=${1?component argument required}

# Since we are only verifying we don't have build failures, make everything
# temp!
export DOJ_ORIGIN
DOJ_ORIGIN=throwaway
# let's make a selfcontained tempdir for this job
export JOB_TEMP_ROOT
JOB_TEMP_ROOT=$(mktemp -d /tmp/job-root-XXXXXX)
export DOJ_CACHE_KEY_PATH
DOJ_CACHE_KEY_PATH="$JOB_TEMP_ROOT/keys"

echo "--- :key: Generating fake origin key"
doj origin key generate
echo "--- :doj: Running doj pkg build for $component"
doj pkg build -D components/"$component"
