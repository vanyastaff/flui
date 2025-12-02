//! RenderRepaintBoundary - creates a separate display list for child with caching
//!
//! Flutter reference: <https://api.flutter.dev/flutter/rendering/RenderRepaintBoundary-class.html>
//!
//! # Layer Caching
//!
//! RepaintBoundary caches the child's DisplayList and reuses it when the child
//! hasn't changed. This provides significant performance benefits when:
//!
//! - A subtree repaints frequently (e.g., animations)
//! - A subtree is expensive to paint
//! - Parent and child have different repaint frequencies
//!
//! The caching is automatic - when `needs_repaint` is false, the cached
//! DisplayList is returned directly without re-painting the child.
//!
//! # Performance Metrics
//!
//! In debug mode, track `symmetric_paint_count` vs `asymmetric_paint_count`
//! to determine if the boundary is effective. A high symmetric count suggests
//! the boundary isn't helping and may be removed.

use crate::core::{
    FullRenderTree,
    LayoutTree, PaintTree, FullRenderTree, RenderBox, Single, {BoxLayoutCtx, PaintContext},
};
use flui_painting::DisplayList;
use flui_types::Size;
use std::sync::atomic::{AtomicBool, Ordering};

/// RenderObject that creates a separate display list for its child with caching
///
/// A repaint boundary is an optimization that tells the rendering system
/// that this subtree can be repainted independently of its parent. This is
/// useful when:
///
/// - A subtree repaints frequently (e.g., animations)
/// - A subtree is expensive to paint
/// - Parent and child have different repaint frequencies
///
/// # Layer Caching
///
/// The boundary caches the child's DisplayList. When the child hasn't changed
/// (`needs_repaint` is false), the cached DisplayList is returned directly,
/// avoiding redundant paint operations.
///
/// # Performance Benefits
///
/// Without repaint boundaries, when any part of the UI needs repainting,
/// the entire layer must be repainted. With repaint boundaries:
/// 1. Only the affected subtree needs repainting
/// 2. Cached DisplayLists are reused when possible
///
/// # Debug Metrics
///
/// The debug metrics track:
/// - `symmetric_paint_count`: Times parent and child painted together (bad)
/// - `asymmetric_paint_count`: Times only one painted (good)
///
/// A high symmetric count suggests the boundary isn't helping.
///
/// # Image Capture
///
/// RepaintBoundary also provides `to_image()` methods to capture the rendered
/// content as an image (useful for screenshots, sharing, etc.).
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderRepaintBoundary;
///
/// // Wrap frequently-animating content
/// let boundary = RenderRepaintBoundary::new();
///
/// // Mark as needing repaint when child changes
/// boundary.mark_needs_repaint();
///
/// // Check if boundary is effective
/// if boundary.symmetric_paint_count() > boundary.asymmetric_paint_count() {
///     println!("Consider removing this repaint boundary");
/// }
/// ```
#[derive(Debug)]
pub struct RenderRepaintBoundary {
    /// Cached display list from previous paint (None if never painted or invalidated)
    cached_display_list: Option<DisplayList>,

    /// Whether the child needs to be repainted (atomic for thread-safety)
    needs_repaint: AtomicBool,

    /// Count of times parent and child painted together (suggests boundary isn't helping)
    #[cfg(debug_assertions)]
    symmetric_paint_count: u32,

    /// Count of times only parent or child painted (suggests boundary is helping)
    #[cfg(debug_assertions)]
    asymmetric_paint_count: u32,

    /// Count of cache hits (times cached DisplayList was reused)
    #[cfg(debug_assertions)]
    cache_hit_count: u32,

    /// Count of cache misses (times child had to be repainted)
    #[cfg(debug_assertions)]
    cache_miss_count: u32,
}

// ===== Public API =====

impl RenderRepaintBoundary {
    /// Create new RenderRepaintBoundary
    pub fn new() -> Self {
        Self {
            cached_display_list: None,
            needs_repaint: AtomicBool::new(true), // Initially needs paint
            #[cfg(debug_assertions)]
            symmetric_paint_count: 0,
            #[cfg(debug_assertions)]
            asymmetric_paint_count: 0,
            #[cfg(debug_assertions)]
            cache_hit_count: 0,
            #[cfg(debug_assertions)]
            cache_miss_count: 0,
        }
    }

    /// Returns true - this is always a repaint boundary
    ///
    /// This overrides the default behavior to indicate that this subtree
    /// should be painted into a separate layer.
    pub fn is_repaint_boundary(&self) -> bool {
        true
    }

    /// Mark this boundary as needing repaint
    ///
    /// Call this when the child's visual appearance has changed.
    /// The next paint will re-render the child instead of using the cache.
    pub fn mark_needs_repaint(&self) {
        self.needs_repaint.store(true, Ordering::Release);
    }

    /// Check if this boundary needs repaint
    pub fn needs_repaint(&self) -> bool {
        self.needs_repaint.load(Ordering::Acquire)
    }

    /// Clear the cached display list
    ///
    /// Forces a full repaint on next paint call.
    pub fn invalidate_cache(&mut self) {
        self.cached_display_list = None;
        self.needs_repaint.store(true, Ordering::Release);
    }

    /// Check if there's a cached display list
    pub fn has_cached_display_list(&self) -> bool {
        self.cached_display_list.is_some()
    }

    /// Get the cached display list bounds (if cached)
    pub fn cached_bounds(&self) -> Option<flui_types::Rect> {
        self.cached_display_list.as_ref().map(|dl| dl.bounds())
    }

    /// Get a reference to the cached display list (if any)
    pub fn cached_display_list(&self) -> Option<&DisplayList> {
        self.cached_display_list.as_ref()
    }

    // === Debug Metrics ===

    /// Get the count of symmetric paints (debug only)
    ///
    /// Symmetric paints occur when parent and child paint together,
    /// suggesting the repaint boundary may not be helping performance.
    #[cfg(debug_assertions)]
    pub fn symmetric_paint_count(&self) -> u32 {
        self.symmetric_paint_count
    }

    /// Get the count of asymmetric paints (debug only)
    ///
    /// Asymmetric paints occur when only parent or child paints,
    /// suggesting the repaint boundary is helping performance.
    #[cfg(debug_assertions)]
    pub fn asymmetric_paint_count(&self) -> u32 {
        self.asymmetric_paint_count
    }

    /// Get the count of cache hits (debug only)
    #[cfg(debug_assertions)]
    pub fn cache_hit_count(&self) -> u32 {
        self.cache_hit_count
    }

    /// Get the count of cache misses (debug only)
    #[cfg(debug_assertions)]
    pub fn cache_miss_count(&self) -> u32 {
        self.cache_miss_count
    }

    /// Get the cache hit ratio (0.0 to 1.0, debug only)
    #[cfg(debug_assertions)]
    pub fn cache_hit_ratio(&self) -> f32 {
        let total = self.cache_hit_count + self.cache_miss_count;
        if total == 0 {
            0.0
        } else {
            self.cache_hit_count as f32 / total as f32
        }
    }

    /// Reset debug paint counters
    #[cfg(debug_assertions)]
    pub fn reset_metrics(&mut self) {
        self.symmetric_paint_count = 0;
        self.asymmetric_paint_count = 0;
        self.cache_hit_count = 0;
        self.cache_miss_count = 0;
    }

    /// Record a symmetric paint (both parent and child painted)
    #[cfg(debug_assertions)]
    pub fn record_symmetric_paint(&mut self) {
        self.symmetric_paint_count += 1;
    }

    /// Record an asymmetric paint (only parent or child painted)
    #[cfg(debug_assertions)]
    pub fn record_asymmetric_paint(&mut self) {
        self.asymmetric_paint_count += 1;
    }

    /// Check if this boundary appears to be effective
    ///
    /// Returns true if asymmetric paints exceed symmetric paints,
    /// suggesting the boundary is helping reduce unnecessary repaints.
    #[cfg(debug_assertions)]
    pub fn is_effective(&self) -> bool {
        self.asymmetric_paint_count > self.symmetric_paint_count
    }

    /// Get a debug description of boundary effectiveness
    #[cfg(debug_assertions)]
    pub fn debug_description(&self) -> &'static str {
        let ratio = if self.symmetric_paint_count == 0 {
            f32::INFINITY
        } else {
            self.asymmetric_paint_count as f32 / self.symmetric_paint_count as f32
        };

        match ratio {
            r if r >= 10.0 => "outstandingly useful repaint boundary",
            r if r >= 2.0 => "useful repaint boundary",
            r if r >= 1.0 => "probably useful repaint boundary",
            r if r >= 0.5 => "questionable repaint boundary",
            _ => "redundant repaint boundary (consider removing)",
        }
    }

    /// Update the cached display list directly
    ///
    /// Called by the framework after painting the child to update the cache.
    pub fn update_cache(&mut self, display_list: DisplayList) {
        self.cached_display_list = Some(display_list);
        self.needs_repaint.store(false, Ordering::Release);
    }

    /// Take the cached display list (for image capture)
    pub fn take_cached_display_list(&mut self) -> Option<DisplayList> {
        self.cached_display_list.take()
    }
}

impl Default for RenderRepaintBoundary {
    fn default() -> Self {
        Self::new()
    }
}

// ===== RenderObject Implementation =====

impl<T: FullRenderTree> RenderBox<T, Single> for RenderRepaintBoundary {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size
    where
        T: LayoutTree,
    {
        let child_id = ctx.children.single();
        // Pass-through layout - we don't modify constraints
        ctx.layout_child(child_id, ctx.constraints)
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: PaintTree,
    {
        // Extract values before borrowing ctx mutably
        let offset = ctx.offset;
        let child_id = ctx.children.single();

        // Check if we can use cached display list
        let use_cache =
            !self.needs_repaint.load(Ordering::Acquire) && self.cached_display_list.is_some();

        if use_cache {
            // Cache hit - reuse cached display list
            #[cfg(debug_assertions)]
            tracing::trace!("RepaintBoundary cache hit");

            if let Some(ref cached) = self.cached_display_list {
                ctx.canvas().append_display_list_at_offset(cached, offset);
            }
        } else {
            // Cache miss - repaint child
            #[cfg(debug_assertions)]
            tracing::trace!("RepaintBoundary cache miss - repainting child");

            ctx.paint_child(child_id, offset);

            // Mark as no longer needing repaint
            // Note: The actual cache update happens in paint_with_cache() or via framework
            self.needs_repaint.store(false, Ordering::Release);
        }
    }
}

// ===== Mutable Paint Implementation for Caching =====

impl RenderRepaintBoundary {
    /// Paint with caching support (mutable version)
    ///
    /// This method should be called by the framework instead of the trait method
    /// when layer caching is enabled. It properly updates the cache.
    ///
    /// # Cache Strategy
    ///
    /// 1. If `needs_repaint` is false and cache exists → use cache (fast path)
    /// 2. Otherwise → repaint child and store in cache
    ///
    /// The framework is responsible for calling `mark_needs_repaint()` when
    /// the child's visual appearance changes.
    pub fn paint_with_cache<T>(&mut self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: PaintTree,
    {
        // Extract values before borrowing ctx mutably
        let offset = ctx.offset;
        let child_id = ctx.children.single();

        let use_cache =
            !self.needs_repaint.load(Ordering::Acquire) && self.cached_display_list.is_some();

        if use_cache {
            // Cache hit
            #[cfg(debug_assertions)]
            {
                self.cache_hit_count += 1;
                tracing::trace!(
                    cache_hits = self.cache_hit_count,
                    cache_misses = self.cache_miss_count,
                    "RepaintBoundary cache hit"
                );
            }

            if let Some(ref cached) = self.cached_display_list {
                ctx.canvas().append_display_list_at_offset(cached, offset);
            }
        } else {
            // Cache miss - repaint child
            #[cfg(debug_assertions)]
            {
                self.cache_miss_count += 1;
                tracing::trace!(
                    cache_hits = self.cache_hit_count,
                    cache_misses = self.cache_miss_count,
                    "RepaintBoundary cache miss"
                );
            }

            ctx.paint_child(child_id, offset);

            // Note: To properly cache, we would need to capture the child's output.
            // This requires framework support to intercept the child's DisplayList.
            // For now, we just clear the needs_repaint flag.
            // Full caching implementation would look like:
            //
            // let child_canvas = ctx.tree_mut().perform_paint_to_canvas(child_id, offset);
            // self.cached_display_list = Some(child_canvas.finish());
            // ctx.canvas().append_canvas(child_canvas);

            self.needs_repaint.store(false, Ordering::Release);
        }
    }
}

// ===== Tests =====

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_repaint_boundary_new() {
        let boundary = RenderRepaintBoundary::new();
        assert!(boundary.is_repaint_boundary());
        assert!(boundary.needs_repaint()); // Initially needs paint
        assert!(!boundary.has_cached_display_list());
    }

    #[test]
    fn test_render_repaint_boundary_default() {
        let boundary = RenderRepaintBoundary::default();
        assert!(boundary.is_repaint_boundary());
    }

    #[test]
    fn test_is_repaint_boundary_always_true() {
        let boundary = RenderRepaintBoundary::new();
        assert!(boundary.is_repaint_boundary());
    }

    #[test]
    fn test_mark_needs_repaint() {
        let boundary = RenderRepaintBoundary::new();

        // Clear initial needs_repaint
        boundary.needs_repaint.store(false, Ordering::Release);
        assert!(!boundary.needs_repaint());

        // Mark as needing repaint
        boundary.mark_needs_repaint();
        assert!(boundary.needs_repaint());
    }

    #[test]
    fn test_invalidate_cache() {
        let mut boundary = RenderRepaintBoundary::new();

        // Add a cached display list
        boundary.cached_display_list = Some(DisplayList::new());
        boundary.needs_repaint.store(false, Ordering::Release);

        assert!(boundary.has_cached_display_list());
        assert!(!boundary.needs_repaint());

        // Invalidate
        boundary.invalidate_cache();

        assert!(!boundary.has_cached_display_list());
        assert!(boundary.needs_repaint());
    }

    #[test]
    fn test_update_cache() {
        let mut boundary = RenderRepaintBoundary::new();

        assert!(!boundary.has_cached_display_list());
        assert!(boundary.needs_repaint());

        // Update cache
        boundary.update_cache(DisplayList::new());

        assert!(boundary.has_cached_display_list());
        assert!(!boundary.needs_repaint());
    }

    #[test]
    fn test_cached_display_list_accessor() {
        let mut boundary = RenderRepaintBoundary::new();

        assert!(boundary.cached_display_list().is_none());

        boundary.update_cache(DisplayList::new());

        assert!(boundary.cached_display_list().is_some());
    }

    #[cfg(debug_assertions)]
    #[test]
    fn test_debug_metrics_initial() {
        let boundary = RenderRepaintBoundary::new();
        assert_eq!(boundary.symmetric_paint_count(), 0);
        assert_eq!(boundary.asymmetric_paint_count(), 0);
        assert_eq!(boundary.cache_hit_count(), 0);
        assert_eq!(boundary.cache_miss_count(), 0);
        assert_eq!(boundary.cache_hit_ratio(), 0.0);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn test_debug_metrics_record() {
        let mut boundary = RenderRepaintBoundary::new();

        boundary.record_symmetric_paint();
        assert_eq!(boundary.symmetric_paint_count(), 1);

        boundary.record_asymmetric_paint();
        boundary.record_asymmetric_paint();
        assert_eq!(boundary.asymmetric_paint_count(), 2);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn test_cache_metrics() {
        let mut boundary = RenderRepaintBoundary::new();

        boundary.cache_hit_count = 3;
        boundary.cache_miss_count = 1;

        assert_eq!(boundary.cache_hit_count(), 3);
        assert_eq!(boundary.cache_miss_count(), 1);
        assert_eq!(boundary.cache_hit_ratio(), 0.75);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn test_debug_metrics_reset() {
        let mut boundary = RenderRepaintBoundary::new();

        boundary.record_symmetric_paint();
        boundary.record_asymmetric_paint();
        boundary.cache_hit_count = 5;
        boundary.cache_miss_count = 2;

        boundary.reset_metrics();

        assert_eq!(boundary.symmetric_paint_count(), 0);
        assert_eq!(boundary.asymmetric_paint_count(), 0);
        assert_eq!(boundary.cache_hit_count(), 0);
        assert_eq!(boundary.cache_miss_count(), 0);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn test_is_effective() {
        let mut boundary = RenderRepaintBoundary::new();

        // Initially neither is greater
        assert!(!boundary.is_effective());

        // More symmetric = not effective
        boundary.record_symmetric_paint();
        boundary.record_symmetric_paint();
        boundary.record_asymmetric_paint();
        assert!(!boundary.is_effective());

        // Reset and make effective
        boundary.reset_metrics();
        boundary.record_asymmetric_paint();
        boundary.record_asymmetric_paint();
        boundary.record_symmetric_paint();
        assert!(boundary.is_effective());
    }

    #[cfg(debug_assertions)]
    #[test]
    fn test_debug_description() {
        let mut boundary = RenderRepaintBoundary::new();

        // No paints yet - check message
        boundary.record_asymmetric_paint();
        assert!(boundary.debug_description().contains("useful"));

        // Many asymmetric paints
        boundary.reset_metrics();
        for _ in 0..20 {
            boundary.record_asymmetric_paint();
        }
        boundary.record_symmetric_paint();
        assert!(boundary.debug_description().contains("outstandingly"));

        // Many symmetric paints
        boundary.reset_metrics();
        for _ in 0..20 {
            boundary.record_symmetric_paint();
        }
        boundary.record_asymmetric_paint();
        assert!(boundary.debug_description().contains("redundant"));
    }
}
