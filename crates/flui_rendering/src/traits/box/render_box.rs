//! RenderBox trait for 2D box layout.

use flui_types::{BoxConstraints, Offset, Size};

use crate::pipeline::PaintingContext;
use crate::traits::RenderObject;

/// Trait for render objects that use 2D cartesian coordinates.
///
/// RenderBox is the primary layout protocol for most UI widgets. It:
/// - Receives [`BoxConstraints`] from its parent (min/max width/height)
/// - Computes its own [`Size`] within those constraints
/// - Positions children using [`Offset`] values
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `RenderBox` abstract class in
/// `rendering/box.dart`.
///
/// # Layout Protocol
///
/// 1. Parent calls `perform_layout()` with constraints
/// 2. Child computes its size within constraints
/// 3. Child returns its size
/// 4. Parent positions child by setting offset in parent data
///
/// # Example
///
/// ```ignore
/// impl RenderBox for MyRenderObject {
///     fn perform_layout(&mut self, constraints: BoxConstraints) -> Size {
///         // Layout children first
///         let child_size = if let Some(child) = self.child_mut() {
///             child.perform_layout(constraints)
///         } else {
///             Size::ZERO
///         };
///
///         // Compute own size based on child
///         constraints.constrain(child_size)
///     }
///
///     fn paint(&self, context: &mut PaintingContext, offset: Offset) {
///         // Paint self, then children
///         if let Some(child) = self.child() {
///             context.paint_child(child, offset);
///         }
///     }
///
///     // ... other required methods
/// }
/// ```
pub trait RenderBox: RenderObject {
    /// Computes the layout of this render object.
    ///
    /// Called by the parent with constraints that specify the allowed
    /// size range. Must return a size within those constraints.
    fn perform_layout(&mut self, constraints: BoxConstraints) -> Size;

    /// Returns the current size of this render object.
    ///
    /// Only valid after `perform_layout` has been called.
    fn size(&self) -> Size;

    /// Paints this render object.
    ///
    /// Called after layout is complete. Should paint this object and
    /// then paint children at their appropriate offsets.
    fn paint(&self, context: &mut PaintingContext, offset: Offset);

    /// Hit tests this render object.
    ///
    /// Returns true if the given position hits this render object or
    /// any of its children.
    fn hit_test(&self, result: &mut BoxHitTestResult, position: Offset) -> bool {
        let size = self.size();
        if position.dx >= 0.0
            && position.dy >= 0.0
            && position.dx < size.width
            && position.dy < size.height
        {
            self.hit_test_children(result, position) || self.hit_test_self(position)
        } else {
            false
        }
    }

    /// Hit tests just this render object (not children).
    fn hit_test_self(&self, _position: Offset) -> bool {
        false
    }

    /// Hit tests children of this render object.
    fn hit_test_children(&self, _result: &mut BoxHitTestResult, _position: Offset) -> bool {
        false
    }

    /// Computes the minimum intrinsic width for a given height.
    fn compute_min_intrinsic_width(&self, _height: f32) -> f32 {
        0.0
    }

    /// Computes the maximum intrinsic width for a given height.
    fn compute_max_intrinsic_width(&self, _height: f32) -> f32 {
        0.0
    }

    /// Computes the minimum intrinsic height for a given width.
    fn compute_min_intrinsic_height(&self, _width: f32) -> f32 {
        0.0
    }

    /// Computes the maximum intrinsic height for a given width.
    fn compute_max_intrinsic_height(&self, _width: f32) -> f32 {
        0.0
    }

    /// Computes the size without actually laying out.
    fn compute_dry_layout(&self, _constraints: BoxConstraints) -> Size {
        Size::ZERO
    }

    /// Computes the distance from top to baseline.
    fn compute_distance_to_baseline(&self, _baseline: TextBaseline) -> Option<f32> {
        None
    }
}

/// Result of a box hit test.
#[derive(Debug, Default)]
pub struct BoxHitTestResult {
    entries: Vec<BoxHitTestEntry>,
}

impl BoxHitTestResult {
    /// Creates a new empty hit test result.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an entry to the result.
    pub fn add(&mut self, entry: BoxHitTestEntry) {
        self.entries.push(entry);
    }

    /// Returns the entries in this result.
    pub fn entries(&self) -> &[BoxHitTestEntry] {
        &self.entries
    }

    /// Returns whether this result has any entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// An entry in a box hit test result.
#[derive(Debug)]
pub struct BoxHitTestEntry {
    /// The local position of the hit.
    pub local_position: Offset,
}

impl BoxHitTestEntry {
    /// Creates a new hit test entry.
    pub fn new(local_position: Offset) -> Self {
        Self { local_position }
    }
}

/// Text baseline types for baseline alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextBaseline {
    /// The alphabetic baseline.
    Alphabetic,
    /// The ideographic baseline.
    Ideographic,
}
