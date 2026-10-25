#!/usr/bin/env bats

load 'helpers'

setup() {
    reset_doj_root
    start_supervisor
}

teardown() {
    stop_supervisor
}

@test "doj svc status: when no supervisor is running" {
    stop_supervisor

    run ${doj} svc status
    assert_failure 1
    assert_output --partial "Unable to contact the Supervisor."
}

@test "doj svc status: when no services are running" {
    run ${doj} svc status
    assert_success
    assert_output --partial "No services loaded"
}

@test "doj svc status: for a single running service" {
    run ${doj} svc load karate/redis

    wait_for_service_to_run redis

    sleep 3 # give the services.dat file time to be
    # written... otherwise the state can show as down

    run ${doj} svc status karate/redis
    assert_success

    # OUTPUT:
    # package                           type        desired  state  elapsed (s)  pid   group
    # karate/redis/4.0.10/20180801003001  standalone  up       up     3            1016  redis.default
    assert_line --regexp "karate/redis/.*/[0-9]{14}\s+standalone\s+up\s+up\s+.*redis.default"
}

@test "doj svc status: for a single service that is not loaded" {
    run ${doj} svc load karate/redis

    wait_for_service_to_run redis

    sleep 3 # give the services.dat file time to be
            # written... otherwise the state can show as down

    run ${doj} svc status karate/nginx # nginx != redis
    assert_failure 1
    assert_output --partial "Service not loaded, karate/nginx"
}

@test "doj svc status: for all running services" {
    run ${doj} svc load karate/redis
    assert_success

    ${doj} pkg install karate/runit --binlink # whyyyyy
    run ${doj} svc load karate/nginx
    assert_success

    wait_for_service_to_run redis
    wait_for_service_to_run nginx

    sleep 3 # let services.dat get written

    run ${doj} svc status
    assert_success

    assert_line --regexp "karate/redis/.*/[0-9]{14}\s+standalone\s+up\s+up\s+.*redis.default"
    assert_line --regexp "karate/nginx/.*/[0-9]{14}\s+standalone\s+up\s+up\s+.*nginx.default"
}
