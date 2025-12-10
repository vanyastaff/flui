//! RenderConstrainedOverflowBox - Imposes custom constraints allowing child overflow
//!
//! Implements Flutter's ConstrainedOverflowBox that applies custom constraints to
//! its child independent of parent constraints, allowing the child to potentially
//! overflow parent boundaries. Parent sizes itself according to incoming constraints,
//! ignoring child's actual size.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderConstrainedOverflowBox` | `RenderConstrainedBox` from `package:flutter/src/rendering/proxy_box.dart` (with overflow) |
//! | `min_width` | `minWidth` property (overrides parent if set) |
//! | `max_width` | `maxWidth` property (overrides parent if set) |
//! | `min_height` | `minHeight` property (overrides parent if set) |
//! | `max_height` | `maxHeight` property (overrides parent if set) |
//! | `alignment` | `alignment` property (AlignmentGeometry) |
//!
//! # Layout Protocol
//!
//! 1. **Determine parent size**
//!    - Parent size = parent constraints.biggest()
//!    - Uses max width/height from parent constraints
//!    - Parent ignores child size (key difference from normal layout)
//!
//! 2. **Create child constraints**
//!    - Override parent constraints with custom min/max values
//!    - If custom value None: use parent constraint value
//!    - If custom value Some(v): use v instead
//!    - Child constraints independent of parent size
//!
//! 3. **Layout child**
//!    - Child laid out with custom constraints
//!    - Child may be larger or smaller than parent
//!    - Child size cached for paint alignment
//!
//! 4. **Return parent size**
//!    - Size based on parent constraints only
//!    - Child size completely ignored for parent sizing
//!    - This allows child to overflow parent bounds
//!
//! # Paint Protocol
//!
//! 1. **Calculate alignment offset**
//!    - If child size != parent size: apply alignment
//!    - Offset = alignment.calculate_offset(child_size, parent_size)
//!    - Centers or positions child within parent bounds
//!
//! 2. **Paint child at offset**
//!    - Child painted at parent offset + alignment offset
//!    - May paint outside parent bounds (overflow)
//!    - No clipping applied (consider wrapping in RenderClipRect)
//!
//! # Performance
//!
//! - **Layout**: O(1) - single child layout with constraint override
//! - **Paint**: O(1) - alignment calculation + child paint
//! - **Memory**: 40 bytes (4 × Option<f32> + Alignment + 2 × Size cache)
//!
//! # Use Cases
//!
//! - **Fixed-size rendering**: Render at specific size regardless of parent
//! - **Overflow scenarios**: Allow child to exceed parent boundaries
//! - **Constraint override**: Apply constraints different from parent
//! - **Debug layouts**: Force specific sizes for testing
//! - **Custom constraint logic**: Implement business rules for sizing
//! - **Modal overlays**: Render content larger than container
//!
//! # Comparison with Related Objects
//!
//! - **vs RenderConstrainedBox**: ConstrainedBox affects parent size, OverflowBox allows overflow
//! - **vs RenderOverflowBox**: OverflowBox loosens constraints, this applies custom constraints
//! - **vs RenderSizedBox**: SizedBox is tight constraints, this can be loose or custom
//! - **vs RenderConstraintsTransformBox**: Transform uses callback, this uses fixed overrides
//!
//! # Important Note
//!
//! Consider wrapping in `RenderClipRect` to avoid confusing hit testing behavior
//! when child overflows parent bounds. Without clipping, hit testing may register
//! events outside visible parent area.
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderConstrainedOverflowBox;
//! use flui_types::Alignment;
//!
//! // Child can be up to 200×200, parent sizes to constraints
//! let overflow = RenderConstrainedOverflowBox::new()
//!     .with_min_width(0.0)
//!     .with_max_width(200.0)
//!     .with_min_height(0.0)
//!     .with_max_height(200.0);
//!
//! // Fixed size child (always 100×100)
//! let fixed = RenderConstrainedOverflowBox::new()
//!     .with_constraints(Some(100.0), Some(100.0), Some(100.0), Some(100.0));
//!
//! // With custom alignment
//! let aligned = RenderConstrainedOverflowBox::new()
//!     .with_max_width(300.0)
//!     .with_alignment(Alignment::TOP_LEFT);
//! ```

use flui_rendering::{BoxLayoutCtx, BoxPaintCtx, RenderBox, Single};
use flui_rendering::{RenderObject, RenderResult};
use flui_types::{Alignment, BoxConstraints, Size};

/// RenderObject that imposes custom constraints on child, allowing overflow.
///
/// Applies different constraints to child than received from parent,
/// potentially allowing child to be larger or smaller than parent.
/// Parent sizes itself based on incoming constraints, ignoring child size.
///
/// # Arity
///
/// `Single` - Must have exactly 1 child.
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
///
/// # Pattern
///
/// **Constraint Modifier with Overflow** - Overrides constraints with custom
/// values, parent ignores child size allowing overflow.
///
/// # Use Cases
///
/// - **Fixed-size rendering**: Always render at specific size
/// - **Overflow scenarios**: Allow child to exceed parent boundaries
/// - **Constraint transformation**: Apply custom constraints
/// - **Debug layouts**: Force specific sizes for testing
/// - **Modal overlays**: Content larger than container
/// - **Custom sizing logic**: Business-rule-based constraints
///
/// # Flutter Compliance
///
/// Matches Flutter's ConstrainedBox (with overflow) behavior:
/// - Parent sizes to incoming constraints (biggest size)
/// - Custom constraints override parent constraints
/// - Child laid out with custom constraints
/// - Child size ignored for parent sizing
/// - Alignment applied when sizes differ
/// - Extends RenderAligningShiftedBox base class
///
/// # Overflow Behavior
///
/// Child can overflow parent because:
/// 1. Parent size determined by parent constraints only
/// 2. Child constraints are custom, independent of parent size
/// 3. No clipping applied during paint
///
/// Wrap in RenderClipRect if clipping is desired.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderConstrainedOverflowBox;
/// use flui_types::Alignment;
///
/// // Child can be up to 200×200
/// let overflow = RenderConstrainedOverflowBox::new()
///     .with_max_width(200.0)
///     .with_max_height(200.0);
///
/// // Fixed 100×100 child
/// let fixed = RenderConstrainedOverflowBox::new()
///     .with_constraints(Some(100.0), Some(100.0), Some(100.0), Some(100.0));
///
/// // Top-left aligned overflow
/// let aligned = RenderConstrainedOverflowBox::new()
///     .with_max_width(300.0)
///     .with_alignment(Alignment::TOP_LEFT);
/// ```
#[derive(Debug)]
pub struct RenderConstrainedOverflowBox {
    /// Minimum width constraint for child (overrides parent if set)
    pub min_width: Option<f32>,
    /// Maximum width constraint for child (overrides parent if set)
    pub max_width: Option<f32>,
    /// Minimum height constraint for child (overrides parent if set)
    pub min_height: Option<f32>,
    /// Maximum height constraint for child (overrides parent if set)
    pub max_height: Option<f32>,
    /// How to align the child within the parent
    pub alignment: Alignment,
    /// Cached parent size for paint phase
    cached_parent_size: Size,
    /// Cached child size for paint phase
    cached_child_size: Size,
}

impl RenderConstrainedOverflowBox {
    /// Create new constrained overflow box with default alignment (center)
    pub fn new() -> Self {
        Self {
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            alignment: Alignment::CENTER,
            cached_parent_size: Size::ZERO,
            cached_child_size: Size::ZERO,
        }
    }

    /// Set minimum width constraint for child
    pub fn with_min_width(mut self, min_width: f32) -> Self {
        self.min_width = Some(min_width);
        self
    }

    /// Set maximum width constraint for child
    pub fn with_max_width(mut self, max_width: f32) -> Self {
        self.max_width = Some(max_width);
        self
    }

    /// Set minimum height constraint for child
    pub fn with_min_height(mut self, min_height: f32) -> Self {
        self.min_height = Some(min_height);
        self
    }

    /// Set maximum height constraint for child
    pub fn with_max_height(mut self, max_height: f32) -> Self {
        self.max_height = Some(max_height);
        self
    }

    /// Set alignment for child positioning
    pub fn with_alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Set all constraints at once
    pub fn with_constraints(
        mut self,
        min_width: Option<f32>,
        max_width: Option<f32>,
        min_height: Option<f32>,
        max_height: Option<f32>,
    ) -> Self {
        self.min_width = min_width;
        self.max_width = max_width;
        self.min_height = min_height;
        self.max_height = max_height;
        self
    }

    /// Create child constraints by overriding parent constraints with custom values
    fn create_child_constraints(&self, parent_constraints: BoxConstraints) -> BoxConstraints {
        BoxConstraints::new(
            self.min_width.unwrap_or(parent_constraints.min_width),
            self.max_width.unwrap_or(parent_constraints.max_width),
            self.min_height.unwrap_or(parent_constraints.min_height),
            self.max_height.unwrap_or(parent_constraints.max_height),
        )
    }
}

impl Default for RenderConstrainedOverflowBox {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderObject for RenderConstrainedOverflowBox {}

impl RenderBox<Single> for RenderConstrainedOverflowBox {
    fn layout(&mut self, mut ctx: BoxLayoutCtx<'_, Single>) -> RenderResult<Size> {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Parent sizes itself according to incoming constraints
        // Uses biggest size available from parent (key: ignores child size)
        let parent_size = ctx.constraints.biggest();

        // Create custom constraints for child (may allow overflow)
        let child_constraints = self.create_child_constraints(ctx.constraints);

        // Layout child with custom constraints
        // Child may be larger or smaller than parent
        let child_size = ctx.layout_child(child_id, child_constraints, true)?;
        self.cached_child_size = child_size;
        self.cached_parent_size = parent_size;

        // Return parent size (ignoring child size)
        // This allows child to overflow parent bounds
        Ok(parent_size)
    }

    fn paint(&self, ctx: &mut BoxPaintCtx<'_, Single>) {
        // Single arity: use ctx.single_child() which returns ElementId directly
        let child_id = ctx.single_child();

        // Calculate child offset based on alignment
        // Note: child may be larger than parent (overflow)
        let child_offset = self
            .alignment
            .calculate_offset(self.cached_child_size, self.cached_parent_size);

        // Paint child at aligned offset
        // May paint outside parent bounds (no clipping)
        ctx.paint_child(child_id, ctx.offset + child_offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let overflow_box = RenderConstrainedOverflowBox::new();
        assert!(overflow_box.min_width.is_none());
        assert!(overflow_box.max_width.is_none());
        assert!(overflow_box.min_height.is_none());
        assert!(overflow_box.max_height.is_none());
        assert_eq!(overflow_box.alignment, Alignment::CENTER);
    }

    #[test]
    fn test_builder_methods() {
        let overflow_box = RenderConstrainedOverflowBox::new()
            .with_min_width(10.0)
            .with_max_width(200.0)
            .with_min_height(20.0)
            .with_max_height(150.0)
            .with_alignment(Alignment::TOP_LEFT);

        assert_eq!(overflow_box.min_width, Some(10.0));
        assert_eq!(overflow_box.max_width, Some(200.0));
        assert_eq!(overflow_box.min_height, Some(20.0));
        assert_eq!(overflow_box.max_height, Some(150.0));
        assert_eq!(overflow_box.alignment, Alignment::TOP_LEFT);
    }

    #[test]
    fn test_with_constraints() {
        let overflow_box = RenderConstrainedOverflowBox::new().with_constraints(
            Some(10.0),
            Some(100.0),
            Some(20.0),
            Some(80.0),
        );

        assert_eq!(overflow_box.min_width, Some(10.0));
        assert_eq!(overflow_box.max_width, Some(100.0));
        assert_eq!(overflow_box.min_height, Some(20.0));
        assert_eq!(overflow_box.max_height, Some(80.0));
    }

    #[test]
    fn test_create_child_constraints_no_override() {
        let overflow_box = RenderConstrainedOverflowBox::new();
        let parent = BoxConstraints::new(50.0, 200.0, 30.0, 150.0);

        let child = overflow_box.create_child_constraints(parent);

        // Should use parent constraints when no overrides
        assert_eq!(child.min_width, 50.0);
        assert_eq!(child.max_width, 200.0);
        assert_eq!(child.min_height, 30.0);
        assert_eq!(child.max_height, 150.0);
    }

    #[test]
    fn test_create_child_constraints_with_overrides() {
        let overflow_box = RenderConstrainedOverflowBox::new()
            .with_min_width(0.0)
            .with_max_width(300.0)
            .with_min_height(0.0)
            .with_max_height(250.0);

        let parent = BoxConstraints::new(50.0, 200.0, 30.0, 150.0);
        let child = overflow_box.create_child_constraints(parent);

        // Should use custom constraints
        assert_eq!(child.min_width, 0.0);
        assert_eq!(child.max_width, 300.0);
        assert_eq!(child.min_height, 0.0);
        assert_eq!(child.max_height, 250.0);
    }

    #[test]
    fn test_create_child_constraints_partial_override() {
        let overflow_box = RenderConstrainedOverflowBox::new()
            .with_max_width(300.0) // Only override max width
            .with_min_height(0.0); // Only override min height

        let parent = BoxConstraints::new(50.0, 200.0, 30.0, 150.0);
        let child = overflow_box.create_child_constraints(parent);

        // Mix of parent and custom constraints
        assert_eq!(child.min_width, 50.0); // From parent
        assert_eq!(child.max_width, 300.0); // Overridden
        assert_eq!(child.min_height, 0.0); // Overridden
        assert_eq!(child.max_height, 150.0); // From parent
    }

    #[test]
    fn test_default() {
        let overflow_box = RenderConstrainedOverflowBox::default();
        assert!(overflow_box.min_width.is_none());
        assert!(overflow_box.max_width.is_none());
        assert_eq!(overflow_box.alignment, Alignment::CENTER);
    }
}
