//! Hit testing infrastructure for pointer interaction.
//!
//! This module is a thin protocol-extension surface over
//! `flui_interaction::routing`.
//! The canonical `HitTestResult` / `HitTestEntry` / `HitTestBehavior`
//! types live in `flui-interaction`; this module re-exports them for caller
//! convenience and owns the rendering-protocol-specific
//! `MatrixTransformPart` helper.
//!
//! # Key Types
//!
//! - [`HitTestResult`]: re-exported from
//!   [`flui_interaction::routing::HitTestResult`] -- canonical
//!   result with data-only entries and a transform stack.
//! - [`HitTestEntry`]: re-exported from
//!   [`flui_interaction::routing::HitTestEntry`] -- carries
//!   render identity, optional pointer and scroll targets, and a cursor request.
//! - [`HitTestBehavior`]: re-exported from
//!   [`flui_interaction::routing::HitTestBehavior`] -- standard
//!   `DeferToChild` / `Opaque` / `Translucent` enum.
//! - [`MatrixTransformPart`]: protocol-specific transform helper
//!   used by the box/sliver hit-test capability types.
//!
//! # Protocol-Specific Types
//!
//! `BoxHitTestResult` / `BoxHitTestEntry` / `SliverHitTestResult` /
//! `SliverHitTestEntry` are in `crate::protocol` (next to the
//! `BoxProtocol` / `SliverProtocol` capability definitions). This
//! module re-exports the common interaction result while those capability
//! types retain their protocol-specific child traversal.
//!
//! # Example
//!
//! ```ignore
//! use flui_rendering::hit_testing::HitTestResult;
//! use flui_foundation::geometry::Offset;
//!
//! let mut result = HitTestResult::new();
//! pipeline_owner.hit_test(&mut result, Offset::new(100.0, 200.0));
//!
//! // Dispatch handlers attached to entries during traversal.
//! result.dispatch(&pointer_event);
//! ```

mod entry;
mod result;
mod transform;

// Canonical types re-exported from flui-interaction. These re-exports
// replaced the in-crate `HitTestEntry` + `HitTestResult` structs, so
// consumers' `use crate::hit_testing::HitTestResult` imports compile
// unchanged.
pub use entry::HitTestEntry;
pub use flui_interaction::routing::{CursorRequest, HitTestBehavior};
// Pointer-event dispatch surface: a `RenderObject` advertises a data-only
// `PointerTarget` (see `RenderObject::pointer_target`) that the pipeline
// attaches to its hit entry; dispatch resolves the target through the
// owner-local interaction lane and delivers a `PointerDispatch` — the event in
// the target's own space alongside the untouched platform one — leaf-first to
// every target (ADR-0027 — executable callbacks never live in render storage).
// `EventPropagation` is carried only by the two arbitrated claim walks, scroll
// signals and trackpad pan-zoom.
pub use flui_interaction::events::{CursorIcon, PointerEvent, PointerEventExt};
pub use flui_interaction::routing::{
    DeviceId, EventPropagation, LocalPayloadTarget, MouseEnterCallback, MouseExitCallback,
    MouseHoverCallback, MouseRegionCallbacks, MouseRegionTarget, MouseTrackerAnnotation,
    PanZoomTarget, PathClipTarget, PointerDispatch, PointerTarget, ScrollTarget, ShaderMaskTarget,
    resolve_path_clip_target, resolve_shader_mask_target,
};
pub use result::HitTestResult;
pub use transform::MatrixTransformPart;
