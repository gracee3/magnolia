# Phase 6 native Sherpa ASR

Status: provenance-blocked foundation, 2026-08-28.

## Implemented without model execution

The tenth workspace crate, `magnolia-asr`, defines versioned normalized ASR
events, increasing partial revisions, immutable finals, optional word alignment,
gap/reset semantics, a bounded off-callback worker, cancellation and clean drain,
and a finalized-segment journal that flushes and syncs before publication may
continue. A deterministic injected recognizer verifies partial/final ordering,
gap resets, stale result rejection, restart recovery, and ordered final retention.

The portable runtime projection carries desired/active state, session, provider,
model identity, queue depth, skips, discontinuities, first-partial/final latency,
RTF, and last error. The accepted adapter and its native sys crate are pinned
exactly to 1.13.4 and are feature-gated; ordinary builds do not link or download
the native library. The accepted configuration remains greedy CPU, two threads,
and endpoint thresholds of 2.4 seconds empty silence, 0.8 seconds trailing speech
silence, and 30 seconds maximum utterance.

`scripts/acquire-phase-6-model.sh` validates the complete lock before touching
the network, downloads to partial files, validates size and SHA-256, rejects
archive traversal, verifies encoder/decoder/joiner/token hashes, and atomically
publishes model and separately locked native artifacts. Normal Magnolia startup
never invokes it.

## Blocking provenance finding

The accepted model is the official GitHub asset ID 191971614,
`sherpa-onnx-streaming-zipformer-en-2023-06-26.tar.bz2`, size 310,414,022 bytes.
The accepted Linux x64 CPU library is the official 1.13.4 no-TTS shared-library
archive, size 9,006,130 bytes. On 2026-08-28 the GitHub asset API reported a null
digest for both, and the official Sherpa documentation/repository published no
archive SHA-256 file. The model repository establishes Apache-2.0 licensing, but
license plus a locally calculated download hash is not an authoritative archive
hash.

The checked-in acquisition lock therefore retains null digests and the command
fails closed before download. Model execution, live microphone transcription,
fixtures, LibriSpeech WER/RTF evaluation, browser acceptance, and Phase 6
promotion have not run. Filling those fields with self-computed values would
weaken the explicit provenance gate and is not accepted evidence.

The host has Intel Iris Xe graphics and no compatible CUDA provider. The GPU tier
is unavailable/not run and would not block CPU promotion after provenance and
the remaining CPU acceptance gates pass.
