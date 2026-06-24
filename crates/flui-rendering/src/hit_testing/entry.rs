//! Hit test entry -- canonical type re-exported from
//! `flui_interaction::routing`.
//!
//! # Cycle 4 R-7/U-4 migration
//!
//! Pre-cycle this file owned a `HitTestEntry` struct carrying a
//! `Weak<dyn HitTestTarget>` reference plus a `MatrixTransformPart`
//! transform and a local position. Its `new_render_view()` and
//! `with_position()` constructors used a file-private `DummyTarget`
//! that filled the trait-object slot when no real target was
//! available -- a tell that the trait-dispatch pattern was not the
//! right shape for FLUI.
//!
//! The interaction-side `flui_interaction::routing::HitTestEntry`
//! carries:
//!   - `target: RenderId` (data-typed, not dyn-dispatched),
//!   - `transform: Option<Matrix4>` (lazy globalization),
//!   - `handler: Option<PointerEventHandler>` (closure-based),
//!   - `scroll_handler: Option<ScrollEventHandler>`,
//!   - `cursor: CursorIcon`.
//!
//! The interaction-side entry covers every responsibility the
//! rendering-side one expressed (transform + local target identity)
//! plus the runtime-dispatch concerns the rendering-side never
//! grew (handler, cursor). Workspace audit (`rg 'impl HitTestTarget'`)
//! showed ONE production impl (`RenderView`) and TWO file-private
//! `DummyTarget` stubs -- the trait-dispatch surface had been
//! parallel-implemented rather than adopted, in violation of the
//! cycle-2 PR #100/U21 parallel-type prohibition.
//!
//! The previous `HitTestEntry` struct, its `Debug` impl, and the
//! file-private `DummyTarget` were deleted. The inherent `hit_test`
//! method on `RenderView` (the sole remaining consumer at the time)
//! was also subsequently deleted as dead code — 0 callers. The
//! canonical hit-test entry point is
//! [`PipelineOwner::hit_test`](crate::pipeline::PipelineOwner::hit_test).
//!
//! `BoxHitTestEntry` / `SliverHitTestEntry` (parallel sibling
//! structs in this module pre-cycle) were deleted in cycle 4 U-3
//! -- see `docs/research/2026-05-22-cycle4-wave2-design.md`.

pub use flui_interaction::routing::HitTestEntry;
