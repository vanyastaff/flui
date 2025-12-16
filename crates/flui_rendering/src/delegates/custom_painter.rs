//! Custom painter delegate for custom painting on a canvas.
//!
//! [`CustomPainter`] allows users to implement custom painting behavior
//! without creating a new render object. It provides methods for painting,
//! hit testing, and accessibility.

use std::any::Any;
use std::fmt::Debug;

use flui_types::{Offset, Size};

use flui_painting::Canvas;

/// Builder for semantics information.
///
/// This is a placeholder type that will be expanded when semantics
/// support is fully implemented.
#[derive(Debug, Clone)]
pub struct SemanticsBuilder {
    // TODO: Implement semantics builder fields
    _private: (),
}

impl SemanticsBuilder {
    /// Creates a new empty semantics builder.
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl Default for SemanticsBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// A delegate that provides custom painting behavior.
///
/// Implement this trait to define custom painting on a canvas. The delegate
/// is used by `RenderCustomPaint` to paint content before or after its child.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::delegates::CustomPainter;
/// use flui_rendering::pipeline::Canvas;
/// use flui_types::Size;
///
/// #[derive(Debug)]
/// struct CheckerboardPainter {
///     cell_size: f32,
/// }
///
/// impl CustomPainter for CheckerboardPainter {
///     fn paint(&self, canvas: &mut Canvas, size: Size) {
///         let cols = (size.width / self.cell_size).ceil() as i32;
///         let rows = (size.height / self.cell_size).ceil() as i32;
///         // Draw checkerboard pattern...
///     }
///
///     fn should_repaint(&self, old_delegate: &dyn CustomPainter) -> bool {
///         if let Some(old) = old_delegate.as_any().downcast_ref::<Self>() {
///             self.cell_size != old.cell_size
///         } else {
///             true
///         }
///     }
/// }
/// ```
pub trait CustomPainter: Send + Sync + Debug {
    /// Paint custom content on the canvas.
    ///
    /// The canvas coordinate space is configured such that the origin is at
    /// the top left of the box. The area of the box is the size argument.
    ///
    /// Paint operations should remain inside the given area.
    fn paint(&self, canvas: &mut Canvas, size: Size);

    /// Whether this painter should repaint when replaced with a new delegate.
    ///
    /// Called when a new instance of the delegate is provided, to check if
    /// the new instance represents different information that requires repainting.
    ///
    /// # Arguments
    ///
    /// * `old_delegate` - The previous painter delegate
    ///
    /// # Returns
    ///
    /// `true` if the painter should repaint, `false` otherwise.
    fn should_repaint(&self, old_delegate: &dyn CustomPainter) -> bool;

    /// Hit test at the given position.
    ///
    /// Returns `true` if the painter considers the given position to be "hit".
    /// This is used for event handling.
    ///
    /// The default implementation returns `false`, meaning the painter doesn't
    /// handle hit testing and events pass through to children.
    fn hit_test(&self, _position: Offset) -> bool {
        false
    }

    /// Build semantics information for accessibility.
    ///
    /// Returns `Some(SemanticsBuilder)` if the painter provides semantic
    /// information, or `None` if it doesn't contribute to the semantics tree.
    fn semantics_builder(&self) -> Option<SemanticsBuilder> {
        None
    }

    /// Whether to rebuild semantics when the delegate changes.
    ///
    /// Called when a new instance of the delegate is provided, to check if
    /// the semantics information needs to be rebuilt.
    fn should_rebuild_semantics(&self, _old_delegate: &dyn CustomPainter) -> bool {
        true
    }

    /// Returns self as `Any` for downcasting.
    ///
    /// This enables comparing delegates of the same concrete type in
    /// `should_repaint` and `should_rebuild_semantics`.
    fn as_any(&self) -> &dyn Any;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestPainter {
        color: u32,
    }

    impl CustomPainter for TestPainter {
        fn paint(&self, _canvas: &mut Canvas, _size: Size) {
            // Test painting
        }

        fn should_repaint(&self, old_delegate: &dyn CustomPainter) -> bool {
            if let Some(old) = old_delegate.as_any().downcast_ref::<Self>() {
                self.color != old.color
            } else {
                true
            }
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    #[test]
    fn test_should_repaint_same_type() {
        let painter1 = TestPainter { color: 0xFF0000 };
        let painter2 = TestPainter { color: 0xFF0000 };
        let painter3 = TestPainter { color: 0x00FF00 };

        assert!(!painter1.should_repaint(&painter2));
        assert!(painter1.should_repaint(&painter3));
    }

    #[test]
    fn test_default_hit_test() {
        let painter = TestPainter { color: 0xFF0000 };
        assert!(!painter.hit_test(Offset::ZERO));
    }

    #[test]
    fn test_default_semantics() {
        let painter = TestPainter { color: 0xFF0000 };
        assert!(painter.semantics_builder().is_none());
    }
}
