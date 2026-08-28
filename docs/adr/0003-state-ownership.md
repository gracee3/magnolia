# ADR 0003: Document, runtime, session, and projection ownership

Status: Accepted, 2026-08-27

## Decision

Durable intent belongs to `WorkspaceDocument`; operational facts to `RuntimeState`; disposable browser choices to `PresentationSession`; clients observe immutable `RuntimeProjection` snapshots. Final transcripts belong to an ordered journal.

## Consequences

Presets do not alter graphs. Missing exact devices do not rewrite documents. Projections distinguish document, target, active, and projection revisions. Runtime controls are excluded from undo. Missing finals are paged by cursor.
