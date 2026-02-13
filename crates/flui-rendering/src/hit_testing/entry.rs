//! Hit test entry types.

use std::sync::{Arc, Weak};

use flui_types::geometry::px;
use flui_types::Offset;

use super::{HitTestTarget, MatrixTransformPart};

// Dummy target for entries without a real target
struct DummyTarget;

impl HitTestTarget for DummyTarget {
    fn handle_event(&self, _event: &super::target::PointerEvent, _entry: &HitTestEntry) {}
}

/// An entry in the hit test path.
///
/// Each entry represents a target that was hit during hit testing,
/// along with the transform needed to convert positions to local coordinates.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `HitTestEntry` class.
#[derive(Clone)]
pub struct HitTestEntry {
    /// The target that was hit.
    target: Weak<dyn HitTestTarget>,

    /// Transform from global coordinates to the target's local coordinates.
    transform: MatrixTransformPart,

    /// The local position of the hit in the target's coordinate system.
    local_position: Offset,
}

impl HitTestEntry {
    /// Creates a new hit test entry.
    pub fn new(
        target: Arc<dyn HitTestTarget>,
        transform: MatrixTransformPart,
        local_position: Offset,
    ) -> Self {
        Self {
            target: Arc::downgrade(&target),
            transform,
            local_position,
        }
    }

    /// Creates a hit test entry with just a local position (no target yet).
    pub fn with_position(local_position: Offset) -> Self {
        Self {
            target: Weak::<DummyTarget>::new(),
            transform: MatrixTransformPart::default(),
            local_position,
        }
    }

    /// Creates a hit test entry for the render view (root entry).
    ///
    /// The render view is always added to hit test results as the root
    /// target, ensuring there's always at least one entry in the result.
    pub fn new_render_view() -> Self {
        Self {
            target: Weak::<DummyTarget>::new(),
            transform: MatrixTransformPart::default(),
            local_position: Offset::ZERO,
        }
    }

    /// Returns a strong reference to the target, if it still exists.
    pub fn target(&self) -> Option<Arc<dyn HitTestTarget>> {
        self.target.upgrade()
    }

    /// Returns the transform for this entry.
    pub fn transform(&self) -> &MatrixTransformPart {
        &self.transform
    }

    /// Returns the local position of the hit.
    pub fn local_position(&self) -> Offset {
        self.local_position
    }

    /// Transforms a global position to local coordinates for this entry.
    pub fn global_to_local(&self, global: Offset) -> Option<Offset> {
        self.transform.global_to_local(global)
    }

    /// Transforms a local position to global coordinates.
    pub fn local_to_global(&self, local: Offset) -> Offset {
        self.transform.local_to_global(local)
    }
}

impl std::fmt::Debug for HitTestEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HitTestEntry")
            .field("local_position", &self.local_position)
            .field("has_target", &self.target.upgrade().is_some())
            .finish()
    }
}

// ============================================================================
// BoxHitTestEntry
// ============================================================================

/// An entry in a box hit test result.
///
/// This is a simpler entry type used directly with RenderBox hit testing
/// when we don't need the full target tracking.
#[derive(Debug, Clone)]
pub struct BoxHitTestEntry {
    /// The local position of the hit.
    pub local_position: Offset,

    /// Optional transform for coordinate conversion.
    transform: Option<MatrixTransformPart>,
}

impl BoxHitTestEntry {
    /// Creates a new box hit test entry.
    pub fn new(local_position: Offset) -> Self {
        Self {
            local_position,
            transform: None,
        }
    }

    /// Creates an entry with a transform.
    pub fn with_transform(local_position: Offset, transform: MatrixTransformPart) -> Self {
        Self {
            local_position,
            transform: Some(transform),
        }
    }

    /// Returns the transform, if any.
    pub fn transform(&self) -> Option<&MatrixTransformPart> {
        self.transform.as_ref()
    }

    /// Transforms a position from global to local coordinates.
    pub fn global_to_local(&self, global: Offset) -> Offset {
        self.transform
            .as_ref()
            .and_then(|t| t.global_to_local(global))
            .unwrap_or(global)
    }
}

impl Default for BoxHitTestEntry {
    fn default() -> Self {
        Self::new(Offset::ZERO)
    }
}

// ============================================================================
// SliverHitTestEntry
// ============================================================================

/// An entry in a sliver hit test result.
///
/// Sliver hit testing uses main/cross axis coordinates instead of x/y.
#[derive(Debug, Clone)]
pub struct SliverHitTestEntry {
    /// Position along the main (scrolling) axis.
    pub main_axis_position: f32,

    /// Position along the cross axis.
    pub cross_axis_position: f32,

    /// Optional transform for coordinate conversion.
    transform: Option<MatrixTransformPart>,
}

impl SliverHitTestEntry {
    /// Creates a new sliver hit test entry.
    pub fn new(main_axis_position: f32, cross_axis_position: f32) -> Self {
        Self {
            main_axis_position,
            cross_axis_position,
            transform: None,
        }
    }

    /// Creates an entry with a transform.
    pub fn with_transform(
        main_axis_position: f32,
        cross_axis_position: f32,
        transform: MatrixTransformPart,
    ) -> Self {
        Self {
            main_axis_position,
            cross_axis_position,
            transform: Some(transform),
        }
    }

    /// Returns the transform, if any.
    pub fn transform(&self) -> Option<&MatrixTransformPart> {
        self.transform.as_ref()
    }

    /// Converts main/cross axis position to an offset based on axis direction.
    ///
    /// For horizontal scrolling: main = x, cross = y
    /// For vertical scrolling: main = y, cross = x
    pub fn to_offset(&self, is_horizontal: bool) -> Offset {
        if is_horizontal {
            Offset::new(px(self.main_axis_position), px(self.cross_axis_position))
        } else {
            Offset::new(px(self.cross_axis_position), px(self.main_axis_position))
        }
    }
}

impl Default for SliverHitTestEntry {
    fn default() -> Self {
        Self::new(0.0, 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::geometry::px;

    #[test]
    fn test_box_hit_test_entry() {
        let entry = BoxHitTestEntry::new(Offset::new(px(10.0), px(20.0)));
        assert_eq!(entry.local_position.dx, 10.0);
        assert_eq!(entry.local_position.dy, 20.0);
        assert!(entry.transform().is_none());
    }

    #[test]
    fn test_box_hit_test_entry_with_transform() {
        let transform = MatrixTransformPart::offset(5.0, 10.0);
        let entry = BoxHitTestEntry::with_transform(Offset::new(px(10.0), px(20.0)), transform);

        assert!(entry.transform().is_some());

        let local = entry.global_to_local(Offset::new(px(15.0), px(30.0)));
        assert_eq!(local.dx, 10.0);
        assert_eq!(local.dy, 20.0);
    }

    #[test]
    fn test_sliver_hit_test_entry() {
        let entry = SliverHitTestEntry::new(100.0, 50.0);
        assert_eq!(entry.main_axis_position, 100.0);
        assert_eq!(entry.cross_axis_position, 50.0);
    }

    #[test]
    fn test_sliver_to_offset_vertical() {
        let entry = SliverHitTestEntry::new(100.0, 50.0);
        let offset = entry.to_offset(false); // vertical
        assert_eq!(offset.dx, 50.0); // cross = x
        assert_eq!(offset.dy, 100.0); // main = y
    }

    #[test]
    fn test_sliver_to_offset_horizontal() {
        let entry = SliverHitTestEntry::new(100.0, 50.0);
        let offset = entry.to_offset(true); // horizontal
        assert_eq!(offset.dx, 100.0); // main = x
        assert_eq!(offset.dy, 50.0); // cross = y
    }

    #[test]
    fn test_hit_test_entry_debug() {
        let entry = HitTestEntry::with_position(Offset::new(px(10.0), px(20.0)));
        let debug_str = format!("{:?}", entry);
        assert!(debug_str.contains("HitTestEntry"));
        assert!(debug_str.contains("local_position"));
    }
}
