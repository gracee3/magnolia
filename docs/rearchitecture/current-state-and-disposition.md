# Current state and disposition

Status: audited from `origin/main` at `384c04e`; decisions describe the accepted
target, not completed removals.

## Architecture map

The root `Cargo.toml` currently lists 21 workspace members. The prototype has
three overlapping module systems:

- `core/src/lib.rs` defines `Source`, `Processor`, `Sink`, and `Transform`;
  `core/src/runtime.rs`
  adds `ModuleRuntime`, `ModuleHost`, bounded Tokio inboxes, `RoutedSignal`, and
  routing metrics.
- `crates/magnolia-module-api/src/lib.rs` defines another `StaticModule` contract.
- `core/src/host/module_handle.rs` wraps `StaticModule` and dynamic adapters in
  the legacy `ModuleImpl`/`ModuleHandle`; `core/src/runtime.rs` separately defines
  another `ModuleHandle` for hosted asynchronous modules.
- `core/src/plugin_adapter.rs` converts the global Rust signal into the C ABI.

The graph is split between `core/src/patch_bay.rs` and daemon orchestration.
Ports still accept broad `DataType` values, including `Any`. The shared signal
model in `crates/magnolia-signals/src/lib.rs` combines text, astrology, audio,
blobs, controls, GPU handles, and a non-cloneable receiver. It has special-case
overflow behavior based on string source names such as `stt_partial`. However,
`crates/speech_to_text/src/processor.rs` emits computed signals with
`source = "speech_to_text"`, so those events do not receive the intended partial
classification. The queue implementation drains all available signals and then
retains only the newest drained item; that behavior does not prove final-event
retention.

There are two custom unsafe SPSC implementations: `core/src/ring_buffer.rs` and
`crates/magnolia-signals/src/ring_buffer.rs`. Audio input callbacks in
`crates/audio_input/src/backend/{pipewire,cpal}.rs` send individual `f32` samples
and ignore failed `try_send` results. `crates/audio_input/src/source.rs` polls
samples into allocated batches. The output path similarly sends each sample in
`crates/audio_output/src/lib.rs`. PipeWire discovery, node properties, format
negotiation, and process callback setup are useful implementation evidence, but
the callback and queue contracts are not retained.

`apps/daemon` combines graph routing, Nannou rendering, custom modal/control UI,
tile registration, input modes, and layout persistence. Egui has been removed;
only stale comments and stub descriptions remain. `TileRenderer` and
`TileRegistry` are defined in `core/src/tile.rs`; `apps/daemon/src/tiles/mod.rs`
only re-exports them from `magnolia_core`. Other notable prototype code includes
navigation state in `apps/daemon/src/input.rs` and legacy layout parsing in
`apps/daemon/src/layout.rs`.

Current tests establish narrower facts than the target plan. `core/src/patch_bay.rs`
tests type/direction checks plus connect/disconnect. `core/src/runtime.rs` tests
module spawn/shutdown/deadline, fanout delivery, bounded-overload metrics, routed
metadata, and the string-based partial/final overflow policy. Signal-ring tests
exercise push/pop, full, wraparound, concurrency, and `f32` samples; caption and
speech-to-text tests exercise event/reducer behavior. Plugin tests cover discovery
and optional loading. No legacy test establishes prepared-graph activation,
superseded completion rejection, or last-good graph retention; those are wholly
new target behaviors.

The ASR slice is real prototype material rather than the target architecture:
`crates/speech_to_text/src/sherpa.rs` contains `LocalSherpaBackend`;
`processor.rs` adapts it to generic signals; `lib.rs` contains backend/event and
loss-sensitive queue tests. `crates/caption_state` separates partial and final
caption reduction, and `apps/stt_bench` contains WER/RTF/latency harness intent.
Model and corpus setup scripts download local artifacts, which the future runtime
must never do automatically.

## Complete disposition matrix

| Existing area | Decision | Evidence retained or replacement |
|---|---|---|
| `core/`: `PatchBay`, `ModuleHost`, both legacy `ModuleHandle`s, `ModuleImpl`, `Source`/`Processor`/`Sink`/`Transform`, `DataType`, UI/GPU resources | Delete in the second hard-cutover commit | Retain graph-validation, lifecycle, and routing-overload test intent. Last-good activation is a new contract, not retained test behavior. |
| `crates/magnolia-signals` and `magnolia-module-api` | Delete | Exact registered stream descriptors and lane-specific executors replace global/duplicated abstractions. |
| Custom rings in `core` and `magnolia-signals` | Delete | Fixed-capacity block edges built on `rtrb`, with measured invariants. |
| `apps/daemon`, `crates/magnolia-ui`, `TileRenderer`, `TileRegistry`, modal stack | Delete at shell cutover | Leptos DOM/canvas studio behind `ApplicationClient`. |
| `KeyboardNav`, `InputMode`, `SelectionState`, raw tile key handlers | Delete with Nannou | Scoped semantic commands with explicit contexts and focus hierarchy. |
| `apps/daemon/src/layout.rs`, `configs/layout.toml` | Delete | Versioned split/tab presets in `WorkspaceDocument`; no importer. |
| `audio_input`, `audio_output` | Extract knowledge, then delete at cutover | Preserve PipeWire device/property discovery and negotiation lessons in documentation/Git history; later replace callbacks, sample sends, locks, wall-clock timestamps, and silent drops. |
| `audio_dsp` | Delete at cutover; reconstruct later | Later add explicit format-convert, channel-map/downmix, resample, gain, and monitor nodes under new contracts. |
| `audio_replay` | Delete at cutover; reconstruct concepts/fixtures later | Later implement deterministic journal replay in `magnolia-observe`. |
| `speech_to_text`, `caption_state`, `stt_bench` | Delete at cutover; reconstruct later | Preserve Sherpa adapter logic, event identity/reducer tests, WER/RTF metrics, and fixtures as Git-history evidence for later new-contract work. |
| `magnolia-config` and `config/transcription.toml` | Delete after typed replacement | Workspace/resource documents replace speculative source reconciliation and layout configuration. |
| Plugin loader/manager/verifier/sandbox, ABI/helper, hello example, tracked `libhello_plugin.so` | Delete, do not park | Static first-party factories; ABI/loading/signing/sandbox work is deferred and recoverable from Git history. |
| `aphrodite`, `kamea`, `logos`, `text_tools`, `caption_demo` | Remove from active workspace | Later capabilities return only as typed modules after the first slice. |
| Legacy fonts, visual assets, glyph maps/tweaks | Delete unless proven necessary and licensed | New design assets require separate provenance and licensing evidence. |
| `.cargo/config.toml`, `rust-toolchain.toml` | Retain and revise as needed | Keep the local Cargo alias and pinned minimal Rust toolchain until replacement gates require a change. |
| `.github/workflows/ci.yml` | Replace during foundation/cutover | The manually dispatched legacy CI checks old core and the whole old workspace; new gates must cover the portable foundation before cutover. |
| `scripts/setup_sherpa_captioning.sh`, `scripts/setup_librispeech_test_clean.sh`, `scripts/run_librispeech_bench.sh` | Delete in the second cutover; reconstruct later | Download/setup and legacy benchmark commands depend on the old ASR applications and are excluded from the foundation slice. |
| `tools/audio_snapshot.sh` | Delete in the second cutover; reconstruct for native audio | It is host-oriented PipeWire diagnostic knowledge, not a portable-foundation dependency. |
| `config/magnolia.env.example` | Delete in the second cutover | It describes legacy local model paths; typed resource configuration replaces it in a later ASR phase. |
| `Cargo.toml`, `Cargo.lock`, default members, build aliases and automation | Rewrite during phases 1 and 2, then prune at cutover | The active workspace must contain only compilable new or retained crates at every commit. |
| `libhello_plugin.so` and plugin example/helper/ABI artifacts | Delete in the second cutover | Tracked binaries and the entire dynamic-plugin experiment remain recoverable from Git history. |
| Every old-core/signals dependent crate: `daemon`, `aphrodite`, `audio_dsp`, `audio_input`, `audio_output`, `audio_replay`, `kamea`, `logos`, `magnolia-module-api`, `magnolia-plugin-helper`, `speech_to_text`, and `text_tools` | Delete or strip the dependency in the second cutover | No uncompilable parking crates. `caption_state`, `caption_demo`, `stt_bench`, `magnolia-config`, `magnolia-ui`, plugin ABI/example, and other unrelated members are also removed at that cutover even where the dependency is indirect or absent. |

## Evidence incorporation from superseded plans

The useful facts carried forward are: native Rust and PipeWire boundaries;
separation of real-time data from control; static first-party modules before any
plugin work; Sherpa-first local streaming; bounded loss policies; WER, first
partial, endpoint-to-final, RTF, CPU/memory/drop measures; and the requirement to
keep astrology/tarot and unrelated application domains outside Magnolia.

Historical claims of ABI stability, sandbox enforcement, hot reload, 5–10 ns
ring performance, production GPU support, live-capture quality, or independently
certified ASR results are not accepted evidence. They require new tests under the
target contracts.
