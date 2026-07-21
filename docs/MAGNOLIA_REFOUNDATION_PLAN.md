# Magnolia Refoundation Plan

Status: accepted direction; implementation pending  
Last reviewed: 2026-07-21  
Primary target: Linux laptop, integrated graphics, no required discrete GPU  
Audience: the project owner and future Magnolia contributors/agents

## Purpose

Magnolia will be a small, headless Rust runtime for composing and operating local capabilities. It will provide module discovery, semantic commands, lifecycle supervision, typed events, permissions, health, metrics, and process coordination.

Magnolia will not put domain logic, user interfaces, large media payloads, model-provider behavior, or agent reasoning in the kernel. DSP, recording, transcription, language models, sigils, and visualizations remain Magnolia capabilities behind explicit module boundaries. Astrology and tarot remain separate projects that may integrate with Magnolia without becoming part of it.

Oracle Studio is a separate future application/workspace that composes Astraeus astrology and tarot directly. It may optionally use Magnolia for recording, transcription, visualization, process control, and agent-facing adapters, but Magnolia must not depend on Oracle Studio, Astraeus, or tarot.

The immediate goal is a clean vertical slice rather than a feature-complete platform:

> A human or agent can start a recording through the same semantic command, observe low-latency levels, stop it, obtain a local Sherpa-ONNX transcript with an explicitly enabled OpenAI fallback, and recover the resulting artifacts and audit history.

## Recorded decisions

1. Rust remains the implementation language for the runtime and first-party modules.
2. Magnolia is headless first. CLI and TUI are the first operational clients; the native visual GUI remains for charts, sigils, and high-rate visualizations.
3. Human and agent clients use the same semantic command interface.
4. MCP is an adapter for agent tool/resource discovery, not Magnolia's kernel ABI and never the real-time audio transport.
5. Codex supplies reasoning and skill execution. Magnolia owns application state, process lifecycle, permissions, audio, and deterministic control loops.
6. PipeWire is the Linux media boundary. Magnolia does not attempt to replace the operating system's audio graph.
7. The real-time audio/DSP data plane is separate from the control/event plane.
8. Static first-party modules come before dynamic plugins. Native in-process plugins are trusted-only; isolation requires a process boundary.
9. Sherpa-ONNX is the first and default STT backend. OpenAI Realtime transcription is the first fallback and requires explicit opt-in.
10. TensorRT/Parakeet integration is removed from Magnolia for now. Its standalone repository remains research history, not a workspace dependency.
11. Astrology and tarot are separate development tracks and repositories. Magnolia contains neither domain core.
12. The embedded Magnolia astrology copy is removed after any Magnolia-specific adapter or rendering work worth preserving is identified. The authoritative astrology project is Astraeus.
13. Skills express reusable workflows. Typed tools, permissions, and runtime policy enforce what may actually happen.
14. No new internal protocol, plugin ABI, storage encryption scheme, or service framework is introduced without a demonstrated need.

The detailed STT decision and benchmark plan is in [STT_BACKEND_PLAN.md](STT_BACKEND_PLAN.md).

## Current baseline

The repository is a useful prototype, but it is not currently a reliable microkernel:

- Cargo workspace loading fails because `crates/parakeet_stt` points to a missing hard-coded TensorRT sibling path.
- Each asynchronous module receives a new OS thread and full Tokio runtime.
- The patch graph is stored in core, but the daemon GUI update loop performs actual signal routing.
- Routed messages identify a source module but not a source port.
- Generic signals mix control messages, domain objects, owned audio, SPSC receivers, blobs, and GPU resources.
- Audio capture allocates batches and generic fan-out can clone complete audio buffers.
- Periodic polling and frame-driven tile updates create unnecessary wakeups.
- UI, WGPU resource maps, layout, and Kamea-specific concepts leak into core.
- Several module abstractions coexist: legacy source/sink/processor traits, `ModuleRuntime`, and `StaticModule`.
- Signing and sandbox code is not enforced by the native plugin loading path.
- There is no established CI, performance baseline, idle-power budget, or validated shutdown behavior.

This plan treats existing code as material to simplify and validate, not as a compatibility contract. There are no legacy API requirements.

## Target architecture

```text
 Codex / voice client / CLI / TUI / native visual studio
                       |
          MCP adapter / local semantic RPC
                       |
              magnoliad control plane
       +---------------+-------------------+
       |               |                   |
 lifecycle + health  commands/events    permissions/audit
       |               |                   |
       +------------ module supervisor ----+
                       |
        +--------------+------------------+
        |              |                  |
   Kamea/DSP       recording/STT      LLM/ASR adapters
   visual modules  artifact store     isolated providers

 PipeWire <-> dedicated RT audio/DSP graph <-> recorder/output
                       |
          bounded visualization and STT taps
```

### Kernel responsibilities

The small Magnolia kernel owns only:

- Module descriptors and stable instance IDs.
- Lifecycle state and supervised start, stop, restart, and shutdown.
- Semantic command dispatch and typed event delivery.
- Settings/state schema discovery and validation.
- Health, metrics, deadlines, and operation status.
- Capability grants and effect/confirmation policy.
- Patch metadata for compatible control and data endpoints.
- Artifact references and audit/provenance records.
- Selection of an execution class for each module.

The kernel does not own:

- Nannou, WGPU, Slint, Ratatui, or another UI toolkit.
- Astrology, tarot, transcription-provider, or LLM-provider domain types.
- PCM samples, model tensors, image pixels, or GPU objects in a generic enum.
- Long-term person/session storage.
- Natural-language interpretation or autonomous policy.

### Execution classes

Modules declare an execution class rather than creating runtimes themselves:

- `Async`: task on one shared Tokio runtime for control, network, and ordinary I/O.
- `Blocking`: bounded worker pool for CPU or blocking library calls.
- `RealtimeAudio`: node in the preallocated audio/DSP graph; no allocation, locks, logging, network, or model inference in the callback.
- `Process`: supervised child process for isolation, incompatible native stacks, or independently restartable services.

Thread priority and affinity are runtime policy, not module-defined behavior. Shutdown has a deadline and escalation path; joining a non-responsive module cannot block the daemon forever.

## Module contract

The first module contract should be a Rust descriptor that can also be serialized. It should contain:

- Module type ID, instance ID, semantic version, and schema version.
- Commands with JSON Schema inputs and structured outputs.
- Read-only state and settings schemas.
- Events with stable names and payload schemas.
- Data ports with format, direction, version, and delivery semantics.
- Lifecycle state and health information.
- Required capabilities such as microphone, filesystem roots, network provider, credentials, or client-vault access.
- Effect class: read-only, local mutation, external side effect, or destructive.
- Idempotency, confirmation, cancellation, timeout, and retry metadata.
- Latency class and optional power/visibility hints.

Example semantic commands:

```text
audio.record.start(device?, format, destination)
audio.record.stop(recording_id)
audio.monitor.set(enabled)
stt.session.start(backend, fallback_policy)
stt.session.stop(session_id)
module.configure(module_id, settings_patch)
```

Every accepted mutation returns an operation or artifact ID. Long-running work emits progress and a terminal result. Events include both UTC timestamps for domain history and monotonic timestamps/durations for latency analysis.

The CLI, TUI, GUI, tests, skills, and MCP adapter all consume this same contract. Tiles are views over module state; they are not the module API.

## Control plane and local IPC

Start with a local Unix-domain socket and a small request/response plus subscription protocol using versioned structured messages. JSON is acceptable during stabilization because it is inspectable and maps directly to schemas. Binary encoding is a later optimization only if measurement justifies it.

Do not expose this socket beyond the local user session by default. On Linux, run `magnoliad` as a user service after its lifecycle is stable. Let the operating system supervise the daemon rather than implementing a second init system.

Provide three adapters over the same client library:

1. `magnolia ctl`: deterministic, scriptable CLI with structured JSON output.
2. `magnolia-tui`: event-driven dashboard for status, topology, settings, logs, sessions, and low-rate meters.
3. `magnolia-mcp`: local stdio MCP server that maps safe semantic commands to tools and state/artifacts to resources.

Codex App Server may later embed an agent conversation in a Magnolia client, but it is not needed for the first runtime or dashboard.

## Real-time audio and DSP

PipeWire owns device discovery, device access, and inter-application routing. Magnolia owns an in-process, block-oriented DSP graph for first-party processing that must share one real-time schedule.

Rules for the audio path:

- Negotiate a block size and sample format once per active graph.
- Preallocate and reuse audio blocks; no `Vec` allocation in callbacks.
- Pass graph-control changes through a bounded lock-free queue and apply immutable graph snapshots at safe block boundaries.
- Never call an LLM, STT backend, filesystem writer, JSON serializer, or logging backend from the real-time callback.
- Recording, visualization, and STT consume bounded taps with explicit overflow policies and drop counters.
- A visualization tap may drop old frames. A recorder must surface data loss. STT may discard replaceable partial work but must preserve control and final events.
- Only compute FFTs or high-rate visual data while a visible subscriber requests them.
- Make sample rate, block size, queue depth, xruns, drops, and measured end-to-end latency observable.

No-discrete-GPU does not require removing all GPU rendering. Integrated graphics can efficiently render wheels and sigils when redraw is event-driven. WGPU must remain outside the kernel and optional for headless builds.

## Speech-to-text

The initial STT stack is deliberately narrow:

- `LocalSherpa`: default offline streaming recognition using a measured CPU/int8 model.
- `OpenAiRealtime`: opt-in fallback for unavailable/unhealthy local recognition or an explicit quality choice.

Fallback policy is configuration, not an implicit error handler. The default is local-only. Enabling cloud fallback must clearly state that microphone audio leaves the machine and may incur cost.

The TensorRT removal sequence is:

1. Copy only the backend-neutral event names, benchmark concepts, replay fixtures, and backpressure lessons that remain useful.
2. Create the new provider-neutral STT contract and mock-backend tests.
3. Remove `crates/parakeet_stt`, `apps/parakeet_stt_demo`, sibling path dependencies, CUDA telemetry, and Parakeet-specific configuration/documentation.
4. Generalize or remove `apps/asr_test`.
5. Confirm Magnolia builds without the TRT repository or NVIDIA software.
6. Add Sherpa-ONNX behind an intentional feature while native packaging is evaluated.
7. Add OpenAI only after local partial/final behavior and fallback semantics are tested.

## External project boundary

Astraeus, tarot, and Oracle Studio have their own requirements, release cadence, tests, licensing, and storage concerns. They are not Magnolia crates.

- `astraeus` owns astrology calculations and astrology-specific chart specifications.
- A new `tarot-rs` owns deck-independent cards, spreads, secure shuffle, and reading-domain rules.
- A later `oracle-studio` owns people, sessions, notes, outcomes, privacy/storage, and combined astro/tarot workflows.
- Magnolia owns general recording, transcription, DSP, visualization, lifecycle, and agent-control capabilities.

Integration begins only through public interfaces. Magnolia may offer generic artifact and command APIs; an Oracle-side adapter may call them. If Magnolia-specific visualization code depends on Astraeus data, keep that adapter optional and outside `magnolia-core`. Magnolia must build and run with none of the external domain repositories present.

Cross-project organization and source-snapshot recommendations are recorded in [PROJECT_ORGANIZATION.md](PROJECT_ORGANIZATION.md).

## Agent operation model

Agents operate Magnolia through the same commands as humans. They do not receive raw memory access to modules and do not execute inside the audio callback.

Use typed tools for atomic capabilities and skills for multi-step workflows. Example skills may include:

- Record, monitor, stop, transcribe, and store a session.
- Review proposed memory changes and apply only approved updates.
- Inspect latency/drop metrics and propose bounded configuration changes.

High-frequency adaptation stays deterministic in Rust. An agent may select a policy or tune bounded parameters from metrics, but it must not close a millisecond-scale feedback loop through model inference.

Each agent mutation records actor, source, command, arguments or redacted hash, result, time, and approval provenance. Read-only discovery should be easy; destructive or externally visible actions require explicit policy or confirmation.

## User interfaces and power behavior

The headless daemon must work with no GUI dependencies. The first TUI is operational rather than graphical:

- Module lifecycle and health.
- Patch/control topology.
- Recording and STT state.
- Low-rate levels, latency, queue depth, drop, and xrun metrics.
- Logs, settings, artifacts, and session summaries.
- Agent command and approval history.

Keep the native visual client for astrology wheels, sigils, layouts, oscilloscopes, spectra, and Lissajous views. It should subscribe only to visible data and redraw on change or at an explicit capped visual rate.

Power behavior is part of correctness:

- An idle daemon has no periodic heartbeat requirement and approaches zero CPU use.
- Inactive modules hold no devices and schedule no work.
- Hidden visualizations request no FFT or high-rate frames.
- Cloud connections exist only during active sessions.
- Local STT thread counts and models are benchmarked on battery.
- Power modes change actual work production, not merely skip occasional tile updates.

## Repository shape

Avoid splitting immediately into dozens of crates. Begin with these boundaries and split further only when independent reuse or dependencies justify it:

```text
core/ or crates/magnolia-core     descriptors, commands, events, lifecycle types
crates/magnolia-runtime           supervisor, routing, IPC, permissions, audit
crates/magnolia-audio             PipeWire boundary and real-time DSP graph
crates/speech_to_text             neutral STT contract and providers
apps/magnoliad                    headless daemon
apps/magnolia                     deterministic CLI
apps/magnolia-tui                 operational dashboard
apps/studio                       optional native visual client, later
adapters/magnolia-mcp             agent-facing adapter, when the CLI contract is stable
```

During migration, existing paths can be retained where renaming would obscure functional changes. Dependency direction matters more than directory aesthetics: domain crates may depend on small shared protocol types, but core must not depend on domains or UI.

## Implementation phases

### Phase 0: restore a truthful baseline

- Record this plan and the STT plan.
- Remove TensorRT/Parakeet from Magnolia as described above.
- Add explicit workspace members/default members rather than broad globs that make experiments mandatory.
- Pin a Rust toolchain and establish formatting, Clippy, tests, and a minimal CI workflow.
- Remove tracked build artifacts.
- Inventory and classify existing crates as retain, extract, replace, defer, or remove.
- Make `cargo metadata`, `cargo check`, and the selected default test suite pass on the target machine.

Exit condition: a clean checkout builds and tests without sibling repositories, CUDA, an ephemeris installation, audio hardware, or a display server. Hardware and ephemeris tests are explicit opt-in suites.

### Phase 1: headless semantic runtime

- Consolidate existing module traits into one descriptor/runtime contract.
- Implement one shared async runtime and bounded blocking workers.
- Move routing out of the GUI.
- Include source port and schema version in routed envelopes.
- Implement supervised lifecycle with deadlines and observable health.
- Add `magnolia ctl` and a local Unix-socket client/server.
- Use mock modules to test command, event, failure, restart, cancellation, and backpressure behavior.

Exit condition: CLI and tests can discover, start, configure, connect, observe, and stop mock modules without any GUI code.

### Phase 2: recording and local transcription slice

- Build the preallocated audio block path and recorder.
- Expose level, queue, xrun, drop, and latency metrics.
- Implement the provider-neutral STT contract and mock backend.
- Integrate and benchmark Sherpa-ONNX.
- Add an event-driven TUI view for recording and transcript state.

Exit condition: a local CLI/TUI session records and transcribes reliably on the T14 with measured CPU, latency, memory, and battery behavior.

### Phase 3: agent parity and OpenAI fallback

- Expose the stable command contract through a local stdio MCP adapter.
- Create the first recording/transcription skill.
- Add explicit permission and audit behavior for agent mutations.
- Implement OpenAI Realtime transcription with reconnect, cancellation, cost/usage metrics, and visible privacy state.
- Test local failure with cloud fallback disabled and enabled.

Exit condition: a human and Codex can execute the same recording workflow, with identical artifacts and explicit provenance.

### Phase 4: visual client and extension boundary

- Adapt the existing tile/layout work into a client of the headless runtime.
- Restore Kamea and audio visualizations behind demand-driven subscriptions.
- Define and test the trusted in-process and isolated process extension boundaries.
- Demonstrate one optional external adapter without adding a domain dependency to core.

Exit condition: the visual client and an external example integration can be installed or omitted independently while the headless Magnolia test suite remains unchanged.

## Measurement and acceptance

Do not describe Magnolia as low-latency, low-power, zero-copy, sandboxed, signed, or hot-reload-safe without a repeatable test demonstrating the claim.

Establish repeatable measurements for:

- Idle daemon CPU, wakeups, and memory.
- Recording xruns and dropped blocks.
- Capture-to-meter and capture-to-recording latency.
- Local command p50/p95 latency under idle and active-audio load.
- STT first partial, endpoint-to-final, real-time factor, word error rate, and partial churn.
- CPU frequency, utilization, memory, and battery discharge during fixed workloads.
- GUI/TUI update cost with views visible and hidden.
- Module shutdown deadlines, crash recovery, and queue overload behavior.

Use representative recordings and fixed replay inputs so regressions are comparable. Benchmarks must record hardware, power source, selected model, block size, thread count, and software revisions.

## Documentation rules

- Architecture claims distinguish implemented, experimental, and planned behavior.
- Every module documents its capabilities, data ownership, failure modes, overflow behavior, and execution class.
- Provider/model configuration contains no hard-coded personal paths or credentials.
- ADRs record decisions that materially affect compatibility, security, privacy, licensing, or real-time behavior.
- User/client records and test fixtures are never embedded in source unless fictional and labeled.
- Plans link to evidence and define exit conditions rather than declaring phases complete by aspiration.

## Outstanding concerns and decisions

These do not block Phase 0, but they must be resolved before the affected phase:

1. **Cloud fallback consent:** confirm whether fallback should be enabled per session, globally with a persistent warning, or only selected manually. The safe default in this plan is disabled.
2. **Sherpa model:** select the exact English streaming model only after recording its license, download source, checksum, size, memory use, and measured T14 performance.
3. **Transcript language:** confirm whether the first release is English-only. This plan assumes English-first.
4. **Audio source:** decide whether STT receives raw microphone audio or a post-DSP voice-cleanup tap. Start raw unless a minimal deterministic preprocessing chain is already justified.
5. **External adapter ownership:** decide whether optional Magnolia adapters live in the consuming repository or a separate integrations repository. The default is the consuming repository.
6. **GUI migration:** retain the existing visual implementation until the headless contract is stable; defer any toolkit rewrite decision until profiling shows a concrete problem.

## Next action

Begin Phase 0 with one narrow restoration change:

1. Preserve the neutral STT contract notes.
2. Remove Magnolia's TensorRT/Parakeet crates, apps, dependencies, and configuration.
3. Replace wildcard workspace membership with explicit/default members.
4. Verify that Cargo can load and check the remaining workspace.
5. Record every remaining build failure as a concrete baseline issue before redesigning runtime code.

No astrology, tarot, GUI, or agent feature work should be mixed into that restoration change.
