# ADR 0007: Sherpa-first ASR and durable sessions

Status: Accepted, 2026-08-27

## Decision

Refactor the streaming Sherpa Zipformer backend first. ASR consumes mono interleaved 16 kHz `f32`. Final transcript segments are durable during transcription. Raw recording is explicit and off by default; enabled recording creates a provenance-complete replay bundle.

## Consequences

Record model paths/hashes, backend version, and actual provider; never download automatically. Partials are replaceable, finals are not. T14 correctness/continuity and RTX 3090 performance certification are distinct tiers.
