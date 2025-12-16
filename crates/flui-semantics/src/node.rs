//! SemanticsNode - Individual node in the semantics tree
//!
//! Each semantics node represents accessible content and corresponds to
//! one or more render objects. Non-boundary render objects merge their
//! semantics into the nearest boundary ancestor.

use flui_foundation::{ElementId, SemanticsId};
use flui_types::geometry::Rect;
use flui_types::Matrix4;

// Use our optimized types from flui-semantics
use crate::configuration::SemanticsConfiguration;
use crate::update::SemanticsNodeData;

// ============================================================================
// SEMANTICS NODE
// ============================================================================

/// A node in the semantics tree.
///
/// Each semantics node corresponds to one or more render objects in the
/// render tree. Non-boundary render objects are merged into their parent
/// semantics boundary.
///
/// # Flutter Protocol
///
/// Similar to Flutter's `SemanticsNode`:
/// - Properties for screen readers (label, hint, value)
/// - Supported actions (tap, scroll, increase/decrease)
/// - Geometry for spatial navigation
/// - Tree structure (parent, children)
///
/// # Example
///
/// ```rust
/// use flui_semantics::{SemanticsNode, SemanticsConfiguration, SemanticsAction};
/// use std::sync::Arc;
///
/// let mut node = SemanticsNode::new();
/// node.config_mut().set_label("Submit");
/// node.config_mut().set_button(true);
/// node.config_mut().add_action(SemanticsAction::Tap, Arc::new(|_, _| {}));
///
/// assert!(node.has_content());
/// assert!(node.config().has_action(SemanticsAction::Tap));
/// ```
#[derive(Debug, Clone, Default)]
pub struct SemanticsNode {
    // ========== Tree Structure ==========
    /// Parent node ID (None for root).
    parent: Option<SemanticsId>,

    /// Child node IDs.
    children: Vec<SemanticsId>,

    // ========== Cross-tree Reference ==========
    /// The render element that owns this semantics node.
    element_id: Option<ElementId>,

    // ========== Semantic Configuration ==========
    /// Full semantic configuration (label, flags, actions, etc.).
    config: SemanticsConfiguration,

    // ========== Geometry ==========
    /// Bounding rectangle in global coordinates.
    rect: Rect,

    /// Transform matrix (stored as 4x4 column-major).
    transform: Option<[f32; 16]>,

    // ========== State ==========
    /// Whether this node is marked dirty and needs update.
    dirty: bool,
}

impl SemanticsNode {
    /// Creates a new empty semantics node.
    pub fn new() -> Self {
        Self {
            parent: None,
            children: Vec::new(),
            element_id: None,
            config: SemanticsConfiguration::new(),
            rect: Rect::ZERO,
            transform: None,
            dirty: true,
        }
    }

    /// Creates a node with an associated element ID.
    pub fn with_element_id(mut self, element_id: ElementId) -> Self {
        self.element_id = Some(element_id);
        self
    }

    /// Creates a node with a configuration.
    pub fn with_config(mut self, config: SemanticsConfiguration) -> Self {
        self.config = config;
        self
    }

    // ========== Tree Structure ==========

    /// Returns the parent node ID.
    #[inline]
    pub fn parent(&self) -> Option<SemanticsId> {
        self.parent
    }

    /// Sets the parent node ID.
    pub fn set_parent(&mut self, parent: Option<SemanticsId>) {
        self.parent = parent;
    }

    /// Returns the child node IDs.
    #[inline]
    pub fn children(&self) -> &[SemanticsId] {
        &self.children
    }

    /// Adds a child node ID.
    pub fn add_child(&mut self, child: SemanticsId) {
        if !self.children.contains(&child) {
            self.children.push(child);
        }
    }

    /// Removes a child node ID.
    pub fn remove_child(&mut self, child: SemanticsId) {
        self.children.retain(|&id| id != child);
    }

    /// Clears all children.
    pub fn clear_children(&mut self) {
        self.children.clear();
    }

    // ========== Cross-tree Reference ==========

    /// Returns the associated element ID.
    #[inline]
    pub fn element_id(&self) -> Option<ElementId> {
        self.element_id
    }

    /// Sets the associated element ID.
    pub fn set_element_id(&mut self, element_id: Option<ElementId>) {
        self.element_id = element_id;
    }

    // ========== Semantic Configuration ==========

    /// Returns the semantic configuration.
    #[inline]
    pub fn config(&self) -> &SemanticsConfiguration {
        &self.config
    }

    /// Returns mutable reference to semantic configuration.
    #[inline]
    pub fn config_mut(&mut self) -> &mut SemanticsConfiguration {
        self.dirty = true;
        &mut self.config
    }

    /// Sets the semantic configuration.
    pub fn set_config(&mut self, config: SemanticsConfiguration) {
        self.config = config;
        self.dirty = true;
    }

    /// Returns the label text.
    #[inline]
    pub fn label(&self) -> Option<&str> {
        self.config.label().map(|l| l.string.as_str())
    }

    /// Returns the value text.
    #[inline]
    pub fn value(&self) -> Option<&str> {
        self.config.value().map(|v| v.string.as_str())
    }

    /// Returns the hint text.
    #[inline]
    pub fn hint(&self) -> Option<&str> {
        self.config.hint().map(|h| h.string.as_str())
    }

    // ========== Geometry ==========

    /// Returns the bounding rectangle.
    #[inline]
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// Sets the bounding rectangle.
    pub fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
        self.dirty = true;
    }

    /// Returns the transform matrix.
    #[inline]
    pub fn transform(&self) -> Option<&[f32; 16]> {
        self.transform.as_ref()
    }

    /// Sets the transform matrix.
    pub fn set_transform(&mut self, transform: Option<[f32; 16]>) {
        self.transform = transform;
        self.dirty = true;
    }

    // ========== State ==========

    /// Returns true if this node is dirty.
    #[inline]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Marks this node as clean.
    pub fn mark_clean(&mut self) {
        self.dirty = false;
    }

    /// Marks this node as dirty.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    // ========== Content Checking ==========

    /// Returns true if this node has semantic content.
    ///
    /// A node has semantic content if it has:
    /// - A label, value, or hint
    /// - Any flags set (button, link, etc.)
    /// - Any supported actions
    pub fn has_content(&self) -> bool {
        self.config.has_content()
    }

    /// Returns true if this node is a semantics boundary.
    ///
    /// Boundary nodes create their own semantics node in the tree.
    /// Non-boundary nodes merge into their parent boundary.
    pub fn is_semantics_boundary(&self) -> bool {
        self.config.is_semantics_boundary() || self.has_content()
    }

    // ========== Data Export ==========

    /// Converts this node to SemanticsNodeData for platform consumption.
    pub fn to_node_data(&self, id: SemanticsId) -> SemanticsNodeData {
        SemanticsNodeData {
            id: (id.get() - 1) as u64,
            flags: self.config.flags().bits(),
            actions: self.config.actions_as_bits(),
            label: self.config.label().map(|l| l.string.clone()),
            value: self.config.value().map(|v| v.string.clone()),
            increased_value: self.config.increased_value().map(|v| v.string.clone()),
            decreased_value: self.config.decreased_value().map(|v| v.string.clone()),
            hint: self.config.hint().map(|h| h.string.clone()),
            tooltip: self.config.tooltip().map(Into::into),
            text_direction: self.config.text_direction(),
            rect: self.rect,
            transform: self.transform.map_or(Matrix4::IDENTITY, Matrix4::from),
            children: self.children.iter().map(|c| (c.get() - 1) as u64).collect(),
            elevation: self.config.elevation(),
            thickness: self.config.thickness(),
            platform_view_id: self.config.platform_view_id(),
            max_value_length: self.config.max_value_length(),
            current_value_length: self.config.current_value_length(),
            scroll_position: self.config.scroll_position(),
            scroll_extent_max: self.config.scroll_extent_max(),
            scroll_extent_min: self.config.scroll_extent_min(),
            scroll_index: self.config.scroll_index(),
            scroll_child_count: self.config.scroll_child_count(),
        }
    }

    // ========== Merging ==========

    /// Merges another node's configuration into this one.
    ///
    /// Used when a non-boundary render object's semantics should be
    /// merged into its parent boundary.
    pub fn merge(&mut self, other: &SemanticsNode) {
        self.config.absorb(&other.config);

        // Expand rect to include other's rect
        if self.rect == Rect::ZERO {
            self.rect = other.rect;
        } else if other.rect != Rect::ZERO {
            self.rect = self.rect.union(other.rect);
        }

        self.dirty = true;
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::SemanticsAction;
    use crate::flags::SemanticsFlag;
    use std::sync::Arc;

    #[test]
    fn test_semantics_node_new() {
        let node = SemanticsNode::new();

        assert!(node.parent().is_none());
        assert!(node.children().is_empty());
        assert!(node.element_id().is_none());
        assert!(node.is_dirty());
        assert!(!node.has_content());
    }

    #[test]
    fn test_semantics_node_with_element_id() {
        let element_id = ElementId::new(42);
        let node = SemanticsNode::new().with_element_id(element_id);

        assert_eq!(node.element_id(), Some(element_id));
    }

    #[test]
    fn test_semantics_node_config() {
        let mut node = SemanticsNode::new();

        node.config_mut().set_label("Submit");
        node.config_mut().set_button(true);

        assert!(node.config().is_button());
        assert_eq!(node.label(), Some("Submit"));
        assert!(node.has_content());
    }

    #[test]
    fn test_semantics_node_actions() {
        let mut node = SemanticsNode::new();

        let handler: crate::SemanticsActionHandler = Arc::new(|_, _| {});
        node.config_mut()
            .add_action(SemanticsAction::Tap, handler.clone());
        node.config_mut()
            .add_action(SemanticsAction::LongPress, handler);

        assert!(node.config().has_action(SemanticsAction::Tap));
        assert!(node.config().has_action(SemanticsAction::LongPress));
        assert!(!node.config().has_action(SemanticsAction::ScrollUp));
    }

    #[test]
    fn test_semantics_node_tree_structure() {
        let mut node = SemanticsNode::new();
        let parent_id = SemanticsId::new(1);
        let child1_id = SemanticsId::new(2);
        let child2_id = SemanticsId::new(3);

        node.set_parent(Some(parent_id));
        assert_eq!(node.parent(), Some(parent_id));

        node.add_child(child1_id);
        node.add_child(child2_id);
        node.add_child(child1_id); // Duplicate - should not be added
        assert_eq!(node.children().len(), 2);

        node.remove_child(child1_id);
        assert_eq!(node.children().len(), 1);
        assert!(!node.children().contains(&child1_id));
        assert!(node.children().contains(&child2_id));

        node.clear_children();
        assert!(node.children().is_empty());
    }

    #[test]
    fn test_semantics_node_geometry() {
        let mut node = SemanticsNode::new();

        let rect = Rect::from_xywh(10.0, 20.0, 100.0, 50.0);
        node.set_rect(rect);
        assert_eq!(node.rect(), rect);

        let transform = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        node.set_transform(Some(transform));
        assert!(node.transform().is_some());
    }

    #[test]
    fn test_semantics_node_dirty_state() {
        let mut node = SemanticsNode::new();
        assert!(node.is_dirty());

        node.mark_clean();
        assert!(!node.is_dirty());

        node.mark_dirty();
        assert!(node.is_dirty());

        // Modifying config should mark dirty
        node.mark_clean();
        node.config_mut().set_label("Test");
        assert!(node.is_dirty());
    }

    #[test]
    fn test_semantics_node_merge() {
        let mut node1 = SemanticsNode::new();
        node1.config_mut().set_label("First");
        node1.config_mut().set_button(true);
        node1.set_rect(Rect::from_xywh(0.0, 0.0, 50.0, 50.0));

        let mut node2 = SemanticsNode::new();
        node2.config_mut().set_label("Second");
        node2.config_mut().set_enabled(Some(true));
        node2.set_rect(Rect::from_xywh(50.0, 0.0, 50.0, 50.0));

        node1.merge(&node2);

        assert!(node1.config().is_button());
        assert_eq!(node1.config().is_enabled(), Some(true));
        // Rect should be union
        assert_eq!(node1.rect().width(), 100.0);
    }

    #[test]
    fn test_semantics_node_to_data() {
        let mut node = SemanticsNode::new();
        node.config_mut().set_label("Test Label");
        node.config_mut().set_button(true);
        node.set_rect(Rect::from_xywh(10.0, 20.0, 100.0, 50.0));

        let id = SemanticsId::new(5);
        let data = node.to_node_data(id);

        assert_eq!(data.id, 4); // id - 1
        assert_eq!(data.label, Some("Test Label".into()));
        assert!(data.flags & SemanticsFlag::IsButton.value() != 0);
        assert_eq!(data.rect, node.rect());
    }
}
