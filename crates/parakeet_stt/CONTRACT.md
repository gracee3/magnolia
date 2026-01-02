### Parakeet STT signal contract (1C)

#### Output events (as `Signal::Computed`)

- `source="stt_partial"`: JSON payload describing a partial hypothesis.
- `source="stt_final"`: JSON payload describing a final transcript (typically on stop/end-of-stream).
- `source="stt_error"`: JSON payload describing an error.
- `source="stt_metrics"`: JSON payload describing telemetry.

All payloads are **extensible**: consumers should ignore unknown fields.

#### Event payload schema v1

Every payload includes:

- `schema_version: 1`
- `kind: "partial" | "final" | "error" | "metrics"`
- `seq: u64` (strictly monotonic per session)

#### Note: FP32 IO on FP16 engines

This integration may pass **FP32** feature tensors at the boundary even when the TensorRT engines are built with **FP16** enabled.\n\nInternal compute still benefits from FP16; switching the IO tensors to FP16 is an **optimization pass** and must be treated as a contract change (buffer sizes and packing).


