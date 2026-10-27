# Dojo

Dojo builds software into signed, self-describing packages and runs them under a
Supervisor that owns their configuration, updates and clustering. It is being
built into an init system (PID 1) and an installer that bootstraps a whole machine.

Dojo is a Strip Mall Karate Company project, created by Jamie Stormbreaker.

## Status

Early. This tree builds with the stable Rust pinned in `rust-toolchain` and is
moving from its 2019 dependencies to current ones, a crate at a time. Packages live in the `karate` origin. The default Builder is
`https://bldr.karate.company`, which is not running yet; set `DOJ_BLDR_URL` to use
another.

## Layout

| Path | What it is |
|---|---|
| `components/doj` | The `doj` command-line tool |
| `components/launcher` | `doj-launch`, which starts and restarts the Supervisor |
| `components/sup` | `doj-sup`, the Supervisor |
| `components/butterfly` | The gossip protocol between Supervisors |
| `components/core` | Packages, keys, identities and paths shared by everything |
| `components/plan-build` | The bash builder `components/plan` replaces; its package plan is gone |
| `components/plan` | `doj-plan-build`, which builds packages from plans written in Fennel, packaged as `karate/doj-plan-build` (its own crate on Rust 1.99.1: `cargo test` there) |
| `components/backline` | `karate/doj-backline`, what a studio puts on PATH |
| `components/studio` | The clean-room build environment |
| `components/pkg-export-docker`, `pkg-export-tar` | Exporters |
| `support/vm` | The Dojo VM for Apple silicon Macs |
| `vendor/ipc-channel` | Servo's ipc-channel, vendored as a snapshot |

## Building

See [BUILDING.md](BUILDING.md).

## The Dojo VM

On an Apple silicon Mac with Nix, `support/vm/boot` boots Dojo in an arm64 VM
under Apple's hypervisor, with its serial console on your terminal (Ctrl-A X
quits). The kernel and userland come from nixpkgs (nixos-26.05) as a stage-0
bootstrap until `karate` packages replace them.

To run `doj-launch` as PID 1, build it in nixpkgs' Linux builder (another arm64
VM, which `support/vm/builder` starts) and boot with the result:

```sh
support/vm/builder start
support/vm/builder payload
support/vm/boot --payload support/vm/out/payload.cpio.gz
```

The VM has one NIC, `eth0`, on QEMU's user-mode network: init leases its
address (10.0.2.15) and default route with DHCP, which is how the Supervisor
finds its own address, and the VM can reach out through the Mac but nothing
reaches in.

`--check` boots and checks that `eth0` has an address, that the Supervisor's
HTTP gateway answers and it found its own address, that the Launcher restarts
the Supervisor after SIGKILL and after `doj sup term`, and that SIGTERM to PID 1
powers the VM off without a kernel panic. `--check-reboot` ends with SIGINT to
PID 1 (the kernel's Ctrl-Alt-Del) and expects a reboot. `support/vm/builder
test <package>...` runs a package's tests and format check in the builder,
`support/vm/builder verify` runs every check a pull request reports (all tests
against `support/vm/expected-failures`, the payload and both boot checks), and
`support/vm/builder stop` stops it. `support/vm/builder` with no arguments lists
the rest.

### x86_64 from karate packages

`support/vm/x86_64/boot <pkgs>` boots the same init on x86_64 with nothing from
nixpkgs in the guest: the `karate/linux` kernel and an initramfs of
`busybox-static`, `doj-launcher`, `doj-sup-static` and `doj`, the newest release
of each in `<pkgs>`, a copy of a builder's `/doj/pkgs/karate`. QEMU emulates the
machine, so it runs on any Mac or Linux host with Nix and is a development loop,
not proof that a machine boots. `--check` runs the checks above with a 600
second deadline, `--initrd <file>` keeps the initramfs and `--log <file>` the
serial console.

### A ring

A dojo is a ring of machines whose Supervisors gossip with each other.
`support/vm/ring` boots two VMs from the same payload that form the first one:

```sh
support/vm/ring --payload support/vm/out/payload.cpio.gz
```

Each VM gets a second NIC, `eth1`, on a link between the two QEMU processes (a
UDP tunnel on the Mac's loopback, which needs no privilege), with the static
addresses 10.0.3.1 and 10.0.3.2. The second Supervisor starts with
`--peer 10.0.3.1`. `ring` prints each VM's serial console as a `nc` command on
a local TCP port, and Ctrl-C ends both VMs. `--port <n>` moves the four local
ports it uses (two for the link, two for the consoles) from 19638.

`ring --check` runs `boot --check` in both VMs, where each Supervisor must also
report two members alive in its gossip (`/butterfly` on the HTTP gateway), and
both must power off cleanly within `boot`'s deadline. The kernel command line
carries a VM's place in the ring to init and the check: `dojo.ring=<addr/prefix>`
for `eth1`, `dojo.peer=<addr>` for the Supervisor and `dojo.members=<n>` for the
check; `boot` takes them as `--ring`, `--link`, `--peer` and `--members`, and
`--console <port>` serves the console on TCP instead of the terminal.

## License

Apache License 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).

Copyright (c) 2016 Chef Software Inc. and/or applicable contributors
Copyright (c) 2026 Strip Mall Karate Company
