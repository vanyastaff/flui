//! RenderViewport - Scrollable container for sliver content with bidirectional support
//!
//! Implements Flutter's viewport protocol for managing scrollable sliver content. Acts
//! as bridge between box protocol (viewport itself) and sliver protocol (children).
//! Supports bidirectional scrolling via center sliver, cache extents for smooth scrolling,
//! and viewport clipping. Fundamental building block for CustomScrollView, ListView, GridView.
//!
//! Flutter reference: <https://api.flutter.dev/flutter/rendering/RenderViewport-class.html>
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderViewport` | `RenderViewport` from `package:flutter/src/rendering/viewport.dart` |
//! | `axis_direction` | `axisDirection` property |
//! | `cross_axis_direction` | `crossAxisDirection` property |
//! | `scroll_offset` | `offset.pixels` (from ViewportOffset) |
//! | `cache_extent` | `cacheExtent` property |
//! | `cache_extent_style` | `cacheExtentStyle` property |
//! | `clip_behavior` | `clipBehavior` property |
//! | `anchor` | `anchor` property (0.0-1.0) |
//! | `center_index` | `center` child (for bidirectional scrolling) |
//!
//! # Architecture
//!
//! `RenderViewport` is a box protocol render object that contains sliver children.
//! It implements bidirectional scrolling via a "center" sliver - slivers before
//! the center grow in reverse direction, while slivers after grow forward.
//!
//! ```text
//! ┌─────────────────────────────────────┐
//! │ viewport (box protocol)              │
//! │  ┌─────────────────────────────────┐ │
//! │  │ reverse slivers (before center) │ │
//! │  ├─────────────────────────────────┤ │
//! │  │ center sliver (scroll origin)   │ │
//! │  ├─────────────────────────────────┤ │
//! │  │ forward slivers (after center)  │ │
//! │  └─────────────────────────────────┘ │
//! └─────────────────────────────────────┘
//! ```
//!
//! # Layout Protocol
//!
//! 1. **Calculate viewport dimensions**
//!    - Use parent BoxConstraints to determine viewport size
//!    - viewport_main_axis_extent = height (vertical) or width (horizontal)
//!    - Cross axis extent from constraints
//!
//! 2. **Calculate cache extent**
//!    - If Pixel style: use cache_extent directly
//!    - If Viewport style: cache_extent × viewport_main_axis_extent
//!    - Cache allows prebuilding off-screen content
//!
//! 3. **Layout center sliver first**
//!    - Create SliverConstraints with scroll_offset and remaining_paint_extent
//!    - Layout center sliver to get SliverGeometry
//!    - Track paint extent and scroll extent
//!
//! 4. **Layout forward slivers** (after center)
//!    - Accumulate scroll offset for each sliver
//!    - Layout until remaining_paint_extent exhausted or no more children
//!    - Cache extent allows layout beyond visible area
//!
//! 5. **Layout reverse slivers** (before center, if bidirectional)
//!    - Layout in reverse growth direction
//!    - Accumulate negative scroll extents
//!    - Calculate min_scroll_extent (can be negative)
//!
//! 6. **Calculate scroll extents**
//!    - min_scroll_extent = total extent of reverse slivers (≤ 0)
//!    - max_scroll_extent = total extent of forward slivers (≥ 0)
//!    - Used for scroll position clamping
//!
//! 7. **Check visual overflow**
//!    - Set has_visual_overflow if content exceeds viewport
//!
//! # Paint Protocol
//!
//! 1. **Apply clipping** (if clip_behavior != Clip::None)
//!    - Clip to viewport bounds
//!    - Prevents sliver content from painting outside viewport
//!
//! 2. **Paint visible slivers**
//!    - Paint only slivers with visible geometry
//!    - Use cached paint_offset from layout
//!    - Paint in order (reverse slivers, center, forward slivers)
//!
//! 3. **Skip invisible slivers**
//!    - Slivers outside viewport not painted (culled)
//!    - Cache extent slivers laid out but not painted if outside viewport
//!
//! # Performance
//!
//! - **Layout**: O(v) where v = visible slivers - only layouts visible + cached
//! - **Paint**: O(v) - only paints visible slivers
//! - **Memory**: O(v + c) where c = cached slivers - not O(total children)!
//! - **Scrolling**: Incremental layout as content enters/exits viewport
//!
//! # Use Cases
//!
//! - **Scrollable lists**: CustomScrollView with SliverList
//! - **Scrollable grids**: CustomScrollView with SliverGrid
//! - **Mixed content**: Lists + grids + headers in single scrollable
//! - **Bidirectional scroll**: Chat apps (scroll up for history, down for new)
//! - **Infinite scroll**: Social feeds, news feeds
//! - **Complex scrolling**: App bars, sticky headers, slivers with different behaviors
//!
//! # Coordinate System
//!
//! - scroll_offset: 0.0 means center sliver is at anchor position
//! - Positive scroll_offset scrolls content in the main axis direction
//! - Negative scroll_offset (bidirectional) scrolls in reverse direction
//! - viewport_main_axis_extent: Height (vertical) or width (horizontal) of viewport
//!
//! # Bidirectional Scrolling
//!
//! The `center_index` field specifies which sliver acts as the origin for scrolling.
//! Slivers before the center are laid out in reverse direction (grow towards
//! main axis negative), while slivers after the center grow in the forward
//! direction (towards main axis positive).
//!
//! ```text
//! Unidirectional (center_index = None):
//! scroll_offset: 0 → [sliver0][sliver1][sliver2]...
//! scroll_offset: 100 → scroll forward ↓
//!
//! Bidirectional (center_index = Some(1)):
//! scroll_offset: -50 → [sliver0] visible (reverse)
//! scroll_offset: 0 → [sliver1] at anchor (center)
//! scroll_offset: 100 → [sliver2][sliver3] visible (forward)
//! ```
//!
//! # Comparison with Related Objects
//!
//! - **vs RenderSliverList**: Viewport contains slivers, SliverList is a sliver
//! - **vs RenderBox scrollables**: Viewport uses sliver protocol for efficiency
//! - **vs RenderShrinkWrappingViewport**: ShrinkWrapping sizes to content, Viewport sizes to constraints
//! - **vs RenderListView**: ListView is widget-level, Viewport is render-level
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderViewport;
//! use flui_types::layout::AxisDirection;
//!
//! // Vertical scrolling viewport
//! let mut viewport = RenderViewport::new(AxisDirection::Down);
//! viewport.set_scroll_offset(100.0);
//! viewport.set_cache_extent(250.0);
//!
//! // Bidirectional scrolling (chat-style)
//! let mut chat_viewport = RenderViewport::new(AxisDirection::Down);
//! chat_viewport.set_center_index(Some(0)); // Center at first sliver
//! chat_viewport.set_anchor(0.5); // Center in viewport
//!
//! // Horizontal scrolling
//! let mut horizontal = RenderViewport::new(AxisDirection::Right);
//! ```

use flui_rendering::{
    BoxProtocol, HitTestContext, HitTestTree, LayoutTree, PaintContext, PaintTree, RenderBox,
    Variable,
};
use flui_rendering::ElementId;
use flui_interaction::HitTestResult;
use flui_types::constraints::{GrowthDirection, ScrollDirection};
use flui_types::layout::{Axis, AxisDirection, CacheExtentStyle};
use flui_types::painting::Clip;
use flui_types::{Offset, Rect, Size, SliverConstraints, SliverGeometry};

/// RenderObject that provides a scrollable viewport for sliver content.
///
/// Bridge between box protocol (viewport itself) and sliver protocol (children).
/// Manages scroll offset, viewport dimensions, cache extent, and bidirectional
/// scrolling. Converts box constraints to sliver constraints for children, handles
/// viewport clipping, and calculates scroll extents for scroll position management.
///
/// # Arity
///
/// `Variable` - Can have any number of sliver children (0+). Only visible and
/// cached slivers are laid out.
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`. Children use sliver
/// protocol (SliverConstraints → SliverGeometry).
///
/// # Pattern
///
/// **Box-to-Sliver Bridge Viewport** - Converts box constraints to sliver constraints,
/// manages scroll offset and viewport dimensions, supports bidirectional scrolling via
/// center sliver, cache extent for smooth scrolling, viewport clipping, incremental
/// layout of visible+cached children.
///
/// # Use Cases
///
/// - **Scrollable lists**: Foundation for ListView, CustomScrollView
/// - **Scrollable grids**: Foundation for GridView
/// - **Mixed scrollables**: Lists + grids + headers in one scrollable
/// - **Bidirectional scroll**: Chat apps (history up, new messages down)
/// - **Infinite scroll**: Social feeds, news apps
/// - **Complex scrolling**: App bars, sticky headers, nested slivers
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderViewport behavior:
/// - Box protocol viewport containing sliver protocol children
/// - Converts scroll offset to SliverConstraints
/// - Supports bidirectional scrolling via center_index
/// - Cache extent for prebuilding off-screen content
/// - Viewport clipping with configurable Clip behavior
/// - Anchor position for scroll origin (0.0-1.0)
/// - Calculates min/max scroll extents
/// - Incremental layout (only visible + cached slivers)
///
/// # Bidirectional Scrolling
///
/// When `center_index` is set, viewport supports scrolling in both directions:
/// - Slivers before center grow in reverse (negative scroll offsets)
/// - Slivers after center grow forward (positive scroll offsets)
/// - Used for chat interfaces (scroll up for history, down for new messages)
///
/// # Coordinate System
///
/// - **scroll_offset = 0.0**: Center sliver positioned at anchor
/// - **Positive offset**: Content scrolls forward (down/right)
/// - **Negative offset**: Content scrolls backward (up/left, if bidirectional)
/// - **anchor**: Position in viewport (0.0=start, 0.5=middle, 1.0=end)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderViewport;
/// use flui_types::layout::AxisDirection;
///
/// // Vertical scrolling (unidirectional)
/// let mut viewport = RenderViewport::new(AxisDirection::Down);
/// viewport.set_scroll_offset(100.0);
/// viewport.set_cache_extent(250.0);
///
/// // Bidirectional (chat-style)
/// let mut chat = RenderViewport::new(AxisDirection::Down);
/// chat.set_center_index(Some(0)); // First sliver is center
/// chat.set_anchor(0.5); // Center in viewport
/// chat.set_scroll_offset(-50.0); // Show history
/// ```
#[derive(Debug)]
pub struct RenderViewport {
    /// Direction of the main axis
    pub axis_direction: AxisDirection,
    /// Cross axis direction
    pub cross_axis_direction: AxisDirection,
    /// Current scroll offset
    pub scroll_offset: f32,
    /// Cache extent for off-screen rendering
    pub cache_extent: f32,
    /// Cache extent style (pixels or viewport fraction)
    pub cache_extent_style: CacheExtentStyle,
    /// Whether to clip content to viewport bounds
    pub clip_behavior: Clip,
    /// Anchor position (0.0 = start, 1.0 = end)
    /// Determines where scroll offset 0.0 places the center sliver
    pub anchor: f32,
    /// Index of the center sliver for bidirectional scrolling
    /// If None, the first sliver is the center (unidirectional scrolling)
    pub center_index: Option<usize>,

    // Layout cache
    size: Size,
    sliver_geometries: Vec<SliverLayoutData>,
    has_visual_overflow: bool,

    // Computed scroll extent
    min_scroll_extent: f32,
    max_scroll_extent: f32,
}

/// Layout data for each sliver child
#[derive(Debug, Clone, Default)]
struct SliverLayoutData {
    /// The sliver's geometry result
    pub geometry: SliverGeometry,
    /// Paint offset relative to viewport
    pub paint_offset: Offset,
    /// Whether this sliver is in the reverse direction
    #[allow(dead_code)]
    pub is_reverse: bool,
}

impl RenderViewport {
    /// Create new viewport with default settings
    ///
    /// # Arguments
    /// * `axis_direction` - Direction of scrolling axis
    pub fn new(axis_direction: AxisDirection) -> Self {
        Self {
            axis_direction,
            cross_axis_direction: match axis_direction.axis() {
                Axis::Vertical => AxisDirection::LeftToRight,
                Axis::Horizontal => AxisDirection::TopToBottom,
            },
            scroll_offset: 0.0,
            cache_extent: 250.0,
            cache_extent_style: CacheExtentStyle::Pixel,
            clip_behavior: Clip::HardEdge,
            anchor: 0.0,
            center_index: None,
            size: Size::ZERO,
            sliver_geometries: Vec::new(),
            has_visual_overflow: false,
            min_scroll_extent: 0.0,
            max_scroll_extent: 0.0,
        }
    }

    /// Set scroll offset
    pub fn set_scroll_offset(&mut self, offset: f32) {
        self.scroll_offset = offset;
    }

    /// Set cache extent
    pub fn set_cache_extent(&mut self, extent: f32) {
        self.cache_extent = extent;
    }

    /// Set cache extent style
    pub fn set_cache_extent_style(&mut self, style: CacheExtentStyle) {
        self.cache_extent_style = style;
    }

    /// Set clip behavior
    pub fn set_clip_behavior(&mut self, behavior: Clip) {
        self.clip_behavior = behavior;
    }

    /// Set anchor position (0.0 = start, 1.0 = end)
    pub fn set_anchor(&mut self, anchor: f32) {
        self.anchor = anchor.clamp(0.0, 1.0);
    }

    /// Set center sliver index for bidirectional scrolling
    pub fn set_center_index(&mut self, index: Option<usize>) {
        self.center_index = index;
    }

    /// Get the axis (vertical or horizontal)
    pub fn axis(&self) -> Axis {
        self.axis_direction.axis()
    }

    /// Get the current size
    pub fn size(&self) -> Size {
        self.size
    }

    /// Check if viewport has visual overflow
    pub fn has_visual_overflow(&self) -> bool {
        self.has_visual_overflow
    }

    /// Get minimum scroll extent (negative for reverse slivers)
    pub fn min_scroll_extent(&self) -> f32 {
        self.min_scroll_extent
    }

    /// Get maximum scroll extent
    pub fn max_scroll_extent(&self) -> f32 {
        self.max_scroll_extent
    }

    /// Get main axis extent from size
    fn main_axis_extent(&self) -> f32 {
        match self.axis() {
            Axis::Vertical => self.size.height,
            Axis::Horizontal => self.size.width,
        }
    }

    /// Get cross axis extent from size
    fn cross_axis_extent(&self) -> f32 {
        match self.axis() {
            Axis::Vertical => self.size.width,
            Axis::Horizontal => self.size.height,
        }
    }

    /// Calculate effective cache extent based on style
    fn effective_cache_extent(&self) -> f32 {
        match self.cache_extent_style {
            CacheExtentStyle::Pixel => self.cache_extent,
            CacheExtentStyle::Viewport => self.cache_extent * self.main_axis_extent(),
        }
    }

    /// Get paint offset for sliver based on layout position
    fn compute_paint_offset(
        &self,
        layout_offset: f32,
        growth_direction: GrowthDirection,
        geometry: &SliverGeometry,
    ) -> Offset {
        let main_axis_offset = match growth_direction {
            GrowthDirection::Forward => layout_offset - self.scroll_offset,
            GrowthDirection::Reverse => {
                self.main_axis_extent()
                    - (layout_offset - self.scroll_offset)
                    - geometry.paint_extent
            }
        };

        match self.axis() {
            Axis::Vertical => Offset::new(0.0, main_axis_offset + geometry.paint_origin),
            Axis::Horizontal => Offset::new(main_axis_offset + geometry.paint_origin, 0.0),
        }
    }

    /// Layout slivers in one direction
    #[allow(clippy::too_many_arguments)]
    fn layout_slivers_in_direction<T>(
        &mut self,
        tree: &mut T,
        sliver_ids: &[ElementId],
        scroll_offset: f32,
        overlap: f32,
        growth_direction: GrowthDirection,
        main_axis_extent: f32,
        cross_axis_extent: f32,
        cache_extent: f32,
    ) -> SliverLayoutResult
    where
        T: LayoutTree,
    {
        let mut remaining_paint_extent = main_axis_extent;
        let mut remaining_cache_extent = cache_extent + main_axis_extent;
        let mut current_scroll_offset = scroll_offset;
        let mut preceding_scroll_extent = 0.0f32;
        let mut max_scroll_obstruction_extent = 0.0f32;
        let mut has_visual_overflow = false;
        let mut total_scroll_extent = 0.0f32;
        let mut current_overlap = overlap;

        let adjusted_axis_direction = match growth_direction {
            GrowthDirection::Forward => self.axis_direction,
            GrowthDirection::Reverse => self.axis_direction.opposite(),
        };

        for &child_id in sliver_ids {
            let sliver_scroll_offset = current_scroll_offset.max(0.0);
            let corrected_cache_origin = -current_scroll_offset.min(0.0);

            let constraints = SliverConstraints {
                axis_direction: adjusted_axis_direction,
                growth_direction,
                user_scroll_direction: ScrollDirection::Idle,
                scroll_offset: sliver_scroll_offset,
                preceding_scroll_extent,
                overlap: current_overlap.max(0.0),
                remaining_paint_extent: remaining_paint_extent.max(0.0),
                cross_axis_extent,
                cross_axis_direction: self.cross_axis_direction,
                viewport_main_axis_extent: main_axis_extent,
                remaining_cache_extent: remaining_cache_extent.max(0.0),
                cache_origin: corrected_cache_origin,
            };

            let geometry = tree
                .perform_sliver_layout(child_id, constraints)
                .unwrap_or_default();

            // Check for scroll correction
            if let Some(correction) = geometry.scroll_offset_correction {
                return SliverLayoutResult {
                    scroll_offset_correction: Some(correction),
                    ..Default::default()
                };
            }

            // Calculate paint offset
            let layout_offset = total_scroll_extent;
            let paint_offset =
                self.compute_paint_offset(layout_offset, growth_direction, &geometry);

            // Store layout data
            let sliver_index = self
                .sliver_geometries
                .iter()
                .position(|_| false)
                .unwrap_or(self.sliver_geometries.len());

            if sliver_index < self.sliver_geometries.len() {
                self.sliver_geometries[sliver_index] = SliverLayoutData {
                    geometry,
                    paint_offset,
                    is_reverse: growth_direction == GrowthDirection::Reverse,
                };
            } else {
                self.sliver_geometries.push(SliverLayoutData {
                    geometry,
                    paint_offset,
                    is_reverse: growth_direction == GrowthDirection::Reverse,
                });
            }

            // Update tracking values
            let effective_layout_extent = geometry.layout_extent.min(remaining_paint_extent);

            total_scroll_extent += geometry.scroll_extent;
            max_scroll_obstruction_extent =
                max_scroll_obstruction_extent.max(geometry.max_scroll_obstruction_extent);
            has_visual_overflow = has_visual_overflow || geometry.has_visual_overflow;

            current_scroll_offset -= geometry.scroll_extent;
            remaining_paint_extent -= effective_layout_extent;
            remaining_cache_extent -= geometry.cache_extent;
            preceding_scroll_extent += geometry.scroll_extent;
            current_overlap = geometry.paint_extent - geometry.layout_extent;

            if remaining_paint_extent <= 0.0 {
                break;
            }
        }

        SliverLayoutResult {
            total_scroll_extent,
            total_paint_extent: main_axis_extent - remaining_paint_extent,
            max_scroll_obstruction_extent,
            has_visual_overflow,
            scroll_offset_correction: None,
        }
    }
}

/// Result of laying out slivers in one direction
#[derive(Debug, Clone, Default)]
struct SliverLayoutResult {
    /// Total scroll extent of all slivers
    pub total_scroll_extent: f32,
    /// Total paint extent used
    #[allow(dead_code)]
    pub total_paint_extent: f32,
    /// Maximum scroll obstruction extent (for pinned headers)
    #[allow(dead_code)]
    pub max_scroll_obstruction_extent: f32,
    /// Whether any sliver has visual overflow
    pub has_visual_overflow: bool,
    /// Scroll offset correction (if layout needs to restart)
    pub scroll_offset_correction: Option<f32>,
}

impl<T: FullRenderTree> RenderBox<T, Variable> for RenderViewport {
    fn layout<T>(&mut self, mut ctx: <'_, T, Variable, BoxProtocol>) -> Size
    where
        T: LayoutTree,
    {
        // Compute viewport size from constraints
        self.size = ctx.constraints.biggest();

        let main_axis_extent = self.main_axis_extent();
        let cross_axis_extent = self.cross_axis_extent();
        let cache_extent = self.effective_cache_extent();

        // Get sliver children
        let children: Vec<ElementId> = ctx
            .children
            .iter()
            .map(|id| ElementId::new(id.get()))
            .collect();

        if children.is_empty() {
            self.min_scroll_extent = 0.0;
            self.max_scroll_extent = 0.0;
            self.has_visual_overflow = false;
            return self.size;
        }

        self.sliver_geometries.clear();
        self.sliver_geometries.reserve(children.len());

        // Determine center index
        let center_index = self.center_index.unwrap_or(0).min(children.len() - 1);

        // Calculate anchor offset
        let center_offset_adjustment = main_axis_extent * self.anchor;

        // Layout forward slivers (center and after)
        let forward_slivers: Vec<ElementId> = children[center_index..].to_vec();
        let forward_scroll_offset = (self.scroll_offset - center_offset_adjustment).max(0.0);

        let forward_result = self.layout_slivers_in_direction(
            ctx.tree_mut(),
            &forward_slivers,
            forward_scroll_offset,
            0.0,
            GrowthDirection::Forward,
            main_axis_extent,
            cross_axis_extent,
            cache_extent,
        );

        // Check for scroll correction
        if let Some(correction) = forward_result.scroll_offset_correction {
            tracing::debug!("Scroll offset correction: {}", correction);
            self.scroll_offset += correction;
            // Layout would need to restart - for now, continue
        }

        // Layout reverse slivers (before center)
        let reverse_result = if center_index > 0 {
            let reverse_slivers: Vec<ElementId> =
                children[..center_index].iter().rev().copied().collect();
            let reverse_scroll_offset = (center_offset_adjustment - self.scroll_offset).max(0.0);

            self.layout_slivers_in_direction(
                ctx.tree_mut(),
                &reverse_slivers,
                reverse_scroll_offset,
                0.0,
                GrowthDirection::Reverse,
                main_axis_extent,
                cross_axis_extent,
                cache_extent,
            )
        } else {
            SliverLayoutResult::default()
        };

        // Calculate scroll extents
        self.min_scroll_extent = -reverse_result.total_scroll_extent;
        self.max_scroll_extent = (forward_result.total_scroll_extent - main_axis_extent).max(0.0);

        // Update visual overflow
        self.has_visual_overflow =
            forward_result.has_visual_overflow || reverse_result.has_visual_overflow;

        self.size
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Variable>)
    where
        T: PaintTree,
    {
        // Apply clipping if needed using chaining API
        let needs_clip = self.clip_behavior != Clip::None;
        if needs_clip {
            let clip_rect = Rect::from_xywh(0.0, 0.0, self.size.width, self.size.height);
            ctx.canvas().saved().clipped_rect(clip_rect);
        }

        // Collect children to avoid borrow issues
        let children: Vec<_> = ctx.children.iter().collect();

        // Paint each sliver at its computed offset
        for (i, child_id) in children.iter().enumerate() {
            if let Some(layout_data) = self.sliver_geometries.get(i) {
                if layout_data.geometry.visible {
                    let paint_offset = ctx.offset + layout_data.paint_offset;
                    ctx.paint_child(*child_id, paint_offset);
                }
            }
        }

        // Restore clipping
        if needs_clip {
            ctx.canvas().restored();
        }
    }

    fn hit_test<T>(
        &self,
        ctx: &HitTestContext<'_, T, Variable, BoxProtocol>,
        result: &mut HitTestResult,
    ) -> bool
    where
        T: HitTestTree,
    {
        // Check if position is within viewport bounds
        if !ctx.contains(ctx.position) {
            return false;
        }

        // Collect children to allow reverse iteration
        let children: Vec<_> = ctx.children.iter().collect();

        // Hit test slivers in reverse paint order (last painted = on top)
        for (i, child_id) in children.iter().enumerate().rev() {
            if let Some(layout_data) = self.sliver_geometries.get(i) {
                if layout_data.geometry.visible {
                    let local_position = ctx.position - layout_data.paint_offset;

                    // Check if position is within sliver's paint extent
                    let in_sliver = match self.axis() {
                        Axis::Vertical => {
                            local_position.dy >= 0.0
                                && local_position.dy < layout_data.geometry.paint_extent
                        }
                        Axis::Horizontal => {
                            local_position.dx >= 0.0
                                && local_position.dx < layout_data.geometry.paint_extent
                        }
                    };

                    if in_sliver {
                        // Let the sliver do its own hit testing
                        // For now, add the viewport to result
                        ctx.add_to_result(result);
                        return true;
                    }
                }
            }

            // Suppress unused variable warning
            let _ = child_id;
        }

        false
    }

    fn hit_test_self(&self, position: Offset, size: Size) -> bool {
        position.dx >= 0.0
            && position.dy >= 0.0
            && position.dx < size.width
            && position.dy < size.height
    }
}

impl Default for RenderViewport {
    fn default() -> Self {
        Self::new(AxisDirection::TopToBottom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_viewport_new() {
        let viewport = RenderViewport::new(AxisDirection::TopToBottom);

        assert_eq!(viewport.axis_direction, AxisDirection::TopToBottom);
        assert_eq!(viewport.scroll_offset, 0.0);
        assert_eq!(viewport.cache_extent, 250.0);
        assert_eq!(viewport.clip_behavior, Clip::HardEdge);
        assert_eq!(viewport.anchor, 0.0);
        assert_eq!(viewport.center_index, None);
    }

    #[test]
    fn test_render_viewport_default() {
        let viewport = RenderViewport::default();

        assert_eq!(viewport.axis_direction, AxisDirection::TopToBottom);
        assert_eq!(viewport.scroll_offset, 0.0);
    }

    #[test]
    fn test_set_scroll_offset() {
        let mut viewport = RenderViewport::new(AxisDirection::TopToBottom);
        viewport.set_scroll_offset(100.0);

        assert_eq!(viewport.scroll_offset, 100.0);
    }

    #[test]
    fn test_set_cache_extent() {
        let mut viewport = RenderViewport::new(AxisDirection::TopToBottom);
        viewport.set_cache_extent(500.0);

        assert_eq!(viewport.cache_extent, 500.0);
    }

    #[test]
    fn test_set_clip_behavior() {
        let mut viewport = RenderViewport::new(AxisDirection::TopToBottom);
        viewport.set_clip_behavior(Clip::AntiAlias);

        assert_eq!(viewport.clip_behavior, Clip::AntiAlias);
    }

    #[test]
    fn test_set_anchor() {
        let mut viewport = RenderViewport::new(AxisDirection::TopToBottom);
        viewport.set_anchor(0.5);
        assert_eq!(viewport.anchor, 0.5);

        // Test clamping
        viewport.set_anchor(1.5);
        assert_eq!(viewport.anchor, 1.0);

        viewport.set_anchor(-0.5);
        assert_eq!(viewport.anchor, 0.0);
    }

    #[test]
    fn test_axis_vertical() {
        let viewport = RenderViewport::new(AxisDirection::TopToBottom);
        assert_eq!(viewport.axis(), Axis::Vertical);

        let viewport = RenderViewport::new(AxisDirection::BottomToTop);
        assert_eq!(viewport.axis(), Axis::Vertical);
    }

    #[test]
    fn test_axis_horizontal() {
        let viewport = RenderViewport::new(AxisDirection::LeftToRight);
        assert_eq!(viewport.axis(), Axis::Horizontal);

        let viewport = RenderViewport::new(AxisDirection::RightToLeft);
        assert_eq!(viewport.axis(), Axis::Horizontal);
    }

    #[test]
    fn test_clip_behavior_default() {
        assert_eq!(Clip::default(), Clip::HardEdge);
    }

    #[test]
    fn test_clip_behavior_variants() {
        assert_ne!(Clip::None, Clip::HardEdge);
        assert_ne!(Clip::HardEdge, Clip::AntiAlias);
        assert_ne!(Clip::AntiAlias, Clip::AntiAliasWithSaveLayer);
    }

    #[test]
    fn test_cache_extent_style_default() {
        assert_eq!(CacheExtentStyle::default(), CacheExtentStyle::Pixel);
    }

    #[test]
    fn test_effective_cache_extent_pixel() {
        let mut viewport = RenderViewport::new(AxisDirection::TopToBottom);
        viewport.size = Size::new(400.0, 600.0);
        viewport.cache_extent = 250.0;
        viewport.cache_extent_style = CacheExtentStyle::Pixel;

        assert_eq!(viewport.effective_cache_extent(), 250.0);
    }

    #[test]
    fn test_effective_cache_extent_viewport() {
        let mut viewport = RenderViewport::new(AxisDirection::TopToBottom);
        viewport.size = Size::new(400.0, 600.0);
        viewport.cache_extent = 0.5;
        viewport.cache_extent_style = CacheExtentStyle::Viewport;

        assert_eq!(viewport.effective_cache_extent(), 300.0); // 0.5 * 600.0
    }

    #[test]
    fn test_set_center_index() {
        let mut viewport = RenderViewport::new(AxisDirection::TopToBottom);
        assert_eq!(viewport.center_index, None);

        viewport.set_center_index(Some(2));
        assert_eq!(viewport.center_index, Some(2));

        viewport.set_center_index(None);
        assert_eq!(viewport.center_index, None);
    }

    #[test]
    fn test_compute_paint_offset_forward_vertical() {
        let mut viewport = RenderViewport::new(AxisDirection::TopToBottom);
        viewport.size = Size::new(400.0, 600.0);
        viewport.scroll_offset = 100.0;

        let geometry = SliverGeometry {
            paint_extent: 50.0,
            paint_origin: 0.0,
            ..Default::default()
        };

        let offset = viewport.compute_paint_offset(150.0, GrowthDirection::Forward, &geometry);

        // layout_offset (150) - scroll_offset (100) = 50
        assert_eq!(offset.dx, 0.0);
        assert_eq!(offset.dy, 50.0);
    }

    #[test]
    fn test_compute_paint_offset_reverse_vertical() {
        let mut viewport = RenderViewport::new(AxisDirection::TopToBottom);
        viewport.size = Size::new(400.0, 600.0);
        viewport.scroll_offset = 0.0;

        let geometry = SliverGeometry {
            paint_extent: 50.0,
            paint_origin: 0.0,
            ..Default::default()
        };

        let offset = viewport.compute_paint_offset(100.0, GrowthDirection::Reverse, &geometry);

        // main_axis_extent (600) - layout_offset (100) - paint_extent (50) = 450
        assert_eq!(offset.dx, 0.0);
        assert_eq!(offset.dy, 450.0);
    }
}
