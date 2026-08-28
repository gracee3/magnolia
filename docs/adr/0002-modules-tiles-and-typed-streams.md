# ADR 0002: Separate modules, tiles, and registered typed streams

Status: Accepted, 2026-08-27

## Decision

Modules are native capabilities; tiles are presentation surfaces. Their relation is many-to-many. Ports use registered `StreamTypeId`, schema version, timing, delivery, and format constraints; no `Any` or global `Signal` exists.

## Consequences

Closing a tile cannot stop a module. Hidden dense tiles release telemetry leases. Graph roles derive from ports; generic tiles render control manifests and specialized tiles bind typed resources.
