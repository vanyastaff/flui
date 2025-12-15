//! Viewport container - multiple sliver children with scroll offset.
//!
//! This is the Rust equivalent of Flutter's `RenderViewport` storage pattern.
//! Use for scrollable containers that hold multiple sliver children.

use crate::parent_data::SliverPhysicalParentData;
use crate::protocol::SliverProtocol;
use crate::view::{CacheExtentStyle, ViewportOffset};
use flui_types::prelude::AxisDirection;
use flui_types::{Axis, Offset, Size};
use std::fmt::Debug;
use std::sync::Arc;

use super::ChildList;
use flui_tree::arity::Variable;

/// Container for viewport render objects that hold multiple sliver children.
///
/// This container stores:
/// - Multiple sliver children with physical parent data
/// - Viewport configuration (axis, direction, anchor, etc.)
/// - Scroll offset via ViewportOffset
/// - Cache extent for preloading content
///
/// # Flutter Equivalence
///
/// This corresponds to the storage in Flutter's `RenderViewport`:
/// - `ContainerRenderObjectMixin<RenderSliver, SliverPhysicalParentData>`
/// - Various viewport configuration fields
///
/// # Example
///
/// ```rust,ignore
/// pub struct RenderViewport {
///     viewport: Viewport,
/// }
///
/// impl RenderViewport {
///     fn layout(&mut self, constraints: BoxConstraints) {
///         // Layout each sliver child
///         for (sliver, data) in self.viewport.children_mut().iter_with_data_mut() {
///             // Layout sliver with constraints
///             let geometry = sliver.perform_layout(sliver_constraints);
///             // Update paint offset
///             data.paint_offset = computed_offset;
///         }
///     }
/// }
/// ```
pub struct Viewport {
    /// The sliver children with their paint offsets.
    children: ChildList<SliverProtocol, Variable, SliverPhysicalParentData>,

    /// The direction in which the scroll view scrolls.
    axis_direction: AxisDirection,

    /// The direction in which the cross axis extends.
    cross_axis_direction: AxisDirection,

    /// The viewport offset that controls scrolling.
    offset: Option<Arc<dyn ViewportOffset>>,

    /// The relative position of the zero scroll offset.
    ///
    /// For example, if `anchor` is 0.5 and the `axis_direction` is
    /// `AxisDirection::Down`, then the zero scroll offset is at the
    /// vertical center of the viewport.
    anchor: f32,

    /// The size of this viewport in the main axis.
    size: Size,

    /// The cache extent in pixels or viewport fraction.
    cache_extent: f32,

    /// How the cache extent is interpreted.
    cache_extent_style: CacheExtentStyle,

    /// The center sliver index (for viewports with center).
    ///
    /// If Some, slivers before this index grow in the opposite direction.
    center_index: Option<usize>,
}

impl Debug for Viewport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Viewport")
            .field("child_count", &self.children.len())
            .field("axis_direction", &self.axis_direction)
            .field("anchor", &self.anchor)
            .field("size", &self.size)
            .field("cache_extent", &self.cache_extent)
            .field("center_index", &self.center_index)
            .finish()
    }
}

impl Default for Viewport {
    fn default() -> Self {
        Self::new(AxisDirection::TopToBottom)
    }
}

impl Viewport {
    /// Creates a new viewport with the given axis direction.
    pub fn new(axis_direction: AxisDirection) -> Self {
        Self {
            children: ChildList::new(),
            axis_direction,
            cross_axis_direction: Self::default_cross_axis_direction(axis_direction),
            offset: None,
            anchor: 0.0,
            size: Size::ZERO,
            cache_extent: 250.0, // DEFAULT_CACHE_EXTENT
            cache_extent_style: CacheExtentStyle::Pixel,
            center_index: None,
        }
    }

    /// Creates a viewport with all configuration options.
    pub fn with_config(
        axis_direction: AxisDirection,
        cross_axis_direction: AxisDirection,
        anchor: f32,
        cache_extent: f32,
        cache_extent_style: CacheExtentStyle,
    ) -> Self {
        Self {
            children: ChildList::new(),
            axis_direction,
            cross_axis_direction,
            offset: None,
            anchor,
            size: Size::ZERO,
            cache_extent,
            cache_extent_style,
            center_index: None,
        }
    }

    /// Returns the default cross axis direction for a given main axis direction.
    fn default_cross_axis_direction(axis_direction: AxisDirection) -> AxisDirection {
        match axis_direction {
            AxisDirection::TopToBottom | AxisDirection::BottomToTop => AxisDirection::LeftToRight,
            AxisDirection::LeftToRight | AxisDirection::RightToLeft => AxisDirection::TopToBottom,
        }
    }

    // ========================================================================
    // Children Access
    // ========================================================================

    /// Returns a reference to the children container.
    pub fn children(&self) -> &ChildList<SliverProtocol, Variable, SliverPhysicalParentData> {
        &self.children
    }

    /// Returns a mutable reference to the children container.
    pub fn children_mut(
        &mut self,
    ) -> &mut ChildList<SliverProtocol, Variable, SliverPhysicalParentData> {
        &mut self.children
    }

    /// Returns the number of sliver children.
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Returns whether the viewport has any children.
    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    // ========================================================================
    // Axis Configuration
    // ========================================================================

    /// Returns the axis direction.
    pub fn axis_direction(&self) -> AxisDirection {
        self.axis_direction
    }

    /// Sets the axis direction.
    pub fn set_axis_direction(&mut self, direction: AxisDirection) {
        self.axis_direction = direction;
    }

    /// Returns the cross axis direction.
    pub fn cross_axis_direction(&self) -> AxisDirection {
        self.cross_axis_direction
    }

    /// Sets the cross axis direction.
    pub fn set_cross_axis_direction(&mut self, direction: AxisDirection) {
        self.cross_axis_direction = direction;
    }

    /// Returns the main axis.
    pub fn axis(&self) -> Axis {
        self.axis_direction.axis()
    }

    // ========================================================================
    // Scroll Configuration
    // ========================================================================

    /// Returns the viewport offset, if set.
    pub fn offset(&self) -> Option<&Arc<dyn ViewportOffset>> {
        self.offset.as_ref()
    }

    /// Sets the viewport offset.
    pub fn set_offset(&mut self, offset: Arc<dyn ViewportOffset>) {
        self.offset = Some(offset);
    }

    /// Clears the viewport offset.
    pub fn clear_offset(&mut self) {
        self.offset = None;
    }

    /// Returns the current scroll offset in pixels.
    pub fn pixels(&self) -> f32 {
        self.offset.as_ref().map(|o| o.pixels()).unwrap_or(0.0)
    }

    /// Returns the anchor position (0.0 to 1.0).
    pub fn anchor(&self) -> f32 {
        self.anchor
    }

    /// Sets the anchor position.
    pub fn set_anchor(&mut self, anchor: f32) {
        self.anchor = anchor.clamp(0.0, 1.0);
    }

    // ========================================================================
    // Cache Extent
    // ========================================================================

    /// Returns the cache extent.
    pub fn cache_extent(&self) -> f32 {
        self.cache_extent
    }

    /// Sets the cache extent.
    pub fn set_cache_extent(&mut self, extent: f32) {
        self.cache_extent = extent;
    }

    /// Returns the cache extent style.
    pub fn cache_extent_style(&self) -> CacheExtentStyle {
        self.cache_extent_style
    }

    /// Sets the cache extent style.
    pub fn set_cache_extent_style(&mut self, style: CacheExtentStyle) {
        self.cache_extent_style = style;
    }

    /// Computes the actual cache extent in pixels.
    ///
    /// For `CacheExtentStyle::Pixel`, returns the raw cache extent.
    /// For `CacheExtentStyle::Viewport`, multiplies by the viewport main axis extent.
    pub fn calculated_cache_extent(&self) -> f32 {
        match self.cache_extent_style {
            CacheExtentStyle::Pixel => self.cache_extent,
            CacheExtentStyle::Viewport => {
                let main_axis_extent = match self.axis() {
                    Axis::Horizontal => self.size.width,
                    Axis::Vertical => self.size.height,
                };
                self.cache_extent * main_axis_extent
            }
        }
    }

    // ========================================================================
    // Geometry
    // ========================================================================

    /// Returns the viewport size.
    pub fn size(&self) -> Size {
        self.size
    }

    /// Sets the viewport size.
    pub fn set_size(&mut self, size: Size) {
        self.size = size;
    }

    /// Returns the main axis extent of the viewport.
    pub fn main_axis_extent(&self) -> f32 {
        match self.axis() {
            Axis::Horizontal => self.size.width,
            Axis::Vertical => self.size.height,
        }
    }

    /// Returns the cross axis extent of the viewport.
    pub fn cross_axis_extent(&self) -> f32 {
        match self.axis() {
            Axis::Horizontal => self.size.height,
            Axis::Vertical => self.size.width,
        }
    }

    // ========================================================================
    // Center Sliver
    // ========================================================================

    /// Returns the center sliver index, if set.
    pub fn center_index(&self) -> Option<usize> {
        self.center_index
    }

    /// Sets the center sliver index.
    pub fn set_center_index(&mut self, index: Option<usize>) {
        self.center_index = index;
    }

    /// Returns whether a sliver index is before the center.
    ///
    /// Slivers before the center grow in the opposite direction.
    pub fn is_before_center(&self, index: usize) -> bool {
        self.center_index.map(|c| index < c).unwrap_or(false)
    }

    // ========================================================================
    // Layout Helpers
    // ========================================================================

    /// Returns the scroll offset for the leading edge of the viewport.
    ///
    /// This is negative when content is scrolled past the start.
    pub fn min_scroll_extent(&self) -> f32 {
        -self.anchor * self.main_axis_extent()
    }

    /// Returns the maximum scroll offset for the trailing edge of the viewport.
    pub fn max_scroll_extent(&self, total_sliver_extent: f32) -> f32 {
        (total_sliver_extent - self.main_axis_extent()).max(self.min_scroll_extent())
    }

    /// Converts a main axis position to a paint offset.
    ///
    /// Takes scroll offset and anchor into account.
    pub fn main_axis_position_to_paint_offset(&self, main_axis_position: f32) -> Offset {
        let anchor_offset = self.anchor * self.main_axis_extent();
        let position = main_axis_position - self.pixels() + anchor_offset;

        match self.axis_direction {
            AxisDirection::TopToBottom => Offset::new(0.0, position),
            AxisDirection::BottomToTop => Offset::new(0.0, self.size.height - position),
            AxisDirection::LeftToRight => Offset::new(position, 0.0),
            AxisDirection::RightToLeft => Offset::new(self.size.width - position, 0.0),
        }
    }
}

/// Type alias for a standard viewport with sliver children.
pub type SliverViewport = Viewport;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_viewport_default() {
        let viewport = Viewport::default();
        assert_eq!(viewport.axis_direction(), AxisDirection::TopToBottom);
        assert_eq!(viewport.anchor(), 0.0);
        assert_eq!(viewport.child_count(), 0);
    }

    #[test]
    fn test_viewport_axis() {
        let viewport = Viewport::new(AxisDirection::LeftToRight);
        assert_eq!(viewport.axis(), Axis::Horizontal);

        let viewport = Viewport::new(AxisDirection::TopToBottom);
        assert_eq!(viewport.axis(), Axis::Vertical);
    }

    #[test]
    fn test_viewport_cross_axis_direction() {
        let viewport = Viewport::new(AxisDirection::TopToBottom);
        assert_eq!(viewport.cross_axis_direction(), AxisDirection::LeftToRight);

        let viewport = Viewport::new(AxisDirection::LeftToRight);
        assert_eq!(viewport.cross_axis_direction(), AxisDirection::TopToBottom);
    }

    #[test]
    fn test_viewport_size() {
        let mut viewport = Viewport::default();
        viewport.set_size(Size::new(400.0, 600.0));

        assert_eq!(viewport.main_axis_extent(), 600.0); // Vertical
        assert_eq!(viewport.cross_axis_extent(), 400.0);
    }

    #[test]
    fn test_viewport_calculated_cache_extent_pixel() {
        let mut viewport = Viewport::default();
        viewport.set_cache_extent(100.0);
        viewport.set_cache_extent_style(CacheExtentStyle::Pixel);
        viewport.set_size(Size::new(400.0, 600.0));

        assert_eq!(viewport.calculated_cache_extent(), 100.0);
    }

    #[test]
    fn test_viewport_calculated_cache_extent_viewport() {
        let mut viewport = Viewport::default();
        viewport.set_cache_extent(0.5);
        viewport.set_cache_extent_style(CacheExtentStyle::Viewport);
        viewport.set_size(Size::new(400.0, 600.0));

        // 0.5 * 600 (vertical main axis)
        assert_eq!(viewport.calculated_cache_extent(), 300.0);
    }

    #[test]
    fn test_viewport_anchor() {
        let mut viewport = Viewport::default();
        viewport.set_anchor(0.5);
        assert_eq!(viewport.anchor(), 0.5);

        // Clamp to valid range
        viewport.set_anchor(1.5);
        assert_eq!(viewport.anchor(), 1.0);

        viewport.set_anchor(-0.5);
        assert_eq!(viewport.anchor(), 0.0);
    }

    #[test]
    fn test_viewport_center_index() {
        let mut viewport = Viewport::default();
        assert!(viewport.center_index().is_none());
        assert!(!viewport.is_before_center(0));

        viewport.set_center_index(Some(2));
        assert!(viewport.is_before_center(0));
        assert!(viewport.is_before_center(1));
        assert!(!viewport.is_before_center(2));
        assert!(!viewport.is_before_center(3));
    }

    #[test]
    fn test_viewport_main_axis_position_to_paint_offset() {
        let mut viewport = Viewport::new(AxisDirection::TopToBottom);
        viewport.set_size(Size::new(400.0, 600.0));

        // No scroll, no anchor
        let offset = viewport.main_axis_position_to_paint_offset(100.0);
        assert_eq!(offset, Offset::new(0.0, 100.0));
    }
}
