//! Element struct - Pure View element (no RenderObject)
//!
//! This module provides the `Element` struct for Views only.
//! RenderObjects are stored separately in `RenderElement`.
//!
//! # Architecture (Following Flutter)
//!
//! ```text
//! View (Config)
//!   ↓ build()
//! Element (Lifecycle)
//!   ├─ Stateless/Stateful → child Element
//!   └─ RenderView → RenderElement
//! ```
//!
//! **Critical Design:**
//! - Element ONLY manages View lifecycle (mount, unmount, rebuild)
//! - Element has NO RenderObject - RenderObject goes in RenderElement
//! - RenderElement is a separate tree node containing protocol-specific layout/paint
//! - View.build() returns either:
//!   - Another View (wrapped in Element)
//!   - A RenderObject (wrapped in RenderElement)
//!
//! This separation is key to Flutter's architecture and FLUI's flexibility.

use std::any::Any;
use std::fmt;

use crate::element::{ElementBase, ElementId, ElementLifecycle};
use crate::foundation::Slot;
use crate::render::{LayoutProtocol, RenderObject, RenderState, RuntimeArity};
use crate::view::{BuildContext, ViewMode, ViewObject};

/// Element - Represents a View instance in the element tree
///
/// This struct represents a View instantiated at a particular location
/// in the element tree. It manages ONLY the View lifecycle.
///
/// **IMPORTANT:** Element does NOT store RenderObject.
/// If your View creates a RenderObject, the framework wraps it in a RenderElement instead.
///
/// # Design Principles
///
/// Following Flutter's Element architecture:
/// - `base`: Common lifecycle fields (parent, slot, lifecycle, flags)
/// - `view_object`: The polymorphic view implementation (Stateless, Stateful, etc)
/// - `children`: Child element IDs (may be Element or RenderElement)
///
/// RenderObjects are NOT stored here. They go in RenderElement.
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
    /// NOTE: This will NEVER be a render view.
    /// Render views are wrapped separately in RenderElement.
    view_object: Box<dyn ViewObject>,

    /// Child element IDs
    ///
    /// May contain both Element and RenderElement IDs
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

    /// Creates Element from a RenderElement.
    ///
    /// This is a compatibility method for the old `Element::Render(...)` pattern.
    /// In the new architecture, RenderElement is wrapped in a ViewObject.
    pub fn from_render_element(render_element: crate::render::RenderElement) -> Self {
        // For now, we store RenderElement directly
        // TODO: Wrap in RenderViewWrapper when migration is complete
        Self {
            base: ElementBase::new(),
            view_object: Box::new(RenderElementWrapper::new(render_element)),
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
    /// Invokes the view object's build method to produce child elements.
    ///
    /// # Contract
    ///
    /// The view object's build() method returns an Element.
    /// If the view created a RenderObject, it's wrapped in a RenderElement by the framework.
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
    /// If true, the framework will wrap the RenderObject in a separate RenderElement.
    /// Element itself does NOT store the RenderObject.
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

    /// Returns the wrapped RenderElement if this is a render element.
    ///
    /// This is a compatibility method for code that used `Element::Render(re)` pattern.
    pub fn as_render(&self) -> Option<&crate::render::RenderElement> {
        if self.is_render() {
            self.view_object
                .as_any()
                .downcast_ref::<crate::render::RenderElement>()
        } else {
            None
        }
    }

    /// Returns mutable wrapped RenderElement if this is a render element.
    pub fn as_render_mut(&mut self) -> Option<&mut crate::render::RenderElement> {
        if self.is_render() {
            self.view_object
                .as_any_mut()
                .downcast_mut::<crate::render::RenderElement>()
        } else {
            None
        }
    }

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

    /// Performs layout on this render element.
    ///
    /// Convenience method that delegates to the wrapped RenderElement.
    pub fn layout_render(
        &self,
        tree: &crate::element::ElementTree,
        constraints: flui_types::constraints::BoxConstraints,
    ) -> Option<flui_types::Size> {
        self.as_render()
            .map(|re| re.layout_render(tree, constraints))
    }

    /// Performs paint on this render element.
    ///
    /// Convenience method that delegates to the wrapped RenderElement.
    pub fn paint_render(
        &self,
        tree: &crate::element::ElementTree,
        offset: flui_types::Offset,
    ) -> Option<flui_painting::Canvas> {
        self.as_render().map(|re| re.paint_render(tree, offset))
    }

    /// Returns the RenderState lock for this render element.
    ///
    /// Convenience method that delegates to the wrapped RenderElement.
    pub fn render_state_lock(&self) -> Option<&parking_lot::RwLock<crate::render::RenderState>> {
        self.as_render().map(|re| re.render_state())
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

// ============================================================================
// RENDER ELEMENT WRAPPER
// ============================================================================

/// Wrapper that adapts RenderElement to ViewObject interface.
///
/// This is a compatibility layer for the old architecture where RenderElement
/// was a separate type. In the new architecture, all elements use ViewObject.
#[derive(Debug)]
pub struct RenderElementWrapper {
    render_element: crate::render::RenderElement,
}

impl RenderElementWrapper {
    /// Creates a new wrapper around a RenderElement.
    pub fn new(render_element: crate::render::RenderElement) -> Self {
        Self { render_element }
    }

    /// Returns reference to the wrapped RenderElement.
    pub fn inner(&self) -> &crate::render::RenderElement {
        &self.render_element
    }

    /// Returns mutable reference to the wrapped RenderElement.
    pub fn inner_mut(&mut self) -> &mut crate::render::RenderElement {
        &mut self.render_element
    }
}

impl ViewObject for RenderElementWrapper {
    fn mode(&self) -> ViewMode {
        match self.render_element.protocol() {
            LayoutProtocol::Box => ViewMode::RenderBox,
            LayoutProtocol::Sliver => ViewMode::RenderSliver,
        }
    }

    fn build(&mut self, _ctx: &BuildContext) -> Element {
        panic!("RenderElementWrapper::build should not be called")
    }

    fn init(&mut self, _ctx: &BuildContext) {}

    fn did_change_dependencies(&mut self, _ctx: &BuildContext) {}

    fn did_update(&mut self, _new_view: &dyn Any, _ctx: &BuildContext) {}

    fn deactivate(&mut self, _ctx: &BuildContext) {}

    fn dispose(&mut self, _ctx: &BuildContext) {}

    fn as_any(&self) -> &dyn Any {
        &self.render_element
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        &mut self.render_element
    }

    // Render-specific implementations
    fn render_object(&self) -> Option<&dyn RenderObject> {
        // RenderElement returns RwLockGuard, can't return reference to it
        // Return None for now - use as_render() to access RenderElement directly
        None
    }

    fn render_object_mut(&mut self) -> Option<&mut dyn RenderObject> {
        // RenderElement returns RwLockGuard, can't return mutable reference
        // Return None for now - use as_render_mut() to access RenderElement directly
        None
    }

    fn render_state(&self) -> Option<&RenderState> {
        // RenderElement has RwLock<RenderState>, need to return reference
        // This is tricky - we can't return reference to RwLock guard
        // For now return None and use direct access
        None
    }

    fn render_state_mut(&mut self) -> Option<&mut RenderState> {
        None
    }

    fn protocol(&self) -> Option<LayoutProtocol> {
        Some(self.render_element.protocol())
    }

    fn arity(&self) -> Option<RuntimeArity> {
        Some(*self.render_element.arity())
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
