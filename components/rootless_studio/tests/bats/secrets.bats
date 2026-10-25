#!/usr/bin/env bats

# TODO (CM): Better parameterize the DOJ_DOCKER_STUDIO_IMAGE
# environment variable.
#
# As it is, you can just execute these tests with that in the
# environment, but it would be nice to make it explicit.
#
# DOJ_DOCKER_STUDIO_IMAGE=my_image bats secrets.bats

@test "doj pkg build (rootless Docker image): secrets are passed" {
    export DOJ_STUDIO_SECRET_FOO=bar
    run doj pkg build -D fixtures/plans/test_build_with_secrets
    [ "$status" -eq 0 ]
}

@test "doj studio run (rootless Docker image): secrets are passed" {
    export DOJ_STUDIO_SECRET_FOO=bar
    run doj studio run -D 'echo The result is $FOO'
    [ "$status" -eq 0 ]
    [[ "$output" =~ "The result is bar" ]]
}
