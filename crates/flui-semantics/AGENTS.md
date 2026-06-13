# AGENTS.md — flui-semantics

Accessibility tree — the fifth tree in FLUI's 5-tree architecture (View → Element → Render → Layer → Semantics).

## What lives here

- `SemanticsNode` — node with accessibility properties (label, role, actions)
- `SemanticsConfiguration` — builder for semantic properties
- `SemanticsOwner` — manages tree lifecycle and platform updates
- `SemanticsAction` — actions assistive tech can perform
- `SemanticsEvent` — notifications to assistive technologies

## Key constraints

- Uses `SmolStr` for labels/hints (O(1) clone, inline storage)
- Uses `SmallVec` for children/actions (stack allocation)
- Uses `FxHashMap` for fast SemanticsId lookups
- `testing` feature exposes test-only constructors (e.g. `SemanticsOwner::new_without_callback`)
- Follows Flutter's semantics protocol closely

## Note

This crate's `Cargo.toml` still uses `edition = "2021"` and does not inherit `workspace.package` fields — it predates the workspace standardization.
