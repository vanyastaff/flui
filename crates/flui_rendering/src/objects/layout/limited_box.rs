//! RenderLimitedBox - limits max width/height when unconstrained
//!
//! Flutter equivalent: `RenderLimitedBox`
//! Source: https://api.flutter.dev/flutter/rendering/RenderLimitedBox-class.html

use crate::core::{BoxProtocol, LayoutContext, PaintContext};
use crate::core::{Optional, RenderBox};
use flui_types::constraints::BoxConstraints;
use flui_types::Size;

/// RenderObject that limits maximum size when unconstrained
///
/// This is useful to prevent a child from becoming infinitely large when
/// placed in an unbounded context. Only applies limits when the incoming
/// constraints are infinite.
///
/// # Without Child
///
/// When no child is present, returns the limited size (useful for reserving bounded space).
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderLimitedBox;
///
/// let limited = RenderLimitedBox::new(100.0, 100.0);
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

impl<T: FullRenderTree> RenderBox<T, Optional> for RenderLimitedBox {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Optional, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let constraints = ctx.constraints;

        // Apply limits only if constraints are infinite
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

        let limited_constraints = BoxConstraints::new(
            constraints.min_width,
            max_width,
            constraints.min_height,
            max_height,
        );

        if let Some(child_id) = ctx.children.get() {
            // Layout child with limited constraints
            ctx.layout_child(child_id, limited_constraints)
        } else {
            // No child - return limited size
            Size::new(max_width, max_height)
        }
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Optional>)
    where
        T: crate::core::PaintTree,
    {
        // If we have a child, paint it at our offset
        if let Some(child_id) = ctx.children.get() {
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
