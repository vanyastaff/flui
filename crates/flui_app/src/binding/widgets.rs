//! Widgets binding - bridge to ElementTree
//!
//! WidgetsBinding manages the widget tree lifecycle and coordinates
//! the build phase with the element tree.

use super::BindingBase;
use flui_core::{
    element::{Element, ElementTree},
    foundation::ElementId,
    view::{IntoElement, View},
};
use parking_lot::RwLock;
use std::sync::Arc;

/// Widgets binding - manages widget tree
///
/// # Architecture
///
/// ```text
/// WidgetsBinding
///   ├─ ElementTree (mutable element tree)
///   └─ root_element (root ElementId)
/// ```
///
/// # Lifecycle
///
/// 1. **attach_root_widget()**: Inflate root widget → Element tree
/// 2. **handle_build_frame()**: Rebuild dirty widgets every frame
/// 3. **detach_root_widget()**: Clean up when app exits
///
/// # Thread-Safety
///
/// Uses Arc<RwLock<>> for thread-safe element tree access.
pub struct WidgetsBinding {
    /// Element tree (mutable widget tree)
    element_tree: Arc<RwLock<ElementTree>>,

    /// Root element ID (if attached)
    root_element: Arc<RwLock<Option<ElementId>>>,
}

impl WidgetsBinding {
    /// Create a new WidgetsBinding
    pub fn new() -> Self {
        Self {
            element_tree: Arc::new(RwLock::new(ElementTree::new())),
            root_element: Arc::new(RwLock::new(None)),
        }
    }

    /// Attach root widget
    ///
    /// Inserts the root widget into the element tree and stores its ID.
    /// This is the entry point for the entire widget tree.
    ///
    /// # Parameters
    ///
    /// - `widget`: The root widget (typically an App or MaterialApp)
    ///
    /// # Panics
    ///
    /// Panics if a root widget is already attached. Call `detach_root_widget()` first.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// binding.attach_root_widget(MyApp::new());
    /// ```
    pub fn attach_root_widget<V>(&self, widget: V)
    where
        V: View + 'static,
    {
        let mut root = self.root_element.write();
        if root.is_some() {
            panic!("Root widget already attached. Call detach_root_widget() first.");
        }

        // Build the widget into an element
        let element = widget.into_element();

        // Insert into element tree
        let mut tree = self.element_tree.write();
        let root_id = tree.insert(element);

        *root = Some(root_id);

        tracing::info!(root_id = ?root_id, "Root widget attached");
    }

    /// Detach root widget
    ///
    /// Removes the root widget from the element tree and cleans up.
    /// This is called when the app exits or when switching root widgets.
    pub fn detach_root_widget(&self) {
        let mut root = self.root_element.write();
        if let Some(root_id) = root.take() {
            let mut tree = self.element_tree.write();
            tree.remove(root_id);

            tracing::info!(root_id = ?root_id, "Root widget detached");
        }
    }

    /// Handle build frame
    ///
    /// Called every frame to rebuild dirty widgets.
    /// This is wired up to SchedulerBinding's persistent frame callback.
    pub fn handle_build_frame(&self) {
        // TODO: Implement build frame handling
        // For now, just log that we're handling a build frame
        tracing::trace!("Handling build frame");
    }

    /// Get shared reference to the element tree
    ///
    /// Used by renderer, gestures, and other framework components.
    #[must_use]
    pub fn element_tree(&self) -> Arc<RwLock<ElementTree>> {
        self.element_tree.clone()
    }

    /// Get root element ID
    ///
    /// Returns None if no root widget is attached.
    #[must_use]
    pub fn root_element(&self) -> Option<ElementId> {
        *self.root_element.read()
    }

    /// Check if a root widget is attached
    #[must_use]
    pub fn has_root(&self) -> bool {
        self.root_element.read().is_some()
    }
}

impl Default for WidgetsBinding {
    fn default() -> Self {
        Self::new()
    }
}

impl BindingBase for WidgetsBinding {
    fn init(&mut self) {
        tracing::debug!("WidgetsBinding initialized");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Simple test widget - just returns a basic element
    #[derive(Debug)]
    struct TestWidget;

    impl flui_core::view::View for TestWidget {
        fn build(self, _ctx: &flui_core::view::BuildContext) -> impl IntoElement {
            // Return a simple element
            // Use ComponentElement as it's the simplest
            flui_core::element::Element::Component(flui_core::element::ComponentElement::new())
        }
    }

    #[test]
    fn test_widgets_binding_creation() {
        let binding = WidgetsBinding::new();
        assert!(!binding.has_root());
        assert_eq!(binding.root_element(), None);
    }

    #[test]
    fn test_attach_root_widget() {
        let binding = WidgetsBinding::new();

        binding.attach_root_widget(TestWidget);

        assert!(binding.has_root());
        assert!(binding.root_element().is_some());
    }

    #[test]
    fn test_detach_root_widget() {
        let binding = WidgetsBinding::new();

        binding.attach_root_widget(TestWidget);
        assert!(binding.has_root());

        binding.detach_root_widget();
        assert!(!binding.has_root());
    }

    #[test]
    #[should_panic(expected = "Root widget already attached")]
    fn test_attach_twice_panics() {
        let binding = WidgetsBinding::new();

        binding.attach_root_widget(TestWidget);
        binding.attach_root_widget(TestWidget); // Should panic
    }

    #[test]
    fn test_handle_build_frame_empty() {
        let binding = WidgetsBinding::new();

        // Should not panic with no root
        binding.handle_build_frame();
    }
}
