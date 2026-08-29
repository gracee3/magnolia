# Phase 5 observation, recording, and replay

Status: promotion-ready candidate on 2026-08-28.

## Implemented topology

Phase 5 adds the ninth workspace crate, `magnolia-observe`. `PipeWireCapture`
publishes canonical 48 kHz stereo blocks to a separate preallocated analysis
edge only while an observer is attached. `ObservationHub` runs meter, waveform,
2,048-sample Hann/50%-overlap real FFT, and sampled diagnostics off the callback
thread. Browser visibility owns analyzer leases; production desktop telemetry
uses the latest native frames, while deterministic host test mode retains its
seeded generator. Canvas drawing remains capped at roughly 30 FPS.

Each native analyzer frame has a versioned header with source-frame range,
capture, block-complete, graph, and analyzer monotonic timestamps plus cumulative
loss and discontinuity state. Callback publication remains bounded and the
callback counting allocator and strict source audit cover the added edge.

`RecordingStorageWorker` is explicit and bounded. It writes chunked `f32le` PCM,
workspace snapshot, timeline, semantic controls, diagnostics, analyzer frames,
telemetry payloads, transcript placeholder, hashes, build/device metadata, and a
versioned manifest to an incomplete staging directory. Finalization syncs files,
atomically renames the directory, and syncs its parent. Recovery is explicit and
validates JSON, PCM frame boundaries, and hashes before publishing a recovered
bundle. `ReplaySource` supports real-time, accelerated, and deterministic-step
clocks and reproduces PCM, timeline, control, analyzer, telemetry, and manifest
hashes.

## Current evidence

The short live gate (`MAGNOLIA_PHASE5_SOAK_SECONDS=5`) passed on the T14 using
the default physical input negotiated as F32LE, 48 kHz, stereo, quantum 1,024.
It observed 924 blocks from 231 callbacks with zero callback allocations or
deallocations, zero dropped frames, and zero ring faults. Native meter,
waveform, spectrum, diagnostics, tile close/reopen, seeded recording, atomic
finalization, corruption rejection, incomplete recovery, and repeated replay
hash comparisons passed. Physical microphone samples were analyzed live but
were never supplied to the recording worker or written to disk.

The first default-duration candidate run reached 84,451 callbacks and 337,804
analyzed blocks with zero allocation, deallocation, drop, or ring faults;
callback p99/p99.9 were 0.20/0.24 ms and analyzer p95 was 0.125 ms. Promotion was
correctly rejected afterward when a rapid workspace transition left the
spectrum lease released. The browser visibility effect had invalidated an
in-flight subscription during an unrelated rerun, allowing its stale completion
to release the replacement. The repair advances generations only at visibility
boundaries and prevents stale completions from releasing a currently desired
lease. A short post-repair gate passed ten consecutive Diagnose/Transcribe
transitions. The rejected run is retained as evidence, not acceptance.

At repaired exact head `501da3e`, the complete `./scripts/verify.sh` gate and
default 1,800-second `./scripts/verify-phase-5-live.sh` gate passed. The accepted
live tier again observed 84,451 callbacks and 337,804 analyzer blocks with zero
callback allocations/deallocations, drops, or ring faults. Callback p99/p99.9
were 0.20/0.23 ms, maximum callback time was 0.486 ms, and analyzer p95 was
0.075 ms. The seeded replay reproduced PCM, timeline, controls, analyzer, and
telemetry hashes. Native meter, waveform, spectrum, diagnostics, ten consecutive
Diagnose/Transcribe transitions, close/reopen, process/node/profile teardown,
and user-session default/mute/volume equality all passed. Physical microphone
samples were not recorded. The documentation-only evidence commit must pass the
same complete and live gates before promotion; no source correction is pending.

## Deferred boundary

Sherpa/model acquisition, ASR event reduction and journals, WER/RTF evaluation,
workspace filesystem persistence, transcript export, and GPU tiers remain Phase
6/7 work. No model is downloaded by Phase 5 startup or verification.
