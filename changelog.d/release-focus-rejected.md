### Fixed

- Closed focus owners retire rejected listeners, handlers, rectangle providers, contexts and traversal policies outside internal borrows. Healthy rejection preserves destruction; rejection during an active unwind retains opaque ownership so the original failure remains authoritative.
