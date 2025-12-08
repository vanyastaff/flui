//! RenderEmpty - a render object that does nothing
//!
//! Implements the simplest possible RenderObject that takes minimum space
//! and renders nothing. Used as a placeholder or for testing.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderEmpty` | N/A - Flutter uses `RenderConstrainedBox` with zero size |
//! | Similar to | `SizedBox.shrink()` widget behavior |
//!
//! # Layout Protocol
//!
//! 1. **Return minimum size**
//!    - Width = `constraints.min_width`
//!    - Height = `constraints.min_height`
//!    - Takes as little space as parent allows
//!
//! # Performance
//!
//! - **Layout**: O(1) - trivial size calculation
//! - **Paint**: O(1) - no-op (nothing to paint)
//! - **Memory**: 0 bytes (zero-sized type)
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderEmpty;
//!
//! // Create empty placeholder
//! let empty = RenderEmpty;
//!
//! // Used in conditional rendering
//! let render_obj = if show_content {
//!     create_content()
//! } else {
//!     RenderEmpty::default()
//! };
//! ```

use flui_rendering::{RenderObject, RenderResult};

use flui_rendering::{BoxLayoutCtx, BoxPaintCtx, Leaf, RenderBox};
use flui_types::Size;

/// A render object that renders nothing.
///
/// Takes minimum space allowed by constraints and paints nothing.
/// This is the simplest possible RenderObject - useful as a placeholder,
/// for conditional rendering, or for testing.
///
/// # Arity
///
/// `Leaf` - Has no children.
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Pattern
///
/// **Minimal Placeholder** - Returns minimum size, paints nothing.
///
/// # Use Cases
///
/// - **Placeholders**: Empty slot in layout tree
/// - **Conditional rendering**: Show nothing in some states
/// - **Testing**: Minimal RenderObject for tests
/// - **Null pattern**: Represent absence of content
/// - **Layout gaps**: Reserve minimum space without rendering
///
/// # Flutter Compliance
///
/// Similar to Flutter's `SizedBox.shrink()` behavior:
/// - Returns minimum size from constraints (smallest allowed size)
/// - Paints nothing (no-op paint method)
/// - No children (leaf node)
///
/// # Comparison with Related Objects
///
/// - **vs RenderSizedBox(0×0)**: SizedBox forces zero size, Empty uses minimum from constraints
/// - **vs RenderSpacer**: Spacer expands to fill space, Empty shrinks to minimum
/// - **vs null/None**: Empty is an actual RenderObject in the tree
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderEmpty;
///
/// // Simple placeholder
/// let empty = RenderEmpty;
///
/// // Conditional rendering
/// let content = if show {
///     create_content()
/// } else {
///     Box::new(RenderEmpty)
/// };
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderEmpty;

impl RenderObject for RenderEmpty {}

impl RenderBox<Leaf> for RenderEmpty {
    fn layout(&mut self, ctx: BoxLayoutCtx<'_, Leaf>) -> RenderResult<Size> {
        // Take minimum space
        let constraints = &ctx.constraints;
        Ok(flui_types::Size::new(
            constraints.min_width,
            constraints.min_height,
        ))
    }

    fn paint(&self, _ctx: &mut BoxPaintCtx<'_, Leaf>) {
        // Nothing to paint
    }
}
