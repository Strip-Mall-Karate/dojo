# Decisions

The dated record. `AGENTS.md` carries the rules these produced; the workq wiki page "Dojo" tells the
longer story.

## 2026-10-25

- **Fork point: 0.79.1** (`edcd8544a`, 10 April 2019), the last release of the source project
  before its binaries went under an end-user license (0.80.0) and, later, a license key. Nothing
  from later releases is taken.
- **History is kept.** The people who built the source project stay in the log; authors are never
  rewritten. The separate core crates repository was subtree-merged with its history; ipc-channel's
  Windows branch is vendored as a snapshot.
- **Rename:** product Dojo; `hab` becomes `doj` everywhere (`doj`, `doj-sup`, `doj-launch`,
  `DOJ_*`, `/doj`). The old name appears nowhere in the tree; Chef Software's copyright lines stay.
- **Everything under karate.company:** package origin `karate`, Builder `bldr.karate.company`,
  Strip Mall Karate Company as maintainer and copyright holder, repository
  `StripMallKarateCompany/dojo` on GitHub, public.
- **Removed:** the old CI and release automation, governance files, website, changelog, CLI
  analytics, and exporters and publishers for retired services (Bintray, ACI, Mesos, Cloud
  Foundry, Kubernetes, Helm). No Expeditor, no Dependabot, no third-party package feeds.
- **A dojo is a primary object: a ring of machines.** Machines install into a dojo; services,
  configuration and keys scope to it.
- **Focus:** PID 1 (grow the Launcher) and a full-machine installer. A container target is
  secondary.
- **License:** Apache 2.0 for everything, inherited files and new work alike (`LICENSE`,
  `NOTICE`); new files carry Strip Mall Karate Company's copyright line.
- **First machines:** a VM on the owner's Apple silicon laptop first (hardware virtualization,
  arm64 guest, never emulation), the physical machine smk-2 (x86_64 UEFI, same hardware profile as
  smk-1) deferred.
- **GUI:** KDE Plasma on Wayland, later; dbus-broker and elogind will run as Supervisor services.
- **PID 1 policy:** as init the Launcher never exits. It restarts a Supervisor that exits for any
  reason, with a delay that starts at one second and doubles to thirty while it keeps exiting
  within a minute of starting. SIGTERM to PID 1 powers the machine off; SIGINT (the kernel's
  Ctrl-Alt-Del) reboots it. `doj sup term` restarts the Supervisor (SIGHUP) when the Launcher is
  PID 1 rather than powering the machine off.
- **Stage-0 bootstrap:** the VM's kernel and userland come from nixpkgs until `karate` packages
  replace them.
- **Toolchain:** the tree ships on Rust 1.33.0 and OpenSSL 1.0.2 and is being moved to current
  stable Rust, Launcher first.

## 2026-10-26

- **No Windows.** Dojo neither runs on Windows nor builds packages for it. Plans build for Linux
  (`x86_64-linux`, `aarch64-linux`, `x86_64-linux-kernel2`) and macOS (`x86_64-darwin`,
  `aarch64-darwin`), and core no longer knows `x86_64-windows`. The Windows code is gone: the
  PowerShell builder and every `plan.ps1`, the Windows service and `win-users`, the Windows
  studio and Docker containers, the Windows branches of core, the Supervisor, the Launcher and
  `doj`, and ipc-channel's Windows platform.
- **macOS is a package platform**, Apple silicon included. Homebrew and Nix are the models for how
  macOS packages are built and relocated.
- **The Supervisor always handles its signals.** `DOJ_FEAT_IGNORE_SIGNALS` is removed: only the
  Windows service set it, and with the Launcher as PID 1 the Supervisor must answer SIGHUP.
- **Real `karate` base packages come before the Launcher proof.** The Fennel-built Launcher is
  compared with the bash-built one using real dependencies (`karate/glibc`, `karate/rust` and the
  rest), not stubs over nixpkgs, so the stage-0 bootstrap of the base packages comes first.
- **Everything moves to current tooling.** The whole tree builds with the newest stable Rust
  (1.99.1, read from `rust-toolchain`), then moves to current nixpkgs and OpenSSL 3, current
  versions of every crate, and edition 2024, a phase at a time, each phase passing the boot checks.
- **Templates render on Handlebars 6 with their old output.** Configuration and hook templates
  were written for handlebars 0.28, which never removed a line holding only a block tag, so
  authors put a tilde on one side of such tags (`{{~#if x}}`, `{{#if x ~}}`) to manage newlines.
  Handlebars 6 removes those lines as the Handlebars spec says, and the tilde then joins lines.
  The renderer rewrites a one-sided tilde on a standalone block tag so the output is what 0.28
  produced, and plan authors keep writing templates as they do now. Rendered against every
  template in `dojo-plans`, output is byte-identical or differs only in blank lines, except that
  `toYaml` writes plain YAML without a leading `---` (same data) and an array printed directly
  loses its trailing `, `. Two things break: `foo[0]` must be written `foo.[0]` (nothing uses
  it), and pest stays at 2.8 until the Supervisor's actix-web 0.7, whose ring 0.13 holds cc at
  1.0.95, is replaced.
- **Old dependencies are replaced, not only upgraded.** Where a dependency reflects what Rust
  offered when the tree was written (zmq, futures 0.1, tokio 0.1, actix-web 0.7), it gives way
  to what a new project would choose today, at its latest release.
- **Gossip travels over TCP on tokio, not ZeroMQ.** Each gossip round opens one connection per
  member and sends each rumor as a frame of a 4-byte big-endian length and its `Wire` message
  (`tokio_util`'s length-delimited codec), with frames capped at 8 MiB. Gossip runs on a tokio
  runtime butterfly owns until the Supervisor moves to tokio 1 and shares its own. SWIM stays
  on its UDP threads: its loops are timers over one socket, and async buys them nothing. A
  Supervisor with this change cannot exchange rumors with one using ZeroMQ, although SWIM
  membership still works between them. Nothing is deployed, so nothing is lost. libzmq leaves
  the build and the payload.
