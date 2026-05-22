//! Doc-stub: site reserved for a future viewport-invalidation hook.
//!
//! The pre-cycle `RenderDirtyPropagation` trait that lived here was deleted in
//! cycle 4 R-5 (audit: `docs/research/2026-05-22-flui-rendering-engine-audit.md`).
//! The trait body was typed against `flui_foundation::ElementId`, but the
//! flui-rendering crate operates on `RenderId`; the cross-tree translation
//! layer is owned by `flui-view`, not this crate. Preserving the wrong-typed
//! trait shape codified the mismatch as a "cost-cheap option" and risked
//! the next implementer treating `ElementId` as the right key for a render-
//! tree dirty-propagation API.
//!
//! Production dirty marking goes through
//! `PipelineOwner::add_node_needing_layout / add_node_needing_paint` invoked
//! from `flui-view` and `flui-hot-reload` — not via a `RenderState` trait. A
//! future viewport-invalidation hook, if introduced, will be designed against
//! `RenderId` at that time; there is no benefit to a forward-declared shape
//! ahead of a concrete first consumer (cycle-1 PR #93 `typestate.rs` deletion
//! is the precedent).
