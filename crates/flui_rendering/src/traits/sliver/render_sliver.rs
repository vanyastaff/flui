//! RenderSliver trait for scrollable content layout.

use flui_types::{Offset, SliverConstraints, SliverGeometry};

use crate::pipeline::PaintingContext;
use crate::traits::RenderObject;

/// Trait for render objects that provide scrollable content.
///
/// RenderSliver is the layout protocol for scrollable content. Slivers:
/// - Receive [`SliverConstraints`] with scroll position and viewport info
/// - Compute what portion is visible and space consumed
/// - Return [`SliverGeometry`] with scroll/paint extents
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `RenderSliver` abstract class.
///
/// # Key Concepts
///
/// - **Scroll Extent**: Total scrollable size of the sliver
/// - **Paint Extent**: How much the sliver paints in the viewport
/// - **Layout Extent**: How much the sliver consumes in the viewport
/// - **Cache Extent**: Extra area to keep rendered for smooth scrolling
pub trait RenderSliver: RenderObject {
    /// Computes the layout of this sliver.
    fn perform_layout(&mut self, constraints: SliverConstraints) -> SliverGeometry;

    /// Returns the current geometry of this sliver.
    fn geometry(&self) -> &SliverGeometry;

    /// Returns the constraints this sliver was laid out with.
    fn constraints(&self) -> &SliverConstraints;

    /// Paints this sliver.
    fn paint(&self, context: &mut PaintingContext, offset: Offset);

    /// Hit tests this sliver.
    fn hit_test(
        &self,
        result: &mut SliverHitTestResult,
        main_axis_position: f32,
        cross_axis_position: f32,
    ) -> bool {
        let geometry = self.geometry();
        let constraints = self.constraints();

        if main_axis_position >= 0.0
            && main_axis_position < geometry.hit_test_extent()
            && cross_axis_position >= 0.0
            && cross_axis_position < constraints.cross_axis_extent
        {
            self.hit_test_children(result, main_axis_position, cross_axis_position)
                || self.hit_test_self(main_axis_position, cross_axis_position)
        } else {
            false
        }
    }

    /// Hit tests just this sliver (not children).
    fn hit_test_self(&self, _main: f32, _cross: f32) -> bool {
        false
    }

    /// Hit tests children of this sliver.
    fn hit_test_children(
        &self,
        _result: &mut SliverHitTestResult,
        _main: f32,
        _cross: f32,
    ) -> bool {
        false
    }
}

/// Result of a sliver hit test.
#[derive(Debug, Default)]
pub struct SliverHitTestResult {
    entries: Vec<SliverHitTestEntry>,
}

impl SliverHitTestResult {
    /// Creates a new empty hit test result.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an entry to the result.
    pub fn add(&mut self, entry: SliverHitTestEntry) {
        self.entries.push(entry);
    }

    /// Returns the entries in this result.
    pub fn entries(&self) -> &[SliverHitTestEntry] {
        &self.entries
    }
}

/// An entry in a sliver hit test result.
#[derive(Debug)]
pub struct SliverHitTestEntry {
    /// Position along main axis.
    pub main_axis_position: f32,
    /// Position along cross axis.
    pub cross_axis_position: f32,
}

impl SliverHitTestEntry {
    /// Creates a new hit test entry.
    pub fn new(main_axis_position: f32, cross_axis_position: f32) -> Self {
        Self {
            main_axis_position,
            cross_axis_position,
        }
    }
}
