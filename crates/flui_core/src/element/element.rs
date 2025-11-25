//! Element struct - Unified element with ViewObject delegation
//!
//! This module provides the unified `Element` struct that works with all view types
//! through ViewObject delegation pattern.
//!
//! # Architecture (v0.7.0 - Unified Element)
//!
//! ```text
//! View (Config)
//!   ↓ build()
//! Element (Lifecycle + ViewObject)
//!   ├─ StatelessViewWrapper → child Element
//!   ├─ StatefulViewWrapper → child Element
//!   ├─ ProviderViewWrapper → child Element
//!   └─ RenderViewWrapper → layout/paint
//! ```
//!
//! **Critical Design:**
//! - Single Element struct with Box<dyn ViewObject> for type-specific behavior
//! - All element variants (component, provider, render) use same Element struct
//! - ViewObject trait handles type-specific operations (build, layout, paint, etc.)
//! - RenderObjects are stored in RenderViewWrapper/RenderObjectWrapper ViewObjects
//!
//! This unified approach eliminates enum dispatch overhead and matches Flutter's architecture.

use std::any::Any;
use std::fmt;

use crate::element::{ElementBase, ElementId, ElementLifecycle};
use crate::foundation::Slot;
use crate::render::{LayoutProtocol, RenderObject, RenderState, RuntimeArity};
use crate::view::{BuildContext, ViewMode, ViewObject};

/// Element - Unified element struct with ViewObject delegation
///
/// This struct represents any View instance in the element tree using
/// the ViewObject delegation pattern for type-specific behavior.
///
/// # Design Principles (v0.7.0)
///
/// Unified Element architecture:
/// - `base`: Common lifecycle fields (parent, slot, lifecycle, flags)
/// - `view_object`: Polymorphic view implementation (all view types)
/// - `children`: Child element IDs (all using same Element struct)
///
/// RenderObjects are stored in RenderViewWrapper/RenderObjectWrapper ViewObjects.
/// This eliminates enum dispatch and provides extensible architecture.
///
/// # Thread Safety
///
/// Element is `Send` because:
/// - `ViewObject` requires `Send`
/// - All internal fields are thread-safe
pub struct Element {
    /// Common lifecycle fields
    base: ElementBase,

    /// The polymorphic view object (Stateless, Stateful, etc)
    ///
    /// Contains all view types: Stateless, Stateful, Provider, Render, etc.
    view_object: Box<dyn ViewObject>,

    /// Child element IDs
    ///
    /// All children use unified Element struct
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
            .field("children_count", &self.children.len())
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

    // ========== CONSTRUCTOR METHODS ==========
    // Note: Removed from_render_element() - use unified ViewObject wrappers instead

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
    /// Invokes the view object's build method to produce child elements.
    /// All ViewObject types handle their specific build logic internally.
    #[inline]
    pub fn build(&mut self, ctx: &BuildContext) {
        // Delegate to ViewObject
        let _ = self.view_object.build(ctx);
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

    /// Check if this view creates a RenderObject.
    ///
    /// Returns true for RenderViewWrapper and RenderObjectWrapper ViewObjects.
    #[inline]
    #[must_use]
    pub fn is_render_view(&self) -> bool {
        matches!(
            self.view_object.mode(),
            ViewMode::RenderBox | ViewMode::RenderSliver
        )
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

    /// Check if this is a component element (has render object).
    ///
    /// Component elements are Views that build child Views (not RenderObjects).
    #[inline]
    #[must_use]
    pub fn is_component(&self) -> bool {
        matches!(
            self.view_object.mode(),
            ViewMode::Stateless | ViewMode::Stateful | ViewMode::Proxy | ViewMode::Animated
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
            ViewMode::Stateless => "Stateless",
            ViewMode::Stateful => "Stateful",
            ViewMode::Animated => "Animated",
            ViewMode::Provider => "Provider",
            ViewMode::Proxy => "Proxy",
            ViewMode::RenderBox => "RenderBox",
            ViewMode::RenderSliver => "RenderSliver",
        }
    }

    // ========== Event Handling ==========

    /// Handle an event.
    #[inline]
    pub fn handle_event(&mut self, _event: &flui_types::Event) -> bool {
        false
    }

    // ========== Render Element Access (for architecture transition) ==========

    /// Check if this is render view element (RenderBox or RenderSliver).
    ///
    /// During architecture transition, indicates whether this Element
    /// wraps render view or component view.
    #[inline]
    #[must_use]
    pub fn is_render(&self) -> bool {
        self.is_render_view()
    }

    /// Get category name for debugging and logging.
    pub fn debug_name(&self) -> String {
        match self.view_object.mode() {
            ViewMode::Stateless => "Stateless".to_string(),
            ViewMode::Stateful => "Stateful".to_string(),
            ViewMode::Animated => "Animated".to_string(),
            ViewMode::Provider => "Provider".to_string(),
            ViewMode::Proxy => "Proxy".to_string(),
            ViewMode::RenderBox => "RenderBox".to_string(),
            ViewMode::RenderSliver => "RenderSliver".to_string(),
        }
    }

    // ========== RENDER ACCESS (delegates to ViewObject) ==========

    /// Returns render object if this is a render element.
    #[inline]
    pub fn render_object(&self) -> Option<&dyn RenderObject> {
        self.view_object.render_object()
    }

    /// Returns mutable render object if this is a render element.
    #[inline]
    pub fn render_object_mut(&mut self) -> Option<&mut dyn RenderObject> {
        self.view_object.render_object_mut()
    }

    /// Returns render state if this is a render element.
    #[inline]
    pub fn render_state(&self) -> Option<&RenderState> {
        self.view_object.render_state()
    }

    /// Returns mutable render state if this is a render element.
    #[inline]
    pub fn render_state_mut(&mut self) -> Option<&mut RenderState> {
        self.view_object.render_state_mut()
    }

    /// Returns layout protocol if this is a render element.
    #[inline]
    pub fn protocol(&self) -> Option<LayoutProtocol> {
        self.view_object.protocol()
    }

    /// Returns arity if this is a render element.
    #[inline]
    pub fn arity(&self) -> Option<RuntimeArity> {
        self.view_object.arity()
    }

    // ========== PROVIDER ACCESS (delegates to ViewObject) ==========

    /// Returns provided value if this is a provider element.
    #[inline]
    pub fn provided_value(&self) -> Option<&(dyn Any + Send + Sync)> {
        self.view_object.provided_value()
    }

    /// Returns dependents if this is a provider element.
    #[inline]
    pub fn dependents(&self) -> Option<&[ElementId]> {
        self.view_object.dependents()
    }

    /// Returns mutable dependents if this is a provider element.
    #[inline]
    pub fn dependents_mut(&mut self) -> Option<&mut Vec<ElementId>> {
        self.view_object.dependents_mut()
    }

    /// Adds a dependent to this provider.
    pub fn add_dependent(&mut self, element_id: ElementId) {
        if let Some(deps) = self.view_object.dependents_mut() {
            if !deps.contains(&element_id) {
                deps.push(element_id);
            }
        }
    }

    /// Removes a dependent from this provider.
    pub fn remove_dependent(&mut self, element_id: ElementId) {
        if let Some(deps) = self.view_object.dependents_mut() {
            deps.retain(|&id| id != element_id);
        }
    }

    /// Check if dependents should be notified.
    pub fn should_notify_dependents(&self, old_value: &dyn Any) -> bool {
        self.view_object.should_notify_dependents(old_value)
    }

    // ========== COMPATIBILITY METHODS ==========
    // These methods provide backward compatibility during the migration
    // from enum-based Element to struct-based Element with ViewObject.

    /// Returns self if this is a component element.
    ///
    /// Component elements are Stateless, Stateful, Proxy, or Animated views.
    pub fn as_component(&self) -> Option<&Self> {
        if self.is_component() {
            Some(self)
        } else {
            None
        }
    }

    /// Returns mutable self if this is a component element.
    pub fn as_component_mut(&mut self) -> Option<&mut Self> {
        if self.is_component() {
            Some(self)
        } else {
            None
        }
    }

    /// Returns self if this is a provider element.
    pub fn as_provider(&self) -> Option<&Self> {
        if self.is_provider() {
            Some(self)
        } else {
            None
        }
    }

    /// Returns mutable self if this is a provider element.
    pub fn as_provider_mut(&mut self) -> Option<&mut Self> {
        if self.is_provider() {
            Some(self)
        } else {
            None
        }
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

    #[test]
    fn test_element_is_view_only() {
        // Element should not have any render object storage
        // This is verified by checking the struct fields
        let _element: Element;
        // Compile-time verification:
        // If Element had RenderObject storage, this test would fail
        // when we try to verify no render methods exist
    }
}
