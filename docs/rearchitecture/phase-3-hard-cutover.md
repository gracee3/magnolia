# Phase 3 hard cutover

Status: in progress on `feature/hard-cutover`

## Frozen base and policy

Phase 2 was locally verified at
`a9bcc5ff41f43cd0e94e79e842c4c26c39d3a825` and merged by PR #5 as normal
merge commit `807b032a12e737cdeca5137004697ea0a2a97537`. All three Phase 2 commits are
ancestors of that merge. The complete local `./scripts/verify.sh` gate at the
exact candidate SHA is authoritative; Magnolia no longer runs repository
GitHub Actions workflows.

Run `33206890066` was intentionally canceled. It is historical evidence of a
canceled run, not a passing gate. Phase 2 closure instead recorded 50 unit
tests, four distinct Rust integration scenarios, five real-Chromium scenarios,
twenty reloads, a 2,000-message telemetry flood, and a clean manual launch and
shutdown at the frozen head.

## Cleanup before implementation

The protected checkout remained tracked-clean at its existing branch and HEAD;
only its generated 1.4 GiB Cargo target was reclaimed. Three clean, merged
Phase 1, Phase 2, and planning worktrees were removed with non-forced Git
operations together with their merged local branches. Global Cargo, browser,
and tool caches were preserved. Available root filesystem space increased from
17 GiB to 28 GiB before Phase 3 builds.

## Staged deletion inventory

The policy commit removes the two Magnolia workflow files. The first breaking
commit removes the Nannou daemon, UI/layout/input/tile presentation stack,
legacy layout configuration, fonts, and unproven visual assets while promoting
the desktop and studio into the root workspace. The second breaking commit
removes the legacy signal/plugin architecture, all remaining dependents and
unrelated applications, obsolete configuration and tooling, and the tracked
plugin binary. Git history is the archive; no compatibility bridge or importer
is introduced.

Final verification evidence, exact counts, residue audits, limitations, and
Phase 4 deferrals will be recorded here before delivery.
