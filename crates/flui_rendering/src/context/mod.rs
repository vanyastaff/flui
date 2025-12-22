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
//! Paint is not protocol-specific - all render objects use `flui_painting::Canvas` directly.
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
//! use flui_rendering::context::{LayoutContext, HitTestContext};
//!
//! // Layout with rich API
//! fn perform_layout(ctx: &mut LayoutContext<BoxProtocol, Single, BoxParentData>) {
//!     let child_size = ctx.layout_single_child_loose();
//!     ctx.position_single_child_at_origin();
//!     ctx.complete_with_size(ctx.constrain(child_size));
//! }
//!
//! // Hit test with rich API
//! fn hit_test(ctx: &mut HitTestContext<BoxProtocol, Single, BoxParentData>) -> bool {
//!     if !ctx.is_within_size(self.width, self.height) {
//!         return false;
//!     }
//!     ctx.add_self(self.id);
//!     true
//! }
//! ```

mod hit_test;
mod layout;
mod paint;

pub use hit_test::HitTestContext;
pub use layout::LayoutContext;
