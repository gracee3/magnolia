# Phase 5 observation, recording, and replay

Status: promotion candidate on 2026-08-28.

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

These measurements are development evidence, not the promotion result. Phase 5
remains a candidate until the same clean exact head passes the default 1,800
second `./scripts/verify-phase-5-live.sh` and complete `./scripts/verify.sh`.

## Deferred boundary

Sherpa/model acquisition, ASR event reduction and journals, WER/RTF evaluation,
workspace filesystem persistence, transcript export, and GPU tiers remain Phase
6/7 work. No model is downloaded by Phase 5 startup or verification.
