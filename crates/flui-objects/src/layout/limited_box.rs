//! RenderLimitedBox - Limits max width/height only when constraints are infinite
//!
//! Implements Flutter's LimitedBox that constrains child size only when the parent
//! provides infinite (unbounded) constraints. Prevents widgets from becoming
//! infinitely large in unbounded contexts like ListView or Row/Column with
//! unconstrained cross-axis.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderLimitedBox` | `RenderLimitedBox` from `package:flutter/src/rendering/proxy_box.dart` |
//! | `max_width` | `maxWidth` property (applies when width infinite) |
//! | `max_height` | `maxHeight` property (applies when height infinite) |
//! | `set_max_width()` | `maxWidth = value` setter |
//! | `set_max_height()` | `maxHeight = value` setter |
//!
//! # Layout Protocol
//!
//! 1. **Check constraint infinity**
//!    - Test if max_width is infinite: `constraints.max_width.is_infinite()`
//!    - Test if max_height is infinite: `constraints.max_height.is_infinite()`
//!    - Only apply limits when constraints are infinite
//!
//! 2. **Calculate limited constraints**
//!    - If width infinite: use `self.max_width`
//!    - If width bounded: use `constraints.max_width` (pass through)
//!    - Same logic for height
//!    - Min constraints always pass through
//!
//! 3. **Layout child (if present)**
//!    - Child laid out with limited constraints
//!    - Child size returned unchanged
//!
//! 4. **No child fallback**
//!    - Return Size::new(max_width, max_height)
//!    - Reserves bounded space even without child
//!
//! # Paint Protocol
//!
//! 1. **Paint child if present**
//!    - Child painted at parent offset
//!    - No transformation or clipping
//!
//! 2. **No child case**
//!    - Nothing painted (empty space reserved)
//!
//! # Performance
//!
//! - **Layout**: O(1) - conditional constraint application + single child layout
//! - **Paint**: O(1) - direct child paint
//! - **Memory**: 8 bytes (2 × f32)
//!
//! # Use Cases
//!
//! - **ListView items**: Prevent infinite height when scrolling vertically
//! - **Row/Column**: Limit cross-axis size when main axis is unconstrained
//! - **Unbounded contexts**: Provide fallback size in infinite constraints
//! - **Nested scrollables**: Prevent double-infinite constraints
//! - **Flex children**: Limit size when flex factor would cause infinite growth
//! - **Debug layouts**: Add temporary bounds to detect infinite constraint bugs
//!
//! # Comparison with Related Objects
//!
//! - **vs RenderConstrainedBox**: ConstrainedBox applies limits always, LimitedBox only when infinite
//! - **vs RenderSizedBox**: SizedBox forces exact size, LimitedBox provides fallback
//! - **vs RenderIntrinsicWidth/Height**: Intrinsic discovers size, LimitedBox limits it
//! - **vs RenderFlex**: Flex distributes space, LimitedBox caps unbounded space
//!
//! # When Limits Apply
//!
//! ```text
//! Parent constraint: 0-400 × 0-600 (bounded)
//! → LimitedBox max_width/height IGNORED (pass through)
//! → Child receives: 0-400 × 0-600
//!
//! Parent constraint: 0-∞ × 0-∞ (infinite)
//! → LimitedBox max_width=100, max_height=100 APPLIED
//! → Child receives: 0-100 × 0-100
//!
//! Parent constraint: 0-∞ × 0-600 (width infinite)
//! → max_width=100 APPLIED, max_height IGNORED
//! → Child receives: 0-100 × 0-600
//! ```
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderLimitedBox;
//!
//! // Limit both dimensions when unconstrained
//! let limited = RenderLimitedBox::new(100.0, 100.0);
//!
//! // Common in ListView (infinite height)
//! let list_item = RenderLimitedBox::new(f32::INFINITY, 80.0);
//!
//! // Common in Row (infinite width)
//! let row_child = RenderLimitedBox::new(200.0, f32::INFINITY);
//! ```

use flui_rendering::{RenderObject, RenderResult};

use flui_rendering::{BoxLayoutCtx, BoxPaintCtx};
use flui_rendering::{Optional, RenderBox};
use flui_types::constraints::BoxConstraints;
use flui_types::Size;

/// RenderObject that limits maximum size only when constraints are infinite.
///
/// Applies max_width/max_height limits only when parent provides unbounded
/// (infinite) constraints. Prevents widgets from becoming infinitely large
/// in unconstrained contexts while allowing normal sizing in bounded contexts.
///
/// # Arity
///
/// `Optional` - Can have 0 or 1 child.
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Pattern
///
/// **Constraint Modifier** - Conditionally modifies constraints based on infinity check.
///
/// # Use Cases
///
/// - **ListView items**: Limit height when scrolling direction is unconstrained
/// - **Row/Column children**: Limit cross-axis when main axis unconstrained
/// - **Unbounded contexts**: Provide fallback max size in infinite constraints
/// - **Nested scrollables**: Prevent double-infinite constraint issues
/// - **Flex children**: Cap size when flex would cause infinite growth
/// - **Debug layouts**: Detect and limit infinite constraint bugs
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderLimitedBox behavior:
/// - Limits only applied when constraints are infinite
/// - Bounded constraints pass through unchanged
/// - Without child: returns limited size
/// - With child: returns child size (constrained)
/// - Extends RenderProxyBox base class
///
/// # Conditional Limiting
///
/// Limits apply independently per dimension:
/// - Width infinite → apply max_width
/// - Width bounded → use constraint's max_width
/// - Same logic for height
///
/// This allows partial limiting (e.g., limit only width, not height).
///
/// # Without Child
///
/// When no child is present, returns the limited size. This reserves bounded
/// space even in infinite contexts, useful for layout placeholders.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderLimitedBox;
///
/// // Limit both dimensions when unconstrained
/// let limited = RenderLimitedBox::new(100.0, 100.0);
///
/// // ListView item (infinite height in scroll direction)
/// let list_item = RenderLimitedBox::new(f32::INFINITY, 80.0);
///
/// // Row child (infinite width in main axis)
/// let row_child = RenderLimitedBox::new(200.0, f32::INFINITY);
/// ```
#[derive(Debug)]
pub struct RenderLimitedBox {
    /// Maximum width when unconstrained
    pub max_width: f32,
    /// Maximum height when unconstrained
    pub max_height: f32,
}

impl RenderLimitedBox {
    /// Create new RenderLimitedBox
    pub fn new(max_width: f32, max_height: f32) -> Self {
        Self {
            max_width,
            max_height,
        }
    }

    /// Set new max width
    pub fn set_max_width(&mut self, max_width: f32) {
        self.max_width = max_width;
    }

    /// Set new max height
    pub fn set_max_height(&mut self, max_height: f32) {
        self.max_height = max_height;
    }
}

impl Default for RenderLimitedBox {
    fn default() -> Self {
        Self {
            max_width: f32::INFINITY,
            max_height: f32::INFINITY,
        }
    }
}

impl RenderObject for RenderLimitedBox {}

impl RenderBox<Optional> for RenderLimitedBox {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Optional>) -> RenderResult<Size> {
        let constraints = ctx.constraints;

        // Apply limits only if constraints are infinite
        // If bounded, pass through parent's constraints unchanged
        let max_width = if constraints.max_width.is_infinite() {
            self.max_width
        } else {
            constraints.max_width
        };
        let max_height = if constraints.max_height.is_infinite() {
            self.max_height
        } else {
            constraints.max_height
        };

        // Create limited constraints
        let limited_constraints = BoxConstraints::new(
            constraints.min_width,
            max_width,
            constraints.min_height,
            max_height,
        );

        // Optional arity: use ctx.children.get() which returns Option<&ElementId>
        if let Some(&child_id) = ctx.children.get() {
            // Layout child with limited constraints
            Ok(ctx.layout_child(child_id, limited_constraints)?)
        } else {
            // No child - return limited size
            // This reserves bounded space even without child
            Ok(Size::new(max_width, max_height))
        }
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Optional>) {
        // Optional arity: use ctx.children.get() which returns Option<&ElementId>
        if let Some(&child_id) = ctx.children.get() {
            // Paint child at parent offset (no transformation)
            ctx.paint_child(child_id, ctx.offset);
        }
        // If no child, nothing to paint
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_limited_box_new() {
        let limited = RenderLimitedBox::new(100.0, 200.0);
        assert_eq!(limited.max_width, 100.0);
        assert_eq!(limited.max_height, 200.0);
    }

    #[test]
    fn test_render_limited_box_default() {
        let limited = RenderLimitedBox::default();
        assert!(limited.max_width.is_infinite());
        assert!(limited.max_height.is_infinite());
    }

    #[test]
    fn test_render_limited_box_set_max_width() {
        let mut limited = RenderLimitedBox::new(100.0, 200.0);
        limited.set_max_width(150.0);
        assert_eq!(limited.max_width, 150.0);
    }

    #[test]
    fn test_render_limited_box_set_max_height() {
        let mut limited = RenderLimitedBox::new(100.0, 200.0);
        limited.set_max_height(250.0);
        assert_eq!(limited.max_height, 250.0);
    }
}
