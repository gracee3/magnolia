# ADR 0008: Delete legacy shell, signals, and plugin system

Status: Accepted, 2026-08-27

## Decision

After the synthetic new-shell proof becomes default, delete the Nannou shell, tile/input/layout stack, global signals and duplicate APIs, custom rings, plugin system, speculative config, all remaining old-core/signals dependents, obsolete automation, unrelated crates, and unproven assets. Egui implementation is already absent.

## Consequences

There is no legacy importer or parking crate. Static first-party factories are the initial extension path. Git history retains old work. The migration plan fixes the two deletion commits and gates.
