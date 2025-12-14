//! SemanticsNode - Individual node in the semantics tree
//!
//! Each semantics node represents accessible content and corresponds to
//! one or more render objects. Non-boundary render objects merge their
//! semantics into the nearest boundary ancestor.

use flui_foundation::{ElementId, SemanticsId};
use flui_types::geometry::Rect;
use flui_types::semantics::{SemanticsAction, SemanticsData, SemanticsProperties, SemanticsRole};

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
/// use flui_semantics::{SemanticsNode, SemanticsProperties, SemanticsRole, SemanticsAction};
///
/// let mut node = SemanticsNode::new();
/// node.set_properties(
///     SemanticsProperties::new()
///         .with_role(SemanticsRole::Button)
///         .with_label("Submit")
/// );
/// node.add_action(SemanticsAction::Tap);
///
/// assert!(node.has_content());
/// assert!(node.has_action(SemanticsAction::Tap));
/// ```
#[derive(Debug, Clone)]
pub struct SemanticsNode {
    // ========== Tree Structure ==========
    /// Parent node ID (None for root).
    parent: Option<SemanticsId>,

    /// Child node IDs.
    children: Vec<SemanticsId>,

    // ========== Cross-tree Reference ==========
    /// The render element that owns this semantics node.
    element_id: Option<ElementId>,

    // ========== Semantic Properties ==========
    /// Semantic properties (label, role, etc.).
    properties: SemanticsProperties,

    /// Supported semantic actions.
    actions: Vec<SemanticsAction>,

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
            properties: SemanticsProperties::default(),
            actions: Vec::new(),
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

    /// Creates a node with properties.
    pub fn with_properties(mut self, properties: SemanticsProperties) -> Self {
        self.properties = properties;
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

    // ========== Semantic Properties ==========

    /// Returns the semantic properties.
    #[inline]
    pub fn properties(&self) -> &SemanticsProperties {
        &self.properties
    }

    /// Returns mutable reference to semantic properties.
    #[inline]
    pub fn properties_mut(&mut self) -> &mut SemanticsProperties {
        self.dirty = true;
        &mut self.properties
    }

    /// Sets the semantic properties.
    pub fn set_properties(&mut self, properties: SemanticsProperties) {
        self.properties = properties;
        self.dirty = true;
    }

    /// Returns the semantic role.
    #[inline]
    pub fn role(&self) -> SemanticsRole {
        self.properties.role()
    }

    /// Returns the label text.
    #[inline]
    pub fn label(&self) -> Option<&str> {
        self.properties.label()
    }

    /// Returns the value text.
    #[inline]
    pub fn value(&self) -> Option<&str> {
        self.properties.value()
    }

    /// Returns the hint text.
    #[inline]
    pub fn hint(&self) -> Option<&str> {
        self.properties.hint()
    }

    // ========== Actions ==========

    /// Returns the supported actions.
    #[inline]
    pub fn actions(&self) -> &[SemanticsAction] {
        &self.actions
    }

    /// Sets the supported actions.
    pub fn set_actions(&mut self, actions: Vec<SemanticsAction>) {
        self.actions = actions;
        self.dirty = true;
    }

    /// Adds an action to the supported actions.
    pub fn add_action(&mut self, action: SemanticsAction) {
        if !self.actions.contains(&action) {
            self.actions.push(action);
            self.dirty = true;
        }
    }

    /// Removes an action from the supported actions.
    pub fn remove_action(&mut self, action: SemanticsAction) {
        let len_before = self.actions.len();
        self.actions.retain(|&a| a != action);
        if self.actions.len() != len_before {
            self.dirty = true;
        }
    }

    /// Returns true if this action is supported.
    pub fn has_action(&self, action: SemanticsAction) -> bool {
        self.actions.contains(&action)
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
    /// - A role other than None
    /// - Any supported actions
    pub fn has_content(&self) -> bool {
        self.properties.has_label()
            || self.properties.has_value()
            || self.properties.has_hint()
            || self.properties.role() != SemanticsRole::None
            || !self.actions.is_empty()
    }

    /// Returns true if this node is a semantics boundary.
    ///
    /// Boundary nodes create their own semantics node in the tree.
    /// Non-boundary nodes merge into their parent boundary.
    pub fn is_semantics_boundary(&self) -> bool {
        self.has_content()
    }

    // ========== Data Export ==========

    /// Converts this node to SemanticsData for platform consumption.
    pub fn to_data(&self) -> SemanticsData {
        SemanticsData::new(self.properties.clone(), self.rect)
    }

    // ========== Merging ==========

    /// Merges another node's properties into this one.
    ///
    /// Used when a non-boundary render object's semantics should be
    /// merged into its parent boundary.
    pub fn merge(&mut self, other: &SemanticsNode) {
        // Merge label (concatenate with space)
        if let Some(other_label) = other.properties.label() {
            if let Some(self_label) = &self.properties.label {
                self.properties.label = Some(format!("{} {}", self_label, other_label));
            } else {
                self.properties.label = Some(other_label.to_string());
            }
        }

        // Merge value (prefer other if self is empty)
        if other.properties.has_value() && !self.properties.has_value() {
            self.properties.value = other.properties.value.clone();
        }

        // Merge hint (prefer other if self is empty)
        if other.properties.has_hint() && !self.properties.has_hint() {
            self.properties.hint = other.properties.hint.clone();
        }

        // Merge role (prefer more specific role)
        if self.properties.role() == SemanticsRole::None
            && other.properties.role() != SemanticsRole::None
        {
            self.properties.role = other.properties.role();
        }

        // Merge actions
        for action in &other.actions {
            self.add_action(*action);
        }

        // Expand rect to include other's rect
        if self.rect == Rect::ZERO {
            self.rect = other.rect;
        } else if other.rect != Rect::ZERO {
            self.rect = self.rect.union(other.rect);
        }

        self.dirty = true;
    }
}

impl Default for SemanticsNode {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_semantics_node_properties() {
        let mut node = SemanticsNode::new();

        let props = SemanticsProperties::new()
            .with_role(SemanticsRole::Button)
            .with_label("Submit");

        node.set_properties(props);

        assert_eq!(node.role(), SemanticsRole::Button);
        assert_eq!(node.label(), Some("Submit"));
        assert!(node.has_content());
    }

    #[test]
    fn test_semantics_node_actions() {
        let mut node = SemanticsNode::new();

        node.add_action(SemanticsAction::Tap);
        node.add_action(SemanticsAction::LongPress);
        node.add_action(SemanticsAction::Tap); // Duplicate - should not be added

        assert_eq!(node.actions().len(), 2);
        assert!(node.has_action(SemanticsAction::Tap));
        assert!(node.has_action(SemanticsAction::LongPress));
        assert!(!node.has_action(SemanticsAction::ScrollUp));

        node.remove_action(SemanticsAction::Tap);
        assert!(!node.has_action(SemanticsAction::Tap));
        assert_eq!(node.actions().len(), 1);
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

        // Modifying properties should mark dirty
        node.mark_clean();
        node.set_properties(SemanticsProperties::new().with_label("Test"));
        assert!(node.is_dirty());
    }

    #[test]
    fn test_semantics_node_merge() {
        let mut node1 = SemanticsNode::new();
        node1.set_properties(SemanticsProperties::new().with_label("First"));
        node1.add_action(SemanticsAction::Tap);
        node1.set_rect(Rect::from_xywh(0.0, 0.0, 50.0, 50.0));

        let mut node2 = SemanticsNode::new();
        node2.set_properties(
            SemanticsProperties::new()
                .with_label("Second")
                .with_role(SemanticsRole::Button),
        );
        node2.add_action(SemanticsAction::LongPress);
        node2.set_rect(Rect::from_xywh(50.0, 0.0, 50.0, 50.0));

        node1.merge(&node2);

        assert_eq!(node1.label(), Some("First Second"));
        assert_eq!(node1.role(), SemanticsRole::Button);
        assert!(node1.has_action(SemanticsAction::Tap));
        assert!(node1.has_action(SemanticsAction::LongPress));
        // Rect should be union
        assert_eq!(node1.rect().width(), 100.0);
    }

    #[test]
    fn test_semantics_node_to_data() {
        let mut node = SemanticsNode::new();
        node.set_properties(SemanticsProperties::new().with_label("Test Label"));
        node.set_rect(Rect::from_xywh(10.0, 20.0, 100.0, 50.0));

        let data = node.to_data();
        assert_eq!(data.properties.label(), Some("Test Label"));
        assert_eq!(data.rect, node.rect());
    }
}
