# Contributor and agent guidance

Magnolia has completed the Phase 3 hard cutover. Read
`docs/rearchitecture/README.md`, `current-state-and-disposition.md`, and the
document for the phase in scope before editing. Implementation and tests are
authoritative; never describe a target or threshold as implemented or
certified without exact evidence.

## Active boundaries

- The workspace has eight members: domain, protocol, client, application,
  runtime, audio, desktop, and studio-web.
- Native Rust owns authoritative state and telemetry production. Leptos owns
  presentation and disposable session state.
- Native audio block transport, explicit conversion primitives, block-boundary
  activation, and Linux PipeWire discovery are implemented. Live PipeWire
  stream capture, devices beyond discovery, ASR, filesystem persistence, GPU
  computation, and hardware certification are not.
- There is no Nannou shell, legacy signal/plugin system, compatibility bridge,
  layout importer, or replacement plugin architecture. Git history is the
  archive.

## Validation and provenance

Use `./scripts/check.sh` for the focused portable loop and
`./scripts/verify.sh` as the authoritative exact-SHA local gate. The latter
includes native workspace tests, denied-warning Clippy, portable and studio
WASM checks, release Trunk output, real-Chromium scenarios, documentation and
repository audits. Use `./scripts/bootstrap-e2e.sh` only when the pinned local
browser-test dependencies are absent. Use `./scripts/run.sh` for the canonical
desktop launch.

Do not download models or corpora, access devices, run capture/benchmarks, add
plugins, or use a GPU unless the scoped phase explicitly authorizes it. Never
commit secrets, local paths, browser artifacts, recordings, transcripts,
models/corpora, unjustified binaries, or unlicensed assets. Record provenance
and actual providers for later measured work.
