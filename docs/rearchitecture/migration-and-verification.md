# Migration and verification

Status: Phases 1 and 2 implemented; Phase 3 in progress; phases 4 through 7 are prescribed future work.

## Commit-sized migration

### 1. Portable contracts

Status: implemented and covered by the Phase 1 handoff gate. See
[Phase 1 foundation](phase-1-foundation.md).

Add only `magnolia-domain`, `magnolia-protocol`, `magnolia-client`,
`magnolia-application`, and `magnolia-runtime`. Include UUID entity/request/
operation/epoch newtypes; checked `u64` revision newtypes; documents and typed
graphs; static domain `ControlDefinition`s; dynamic protocol
`ControlManifest`s; major/minor negotiation; sequenced commands and a per-client
1,024-receipt replay window; atomic typed `WorkspaceEdit` batches; immutable full
projections with async non-consuming waits; an async browser-compatible client
boundary; transactions, undo/redo, persistence and runtime ports; in-process and
scripted mock clients; in-memory persistence; a deterministic `MockRuntime`;
golden fixtures; and shared scenarios. Use synthetic descriptors only.

Exit gate: malformed graphs and frames fail deterministically; supported
in-window retries return the original receipt, while expired/conflicting
sequences are rejected without execution; supported documents round-trip;
unsupported protocol/schema majors are rejected; concurrent projection observers
do not consume one another's updates; stream delivery and module configuration
schemas are validated; document-only edits do not create or supersede runtime
operations; domain/protocol/client compile for `wasm32-unknown-unknown`; the full
mock round trip covers persistence, revision conflicts, successful activation,
last-good failure, and stale completion rejection.

Microphone, PipeWire, Sherpa, Leptos, Chromium transport, and legacy deletion are
excluded from this first slice.

### 2. New shell proof

Status: implemented and covered by the Phase 2 handoff gate. See
[Phase 2 shell proof](phase-2-shell-proof.md).

Add `magnolia-desktop` and `magnolia-studio-web` with synthetic modules. Prove authenticated loopback handshake, immediate snapshot, command/receipt, projection updates, reload/reconnect, transcript cursors, bounded binary telemetry, retained split/tab layouts, focus routing, and browser automation.

Exit gate: `wasm32-unknown-unknown` compiles, `trunk build --release` succeeds, Chromium E2E tests pass, twenty active-stream reloads retain the runtime/graph, and telemetry overload cannot delay control receipts. No Nannou/Leptos bridge is created.

### 3. Hard cutover and exact deletion point

Make the new desktop binary the default only after phase 2 gates pass. Then make two explicit breaking commits:

1. `refactor!: remove the legacy Nannou shell and layout/input stack`
2. `refactor!: remove legacy Signal, plugin ABI, and unrelated crates`

The first deletes `apps/daemon`, `magnolia-ui`, Nannou dependencies, tile
registries/traits, modal/input modes, raw key handling, legacy layout
code/configuration, and unneeded visual assets (Egui implementation is already
absent). The second deletes or strips every remaining dependency on old `core` or
`magnolia-signals`: old `core`, `magnolia-signals`, `magnolia-module-api`, both
custom rings, plugin loader/manager/verifier/sandbox/ABI/helper/example and
tracked binaries, `magnolia-config`, `audio_input`, `audio_output`, `audio_dsp`,
`audio_replay`, `speech_to_text`, `caption_state`, `stt_bench`, and unrelated
Aphrodite/Kamea/Logos/text/caption-demo members. It also removes obsolete setup/
benchmark scripts, `tools/audio_snapshot.sh`, legacy environment examples, and
old-core CI/configuration. Audio, caption, benchmark, and Sherpa concepts are
reconstructed later from Git history. Git history is the archive; no parking
crate, uncompilable workspace member, or importer remains.

Exit gate: the default workspace, desktop launch, Mock scenarios, and clean-tree search contain no legacy dependency, type, mode, layout format, plugin entrypoint, or tracked binary. Deletion occurs neither before shell proof nor after native audio begins.

### 4. Native audio

Implement PipeWire discovery/hotplug, exact/default selectors, fixed-block graph,
conversion/mapping/resampling, capture/monitor commands, block-boundary
activation, and overload accounting. Same-lane nodes borrow prepared buffers;
each cross-lane tap uses paired `rtrb` free/ready queues of blocks allocated at
graph preparation. Reclaim old graphs off-callback after an epoch-safe handoff.

Exit gate: graph validation and last-good tests pass; a counting allocator and instrumentation prove the callback has no allocation, locks, logging, serialization, filesystem or blocking calls; induced overflow is visible.

### 5. Observation

Add native analyzers, diagnostics, telemetry leases, latency accounting, recording, atomic bundle finalization, and deterministic replay.

Exit gate: every delivery policy passes overload tests; hidden dense tiles stop leases; record/replay hashes and gaps compare as specified.

### 6. Native ASR

Refactor the existing English streaming Zipformer model set behind `magnolia-asr`. Normalize session/segment/revision/sequence events and journal finals durably. Store paths, SHA-256 hashes, backend version, and actual provider; never download models automatically.

Exit gate: reducer, gap, endpoint, ordering, journal recovery, WER/RTF, and backend error tests pass. Record the T14 tier; leave the 3090 tier explicitly pending until run there.

### 7. Persistence and acceptance

Add atomic workspace save/restore, unsupported-schema handling, undo/redo, transcript paging/export, shared Mock/Real scenarios, T14 certification, delegated 3090 certification, and a final documentation truth sweep.

Exit gate: automated gates and applicable measured targets pass with evidence; docs label every unrun tier or nondeterministic limitation.

## Automated gates

- Graph: port/schema/format/clock/delivery mismatch, module-configuration schemas, cycles, delay, fan-in, capacities, activation failure, and last-good retention.
- Real-time: counting allocator plus review/instrumentation for forbidden locks, logging, serialization, filesystem, blocking, and per-sample queues.
- Protocol: JSON and `postcard` goldens, major-version rejection, malformed frames, idempotency, revision conflicts, and reconnect cursors.
- Web: WASM compile, release Trunk build, focus, shortcut conflicts, pointer parity, layout, captions, reconnect, reload, and accessibility.
- Shared Mock/Real scenarios: receipts, revisions, device loss, activation, persistence, undo/redo, and transcript recovery.
- Overload: every policy, counters, discontinuities, control isolation, and final retention.
- Persistence: atomic save/restore, interrupted recording recovery, transcript paging, unsupported schemas, and replay comparisons.
- Hardware: 30-minute T14 continuity/latency and corpus ASR runs; separate actual-provider RTX 3090 run.

`scripts/verify.sh` is the authoritative, non-skippable local gate for an exact
candidate SHA. It includes the focused Rust checks, native desktop tests, studio
WASM/Trunk build, Chromium E2E, documentation, repository-policy, and whitespace
audits. Magnolia has no repository Actions workflow or required remote status;
the PR must record the exact locally verified SHA and results. Later audio/ASR
phases must extend this entry point only when their prerequisites apply.

## Risks and required evidence

| Risk | Gate or mitigation |
|---|---|
| PipeWire quantum/hotplug differs by device | Record negotiation/hotplug scenarios; exact selectors never silently fall back. |
| `rtrb` callback or capacity behavior | Counting allocator, overload harness, budget histograms, explicit gap tests. |
| Sherpa licensing/packaging | Record source/terms/hashes; keep paths local; no automatic download. |
| Chromium app-mode/profile/origin behavior | Test fresh launch, token removal, origin rejection, reconnect, isolation. |
| WebGL2 fallback and dense-frame cost | Browser GPU accelerates presentation only; Canvas/WebGL tests and 30 FPS criterion; WebGPU is optional. |
| Journal/disk failure | Project storage errors; never acknowledge a final before durable append. |
| Hardware claims drift | Pin commit/build/model/corpus/provider and retain raw measurements. |

## Deferred work

Plugin ABI/loading/hot reload/signing/sandboxing, browser-only runtime, Tauri, Qwen, cloud ASR, shared Mirabile/Digital Liquid Light Lab frameworks, virtual devices, beamforming, multi-GPU scheduling, production rendering claims, and legacy import are deferred and require a later accepted phase or ADR.
