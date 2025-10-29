//! DynElement - Object-safe trait for heterogeneous element storage

use std::fmt;

use parking_lot::RwLock;

use crate::element::ElementId;
use crate::render::{RenderNode, RenderState};
use crate::widget::DynWidget;

/// Element lifecycle states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementLifecycle {
    /// Element created but not yet mounted
    Initial,
    /// Element is active in the tree
    Active,
    /// Element removed from tree but might be reinserted
    Inactive,
    /// Element permanently removed
    Defunct,
}

/// Object-safe base trait for all elements
///
/// This trait provides the minimal interface needed for heterogeneous element storage
/// in the ElementTree. All element types (ComponentElement, StatefulElement,
/// RenderElement) implement this trait.
///
/// # Design Pattern: Two-Trait Approach
///
/// Similar to Widget/DynWidget and Render/RenderNode:
/// - **DynElement** (this trait) - Object-safe for `Box<dyn DynElement>`
/// - **Element** - Has associated types for zero-cost concrete operations
pub trait DynElement: fmt::Debug + Send + Sync {
    // ========== Tree Structure ==========

    /// Get parent element ID
    fn parent(&self) -> Option<ElementId>;

    /// Get children iterator
    fn children_iter(&self) -> Box<dyn Iterator<Item = ElementId> + '_>;

    // ========== Lifecycle ==========

    /// Get current lifecycle state
    fn lifecycle(&self) -> ElementLifecycle;

    /// Mount this element into the tree
    ///
    /// # Arguments
    /// - `parent`: Parent element ID (None for root)
    /// - `slot`: Position in parent's child list
    fn mount(&mut self, parent: Option<ElementId>, slot: usize);

    /// Unmount this element from the tree
    fn unmount(&mut self);

    /// Deactivate (element removed but might be reinserted)
    fn deactivate(&mut self);

    /// Activate (element reinserted after deactivation)
    fn activate(&mut self);

    // ========== Widget Management ==========

    /// Get the widget this element holds
    fn widget(&self) -> &dyn DynWidget;

    /// Update with new widget (dynamic version)
    ///
    /// This performs a downcast to check widget type compatibility.
    fn update_any(&mut self, new_widget: Box<dyn DynWidget>);

    // ========== Rebuild ==========

    /// Check if element is dirty (needs rebuild)
    fn is_dirty(&self) -> bool;

    /// Mark element as dirty
    fn mark_dirty(&mut self);

    /// Perform rebuild
    ///
    /// Returns list of child widgets that need to be mounted:
    /// (parent_id, child_widget, slot)
    fn rebuild(&mut self, element_id: ElementId) -> Vec<(ElementId, Box<dyn DynWidget>, usize)>;

    // ========== Render Access ==========

    /// Get Render if this is a RenderElement
    fn render_object(&self) -> Option<&RenderNode> {
        None
    }

    /// Get mutable Render if this is a RenderElement
    fn render_object_mut(&mut self) -> Option<&mut RenderNode> {
        None
    }

    // ========== RenderState Access ==========

    /// Get raw pointer to RenderState if this is a RenderElement
    ///
    /// Returns None for ComponentElement and StatefulElement.
    ///
    /// # Safety
    ///
    /// Caller must ensure pointer is used safely and respects RwLock semantics.
    /// This is needed because we can't return `&RwLock<RenderState>` with proper
    /// lifetime from a trait object.
    fn render_state_ptr(&self) -> Option<*const RwLock<RenderState>> {
        None
    }

    // ========== Helper Methods ==========

    /// Forget a child (called when child is unmounted)
    fn forget_child(&mut self, child_id: ElementId);

    /// Update slot for a child
    fn update_slot_for_child(&mut self, child_id: ElementId, new_slot: usize);
}

/// Boxed Element trait object
pub type BoxedElement = Box<crate::element::Element>;
