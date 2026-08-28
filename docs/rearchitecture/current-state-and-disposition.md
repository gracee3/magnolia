# Current state and disposition

Status: Phase 3 hard cutover implemented on 2026-08-28.

## Active workspace

The root workspace contains exactly seven members:

- `magnolia-domain`: typed documents, graphs, identifiers, and validation;
- `magnolia-protocol`: versioned control and telemetry wire contracts;
- `magnolia-client`: the browser-compatible client boundary;
- `magnolia-application`: authoritative command and projection service;
- `magnolia-runtime`: deterministic runtime port and current mock runtime;
- `magnolia-desktop`: loopback host, authentication, telemetry, and Chromium
  lifecycle; and
- `magnolia-studio-web`: Leptos CSR presentation and disposable session state.

`magnolia-desktop` is the default native member. One root `Cargo.lock` is
authoritative. `./scripts/run.sh` builds the studio and launches the loopback
desktop with a dedicated temporary Chromium profile.

The implemented shell still uses synthetic descriptors and `MockRuntime`. It
does not provide devices, audio, ASR, filesystem persistence, GPU computation,
or production/hardware certification.

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

Phase 4 begins native audio under the accepted typed fixed-block contracts.
Observation, recording/replay, and ASR follow only in their documented later
phases. Plugin/marketplace work, Tauri, SSR, legacy import, model downloads,
device access, GPU work, and hardware certification remain explicitly deferred.
