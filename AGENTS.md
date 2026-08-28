# Contributor and agent guidance

Magnolia's accepted target architecture is documented under `docs/rearchitecture/` and `docs/adr/`; it is not the current implementation. Before editing, read the rearchitecture index, current-state disposition, the document for the phase in scope, and relevant source/tests. Never describe a target contract or threshold as implemented or certified.

## Current and target boundaries

- The current tree remains a legacy experimental Rust/Nannou prototype until the migration gates say otherwise.
- Native Rust will own devices, graph/runtime state, scheduling, ASR, persistence, telemetry production, and authoritative GPU computation/resources. Leptos owns presentation/session state; browser GPU APIs may accelerate presentation only.
- Modules and tiles are independent many-to-many concepts.
- Magnolia remains domain-neutral and does not share a framework with Mirabile, Digital Liquid Light Lab, astrology, tarot, people, journal, or vault domains.
- No legacy layout/plugin compatibility importer is planned.

## Validation and provenance

For documentation-only changes run `git diff --check` and validate links/path references with focused searches. For Phase 1 foundation changes use `scripts/check.sh` as the fast gate and `scripts/verify.sh` as the handoff gate. These scripts are intentionally scoped to the five portable foundation crates and must be extended as later native, WASM, browser, and hardware phases arrive.

Do not download models or corpora, access devices, run capture/benchmarks, launch the GUI, build plugins, or use a GPU without explicit authorization. Never commit recordings, transcripts, model/corpus payloads, `.env`, secrets, local paths, raw host captures, unjustified binaries, or unlicensed assets. Record provenance and actual execution providers. Do not claim ABI stability, sandbox/signing/hot-reload enforcement, real-time performance, ASR accuracy, browser continuity, or GPU support without evidence for the exact path and host.

## Migration discipline

Keep implementation phases commit-sized. Do not create a Nannou/Leptos bridge, park deleted compatibility code, or perform the hard cutover before its synthetic shell gates pass. The legacy shell and plugin deletions occur at the exact two breaking commits in the migration plan. Preserve active worktrees and user-owned changes; inspect status and topology before modifying or integrating branches.

Use a focused branch. Delivery expectations (commit, push, PR, or local-only) are set by the current task; do not infer remote publication authorization.
