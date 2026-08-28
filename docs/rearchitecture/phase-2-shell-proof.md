# Phase 2 native desktop and Leptos shell proof

Status: implemented; browser and workflow verification required at the final Phase 2 commit.

Phase 2 adds the first visible version of the target Magnolia cockpit. It uses
the Phase 1 application contracts and `MockRuntime`; it does not bridge to the
legacy Nannou application or claim real audio, device, ASR, GPU, or filesystem
persistence support.

## Launch

From the repository root, build the CSR application, start the loopback host,
and open its dedicated Chromium app window with one command:

```bash
./scripts/run-phase-2.sh
```

Pass `--chromium /absolute/path/to/chromium` when discovery is insufficient.
The same native host prints its local launch URL if Chromium is unavailable.
It always uses a dedicated temporary profile and never opens the user's normal
Chromium profile.

## Source and dependency boundary

Phase 2 adds:

- `apps/magnolia-desktop`: native Axum 0.8/Tokio 1 loopback host, static asset
  service, session authority, control and telemetry WebSockets, native
  `ApplicationService`, deterministic `MockRuntime`, Chromium process manager,
  and integration-test endpoints available only in authenticated test mode;
- `crates/magnolia-studio-web`: Leptos 0.8.20 CSR application built by Trunk,
  browser `ApplicationClient`, retained workspace, semantic command registry,
  schema controls, Canvas2D visualizations, and reconnect lifecycle;
- `tests/e2e`: Playwright 1.62.1/Chromium system suite, pinned for Node 24.20.0;
- portable transport DTOs in `magnolia-protocol`, telemetry release in
  `magnolia-client`, and application-owned final transcript/diagnostic
  projection support in `magnolia-application`.

Native-only dependencies include Axum, Tokio, Tower HTTP, `getrandom`, Base64,
`subtle`, `tempfile`, and tracing. Browser-only dependencies include Leptos,
`web-sys`, `wasm-bindgen`, `gloo-timers`, `postcard`, and `send_wrapper`.
Axum, Tokio, WebSocket, Leptos, and browser APIs do not enter
`magnolia-domain`, `magnolia-protocol`, or `magnolia-client`.

The new desktop and web crates temporarily use the checked-in workspace rooted
at `apps/magnolia-desktop/Cargo.toml`. The root legacy workspace is pinned to a
`wgpu`/`web-sys` generation incompatible with Leptos 0.8's current `js-sys`.
Isolating the new workspace avoids upgrading code scheduled for Phase 3
deletion. Phase 3 must make the new workspace canonical after deleting that
legacy dependency conflict.

## Process topology

```text
dedicated Chromium app window
  ├─ loopback HTTP static requests ────┐
  ├─ ordered JSON control WebSocket ──────────┼─ 127.0.0.1-only Axum host
  └─ bounded postcard telemetry WebSocket ────┘          │
                                                         ├─ ApplicationService
                                                         ├─ InMemoryPersistence
                                                         └─ native MockRuntime
```

The host selects an ephemeral port by default, serves the Trunk distribution,
and owns one application/runtime epoch. Reloading, closing, or reconnecting the
browser does not recreate that service. Shutdown stops the listener, synthetic
producer, dedicated child Chromium process, and temporary profile.

## Authentication and origin policy

- The listener binds only to `127.0.0.1`; the bound address is checked before
  the host publishes readiness.
- A 256-bit random launch authority is carried after `#token=`. Fragments are
  excluded from HTTP requests and ordinary access logs, and the web client
  removes it immediately with `history.replaceState`.
- Normal readiness output prints the origin without the authority. The full
  fragment URL is exposed only to the dedicated browser, an explicit
  `--no-browser` launch, the authenticated test harness, or as the actionable
  manual fallback when browser launch fails.
- The launch token expires after five minutes by default and can be exchanged
  once. The resulting 256-bit process-local session is bound to one client ID,
  expires after twelve hours by default, and extends on valid use.
- Both WebSocket upgrades require the exact host origin. Missing, `null`,
  wildcard, alternate-host, and otherwise mismatched origins are rejected
  before authentication.
- Missing, malformed, expired, consumed, wrong, and cross-client credentials
  fail explicitly. Credential `Debug` output is redacted.
- Test mutation routes exist only with `--test-mode`, require a separate random
  header authority, and are not routed by a normal launch.

## Control transport

The browser adapter implements the async, non-global-`Send`
`ApplicationClient` boundary. `send_wrapper` is used only as a Leptos
single-browser-thread arena guard; it does not change the portable trait.

The first control message authenticates and negotiates the protocol. A valid
connection immediately receives an immutable full projection and a transcript
page. Commands remain semantic `CommandEnvelope`s; receipts, replay,
revision checks, typed edit validation, and runtime activation remain owned by
the native application. The web client keeps its monotonic request sequence in
`sessionStorage`, so an exact retry reuses the original envelope and receipt.

Projection waits remain event-driven and non-consuming. Reconnect sends the
known runtime epoch, projection revision, and transcript cursor. Pending waits
survive a transport replacement, while in-flight request calls fail explicitly
and can be retried idempotently. Dropping browser-side projection waiters or
telemetry observers removes their listeners without a polling task.

## Retained cockpit and state ownership

The authoritative workspace document contains typed modules, graph edges, tile
bindings, promoted settings, and five named split/tab layout presets:
Capture, Transcribe, Patch, Diagnose, and Perform. Stable tile IDs are separate
from the synthetic source, processor, and sink IDs.

Initial specialized tiles cover source, processor, sink, patch graph, generic
schema-driven controls, runtime status, diagnostics/history, meter, waveform,
spectrum, and synthetic partial/final transcript views. Tabs remain mounted;
hidden dense tabs release their telemetry leases. A closed presentation tile is
locally hidden and can be reopened without a runtime or document operation.

Leptos owns active-workspace presentation, focus, menus, command search, local
layout drafts, hidden tiles, canvas buffers, and animation state. The active
workspace is retained across a browser reload in session storage. A layout
preview is visibly marked `LOCAL DRAFT`; only `Commit` sends a typed
`PutPreset` document edit. Graph, configuration, runtime revisions, operation
history, receipts, and final transcript state are read from immutable native
projections.

One central command registry maps command IDs to pointer actions and scoped
keybindings. `Ctrl+K`, `Alt+1` through `Alt+5`, `G`, `L`, `Shift+L`, `U`,
`Shift+U`, `R`, and `F6` dispatch semantic presentation or application
actions. Text boxes, search fields, number inputs, and selectors retain normal
browser editing; their raw keyboard events do not enter business logic.

## Binary telemetry and backpressure

Telemetry is never placed on the ordered control command queue. A separate
authenticated WebSocket carries binary `postcard` envelopes with runtime epoch,
schema, monotonic sequence/timestamp range, stream/subscription identity, queue
depth, cumulative loss, and discontinuity.

- Meter and partial-caption streams use latest-value replacement.
- Waveform and spectrum frames use bounded drop-oldest delivery.
- Diagnostics carry two-entry batches and explicit loss since the previous
  delivered batch.
- Final transcripts are application-owned, ordered, and cursor-addressable;
  replaceable partials remain telemetry keyed by segment and revision.
- Negotiated rates are enforced against a 30 Hz producer. Per-stream capacities
  are clamped to 1 through 64, and the connection queue has a hard 64-frame
  ceiling. Drops are attributed to the actual discarded stream.

Dense views hold one small local frame buffer, draw on
`requestAnimationFrame` at an approximately 30 FPS ceiling, and never create a
signal or DOM node per sample/bin. Cleanup and hidden-state effects release
leases. Async subscription completion uses an independent lifetime guard, so a
layout change cannot touch a disposed Leptos signal.

Telemetry health is projected at a low cadence in one atomic diagnostic batch.
The overload test raises the deterministic producer to 2,000 frames per due
lease, observes bounded drops/discontinuities, and still requires a document
receipt and projection within a CI-tolerant ten seconds. This is structural
overload evidence for the synthetic path, not a production latency benchmark.

## Verification evidence

The repository handoff entry point is:

```bash
./scripts/verify.sh
```

It runs the locked Phase 1 checks and integration scenario, both Cargo
workspaces' formatting/native tests/Clippy, portable and studio WASM checks, a
release Trunk build, Playwright, Markdown links, changed-path auditing, and Git
whitespace checks. `scripts/check-phase-2.sh` is the Phase 2 component used by
that entry point. `scripts/bootstrap-e2e.sh` installs a checksummed project-local
Node distribution only when Node/npm are absent, then installs the locked
Playwright package.

The Chromium suite contains five isolated tests covering the required system
scenarios: authenticated launch and snapshot; graph receipt/projection;
successful, failed, last-good, and stale activation; document-only edits and
receipt replay; local drafts; pointer/keyboard/focus/text semantics; all dense
tiles and diagnostics; overload isolation and lease release; forced disconnect
and reconnect; twenty active-stream reloads with stable runtime/graph/activation
and bounded subscriptions; transcript cursor continuation; invalid credential;
invalid origin; unsupported protocol major; and unexpected console-error
collection. Screenshots and traces are retained and uploaded only on failure.

Native integration tests independently drive the real WebSocket handshake,
immediate snapshot, document receipt/replay, exact-origin rejection, wrong
credential rejection, and unsupported-major response. Unit tests cover session
expiry/single use, process launch discovery, bounded queue policies, negotiated
rate/capacity, explicit diagnostic loss, transcript ordering, and atomic
diagnostic projection.

Final test counts, workflow URL, and passing commit are recorded in the Phase 2
pull request rather than frozen into this implementation document.

## Known limitations and Phase 3 prerequisites

- Persistence is in memory and the runtime/data are deterministic synthetic
  fixtures. No device is enumerated or accessed.
- Canvas2D is the dense presentation path. No browser or native GPU claim is
  made.
- The dedicated browser profile is intentionally temporary; only presentation
  session state survives reloads in that profile, not a process restart.
- The shell is not yet the default application. The Nannou daemon and all other
  legacy crates remain present but share no state or UI abstraction with it.
- Test-only disconnect/failure/flood controls are unavailable in a normal host.

Before Phase 3, this PR must be reviewed, green at its final SHA, and merged.
Phase 3 then starts from updated `main`, makes the new desktop workspace and
launch path canonical, performs the two prescribed legacy deletion commits,
removes the temporary Cargo-workspace isolation, and reruns the synthetic shell
gates. Phase 3 must not begin audio, ASR, model, device, or GPU work.
