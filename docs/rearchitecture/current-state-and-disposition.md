# Current state and disposition

Status: Phases 4 and 5 promoted; Phase 6 provenance-blocked foundation on 2026-08-28.

## Active workspace

The root workspace contains ten crates:

- `magnolia-domain`: typed documents, graphs, identifiers, and validation;
- `magnolia-protocol`: versioned control and telemetry wire contracts;
- `magnolia-client`: the browser-compatible client boundary;
- `magnolia-application`: authoritative command and projection service;
- `magnolia-runtime`: native audio composition plus an injected deterministic
  runtime used only by tests;
- `magnolia-audio`: preallocated audio blocks, bounded block edges, explicit
  transforms, persistent Linux PipeWire registry/default tracking, negotiated
  capture, muted monitoring, callback instrumentation, and hotplug recovery;
- `magnolia-observe`: leased native meter/waveform/spectrum/diagnostic analyzers,
  capture-to-browser timestamps, an explicit recording storage worker, atomic
  provenance bundles, incomplete-bundle recovery, and deterministic replay;
- `magnolia-asr`: portable normalized ASR events, bounded inference-worker
  coordination, gap/reset handling, strict revision reduction, durable final
  journals, and a feature-gated Sherpa 1.13.4 adapter boundary;
- `magnolia-desktop`: loopback host, authentication, telemetry, and Chromium
  lifecycle; and
- `magnolia-studio-web`: Leptos CSR presentation and disposable session state.

`magnolia-desktop` is the default native member. One root `Cargo.lock` is
authoritative. `./scripts/run.sh` builds the studio and launches the loopback
desktop with a dedicated temporary Chromium profile.

The descriptor registry retains the synthetic chain for deterministic shell
tests and adds a strict native path: PipeWire input, sample conversion, channel
mapping, resampling, capture mute, and optional monitor output. Normal desktop
composition uses `NativeRuntime`; `MockRuntime` is selected only by test mode.
PipeWire device selection and live controls appear in the cockpit without
mixing runtime-only state into workspace history. Production dense telemetry is
fed by native analyzer frames; deterministic test mode retains seeded telemetry.
Sherpa model execution remains provenance-blocked. Filesystem workspace
persistence and GPU computation remain deferred.

## Completed deletion

Phase 3 removed the Nannou daemon and UI, legacy tile/layout/input/modal stack,
fonts and visual assets, old core and global signal abstractions, duplicate
module APIs, custom rings, plugin ABI/helper/loader/manager/hot-reload/signing/
sandbox experiments, tracked plugin binary and example, and every remaining
dependent or unrelated crate and application. It also removed legacy audio,
replay, caption, STT, benchmark, environment, setup/download, and diagnostic
tooling.

There is no importer, compatibility bridge, parking crate, or replacement
plugin system. Git history is the archive. Historical Phase 1/2 and planning
records retain source-labeled references to removed paths as evidence; they do
not describe active code.

## Deferred reconstruction

Phases 4 and 5 were promoted after their clean exact heads passed complete
repository and 30-minute live gates. Phase 6 has a tested model-free foundation,
but the official model and native-library release records contain no
authoritative SHA-256 digests, so acquisition, execution, WER/RTF acceptance,
and promotion are blocked. Workspace persistence begins only after Phase 6 can
be promoted. Plugin/marketplace work, Tauri, SSR, legacy import, unvalidated
model downloads, and GPU work remain explicitly deferred.
