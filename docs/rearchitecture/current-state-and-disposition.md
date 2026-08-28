# Current state and disposition

Status: Phase 4 native-audio promotion candidate on 2026-08-28.

## Active workspace

The root workspace contains the seven Phase 3 members plus the Phase 4
`magnolia-audio` crate:

- `magnolia-domain`: typed documents, graphs, identifiers, and validation;
- `magnolia-protocol`: versioned control and telemetry wire contracts;
- `magnolia-client`: the browser-compatible client boundary;
- `magnolia-application`: authoritative command and projection service;
- `magnolia-runtime`: native audio composition plus an injected deterministic
  runtime used only by tests;
- `magnolia-audio`: preallocated audio blocks, bounded block edges, explicit
  transforms, persistent Linux PipeWire registry/default tracking, negotiated
  capture, muted monitoring, callback instrumentation, and hotplug recovery;
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
mixing runtime-only state into workspace history. ASR, observation,
recording/replay, filesystem persistence, and GPU computation remain deferred.

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

Phase 4 is implemented but is not promoted until the clean exact head passes
the 30-minute live gate and complete repository gate. Observation and
recording/replay begin in Phase 5; ASR begins in Phase 6; persistence begins in
Phase 7. Plugin/marketplace work, Tauri, SSR, legacy import, model downloads,
and GPU work remain explicitly deferred.
