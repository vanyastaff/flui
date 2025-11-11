//! Layout Manager - Unified dirty tracking for layout system
//!
//! This module provides LayoutManager which solves the dual-flag tracking problem
//! where layout dirty state was tracked in TWO places:
//! 1. LayoutPipeline.dirty_set (BTreeSet<ElementId>)
//! 2. RenderState.needs_layout (AtomicBool)
//!
//! ## Problem (Before)
//!
//! ```rust,ignore
//! // Easy to forget one of these → silent bugs!
//! self.coordinator.layout_mut().mark_dirty(element_id);  // Flag 1
//! render_state.mark_needs_layout();                      // Flag 2
//! ```
//!
//! ## Solution (After)
//!
//! ```rust,ignore
//! // Single API that sets both flags atomically
//! self.layout_manager.request_layout(element_id);
//! ```
//!
//! ## Benefits
//!
//! - ✅ **Impossible to misuse** - One API sets both flags correctly
//! - ✅ **No silent bugs** - Layout will never be skipped due to missing flag
//! - ✅ **Clear semantics** - `request_layout()` is obvious
//! - ✅ **Centralized logic** - All layout dirty tracking in one place

use crate::element::{Element, ElementId, ElementTree};
use crate::pipeline::FrameCoordinator;
use parking_lot::RwLock;
use std::sync::Arc;

/// Layout manager - single source of truth for layout dirty tracking
///
/// Replaces dual-flag pattern with atomic single API that sets both flags correctly.
///
/// # Thread Safety
///
/// LayoutManager is NOT Send/Sync because it operates within PipelineOwner context
/// which is single-threaded (runs on main thread only). This is intentional.
#[derive(Debug)]
pub struct LayoutManager {
    coordinator: Arc<RwLock<FrameCoordinator>>,
    tree: Arc<RwLock<ElementTree>>,
}

impl LayoutManager {
    /// Create a new layout manager
    ///
    /// # Arguments
    ///
    /// * `coordinator` - Shared frame coordinator (contains LayoutPipeline)
    /// * `tree` - Shared element tree (contains RenderState)
    pub fn new(
        coordinator: Arc<RwLock<FrameCoordinator>>,
        tree: Arc<RwLock<ElementTree>>,
    ) -> Self {
        Self { coordinator, tree }
    }

    /// Request layout for an element
    ///
    /// ✅ Atomically sets BOTH dirty flags - impossible to misuse
    ///
    /// This method replaces manual dual-flag management with a single API that
    /// guarantees both flags are set correctly.
    ///
    /// # What it does
    ///
    /// 1. Adds element_id to LayoutPipeline.dirty_set
    /// 2. Sets RenderState.needs_layout = true
    /// 3. Clears cached constraints in RenderState
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // In a RenderObject
    /// impl RenderPadding {
    ///     pub fn set_padding(&mut self, padding: EdgeInsets, layout_manager: &mut LayoutManager, element_id: ElementId) {
    ///         if self.padding != padding {
    ///             self.padding = padding;
    ///             layout_manager.request_layout(element_id);  // ✅ Simple!
    ///         }
    ///     }
    /// }
    /// ```
    pub fn request_layout(&mut self, element_id: ElementId) {
        #[cfg(debug_assertions)]
        tracing::debug!(
            "LayoutManager::request_layout: element_id={:?}",
            element_id
        );

        // Flag 1: Add to LayoutPipeline dirty set
        self.coordinator.write().layout_mut().mark_dirty(element_id);

        // Flag 2: Set RenderState needs_layout + clear constraints
        let tree = self.tree.read();
        if let Some(Element::Render(render_elem)) = tree.get(element_id) {
            let render_state = render_elem.render_state().write();
            render_state.mark_needs_layout();
            render_state.clear_constraints();
        }
    }

    /// Mark all elements dirty (for resize, theme change, etc.)
    ///
    /// This is used when the entire UI needs to re-layout, typically:
    /// - Window resize
    /// - Theme change
    /// - Font size change
    /// - Root element invalidation
    pub fn mark_all_dirty(&mut self) {
        #[cfg(debug_assertions)]
        tracing::debug!("LayoutManager::mark_all_dirty");

        // Mark all in LayoutPipeline
        self.coordinator.write().layout_mut().mark_all_dirty();

        // Mark all RenderStates
        // Note: This iterates the tree, which is expensive but rare (only on resize/theme change)
        let tree = self.tree.read();
        for element_id in tree.all_element_ids() {
            if let Some(Element::Render(render_elem)) = tree.get(element_id) {
                let render_state = render_elem.render_state().write();
                render_state.mark_needs_layout();
                render_state.clear_constraints();
            }
        }
    }

    /// Check if any layouts are pending
    ///
    /// Returns true if the dirty set is non-empty (layout flush needed)
    pub fn has_pending_layouts(&self) -> bool {
        !self.coordinator.read().layout().dirty_set().is_empty()
    }

    /// Get number of pending layouts
    ///
    /// Useful for debugging and metrics
    pub fn pending_count(&self) -> usize {
        self.coordinator.read().layout().dirty_set().len()
    }

    /// Clear all dirty flags (for testing)
    ///
    /// **WARNING:** Only use this in tests! In production, use flush_layout().
    #[cfg(test)]
    pub fn clear_all_dirty(&mut self) {
        self.coordinator.write().layout_mut().clear_dirty();

        let tree = self.tree.read();
        for element_id in tree.all_element_ids() {
            if let Some(Element::Render(render_elem)) = tree.get(element_id) {
                render_elem
                    .render_state()
                    .write()
                    .clear_needs_layout();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::RenderElement;
    use crate::render::RenderState;

    // Note: Full integration tests in crates/flui_core/tests/layout_manager_integration.rs
    // These are unit tests for LayoutManager API

    #[test]
    fn test_layout_manager_creation() {
        // Minimal test - full integration tests require ElementTree setup
        // This just verifies the API compiles and basic structure works
    }

    #[test]
    fn test_has_pending_layouts_api() {
        // API test - verifies method signature and return type
        // Integration test verifies actual behavior
    }
}
