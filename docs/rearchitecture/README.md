# Magnolia native runtime and Leptos studio rearchitecture

Status: **accepted target architecture; Phases 1 and 2 implemented**

Accepted: 2026-08-27

Implementation status: Phase 1 adds portable contracts and a deterministic mock
round trip. Phase 2 adds the authenticated native desktop, Leptos cockpit,
bounded synthetic telemetry, and Chromium lifecycle proof. Phase 3 and later
remain unimplemented.

This package is the controlling plan for replacing Magnolia's prototype runtime
and Nannou shell. Phase 1 source/tests establish only the portable contracts and
mock behavior recorded here; they do not establish native execution, real-time
properties, filesystem persistence, browser studio/transport, or later-phase
acceptance results. Current behavior remains whatever source and tests on `main`
demonstrate.

## Controlling decisions

- Native Rust is authoritative for devices, graph state, scheduling, ASR,
  persistence, telemetry production, and GPU computation/resources. Browser GPU
  APIs may accelerate presentation only.
- Leptos 0.8 CSR, built with Trunk, owns presentation and disposable interaction
  state only.
- Modules and tiles are independent, many-to-many concepts. A module can have no
  tile or several tiles; a tile can bind several typed runtime resources.
- The first desktop shell is a loopback native service plus a dedicated Chromium
  app window. Tauri is deferred and, if introduced, must use the same client
  boundary. Linux Tauri uses WebKitGTK rather than Chromium; Tauri commands and
  channels, not generic events, are the relevant later high-throughput boundary.
- Correctness and continuity are certified on the current ThinkPad T14. The
  separate ASR performance tier runs on an RTX 3090 host and reports the actual
  execution provider.
- The existing Sherpa streaming Zipformer implementation is refactored first.
  An adapter boundary permits later native ASR engines.
- There is no importer for `configs/layout.toml`, the plugin ABI, or other legacy
  compatibility formats.
- Raw recording is explicit and off by default. Final transcript text is durable
  whenever a transcription session is active.
- First-party module factories are statically registered. Magnolia does not
  extract a shared framework with Mirabile or Digital Liquid Light Lab.

## Terms

- **Module**: a native executable capability with typed ports and a lifecycle.
- **Tile**: a web presentation surface bound to zero or more runtime resources.
- **Workspace document**: durable user intent: graph, configuration, bindings,
  promoted settings, and named layout presets.
- **Runtime state**: native, process-local facts about devices, nodes, queues,
  clocks, operations, errors, and graph activation.
- **Presentation session**: disposable browser state such as focus, selection,
  inspectors, zoom, and drafts.
- **Projection**: immutable, versioned native state published to clients.
- **Runtime epoch**: identity of one native process lifetime; monotonic clocks and
  sequence numbers are interpreted within it.
- **Target graph**: validated graph requested by the durable document.
- **Active graph**: last-good graph currently executing.

## Document index

- [Current state and disposition](current-state-and-disposition.md) maps current
  paths and symbols to keep, refactor, or delete decisions.
- [Planning audit](audit-2026-08-28.md) records inspected evidence, corrections,
  and host-tool snapshots without claiming build or device validation.
- [Phase 1 foundation](phase-1-foundation.md) records the implemented crates,
  mock scenarios, verification commands, and deferred boundaries.
- [Phase 2 shell proof](phase-2-shell-proof.md) records the native/browser
  topology, authentication, retained cockpit, telemetry, E2E evidence, and
  Phase 3 prerequisites.
- [Target architecture and protocol](target-and-protocol.md) defines crate
  direction, graph contracts, ownership, façade, transport, and studio rules.
- [Microphone and ASR slice](microphone-asr-slice.md) defines a later native-audio
  vertical slice, after the portable mock foundation and hard cutover.
- [Migration and verification](migration-and-verification.md) defines commit-sized
  phases, exact deletion gates, tests, risks, and deferred work.
- [ADRs](../adr/) record the eight accepted decisions.

## Authority and change control

The ADRs and this index settle target direction. The detailed documents settle
the contracts and gates. Where implementation differs, that difference is an
uncompleted migration or a proposed ADR amendment—not evidence that the target
already exists. Benchmarks are targets until a recorded run identifies the host,
build, input fixture, backend/provider, and result.
