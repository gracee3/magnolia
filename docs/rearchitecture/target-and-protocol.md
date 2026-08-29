# Target architecture and protocol

Status: accepted contract; Phases 1 through 5 are promoted and the model-free
Phase 6 ASR foundation is implemented. Sherpa execution and its CPU acceptance
remain blocked on authoritative artifact hashes.

## Crates and dependency direction

| Crate | Responsibility |
|---|---|
| `magnolia-domain` | Portable UUID ID newtypes, checked revision newtypes, documents, static module/control definitions, graph types, and validation. |
| `magnolia-protocol` | Wire DTOs, dynamic `ControlManifest`s, commands, receipts, full projections, handshake, telemetry, and transcript types; depends only on portable domain types. |
| `magnolia-client` | Async portable `ApplicationClient` with browser-local futures and scripted `MockApplicationClient`. |
| `magnolia-application` | Authoritative service, transactions, undo/redo, persistence and runtime ports, manifest materialization, projection publication, and an in-process client. |
| `magnolia-runtime` | Native runtime adapter; begins as deterministic `MockRuntime`, then gains graph compilation, lifecycle, activation, clocks, and execution lanes. |
| `magnolia-audio` | PipeWire capture/output, explicit conversion/channel-map/resample nodes, monitoring. |
| `magnolia-observe` | Counters, latency accounting, analyzer frames, session recording, replay. |
| `magnolia-asr` | Bounded inference coordination, durable final journal, reducer, and feature-gated Sherpa adapter. |
| `magnolia-studio-web` | Leptos 0.8 CSR workbench. |
| `magnolia-desktop` | Composition root, static assets, loopback transport, authentication, Chromium launch. |

Dependencies point inward: desktop composes web/application/runtime/audio/ASR;
application depends on client/protocol/domain and persistence/runtime ports; runtime and native
adapters depend on domain contracts, never on web types. Protocol does not depend
on native devices, Tokio, PipeWire, browser APIs, or application implementation.
`magnolia-desktop` statically registers first-party factories.

## Module and graph contract

`StreamTypeId` plus schema version replaces `DataType::Any` and `Signal`. Each
port also declares timing, delivery policy, and format constraints. Source,
processor, and sink are descriptive roles derived from port direction; they are
not separate trait families.

Cold-path `ModuleDescriptor` contains module type/version, typed ports,
configuration schema, static `ControlDefinition`s, execution lane, and
capabilities. Static definitions live in `magnolia-domain`; the application
materializes protocol `ControlManifest`s containing current values, availability,
disabled reasons, pending state, and semantic command identities. This keeps
stateful wire/UI facts out of domain descriptors. The
four executor contracts are deliberately separate:

1. Real-time nodes process preallocated fixed audio blocks.
2. Near-real-time workers compute meters, previews, FFTs, and bounded transforms.
3. Asynchronous workers handle control, ordinary I/O, and inference coordination.
4. Storage workers journal, record, persist, and export.

Graph validation rejects missing or incorrect ports, stream
schema/format/delivery-policy mismatches, invalid module configuration under the
descriptor's supported portable schema, cycles without an explicit delay node,
missing clock or resampling bridges, invalid fan-in, and unbounded cross-lane
edges. Compilation and resource preparation happen off-thread. Activation swaps
an immutable prepared graph at an execution-safe point; real-time audio
subgraphs swap specifically at an audio block boundary. Failure leaves the
last-good active graph running, keeps target and active revisions distinct, and
publishes a structured error.

The foundation configuration-schema subset accepts boolean schemas plus
`type`, `enum`, `properties`, `required`, `additionalProperties`, and `items`;
annotation-only title/description/default and schema/ID fields are also allowed.
Descriptor registration rejects unsupported assertion keywords so validation is
never silently bypassed. The subset can be widened deliberately with contract
tests when native modules need more constraints.

Same-lane real-time nodes use borrowed buffers from an immutable, preallocated
prepared graph. Every cross-lane tap owns paired `rtrb` free/ready queues whose
stored blocks are allocated during graph preparation; the producer takes an
empty block, fills it, and publishes it, while the consumer returns it to the
free queue. This is explicit because `rtrb` allocates its ring up front but cannot
prevent allocation performed by a stored value. Old graphs are reclaimed away
from the callback only after an epoch-safe handoff. See the
[`rtrb` documentation](https://docs.rs/rtrb/latest/rtrb/). The callback performs
no allocation, lock acquisition, logging, serialization, filesystem access,
inference, or blocking call.

## State ownership

`WorkspaceDocument` is versioned pretty JSON. It contains the durable graph,
module configurations, tile bindings, promoted settings, and named split/tab
presets. Device selection is either a property-based exact fingerprint or an
explicit `follow default input` selector. A missing exact device remains
unresolved without rewriting the document.

`RuntimeState` owns devices, instances, queues, clocks, active/target graph
revisions, operations, errors, and health. `PresentationSession` owns the active
preset, focus, selection, inspectors, zoom, drafts, and other disposable browser
state. Preset switching and tile visibility never mutate the runtime graph.

`RuntimeProjection` is an immutable snapshot containing runtime epoch,
monotonic projection version, document revision, active/target graph revisions,
module/device/stream status, operations, transcript and diagnostics summaries,
and control manifests.

Foundation IDs are UUID-backed newtypes, separated by role: entity, client,
request, operation, and runtime epoch identifiers cannot be interchanged.
Document, target-graph, active-graph, and projection revisions are distinct
checked `u64` newtypes; overflow is an explicit error, never wraparound.

Final transcripts live in an ordered local session journal, not in the workspace
document. A projection carries transcript revision and a recent tail; clients
page missing finals by cursor. A `TelemetrySubscription` is a non-durable lease
with requested and negotiated rate, capacity, and delivery policy.

## Application façade

The native `ApplicationService` and portable, transport-neutral
`ApplicationClient` expose the same operations. Client operations are async and
their futures do not require `Send`, allowing a WASM adapter to use browser-local
WebSocket state:

- protocol handshake/connect;
- immediate full snapshot;
- projection updates after a specified version;
- semantic command dispatch;
- bounded telemetry subscription;
- transcript paging by cursor.

Handshake negotiates protocol major/minor versions. An unsupported major is
rejected; peers select compatible behavior within a supported major rather than
silently interpreting an unknown contract.

`CommandEnvelope` carries protocol version, client ID, request ID, a per-client
monotonic request sequence, expected document revision, and the semantic
command. `CommandReceipt` reports
accepted/rejected, a structured error, resulting document revision, target graph
revision, and optional asynchronous operation ID. The application caches the
newest 1,024 receipts per client. An in-window retry with the identical sequence,
request ID, and payload returns the original receipt without re-execution. A
conflicting reuse or a sequence older than the retained window is rejected and
never executed.

The initial publication API sends immutable full `RuntimeProjection` snapshots.
`wait_for_projection(after)` is non-consuming: concurrent observers waiting
after the same revision can all receive the next snapshot. Delta framing is
deferred until measurement justifies its additional recovery semantics.

Optimistic revision guards protect durable changes. Undo/redo includes durable
document, graph, configuration, and layout mutations. Capture/monitor mute,
recording state, telemetry subscriptions, and live samples never enter history.
Durable mutations use atomic typed `WorkspaceEdit` batches covering module
instances, edges, configurations, tile bindings, named presets, and promoted
settings. A committed transaction advances the document revision, but only a
material graph change advances the target revision, creates an activation
operation, or supersedes a pending activation. JSON Patch is not a command or
persistence contract.

## Foundation mock round trip

The first implementation slice contains only domain, protocol, client,
application, and runtime foundation code with synthetic descriptors, in-memory
persistence, and deterministic `MockRuntime`:

1. Connect, negotiate the protocol, and obtain an immediate full snapshot.
2. Submit a valid synthetic typed-graph `WorkspaceEdit` batch.
3. Atomically persist through the in-memory port, advance document and target
   revisions, enqueue activation, and return a receipt with an operation ID.
4. Pump deterministic mock runtime events and project successful activation.
5. Induce a later activation failure and prove the previous active revision
   remains last-good while target and error state advance.
6. Complete a superseded older target and prove its stale completion is ignored.

Shared scripted scenarios exercise the mock and later real adapters. This first
slice excludes microphones, PipeWire, Sherpa, Leptos, Chromium transport, and
legacy deletion.

## Stream envelopes, clocks, and delivery

Every stream envelope identifies runtime epoch, stream, clock, schema version,
monotonic sequence, source-time range, emit time, queue depth, cumulative dropped
count, and discontinuity marker. Audio frame indices are the primary audio clock.
Runtime events use monotonic nanoseconds relative to the runtime epoch. UTC is
added only off the real-time path for files and human display.

| Stream | Clock and ordering | Delivery |
|---|---|---|
| Native PCM | Exact audio-frame positions | Fixed SPSC blocks; producer never blocks; overflow creates a visible gap. |
| Level meter | Derived audio range | Latest only, with skipped-count accounting. |
| Waveform preview | Derived audio range | Bounded drop-oldest min/max frames. |
| Spectrogram preview | Derived audio range | Bounded drop-oldest spectral columns. |
| Partial caption | Session/segment/revision | Latest revision per segment. |
| Final transcript | Ordered segment journal | Durable; never silently dropped. |
| Graph command | Per-client request order | Receipt plus optimistic revision guard. |
| Diagnostics | Runtime monotonic batches | Bounded sampling plus cumulative counters. |

## Native/WASM transport

The desktop binds an embedded static-asset server to `127.0.0.1` on a random
port and launches Chromium in app mode with a dedicated Magnolia profile. Each
launch generates a random token delivered in the URL fragment. The client uses
it once during handshake, removes it from browser history, and the server enforces
the exact loopback origin on every mutable connection.

One ordered JSON control WebSocket carries handshake, commands, receipts,
projections, and transcript recovery. A separate binary `postcard` telemetry
WebSocket carries lossy bounded visualization frames. Thus visualization pressure
cannot block commands or durable final transcript recovery.

Reconnect supplies the last runtime epoch plus projection and transcript cursors.
The client immediately obtains a full authoritative snapshot and recreates its
non-durable subscriptions. A changed epoch invalidates old monotonic sequence
assumptions. Implement `WebSocketApplicationClient` and
`MockApplicationClient`; a later Tauri adapter must not fork the protocol.

## Leptos studio contract

DOM controls own settings, captions, layout, and accessibility. Canvas2D,
WebGL2, or optional WebGPU may accelerate presentation only; native Rust owns
authoritative GPU computation and resources. Native near-real-time nodes compute
level metrics, waveform decimation, and FFT/spectrogram columns. Sample-rate PCM
never enters Leptos signals, and browser GPU output never becomes runtime truth.

The retained workspace model is split trees, tab stacks, and named **Capture**,
**Transcribe**, **Patch**, **Diagnose**, and **Perform** presets. Semantic command
routing has explicit global, workspace, tile, inspector, and text-entry contexts,
visible focus rings, conflict detection, and keyboard/pointer parity. Patch and
layout editing are local contexts, not global modes.

Native `ControlManifest` entries describe semantic kind, value/options,
availability, disabled reason, pending state, and command identity. Generic tiles
render manifests; specialized tiles bind one or more typed runtime resources.
Closing or hiding a tile changes presentation only. Hidden dense tiles release
telemetry leases. Module lifecycle changes require explicit application commands.
