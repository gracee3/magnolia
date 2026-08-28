# Phase 4 native audio

Status: in progress

Base: Phase 3 merge commit
`3e6391f90ee2e7f9ea0fa4388693be790d4f2021`.

## Implemented foundation

`magnolia-audio` is a new inward-pointing workspace crate. It provides validated
native audio formats, fixed-capacity preallocated `AudioBlock`s, and paired
`rtrb` free/ready queues. The callback-side producer never waits or allocates:
when no free block exists it increments a cumulative drop counter, and the next
published block carries an explicit discontinuity count. Edge snapshots expose
published, consumed, dropped, and high-water counts.

Explicit caller-buffer transforms cover little-endian `i16` to `f32`, channel
downmix, and stateful linear sample-rate conversion. These establish dataflow
contracts, not audio-quality or performance certification.

`magnolia-runtime` now provides a one-slot prepared-graph handoff. Control code
allocates and queues a complete replacement; callback code swaps it only at a
block boundary and sends the previous graph to a retired queue for destruction
off the callback. A failed/full preparation leaves the last-good graph active.

On Linux, PipeWire registry discovery enumerates current `Audio/Source` and
`Audio/Source/Virtual` nodes after a core sync. Exact node selectors return an
explicit unresolved error and never fall back; following the default remains a
separate selector.

The discovery example was exercised on the implementation host and returned two
physical PipeWire input-source nodes. This proves current-host registry access
only; it does not prove capture, hotplug behavior, default-device metadata, or
portable hardware support.

The focused locked gate passes 56 unit tests and four Rust integration
scenarios, including induced block-edge overflow/discontinuity, explicit
caller-buffer transforms, exact-selector failure, block-boundary swapping,
off-callback reclamation, and last-good retention when the pending slot is full.

## Not yet implemented or proven

There is no live PipeWire stream/callback connection yet, no hotplug/default
metadata tracking, no negotiated quantum publication, no capture/monitor
commands, no compiled runtime graph, and no device-loss recovery. The current
linear resampler is a contract implementation, not a production-quality DSP
selection. Allocation/lock instrumentation and induced real callback overload
tests remain required before any real-time claim.

ASR, models, analyzers, recording/replay, GPU work, legacy import, Tauri/SSR,
and hardware benchmarks remain outside this phase increment.
