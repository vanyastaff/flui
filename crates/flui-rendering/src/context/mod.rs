//! Rich context implementations for layout and hit testing.
//!
//! This module provides high-level context types that wrap the capability traits
//! and provide ergonomic APIs for common operations.
//!
//! # Context Types
//!
//! - [`LayoutContext`]: Rich layout API with constraint helpers and child operations
//! - [`HitTestContext`]: Rich hit testing API with position helpers and child testing
//!
//! For painting, use [`CanvasContext`] for low-level canvas operations.
//!
//! # Type Aliases for RenderBox
//!
//! - [`BoxLayoutContext`]: Layout context for box protocol
//! - [`BoxHitTestContext`]: Hit test context for box protocol
//!
//! # Architecture
//!
//! Contexts wrap the underlying capability implementations and provide:
//! - **Scoped operations**: `with_save()`, `with_translate()`
//! - **Chaining API**: Fluent builder pattern for sequential operations
//! - **Conditional operations**: `when()`, `when_else()`
//! - **Child helpers**: `layout_child()`, `hit_test_child()`
//!
//! # Example
//!
//! ```ignore
//! use flui_rendering::context::{BoxLayoutContext, BoxHitTestContext};
//! use flui_rendering::context::CanvasContext;
//!
//! impl RenderBox for MyWidget {
//!     type Arity = Single;
//!     type ParentData = BoxParentData;
//!
//!     fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<Single, BoxParentData>) {
//!         let child_size = ctx.layout_single_child_loose();
//!         ctx.position_single_child_at_origin();
//!         ctx.complete_with_size(ctx.constrain(child_size));
//!     }
//!
//!     fn paint(&self, ctx: &mut BoxPaintContext<'_, Single, BoxParentData>) {
//!         // Draw self
//!         ctx.canvas().draw_rect(...);
//!         // Paint child
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

mod canvas;
mod clip;
mod hit_test;
mod layout;
mod paint;

pub use canvas::{Canvas, CanvasContext, Paint, PaintStyle, Picture};
pub use clip::ClipContext;
pub use hit_test::HitTestContext;
pub use layout::LayoutContext;
pub use paint::{ChildPaintInfo, PaintChildCallback, PaintContext};

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

/// Paint context for RenderBox.
///
/// This is the context type passed to `RenderBox::paint()`.
pub type BoxPaintContext<'ctx, A, PD> = PaintContext<'ctx, BoxProtocol, A, PD>;

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

/// Paint context for RenderSliver.
///
/// This is the context type passed to `RenderSliver::paint()`.
pub type SliverPaintContext<'ctx, A, PD> = PaintContext<'ctx, SliverProtocol, A, PD>;
