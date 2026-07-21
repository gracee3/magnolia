# Phase 0 crate inventory

Status: initial baseline classification  
Reviewed: 2026-07-21

This inventory describes migration intent, not implementation maturity.

| Area | Classification | Phase 0 treatment |
| --- | --- | --- |
| `core` | Replace incrementally | Keep compiling while the headless runtime contract supersedes UI and domain leakage. |
| `magnolia-module-api`, `magnolia-signals` | Extract/review | Keep as candidate protocol boundaries; do not deepen the global signal enum. |
| `audio_input`, `audio_output`, `audio_dsp`, `audio_replay` | Retain/rework | Keep available, but exclude hardware-facing crates from the default baseline until opt-in tests exist. |
| `magnolia-config` | Retain | Remove provider-specific TensorRT configuration; keep shared configuration helpers. |
| `magnolia-plugin-abi`, `magnolia-plugin-helper`, `hello_plugin` | Defer | Keep compiling; security and isolation claims require later validation. |
| `logos`, `text_tools` | Defer | Keep compiling without expanding features. |
| `kamea`, `magnolia-ui`, `daemon` | Defer | Preserve visual work while excluding it from the headless default baseline. |
| `aphrodite` | Remove after review | Excluded from the default baseline; astrology belongs in Astraeus. Preserve only Magnolia-specific adapter or rendering behavior worth retaining. |
| `parakeet_stt`, `parakeet_stt_demo`, `asr_test` | Remove | TensorRT/CUDA-specific implementation and sibling dependencies are outside the new foundation. Neutral event names and benchmark requirements remain documented in `STT_BACKEND_PLAN.md`. |

The default members are the hardware-, display-, ephemeris-, and CUDA-independent
subset. The full workspace remains useful for identifying deferred build failures.
