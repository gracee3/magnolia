# ADR 0006: PipeWire real-time graph and block-boundary activation

Status: Accepted, 2026-08-27

## Decision

PipeWire is the Linux device boundary. Magnolia compiles a fixed-block graph off-thread, uses borrowed preallocated buffers within a lane and paired `rtrb` free/ready block queues across lanes, and activates prepared graphs at block boundaries. Old graphs are reclaimed off-callback after an epoch-safe handoff. Failure retains the last-good graph.

## Consequences

Conversion, mapping, resampling, gain, and monitoring are explicit nodes. Cross-lane queues carry blocks. Callback constraints and overload are tested. Custom unsafe rings are deleted.
