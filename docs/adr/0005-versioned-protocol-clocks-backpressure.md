# ADR 0005: Versioned commands, projections, clocks, and backpressure

Status: Accepted, 2026-08-27

## Decision

Commands use major/minor-versioned envelopes, revision guards, per-client monotonic request sequences, bounded receipt replay, and receipts. Projection/transcript cursors are monotonic within an epoch. Initial projections are immutable full snapshots with non-consuming concurrent waits. Audio uses frame positions; other runtime time is monotonic. Every stream is bounded with explicit gap behavior.

## Consequences

Ordered JSON control and binary `postcard` telemetry use separate WebSockets. Reconnect gets a snapshot and recreates leases. UTC is added off the real-time path. No overflow is silent.
