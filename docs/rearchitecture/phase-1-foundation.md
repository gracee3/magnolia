# Phase 1 portable foundation implementation

Status: implemented and locally verified on 2026-08-28 with Rust 1.97.1.

This record distinguishes the implemented portable foundation from later target
work. It is evidence for Phase 1 only; it is not evidence of native audio,
real-time safety, browser continuity, ASR accuracy/performance, GPU behavior, or
hardware certification.

## Implemented crates

| Crate | Implemented responsibility |
|---|---|
| `magnolia-domain` | UUID role newtypes, checked revision newtypes, static descriptors/control definitions, typed graph and layout/document data, atomic `WorkspaceEdit` batches, graph/document validation, pretty-JSON round trips, and synthetic descriptors. |
| `magnolia-protocol` | Major/minor negotiation, strict JSON command DTOs, sequenced envelopes, receipts/errors, dynamic control manifests, immutable full projections, operation/module/device/stream status types, transcript/diagnostic placeholders, clocked telemetry DTOs, and JSON/postcard golden fixtures. |
| `magnolia-client` | Portable `ApplicationClient` plus an ordered, argument-checking scripted `MockApplicationClient`. |
| `magnolia-application` | Authoritative transactions, handshake-bound commands, optimistic revision checks, a per-client 1,024-receipt replay window, atomic in-memory persistence, undo/redo, control-manifest materialization, non-consuming projection waits, runtime ports, and an in-process client. |
| `magnolia-runtime` | Explicitly driven deterministic `MockRuntime`, captured activation requests, ordered success/failure injection, and target-specific completion. |

The new crates do not depend on Nannou, PipeWire, Sherpa, Leptos, Chromium,
Tokio, device APIs, or legacy `magnolia_core`/`magnolia-signals` types. Legacy
crates remain present and unchanged because deletion belongs to the later hard
cutover.

## Verified foundation round trip

The integration scenario in
`crates/magnolia-runtime/tests/foundation_round_trip.rs` proves:

1. a supported client connects and receives the immediate revision-zero full
   snapshot;
2. one atomic batch adds a valid synthetic source-to-sink graph;
3. persistence succeeds before document/target publication and activation is
   enqueued with an operation ID;
4. an explicit mock success advances the active revision;
5. a later injected activation failure advances target/error/operation state
   while retaining the prior active revision as last-good;
6. a completion for a superseded target is ignored without publishing a new
   projection, while the current target can subsequently activate.

Focused tests additionally cover malformed/unknown wire fields, JSON and
postcard goldens, unsupported protocol/document majors, descriptor and graph
errors, cross-lane capacity, cycles/delay, atomic edit rollback, persistence
failure rollback, optimistic revision conflicts, exact receipt replay,
conflicting/expired sequences, request-ID reuse, handshake enforcement,
undo/redo, stateful control manifests, and concurrent non-consuming observers.

## Gates

`./scripts/check.sh` runs formatting, locked checks/tests, and warning-denying
Clippy for exactly the five foundation crates. `./scripts/verify.sh` reruns that
gate, explicitly exercises the full integration scenario, checks patch
whitespace, and validates internal Markdown links. CI invokes the handoff gate;
the separate legacy baseline remains manually runnable.

## Explicitly deferred

- Filesystem persistence and recovery; the implemented port is in-memory.
- Native graph compilation/execution; `MockRuntime` is not production support.
- Telemetry delivery; the DTO/golden exists, while the in-process subscription
  method reports that observation is deferred.
- Transcript storage/paging; the Phase 1 in-process client returns an empty
  revision-zero page.
- PipeWire, audio buffers/lanes, recording/replay, Sherpa or other ASR engines.
- Leptos, Chromium, loopback authentication/WebSockets, and browser automation.
- Legacy deletion, device/model access, benchmarks, and hardware certification.
