# Magnolia

**Current status:** Phase 3 hard cutover is implemented and the Phase 4 native-audio candidate is undergoing its final exact-head gate. The Leptos desktop composes the native runtime in production and retains an injected deterministic mock only for tests. The audio path provides persistent PipeWire registry/default metadata, durable exact/default selectors, negotiated capture, preallocated format/channel/rate adaptation, safe muted monitoring, bounded callback edges, hotplug recovery, and runtime diagnostics. The 30-minute promotion soak remains the boundary between implemented and Phase 4 accepted. Implementation and tests—not old feature claims—are authoritative.

**Accepted target:** a native authoritative, typed, fixed-block runtime with a Leptos 0.8 CSR/Trunk studio served over authenticated loopback and opened in a dedicated Chromium app window. Phases 1 through 3 are implemented and Phase 4 is in progress. ASR reconstruction, filesystem persistence, and hardware certification are not implemented.

Sherpa is the first ASR adapter reconstructed/refactored in the later native-ASR
phase; it is not the first implementation work. Phase 1 is the portable
domain/protocol/client/application/runtime foundation and deterministic mock
round trip.

Read [the rearchitecture index](docs/rearchitecture/README.md) for controlling status, terminology, decisions, and the migration package. The [current-state disposition](docs/rearchitecture/current-state-and-disposition.md) identifies what is refactored and what is deleted after the new-shell proof. Accepted decisions are in [docs/adr](docs/adr/).

The [Phase 1 implementation record](docs/rearchitecture/phase-1-foundation.md)
maps the new crates, verified scenarios, and explicit deferred boundaries. Run
`./scripts/check.sh` for the focused foundation gate. The
[Phase 2 shell record](docs/rearchitecture/phase-2-shell-proof.md) maps the
desktop/web topology, session policy, cockpit, telemetry, E2E coverage, and
cutover prerequisites. Run `./scripts/verify.sh` for the complete handoff gate.

## New cockpit proof

Build the Leptos CSR bundle, start the loopback native host, and open its
dedicated Chromium app window with:

```bash
./scripts/run.sh
```

The desktop cockpit is the default native application after the Phase 3
cutover. Normal composition uses `NativeRuntime`; `--test-mode` injects
`MockRuntime`. The Diagnostics tile exposes native input selection, capture and
monitor controls, negotiated values, callback percentiles, loss counters, and
the last runtime error. Monitoring starts disabled, muted, and at zero gain.

## Current boundary

Phase 2 added the `magnolia-desktop`/`magnolia-studio-web` shell,
loopback control and bounded binary telemetry transports, the retained cockpit,
and real Chromium automation. Phase 2 was merged by PR #5 after the exact head
passed the owner-authorized local gate; Actions run `33206890066` was canceled,
not passed. Phase 3 completed the hard cutover under the same exact-SHA local
verification policy. It does not add PipeWire, real-time audio, device access,
model downloads, ASR, benchmarks, filesystem persistence, or hardware
certification. Phase 4 host evidence is recorded in the
[Phase 4 record](docs/rearchitecture/phase-4-native-audio.md). ASR model and GPU
figures remain later-phase acceptance targets until evidence from the exact
build, model, corpus, and provider is recorded.
