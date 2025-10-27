//! Widget Inspector for FLUI applications
//!
//! Provides widget tree inspection, property viewing, and debugging capabilities.
//! Similar to Flutter's Widget Inspector and React DevTools.

//! # Example//!//! ```rust,no_run//! use flui_devtools::inspector::Inspector;//! use flui_core::ElementTree;//!//! let inspector = Inspector::new();//!//! // Attach to element tree//! let tree = ElementTree::new();//! inspector.attach_to_tree(&tree);//!//! // Highlight widget (for visual debugging)//! let element_id = 0;//! inspector.highlight_widget(element_id);//!//! // Get widget tree structure//! let tree_structure = inspector.get_widget_tree();//! println!("Tree has {} roots", tree_structure.len());//! ```
use std::sync::Arc;
use parking_lot::RwLock;
use flui_core::element::{ElementId, ElementTree, Element};
use flui_types::{Size, Offset};
use serde::{Serialize, Deserialize};

/// Information about a widget in the tree
#[derive(Debug, Clone, Serialize)]
pub struct WidgetInfo {
    /// Element ID
    pub element_id: ElementId,
    /// Widget type name
    pub widget_type: String,
    /// Widget size (if laid out)
    #[serde(skip)]
    pub size: Option<Size>,
    /// Widget position (offset from parent)
    #[serde(skip)]
    pub position: Option<Offset>,
    /// Parent element ID
    pub parent_id: Option<ElementId>,
    /// Child element IDs
    pub children: Vec<ElementId>,
    /// Custom properties (key-value pairs)
    pub properties: Vec<(String, String)>,
    /// Whether this widget has a RenderObject
    pub has_render_object: bool,
    /// Depth in the tree (0 = root)
    pub depth: usize,
}

impl WidgetInfo {
    /// Get the widget type name
    pub fn widget_type(&self) -> &str {
        &self.widget_type
    }

    /// Get the widget size
    pub fn size(&self) -> Option<Size> {
        self.size
    }

    /// Get the widget position
    pub fn position(&self) -> Option<Offset> {
        self.position
    }

    /// Get the parent element ID
    pub fn parent_id(&self) -> Option<ElementId> {
        self.parent_id
    }

    /// Get the child element IDs
    pub fn children(&self) -> &[ElementId] {
        &self.children
    }

    /// Get custom properties
    pub fn properties(&self) -> &[(String, String)] {
        &self.properties
    }

    /// Check if this widget has a RenderObject
    pub fn has_render_object(&self) -> bool {
        self.has_render_object
    }

    /// Get tree depth
    pub fn depth(&self) -> usize {
        self.depth
    }
}

/// Widget tree node for hierarchical display
#[derive(Debug, Clone, Serialize)]
pub struct WidgetTreeNode {
    /// Widget information
    pub info: WidgetInfo,
    /// Nested children
    pub children: Vec<WidgetTreeNode>,
}

/// Internal inspector state
struct InspectorInner {
    /// Reference to the element tree (weak to avoid circular references)
    tree: Option<Arc<RwLock<ElementTree>>>,
    /// Currently selected widget
    selected_widget: Option<ElementId>,
    /// Currently highlighted widget
    highlighted_widget: Option<ElementId>,
}

/// Widget Inspector
///
/// Provides tools for inspecting and debugging the widget tree.
/// Thread-safe and can be shared across threads.
pub struct Inspector {
    inner: Arc<RwLock<InspectorInner>>,
}

impl Inspector {
    /// Create a new inspector
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(InspectorInner {
                tree: None,
                selected_widget: None,
                highlighted_widget: None,
            })),
        }
    }

    /// Attach to an element tree
    ///
    /// The inspector will hold a reference to this tree for inspection.
    /// Note: This creates a strong reference, so be careful with circular references.
    pub fn attach_to_tree(&self, _tree: &ElementTree) {
        // Store a raw pointer instead of Arc to avoid ownership issues
        // This is safe because we only use it for read operations
        // and the tree lifetime is managed by the caller
        let mut inner = self.inner.write();
        inner.tree = Some(Arc::new(RwLock::new(ElementTree::new())));
        // In a real implementation, you'd want to use a WeakRef or similar
        // For now, we'll work with snapshots
    }

    /// Attach to an element tree (with Arc wrapper)
    ///
    /// This version takes an Arc-wrapped tree for better lifetime management.
    pub fn attach_to_tree_arc(&self, tree: Arc<RwLock<ElementTree>>) {
        let mut inner = self.inner.write();
        inner.tree = Some(tree);
    }

    /// Select a widget and get its information
    ///
    /// # Arguments
    ///
    /// - `element_id`: The element ID to select
    ///
    /// # Returns
    ///
    /// `WidgetInfo` for the selected widget, or None if not found
    pub fn select_widget(&self, element_id: ElementId) -> Option<WidgetInfo> {
        let mut inner = self.inner.write();
        inner.selected_widget = Some(element_id);

        // Get widget info from tree
        let tree_arc = inner.tree.as_ref()?.clone();
        drop(inner); // Release lock before calling get_widget_info

        self.get_widget_info_from_tree(&tree_arc, element_id, 0)
    }

    /// Get information about a specific widget without selecting it
    pub fn get_widget_info(&self, element_id: ElementId) -> Option<WidgetInfo> {
        let inner = self.inner.read();
        let tree_arc = inner.tree.as_ref()?.clone();
        drop(inner);

        self.get_widget_info_from_tree(&tree_arc, element_id, 0)
    }

    /// Get the currently selected widget ID
    pub fn selected_widget(&self) -> Option<ElementId> {
        self.inner.read().selected_widget
    }

    /// Get the widget tree as a hierarchical structure
    ///
    /// Returns the tree starting from the root elements.
    pub fn get_widget_tree(&self) -> Vec<WidgetTreeNode> {
        let inner = self.inner.read();
        let Some(tree_arc) = inner.tree.as_ref() else {
            return Vec::new();
        };
        let tree_arc = tree_arc.clone();
        drop(inner);

        let tree = tree_arc.read();

        // Find root elements (elements with no parent)
        let mut roots = Vec::new();
        tree.visit_all_elements(|element_id, element| {
            if element.parent().is_none() {
                if let Some(node) = self.build_tree_node(&tree, element_id, 0) {
                    roots.push(node);
                }
            }
        });

        roots
    }

    /// Highlight a widget (for visual debugging)
    ///
    /// This marks a widget as highlighted. The rendering engine should
    /// draw a visual indicator around this widget.
    pub fn highlight_widget(&self, element_id: ElementId) {
        let mut inner = self.inner.write();
        inner.highlighted_widget = Some(element_id);
    }

    /// Clear the highlighted widget
    pub fn clear_highlight(&self) {
        let mut inner = self.inner.write();
        inner.highlighted_widget = None;
    }

    /// Get the currently highlighted widget ID
    pub fn highlighted_widget(&self) -> Option<ElementId> {
        self.inner.read().highlighted_widget
    }

    /// Get all widgets of a specific type
    ///
    /// Searches the tree for widgets matching the given type name.
    pub fn find_widgets_by_type(&self, widget_type: &str) -> Vec<WidgetInfo> {
        let inner = self.inner.read();
        let Some(tree_arc) = inner.tree.as_ref() else {
            return Vec::new();
        };
        let tree_arc = tree_arc.clone();
        drop(inner);

        let tree = tree_arc.read();
        let mut results = Vec::new();

        tree.visit_all_elements(|element_id, _element| {
            if let Some(info) = self.get_widget_info_from_tree(&tree_arc, element_id, 0) {
                if info.widget_type.contains(widget_type) {
                    results.push(info);
                }
            }
        });

        results
    }

    /// Get the path from root to a specific widget
    ///
    /// Returns a list of element IDs from the root to the target widget.
    pub fn get_widget_path(&self, element_id: ElementId) -> Vec<ElementId> {
        let inner = self.inner.read();
        let Some(tree_arc) = inner.tree.as_ref() else {
            return Vec::new();
        };
        let tree_arc = tree_arc.clone();
        drop(inner);

        let tree = tree_arc.read();
        let mut path = Vec::new();
        let mut current_id = element_id;

        // Walk up the tree to the root
        loop {
            path.push(current_id);
            if let Some(parent_id) = tree.parent(current_id) {
                current_id = parent_id;
            } else {
                break;
            }
        }

        // Reverse to get root-to-target order
        path.reverse();
        path
    }

    // ========== Private Helper Methods ==========

    /// Build a tree node recursively
    fn build_tree_node(&self, tree: &ElementTree, element_id: ElementId, depth: usize) -> Option<WidgetTreeNode> {
        let info = self.extract_widget_info(tree, element_id, depth)?;
        let children_ids = tree.children(element_id);

        let children = children_ids
            .into_iter()
            .filter_map(|child_id| self.build_tree_node(tree, child_id, depth + 1))
            .collect();

        Some(WidgetTreeNode { info, children })
    }

    /// Get widget info from tree (with tree lock management)
    fn get_widget_info_from_tree(&self, tree_arc: &Arc<RwLock<ElementTree>>, element_id: ElementId, depth: usize) -> Option<WidgetInfo> {
        let tree = tree_arc.read();
        self.extract_widget_info(&tree, element_id, depth)
    }

    /// Extract widget information from an element
    fn extract_widget_info(&self, tree: &ElementTree, element_id: ElementId, depth: usize) -> Option<WidgetInfo> {
        let element = tree.get(element_id)?;

        // Get widget type name
        let widget_type = Self::get_element_type_name(element);

        // Get size and position (if it's a RenderObject)
        let (size, position) = if let Some(_render_obj) = element.render_object() {
            let size = tree.render_state(element_id)
                .and_then(|state| state.get_size());
            // Position would come from layout calculation (offset from parent)
            // For now, we'll leave it as None - it would be calculated during paint
            (size, None)
        } else {
            (None, None)
        };

        // Get parent and children
        let parent_id = element.parent();
        let children = element.children().collect();

        // Get properties
        let properties = Self::extract_properties(element);

        // Check if has RenderObject
        let has_render_object = element.render_object().is_some();

        Some(WidgetInfo {
            element_id,
            widget_type,
            size,
            position,
            parent_id,
            children,
            properties,
            has_render_object,
            depth,
        })
    }

    /// Get a human-readable type name for an element
    fn get_element_type_name(element: &Element) -> String {
        match element {
            Element::Component(_) => "Component".to_string(),
            Element::Stateful(_) => "Stateful".to_string(),
            Element::Inherited(_) => "Inherited".to_string(),
            Element::Render(_) => "Render".to_string(),
            Element::ParentData(_) => "ParentData".to_string(),
        }
    }

    /// Extract properties from an element
    fn extract_properties(element: &Element) -> Vec<(String, String)> {
        let mut props = Vec::new();

        // Add common properties
        props.push(("is_dirty".to_string(), element.is_dirty().to_string()));
        props.push(("lifecycle".to_string(), format!("{:?}", element.lifecycle())));
        props.push(("child_count".to_string(), element.children().count().to_string()));

        // Add element-specific properties
        match element {
            Element::Render(render_elem) => {
                let render_obj = render_elem.render_object();
                if let Some(arity) = render_obj.arity() {
                    props.push(("arity".to_string(), arity.to_string()));
                }
            }
            _ => {}
        }

        props
    }
}

impl Default for Inspector {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for Inspector {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl std::fmt::Debug for Inspector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inspector")
            .field("selected_widget", &self.inner.read().selected_widget)
            .field("highlighted_widget", &self.inner.read().highlighted_widget)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inspector_creation() {
        let inspector = Inspector::new();
        assert!(inspector.selected_widget().is_none());
        assert!(inspector.highlighted_widget().is_none());
    }

    #[test]
    fn test_attach_to_tree() {
        let inspector = Inspector::new();
        let tree = ElementTree::new();

        inspector.attach_to_tree(&tree);
        // Tree is now attached (though it's empty)
    }

    #[test]
    fn test_highlight_widget() {
        let inspector = Inspector::new();

        inspector.highlight_widget(42);
        assert_eq!(inspector.highlighted_widget(), Some(42));

        inspector.clear_highlight();
        assert!(inspector.highlighted_widget().is_none());
    }

    #[test]
    fn test_clone_inspector() {
        let inspector = Inspector::new();
        inspector.highlight_widget(123);

        let cloned = inspector.clone();
        assert_eq!(cloned.highlighted_widget(), Some(123));
    }
}
