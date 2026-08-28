# Phase 3 hard cutover

Status: implemented on `feature/hard-cutover`; awaiting review and merge

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

## Final state and verification

The root workspace contains exactly seven members: domain, protocol, client,
application, runtime, desktop, and studio-web. The cutover changes 189 tracked
paths relative to the Phase 2 merge base, with 170 tracked paths deleted,
35,496 lines removed, and no tracked binary remaining. One root lockfile covers
the entire workspace.

The final candidate gate passed formatting, native all-target checks, 50 unit
tests, four distinct Rust integration scenarios, denied-warning Clippy,
portable and studio `wasm32-unknown-unknown` checks, a release Trunk build, and
five real-Chromium scenarios. Browser coverage includes twenty active-stream
reloads and the 2,000-message telemetry flood. Markdown links, changed-tree
whitespace, required tools, exact workspace membership, tracked binaries,
workflow absence, and legacy source/dependency residue are checked by the same
non-skippable `./scripts/verify.sh` entry point.

The canonical `./scripts/run.sh` lifecycle was also exercised with a loopback
host and dedicated temporary Chromium profile. Ctrl-C stopped the native host
and its browser child, removed the profile, and left no Magnolia-started process.

## Limitations and Phase 4 boundary

The active runtime remains `MockRuntime` with synthetic descriptors. The
cutover does not implement devices, audio, PipeWire, ASR, recording/replay,
filesystem persistence, GPU computation, model acquisition, a plugin system,
legacy import, or hardware certification. Phase 4 may begin the accepted native
audio work; later observation and ASR phases must reconstruct only the needed
concepts from documented contracts and Git history.
