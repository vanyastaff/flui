//! RenderRepaintBoundary - optimization boundary for repainting

use crate::core::{
    RenderBox, Single, {BoxProtocol, LayoutContext, PaintContext},
};
use flui_types::Size;
use std::sync::atomic::{AtomicBool, Ordering};

/// RenderObject that creates a repaint boundary
///
/// This widget creates a separate paint layer, isolating the child's
/// repainting from its ancestors. When the child repaints, only this
/// subtree needs to be repainted, not the entire widget tree.
///
/// Useful for optimizing performance when a widget repaints frequently
/// (e.g., animations, videos, interactive elements).
///
/// # Layer Caching
///
/// RenderRepaintBoundary uses layer caching to avoid repainting when
/// only the parent changes. The cache is invalidated when:
/// - The child needs layout (size changed)
/// - The child needs paint (content changed)
/// - The boundary is explicitly marked dirty
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderRepaintBoundary;
///
/// // Create repaint boundary for animated child
/// let boundary = RenderRepaintBoundary::new();
/// ```
#[derive(Debug)]
pub struct RenderRepaintBoundary {
    /// Whether this boundary is currently active
    pub is_repaint_boundary: bool,

    /// Cache dirty flag - tracks if child needs repainting
    ///
    /// Using AtomicBool for thread-safe interior mutability since we need
    /// to update this flag during paint phase (which takes &self).
    cache_dirty: AtomicBool,
}

impl RenderRepaintBoundary {
    /// Create new RenderRepaintBoundary
    pub fn new() -> Self {
        Self {
            is_repaint_boundary: true,
            cache_dirty: AtomicBool::new(true), // Start dirty to force initial paint
        }
    }

    /// Create inactive boundary
    pub fn inactive() -> Self {
        Self {
            is_repaint_boundary: false,
            cache_dirty: AtomicBool::new(true),
        }
    }

    /// Set whether this is a repaint boundary
    pub fn set_is_repaint_boundary(&mut self, is_boundary: bool) {
        self.is_repaint_boundary = is_boundary;
        if is_boundary {
            self.mark_cache_dirty();
        }
    }

    /// Mark the cache as dirty, forcing a repaint on next paint
    pub fn mark_cache_dirty(&self) {
        self.cache_dirty.store(true, Ordering::Relaxed);
    }

    /// Check if the cache is dirty
    pub fn is_cache_dirty(&self) -> bool {
        self.cache_dirty.load(Ordering::Relaxed)
    }

    /// Mark the cache as clean (called after successful paint)
    fn mark_cache_clean(&self) {
        self.cache_dirty.store(false, Ordering::Relaxed);
    }
}

impl Default for RenderRepaintBoundary {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderBox<Single> for RenderRepaintBoundary {
    fn layout<T>(&mut self, mut ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size
    where
        T: crate::core::LayoutTree,
    {
        let child_id = ctx.children.single();

        // Layout child
        let size = ctx.layout_child(child_id, ctx.constraints);

        // Mark cache dirty on layout since size may have changed
        // This ensures we repaint when the child's layout changes
        if self.is_repaint_boundary {
            self.mark_cache_dirty();
        }

        size
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Single>)
    where
        T: crate::core::PaintTree,
    {
        let child_id = ctx.children.single();

        // Paint child
        // The cache dirty flag is tracked but actual caching is performed
        // at the compositor level via CachedLayer in flui_engine.
        //
        // The rendering pipeline can query is_cache_dirty() to determine
        // whether to create a new CachedLayer or reuse an existing one.
        ctx.paint_child(child_id, ctx.offset);

        // Mark cache as clean after successful paint
        // This allows the framework to skip repainting if only ancestors change
        if self.is_repaint_boundary {
            self.mark_cache_clean();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_repaint_boundary_new() {
        let boundary = RenderRepaintBoundary::new();
        assert!(boundary.is_repaint_boundary);
    }

    #[test]
    fn test_render_repaint_boundary_inactive() {
        let boundary = RenderRepaintBoundary::inactive();
        assert!(!boundary.is_repaint_boundary);
    }

    #[test]
    fn test_render_repaint_boundary_default() {
        let boundary = RenderRepaintBoundary::default();
        assert!(boundary.is_repaint_boundary);
    }

    #[test]
    fn test_render_repaint_boundary_set_is_repaint_boundary() {
        let mut boundary = RenderRepaintBoundary::new();
        boundary.set_is_repaint_boundary(false);
        assert!(!boundary.is_repaint_boundary);
    }

    #[test]
    fn test_render_repaint_boundary_cache_dirty_initial() {
        let boundary = RenderRepaintBoundary::new();
        assert!(boundary.is_cache_dirty()); // Should start dirty
    }

    #[test]
    fn test_render_repaint_boundary_mark_cache_dirty() {
        let boundary = RenderRepaintBoundary::new();

        // Manually mark as clean for testing
        boundary.mark_cache_clean();
        assert!(!boundary.is_cache_dirty());

        // Mark dirty
        boundary.mark_cache_dirty();
        assert!(boundary.is_cache_dirty());
    }

    #[test]
    fn test_render_repaint_boundary_cache_invalidation_on_enable() {
        let mut boundary = RenderRepaintBoundary::inactive();

        // Mark clean
        boundary.mark_cache_clean();
        assert!(!boundary.is_cache_dirty());

        // Enabling boundary should mark dirty
        boundary.set_is_repaint_boundary(true);
        assert!(boundary.is_cache_dirty());
    }

    #[test]
    fn test_render_repaint_boundary_cache_clean_after_paint() {
        let boundary = RenderRepaintBoundary::new();

        // Initial state is dirty
        assert!(boundary.is_cache_dirty());

        // After marking clean (simulating paint completion)
        boundary.mark_cache_clean();
        assert!(!boundary.is_cache_dirty());
    }
}
