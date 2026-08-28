# Magnolia

**Current status:** Phase 3 hard cutover in progress. The Leptos desktop is the default native application and the Nannou daemon, custom modal/control UI, legacy tile/layout presentation stack, fonts, and unproven visual assets are removed. The old `magnolia_core`/`Signal` and plugin crates remain only until the second breaking deletion commit. Implementation and tests—not old feature claims—are authoritative.

**Accepted target:** a native authoritative, typed, fixed-block runtime with a Leptos 0.8 CSR/Trunk studio served over authenticated loopback and opened in a dedicated Chromium app window. Migration is in progress: Phase 1's portable contracts/deterministic mock round trip and Phase 2's authenticated native desktop/Leptos shell proof are implemented. Native audio, ASR reconstruction, filesystem persistence, and the hard cutover are not.

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
cutover. It uses only synthetic source/processor/sink descriptors and the native
`MockRuntime`; it does not access audio or other devices.

## Migration boundary

Phase 2 added the `magnolia-desktop`/`magnolia-studio-web` shell,
loopback control and bounded binary telemetry transports, the retained cockpit,
and real Chromium automation. Phase 2 was merged by PR #5 after the exact head
passed the owner-authorized local gate; Actions run `33206890066` was canceled,
not passed. Phase 3 is performing the hard cutover under the same exact-SHA
local verification policy. It does not add PipeWire, real-time audio, device
access, model downloads, ASR, benchmarks, filesystem persistence, legacy
deletion, or hardware certification. T14 and RTX 3090 figures remain acceptance
targets until evidence from the exact build, model, corpus, and provider is
recorded.
