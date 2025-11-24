//! Element struct - Unified element type for all views
//!
//! This module provides the unified `Element` struct that replaces the old
//! `enum Element` with a single struct containing a `ViewObject`.
//!
//! # Architecture
//!
//! Following Flutter's Element design:
//! - Element is the instantiation of a View at a particular location in the tree
//! - Element manages lifecycle (mount, update, unmount)
//! - Element bridges View tree and Render tree
//!
//! # Performance
//!
//! Using `Box<dyn ViewObject>` provides:
//! - Single allocation per element
//! - Dynamic dispatch for view operations
//! - Consistent memory layout
//! - Easy extension for new view types

use std::any::Any;
use std::fmt;

use crate::element::{ElementBase, ElementId, ElementLifecycle};
use crate::foundation::Slot;
use crate::render::RenderObject;
use crate::view::{BuildContext, ViewMode, ViewObject};

/// Element - Unified element type for all views
///
/// This struct represents a View instantiated at a particular location
/// in the element tree. It manages the lifecycle of the view and bridges
/// between the View tree and Render tree.
///
/// # Design
///
/// Following Flutter's architecture:
/// - `base`: Common lifecycle fields (parent, slot, lifecycle, flags)
/// - `view_object`: The polymorphic view implementation
/// - `children`: Child element IDs
///
/// # Thread Safety
///
/// Element is `Send` because:
/// - `ViewObject` requires `Send`
/// - All internal fields are thread-safe
pub struct Element {
    /// Common lifecycle fields
    base: ElementBase,

    /// The polymorphic view object
    view_object: Box<dyn ViewObject>,

    /// Child element IDs
    children: Vec<ElementId>,
}

// Explicitly implement Send since ViewObject is Send
unsafe impl Send for Element {}

impl fmt::Debug for Element {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Element")
            .field("parent", &self.base.parent())
            .field("lifecycle", &self.base.lifecycle())
            .field("mode", &self.view_object.mode())
            .field("children", &self.children)
            .finish()
    }
}

impl Element {
    /// Creates a new Element with the given view object.
    pub fn new(view_object: Box<dyn ViewObject>) -> Self {
        Self {
            base: ElementBase::new(),
            view_object,
            children: Vec::new(),
        }
    }

    // ========== Lifecycle Methods (delegated to ElementBase) ==========

    /// Mount element to tree.
    #[inline]
    pub fn mount(&mut self, parent: Option<ElementId>, slot: Option<Slot>) {
        self.base.mount(parent, slot);
    }

    /// Unmount element from tree.
    #[inline]
    pub fn unmount(&mut self) {
        self.base.unmount();
    }

    /// Activate element.
    #[inline]
    pub fn activate(&mut self) {
        self.base.activate();
    }

    /// Deactivate element.
    #[inline]
    pub fn deactivate(&mut self) {
        self.base.deactivate();
    }

    /// Get current lifecycle state.
    #[inline]
    #[must_use]
    pub fn lifecycle(&self) -> ElementLifecycle {
        self.base.lifecycle()
    }

    // ========== Parent/Slot Accessors ==========

    /// Get parent element ID.
    #[inline]
    #[must_use]
    pub fn parent(&self) -> Option<ElementId> {
        self.base.parent()
    }

    // ========== Dirty Tracking (delegated to ElementBase) ==========

    /// Check if element needs rebuild.
    #[inline]
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.base.is_dirty()
    }

    /// Mark element as needing rebuild.
    #[inline]
    pub fn mark_dirty(&mut self) {
        self.base.mark_dirty();
    }

    /// Clear dirty flag.
    #[inline]
    pub fn clear_dirty(&mut self) {
        self.base.clear_dirty();
    }

    // ========== View Object Access ==========

    /// Get view mode (Stateless, Stateful, etc.).
    #[inline]
    #[must_use]
    pub fn mode(&self) -> ViewMode {
        self.view_object.mode()
    }

    /// Build this element.
    ///
    /// Calls the view object's build method to produce child elements.
    /// Returns the old Element type until migration is complete.
    #[inline]
    pub fn build(&mut self, ctx: &BuildContext) -> crate::element::element::Element {
        self.view_object.build(ctx)
    }

    /// Initialize after mounting.
    #[inline]
    pub fn init(&mut self, ctx: &BuildContext) {
        self.view_object.init(ctx);
    }

    /// Called when dependencies change.
    #[inline]
    pub fn did_change_dependencies(&mut self, ctx: &BuildContext) {
        self.view_object.did_change_dependencies(ctx);
    }

    /// Update with new view configuration.
    #[inline]
    pub fn did_update(&mut self, new_view: &dyn Any, ctx: &BuildContext) {
        self.view_object.did_update(new_view, ctx);
    }

    /// Called when element is deactivated.
    #[inline]
    pub fn on_deactivate(&mut self, ctx: &BuildContext) {
        self.view_object.deactivate(ctx);
    }

    /// Called when element is permanently removed.
    #[inline]
    pub fn dispose(&mut self, ctx: &BuildContext) {
        self.view_object.dispose(ctx);
    }

    /// Get render object if this is a render view.
    #[inline]
    #[must_use]
    pub fn render_object(&self) -> Option<&dyn RenderObject> {
        self.view_object.render_object()
    }

    /// Get mutable render object if this is a render view.
    #[inline]
    #[must_use]
    pub fn render_object_mut(&mut self) -> Option<&mut dyn RenderObject> {
        self.view_object.render_object_mut()
    }

    /// Access view object for downcasting.
    #[inline]
    #[must_use]
    pub fn view_object(&self) -> &dyn ViewObject {
        &*self.view_object
    }

    /// Mutable access to view object.
    #[inline]
    #[must_use]
    pub fn view_object_mut(&mut self) -> &mut dyn ViewObject {
        &mut *self.view_object
    }

    // ========== Child Management ==========

    /// Get child element IDs.
    #[inline]
    #[must_use]
    pub fn children(&self) -> &[ElementId] {
        &self.children
    }

    /// Get mutable child element IDs.
    #[inline]
    #[must_use]
    pub fn children_mut(&mut self) -> &mut Vec<ElementId> {
        &mut self.children
    }

    /// Add a child element.
    #[inline]
    pub fn add_child(&mut self, child_id: ElementId) {
        self.children.push(child_id);
    }

    /// Remove a child element.
    #[inline]
    pub fn remove_child(&mut self, child_id: ElementId) {
        self.children.retain(|&id| id != child_id);
    }

    /// Clear all children.
    #[inline]
    pub fn clear_children(&mut self) {
        self.children.clear();
    }

    /// Set children from iterator.
    #[inline]
    pub fn set_children(&mut self, children: impl IntoIterator<Item = ElementId>) {
        self.children = children.into_iter().collect();
    }

    /// Check if element has children.
    #[inline]
    #[must_use]
    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    /// Get first child (for single-child elements).
    #[inline]
    #[must_use]
    pub fn first_child(&self) -> Option<ElementId> {
        self.children.first().copied()
    }

    /// Forget a child (called when child is unmounted).
    #[inline]
    pub fn forget_child(&mut self, child_id: ElementId) {
        self.remove_child(child_id);
    }

    // ========== Predicates ==========

    /// Check if this is a render element (has render object).
    #[inline]
    #[must_use]
    pub fn is_render(&self) -> bool {
        self.view_object.render_object().is_some()
    }

    /// Check if this is a component element (no render object).
    #[inline]
    #[must_use]
    pub fn is_component(&self) -> bool {
        matches!(
            self.view_object.mode(),
            ViewMode::Stateless | ViewMode::Stateful
        )
    }

    /// Check if this is a provider element.
    #[inline]
    #[must_use]
    pub fn is_provider(&self) -> bool {
        matches!(self.view_object.mode(), ViewMode::Provider)
    }

    /// Get element category name for debugging.
    #[inline]
    #[must_use]
    pub fn category(&self) -> &'static str {
        match self.view_object.mode() {
            ViewMode::Stateless | ViewMode::Stateful => "Component",
            ViewMode::Animated => "Animated",
            ViewMode::Provider => "Provider",
            ViewMode::Proxy => "Proxy",
            ViewMode::RenderBox | ViewMode::RenderSliver => "Render",
        }
    }

    // ========== Event Handling ==========

    /// Handle an event.
    #[inline]
    pub fn handle_event(&mut self, _event: &flui_types::Event) -> bool {
        // TODO: Delegate to view_object when ViewObject has handle_event
        false
    }
}

// ============================================================================
// TRAIT IMPLEMENTATIONS
// ============================================================================

impl From<Box<dyn ViewObject>> for Element {
    fn from(view_object: Box<dyn ViewObject>) -> Self {
        Element::new(view_object)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // TODO: Add tests after ViewObject wrappers are complete
}
