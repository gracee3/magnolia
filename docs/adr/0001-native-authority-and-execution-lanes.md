# ADR 0001: Native authority and four execution lanes

Status: Accepted, 2026-08-27

## Decision

Native Rust owns devices, graph state, scheduling, ASR, persistence, telemetry production, and authoritative GPU computation and resources. Modules use separate real-time, near-real-time, asynchronous, and storage contracts. Browser WebGL2/WebGPU may accelerate presentation only; the browser is never an authority or audio scheduler.

## Consequences

Lane crossings are bounded and typed. Real-time code cannot allocate, lock, log, serialize, access files, or block. Native projections survive UI reloads. Existing competing traits and UI/GPU coupling are replaced.
