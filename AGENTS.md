# AGENTS.md — Dojo

Guidance for coding agents (Claude Code, Codex and others) changing **this repository**. `README.md`
says what Dojo is and how to build it; this file is about changing it safely, and leads with what
the source does not show.

## What this is

Dojo builds software into signed packages and runs them under a Supervisor. It is being grown into
an operating system: the Launcher (`doj-launch`) is PID 1, a full-machine installer is coming, and a
**dojo** is a ring of machines whose Supervisors gossip. It is a Strip Mall Karate Company product.

The tree descends from the 0.79.1 release of an Apache-2.0 project by Chef Software Inc. and
contributors (see `NOTICE` and `LICENSE-APACHE`). Nothing from that project's later releases is
taken, whatever it fixes: later work was done under other ownership that this product wants nothing
from. Re-implement instead.

The dated record of decisions is `docs/decisions.md`; the longer account lives on the workq wiki
page "Dojo" in project SMK (`smk wiki list --match Dojo`).

## Names and licensing

- The product is Dojo; the command is `doj`; binaries are `doj-sup` and `doj-launch`; environment
  variables are `DOJ_*`; the filesystem root is `/doj`; the package origin is `karate`; Builder is
  `https://bldr.karate.company` (not running yet; `DOJ_BLDR_URL` overrides it).
- **The old project's name appears nowhere in the tree**, not in code, comments, docs, fixtures or
  commit subjects. `NOTICE` shows how to credit its authors without naming it.
- Inherited files keep their `Copyright (c) 20xx Chef Software Inc.` headers unchanged. New files
  start with the Apache 2.0 header in `components/launcher/src/init.rs`, with Strip Mall Karate
  Company's copyright line. Everything is under the Apache License 2.0 (`LICENSE`, `NOTICE`).
- A mechanical rename can mint a name a stranger can register (a GitHub owner, a Docker Hub
  namespace, a package feed). Any renamed hostname or namespace is checked before it ships.

## History

- Contributors' history is kept and never rewritten: no rebase, squash or amend of pushed commits,
  no author changes. Pull requests land with **merge commits**.
- Commit subjects are imperative; bodies say why. No AI attribution or co-author lines.

## Toolchain and building

- The tree is an smk project (`smkproj.toml`): `direnv allow` loads its dev shell, and `make checks`
  (`smk format --check`, `smk lint`, `smk typecheck`) passes before a PR opens; `smk format` fixes
  formatting. Rust and the code on its way out are excluded, and `smkproj.toml` says why; Rust is
  formatted with `builder fmt` and tested in the VM.
- `rust-toolchain` at the root pins the Rust the tree builds with; `support/vm/guest/env.sh` supplies
  the C libraries through Nix. Read both before changing a dependency; the lock is built `--locked`.
- The Dojo VM (`support/vm`, README "The Dojo VM") is the development loop: an arm64 Linux guest
  under Apple's hypervisor, never emulation. `support/vm/builder` runs the Linux VM that builds
  the binaries, `boot --payload` boots them with `doj-launch` as PID 1, and `boot --check` and
  `--check-reboot` are the acceptance checks for anything touching the Launcher, the Supervisor's
  startup, or the VM scripts. Both checks pass before a PR opens, and a check that cannot fail is
  not evidence: break it once and show the failure.
- One builder VM runs per state directory (`DOJO_BUILDER_STATE`) and ssh port; parallel work in
  separate worktrees (`~/code/dojo-trees/<name>`) needs a builder of its own. Set both on every
  `builder` command: without them it finds the default VM, which may be another session's.
- Every `builder` command but `ssh` copies the tree over the VM's `~/dojo`, so a file the VM
  generated is gone at the next command unless one copied it back. `builder run` returns a changed
  `Cargo.lock`, and `builder fmt` returns the files rustfmt rewrote. `.rustfmt.toml` needs the
  nightly rustfmt `env.sh` provides, so format with `builder fmt` rather than by hand.
- The `support/vm/guest` scripts run on any Linux with Nix, not only in the builder VM.

## Testing

- x86_64 proof runs on the local-network Linux machine under the `local-network` skill's rules
  (a throwaway checkout, a throwaway `GIT_CONFIG_GLOBAL`, `nice -n 15`, delete what you created,
  confirm no processes remain). The VM proves arm64.
- **A test run is only as good as the binaries that started.** While the tree linked OpenSSL
  1.0.2 from an old nixpkgs, every test binary that loaded libcrypto died at load (`libdl.so.2:
undefined symbol: _dl_catch_error`, exit 127), so seven test binaries silently never ran for the
  first months of this fork. A loader error is not counted as a failed test: count the binaries
  that ran against the ones cargo built before calling a run green. `builder verify` does that
  count, and it is the run a PR body reports.
- Expected failures that are not regressions: the Supervisor fixtures carry `TARGET x86_64-linux`
  and cannot load on arm64; `compile_hook_table` and the Supervisor integration tests install
  `karate/busybox-static` from Builder, which does not exist yet; `dojo_common`'s templating tests
  want a real, writable `/doj` with packages installed. `support/vm/expected-failures` names them;
  a change that makes one pass removes it.

## Landing

The repository is `StripMallKarateCompany/dojo` on GitHub. Finished work opens a pull request against
`main` (`smk pr create`) whose body states what changed and the evidence, and requests its merge without
asking, as the smk `pr` skill does on GitHub.
