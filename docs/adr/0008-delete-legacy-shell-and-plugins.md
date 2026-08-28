# ADR 0008: Delete legacy shell, signals, and plugin system

Status: Implemented, 2026-08-28

## Decision

After the synthetic new-shell proof becomes default, delete the Nannou shell, tile/input/layout stack, global signals and duplicate APIs, custom rings, plugin system, speculative config, all remaining old-core/signals dependents, obsolete automation, unrelated crates, and unproven assets. Egui implementation is already absent.

## Consequences

There is no legacy importer or parking crate. Static first-party factories are
the initial extension path. Git history retains old work. Phase 3 implemented
the two deletion commits and gates without rewriting this decision's accepted
history.
