//! Rich context implementations for layout and hit testing.
//!
//! This module provides high-level context types that wrap the capability
//! traits and provide ergonomic APIs for common operations.
//!
//! # Context Types
//!
//! - [`LayoutContext`]: Rich layout API with constraint helpers and child
//!   operations
//! - [`HitTestContext`]: Rich hit testing API with position helpers and child
//!   testing
//! - [`PaintCx`]: Sans-IO fragment-recording paint context (see
//!   [`paint_cx`](self) module docs for the model)
//!
//! # Type Aliases for RenderBox
//!
//! - [`BoxLayoutContext`]: Layout context for box protocol
//! - [`BoxHitTestContext`]: Hit test context for box protocol
//!
//! # Example
//!
//! ```ignore
//! use flui_rendering::context::{BoxLayoutContext, BoxHitTestContext, PaintCx};
//!
//! impl RenderBox for MyWidget {
//!     type Arity = Single;
//!     type ParentData = BoxParentData;
//!
//!     fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<Single, BoxParentData>) -> Size {
//!         let child_size = ctx.layout_single_child_loose();
//!         ctx.position_single_child_at_origin();
//!         ctx.constrain(child_size)
//!     }
//!
//!     fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
//!         // Draw self in LOCAL coordinates (origin pre-translated).
//!         ctx.canvas().draw_rect(...);
//!         // Splice the child at its laid-out offset.
//!         ctx.paint_child();
//!     }
//!
//!     fn hit_test(&self, ctx: &mut BoxHitTestContext<Single, BoxParentData>) -> bool {
//!         if !ctx.is_within_size(self.size.width, self.size.height) {
//!             return false;
//!         }
//!         ctx.add_self(self.id);
//!         true
//!     }
//! }
//! ```

mod hit_test;
mod intrinsics;
mod layout;
mod paint_cx;
pub mod proxy_queries;

pub use flui_painting::{Canvas, DisplayList, Paint, PaintStyle};
pub use hit_test::HitTestContext;
// Promoted to `testing` feature so flui-objects' cross-crate tests can reach
// leaf_intrinsics/leaf_dry_layout/leaf_dry_baseline (see flui-objects extraction plan §7).
#[cfg(any(test, feature = "testing"))]
pub use intrinsics::test_support as intrinsics_test_support;
pub use intrinsics::{
    BoxDryBaselineCtx, BoxDryLayoutCtx, BoxIntrinsicsCtx, DryBaselineChildRequest,
    DryBaselineChildResponse, IntrinsicChildChannel,
};
pub use layout::LayoutContext;
// FragmentRecorder and PaintCx are unconditionally pub: FragmentRecorder
// appears in the public `RenderObject::paint_raw` trait signature (users must
// be able to name the type), and PaintCx is the render-object paint surface.
// PaintFragment is pub because it is the return type of `FragmentRecorder::finish`.
//
// FragmentOp is crate-private in its definition (only the recorder writes ops,
// only the pipeline composer reads them). It is re-exported as `pub` only when
// the `testing` feature (or `#[cfg(test)]`) is active, so flui-objects' test
// build (which enables `flui-rendering/testing` via its dev-dep) can
// pattern-match on recorded ops without ossifying the paint IR as stable API.
// PaintFragment itself is pub (needed for finish() return type), but its
// `.ops` field is crate-private; the testing-gated `PaintFragment::ops()`
// accessor exposes it to cross-crate tests.
//
// FragmentClip has ZERO cross-crate consumers (used only crate-internally by
// the paint encoder in paint_cx.rs and the composer in pipeline/owner). It is
// not re-exported as `pub` — only as `pub(crate)` for the pipeline composer.
pub(crate) use paint_cx::FragmentClip;
#[cfg(any(test, feature = "testing"))]
pub use paint_cx::FragmentOp;
#[cfg(not(any(test, feature = "testing")))]
pub(crate) use paint_cx::FragmentOp;
pub use paint_cx::{FragmentRecorder, PaintCx, PaintFragment};

// ============================================================================
// Protocol Type Aliases
// ============================================================================
use crate::protocol::{BoxProtocol, SliverProtocol};

// ────────────────────────────────────────────────────────────────────────────
// Box Protocol
// ────────────────────────────────────────────────────────────────────────────

/// Layout context for RenderBox.
///
/// This is the context type passed to `RenderBox::perform_layout()`.
pub type BoxLayoutContext<'ctx, A, PD> = LayoutContext<'ctx, BoxProtocol, A, PD>;

/// Hit test context for RenderBox.
///
/// This is the context type passed to `RenderBox::hit_test()`.
pub type BoxHitTestContext<'ctx, A, PD> = HitTestContext<'ctx, BoxProtocol, A, PD>;

// ────────────────────────────────────────────────────────────────────────────
// Sliver Protocol
// ────────────────────────────────────────────────────────────────────────────

/// Layout context for RenderSliver.
///
/// This is the context type passed to `RenderSliver::perform_layout()`.
pub type SliverLayoutContext<'ctx, A, PD> = LayoutContext<'ctx, SliverProtocol, A, PD>;

/// Hit test context for RenderSliver.
///
/// This is the context type passed to `RenderSliver::hit_test()`.
pub type SliverHitTestContext<'ctx, A, PD> = HitTestContext<'ctx, SliverProtocol, A, PD>;
