# Phase 4 native audio

Status: in progress

Base: Phase 3 merge commit
`3e6391f90ee2e7f9ea0fa4388693be790d4f2021`.

## Implemented candidate

`magnolia-audio` is a new inward-pointing workspace crate. It provides validated
native audio formats, fixed-capacity preallocated `AudioBlock`s, and paired
`rtrb` free/ready queues. The callback-side producer never waits or allocates:
when no free block exists it increments a cumulative drop counter, and the next
published block carries an explicit discontinuity count. Edge snapshots expose
published, consumed, dropped, and high-water counts.

Explicit caller-buffer transforms cover negotiated `F32LE`, `S16LE`, and
`S32LE`; mono duplication or FL/FR stereo mapping; and streaming linear
resampling from 8--192 kHz into 48 kHz stereo 256-frame Magnolia blocks.
PipeWire quanta up to 8,192 frames are split or combined without assuming the
callback size equals the Magnolia block size.

`magnolia-runtime` now provides a one-slot prepared-graph handoff. Control code
allocates and queues a complete replacement; callback code swaps it only at a
block boundary and sends the previous graph to a retired queue for destruction
off the callback. A failed/full preparation leaves the last-good graph active.

On Linux, a dedicated PipeWire loop tracks device and node globals plus
`default.audio.source` and `default.audio.sink` metadata. Runtime identifiers
derive only from `node.name`, `device.api`, and `object.path`; labels stay
presentation-only. Exact selectors never fall back. Follow-default selection
is unresolved until metadata names an available source, and a changed default
causes explicit stream replacement without rewriting the workspace.

Production desktop composition uses `NativeRuntime`; test mode injects
`MockRuntime`. Runtime-only start/stop, capture mute, monitor enable/mute/gain
commands do not enter persistence or undo history. Durable device-selector
edits do. The cockpit publishes available devices, resolved identities,
format/rate/channels/positions/quantum, revisions, callback percentiles,
overrun/underrun/drop/discontinuity counters, and the last error.

The callback path is limited to mapped buffers, prepared storage, `rtrb`,
atomics, and bounded loops. A callback-thread allocator counts both allocation
and deallocation. Timing uses a preallocated atomic histogram. Monitoring starts
disabled, muted, and at zero gain; gain ramps over 50 ms. Dormant/startup edges
do not manufacture overflow or underrun counts. Resource destruction and
replacement remain on control threads.

The native compiler accepts only one ordered input -> conversion -> channel map
-> resample -> capture mute path with an optional monitor sink. Unsupported,
incomplete, duplicate, or miswired candidates fail before publication and the
application retains its last-good active revision. Failed selector candidates
also leave the active selector unchanged.

Device loss retires streams off-callback, retains exact/default intent, moves
the projection to degraded state, and retries at a bounded interval. The same
exact fingerprint or a newly resolved default recovers with cumulative counters
and explicit discontinuity increments. Temporary PipeWire nodes are supported
without inventing a physical device identity.

## Host evidence

The implementation host is the ThinkPad T14 running PipeWire 1.6.2. The exact
physical input opened for proof was
`alsa_input.pci-0000_00_1f.3-platform-skl_hda_dsp_generic.HiFi__Mic1__source`.
It remained session-muted and no samples were stored. Negotiation reported
`F32LE`, 48 kHz, FL/FR stereo, and a 1,024-frame quantum.

A three-second capture proof reported 138 callbacks, zero faults/drops, zero
callback allocations/deallocations, 0.13 ms p99, and 0.14 ms p99.9. A separate
muted capture-to-default-output proof reported zero faults, drops, underruns,
or allocations across three consecutive runs; representative p99 values were
0.14 ms capture and 0.05 ms output. At the negotiated input quantum, the Phase
4 limits are 5.33 ms p99 and 10.67 ms p99.9.

A temporary `pw-loopback` source proved exact-fingerprint loss and recovery:
the runtime progressed from running to degraded to running, retained cumulative
callback counts, emitted two discontinuities, and performed capture-mute plus
silent monitor controls capped at 0.03 gain. The temporary nodes and processes
were removed and default source/sink mute and volume state remained unchanged.

A separate headless Chromium proof used the production `NativeRuntime`, selected
the metadata-resolved default input, started capture, reloaded the cockpit
twenty times, and observed a monotonic callback count before an explicit stop.
The browser and its temporary profile exited with the native host; reload and
tile presentation state did not restart the audio runtime.

`scripts/verify-phase-4-live.sh` encodes physical open, the default 30-minute
capture soak, twenty start/stop cycles, muted monitor proof, percentile and
zero-loss assertions, twenty production-runtime browser reloads,
temporary-source loss/recovery, safe control toggles, session-state comparison,
and process/node cleanup. Shortened development runs do not satisfy promotion;
the clean exact-head run must use its defaults.

## Deferred beyond Phase 4

The linear resampler is the accepted initial path, not a general
production-quality DSP selection. Analyzers, telemetry replacement,
recording/replay, ASR/models, persistence, transcript export, GPU work, legacy
import, and Tauri/SSR remain outside Phase 4. Physical microphone audio is never
recorded by this gate.
