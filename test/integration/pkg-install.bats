#!/usr/bin/env bats

load 'helpers'

setup() {
    reset_doj_root
}

@test "doj pkg install: origin/name (standalone service)" {
    run ${doj} pkg install karate/redis
    assert_success

    latest_redis=$(latest_from_builder karate/redis stable)
    assert_package_and_deps_installed "${latest_redis}"
}

@test "doj pkg install: origin/name/version (standalone service)" {
    run ${doj} pkg install karate/redis/3.2.4
    assert_success

    latest_redis=$(latest_from_builder karate/redis/3.2.4 stable)
    assert_package_and_deps_installed "${latest_redis}"
}

@test "doj pkg install: origin/name/version/release (standalone service)" {
    desired_version="karate/redis/3.2.3/20160920131015"

    run ${doj} pkg install "${desired_version}"
    assert_success
    assert_package_and_deps_installed "${desired_version}"
}

@test "doj pkg install: local hart file (standalone service)" {
    desired_version="karate/redis/3.2.4/20170514150022"

    # First, grab a hart file! Then set it aside and clean everything
    # out of /doj. This way, we'll have a hart file, but nothing else,
    # which is exactly what we want.
    run ${doj} pkg install "${desired_version}"
    assert_success
    cp $(cached_artifact_for "${desired_version}") /tmp
    reset_doj_root

    # Now, install from just the local hart file
    run ${doj} pkg install /tmp/core-redis-3.2.4-20170514150022-x86_64-linux.hart
    assert_success
    assert_package_and_deps_installed ${desired_version}
}

@test "doj pkg install: local hart from /doj/cache/artifacts (standalone service)" {
    desired_version="karate/redis/3.2.4/20170514150022"

    # First, grab a hart file!
    run ${doj} pkg install "${desired_version}"
    assert_success
    # We don't want to remove everything in /doj, because we want the
    # artifact cache to remain.
    remove_installed_packages
    empty_key_cache

    # Now install from the local hart file *from the cache*
    run ${doj} pkg install "$(cached_artifact_for ${desired_version})"
    assert_success
    assert_package_and_deps_installed "${desired_version}"
}

@test "doj pkg install: installing from a file that isn't a hart fails" {
    echo "lolwut" > /tmp/not-a.hart

    run ${doj} pkg install /tmp/not-a.hart
    assert_failure
    [[ "$output" =~ "Can't read keyname" ]]
}

@test "doj pkg install: installing from a nonexistent file that looks a hart fails" {
    run ${doj} pkg install looks-like/an-ident.hart
    assert_failure
    [[ "$output" =~ "File not found at: looks-like/an-ident.hart" ]]
}
