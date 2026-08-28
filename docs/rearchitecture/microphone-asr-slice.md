# Microphone and ASR vertical slice

Status: accepted later vertical slice; not implemented or certified.

This begins only after the portable domain/protocol/client/application/runtime
foundation, deterministic mock round trip, synthetic shell proof, and hard
cutover gates. It is not the first implementation slice.

## Scope and graph

The first native graph is:

```text
PipeWireCapture
  -> SampleFormatConvert
  -> ChannelMap/Downmix
  -> Resample
  -> { Level, Waveform, Spectrogram, SherpaASR,
       optional Monitor, optional Recorder }
```

Native capture format is always explicit. The Sherpa boundary is mono interleaved
`f32` at 16 kHz. Raw PCM bitrate is derived as sample rate × channels × sample
width; there is no codec bitrate until an encoder module exists.

`CaptureMute` zeros source blocks while preserving clock/frame continuity.
`MonitorMute` gates only the output sink. Analyzer bypass suspends that analyzer.
`StopGraph` explicitly stops execution. Monitoring and raw recording default off.

## Semantic operations

This microphone/ASR slice provides semantic commands for selecting/following a device,
preparing/activating/stopping the graph, starting/stopping transcription,
capture mute, monitor enable/mute, analyzer enable/rate, recording start/stop,
workspace save, undo/redo of durable edits, and transcript export. Device loss,
graph activation, model load, recording finalization, and export may return an
operation ID and publish progress.

Commands use `CommandEnvelope` revision and idempotency rules. Runtime controls
do not enter undo history. Starting transcription opens a local transcript
journal before recognition events are accepted. Stopping drains and finalizes
ordered finals before the session reports complete.

## End-to-end dataflow and backpressure

1. PipeWire negotiation resolves the requested device fingerprint/default
   selector and publishes the exact format and quantum.
2. The callback fills preallocated, frame-indexed blocks and never blocks.
3. Explicit conversion, mapping, and resampling nodes produce the ASR contract.
4. Fixed bounded taps feed analyzers, ASR, monitor, and optional recorder with
   policy-specific counters and discontinuities.
5. Near-real-time analyzers publish decimated derived frames only while leased.
6. Sherpa consumes blocks off the callback, emitting events keyed by session,
   segment, revision, and sequence.
7. Replaceable partials update the latest segment revision. Finals append in
   order to the journal before projection publication acknowledges durability.
8. The browser receives projections over control and previews over telemetry;
   reload/reconnect has no effect on the native graph.

PCM overflow increments queue/drop counters and creates a gap. An ASR gap closes
or explicitly marks the affected partial; it never fabricates continuity. Final
transcript events are journaled rather than sent through a lossy queue.

## Sherpa adapter and model provenance

Refactor the existing streaming Zipformer backend first: epoch-99 INT8 encoder
and joiner, decoder, and `tokens.txt`. Preserve backend-neutral event/reducer and
WER/RTF harness concepts. The adapter records configured paths, SHA-256 hashes,
Sherpa/backend version, model identity, and the actual execution provider. It
must not infer CUDA from installed hardware and must never download a model.

Later native ASR engines implement the same adapter/event boundary. Qwen and
hosted fallback are outside this slice.

## Diagnostics and latency accounting

Expose negotiated device/format/quantum, callback budget, callback duration
histograms, xruns, per-edge capacity/depth/high-water/drop counts, discontinuity
positions, analyzer compute and publish rates, telemetry negotiation/skips,
command-to-receipt and command-to-projection latency, ASR RTF, first partial,
endpoint-to-final, partial revisions, journal cursor, and recording status.

Latency boundaries use source frame positions and runtime monotonic timestamps.
UTC metadata is attached by a non-real-time worker. The UI labels missing,
unresolved, stopped, degraded, and failed states rather than substituting a
device or suppressing a gap.

## Session recording and replay

Recording is explicit, opt-in, and visibly active. A bundle contains:

- manifest and workspace snapshot;
- model, runtime, and build hashes;
- chunked `f32le` PCM;
- block timeline and gaps;
- semantic command/event log;
- ordered transcript journal;
- diagnostics and negotiated resource metadata.

Creation is staged and finalized atomically by a storage worker. An interruption
leaves a detectable recoverable/incomplete bundle. Replay schedules from recorded
frame positions either in real time or as fast as possible. Same-version replay
compares PCM/timeline hashes and deterministic derived/transcript events.

## Measurable acceptance criteria

- At 48 kHz stereo, real-time processing uses less than 25% of device quantum at
  p99 and less than 50% at p99.9 during a 30-minute T14 run, with zero
  runtime-induced xruns or silent sample loss.
- Capture-to-level p95 is at most 50 ms; waveform at most 100 ms; spectrogram at
  most 200 ms.
- Dense displays sustain 30 FPS with p95 frame time at most 33.3 ms. 60 FPS is an
  optional enhancement.
- Loopback command receipt p95 is at most 50 ms and authoritative projection
  visibility p95 is at most 100 ms.
- T14 Sherpa RTF is at most 1.0 on the pinned corpus.
- The RTX 3090 tier targets first-partial p95 at most 800 ms and endpoint-to-final
  p95 at most 1.5 s, reporting the actual CPU/CUDA provider.
- Pinned LibriSpeech test-clean fixtures achieve WER at most 15%, with 100%
  ordered final-segment retention.
- Twenty UI reload/reconnect cycles during capture leave graph revision and audio
  execution uninterrupted.
- Every induced overflow increments the correct counters and discontinuity.
- Same-version replay reproduces PCM/timeline hashes exactly and transcript events
  exactly where the backend is deterministic.

Passing one tier does not imply passing the other. Results record host, adapter,
PipeWire quantum, build/profile, model hashes, corpus identity, warmup, duration,
and raw measurements.
