# smk context: dojo

Grounding for the smk agent skills. AGENTS.md is authoritative for policy; this file points at
the commands and facts the skills need.

- Build: `support/vm/builder start`, then `support/vm/builder payload` (the arm64 binaries into
  `support/vm/out/payload.cpio.gz`) and `support/vm/builder test <package>...` (tests and the
  format check in the builder VM). `support/vm/builder stop` when done. x86_64 builds and tests
  run on the local-network Linux machine with the same `support/vm/guest` scripts.
- Acceptance: `support/vm/boot --check --payload support/vm/out/payload.cpio.gz` and
  `--check-reboot`. `boot --check` without a payload checks the base image.
- Tracker: workq, project SMK, label `dojo`. The record is the wiki page "Dojo"
  (`smk wiki list --match Dojo`); `docs/decisions.md` in the tree is the dated log. This tree has
  no dev shell, so `smk issue` and `smk wiki` need the workq credential from a shell that loads
  it (the smk-cli dev shell does); without it they fail with HTTP 401.
- Host: GitHub, `Strip-Mall-Karate/dojo`. Pull requests target `main` and land with merge
  commits; no CI runs, and every merge needs the owner's go-ahead.
- Proof: the Dojo VM (hardware virtualization on the Mac) for arm64; smk-1 for x86_64 under the
  `local-network` skill's rules.
