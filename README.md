# Dojo

Dojo builds software into signed, self-describing packages and runs them under a
Supervisor that owns their configuration, updates and clustering. It is being
built into an init system (PID 1) and an installer that bootstraps a whole machine.

Dojo is created and maintained by Jamie Stormbreaker.

## Status

Early. This tree builds with Rust 1.33.0, pinned in `rust-toolchain`, and is
being modernized. Dojo runs no public Builder: set `DOJ_BLDR_URL` to your own.
The built-in default, `https://bldr.dojo.invalid`, never resolves.

## Layout

| Path | What it is |
|---|---|
| `components/doj` | The `doj` command-line tool |
| `components/launcher` | `doj-launch`, which starts and restarts the Supervisor |
| `components/sup` | `doj-sup`, the Supervisor |
| `components/butterfly` | The gossip protocol between Supervisors |
| `components/core` | Packages, keys, identities and paths shared by everything |
| `components/plan-build` | Builds a package from a plan |
| `components/studio` | The clean-room build environment |
| `components/pkg-export-docker`, `pkg-export-tar` | Exporters |
| `vendor/ipc-channel` | Servo's ipc-channel with Windows support |

## Building

See [BUILDING.md](BUILDING.md).

## License

Apache License 2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).

Copyright (c) 2016 Chef Software Inc. and/or applicable contributors
