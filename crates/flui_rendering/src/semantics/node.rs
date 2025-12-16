//! Semantics nodes representing accessible elements.
//!
//! This module provides the core `SemanticsNode` type which represents
//! a single accessible element in the semantics tree.

use std::collections::HashSet;
use std::num::NonZeroU64;

use flui_types::{Matrix4, Rect};

use super::{
    AttributedString, SemanticsAction, SemanticsConfiguration, SemanticsSortKey, SemanticsTag,
    TextDirection,
};

// ============================================================================
// SemanticsNodeId
// ============================================================================

/// Unique identifier for a semantics node.
///
/// Uses NonZeroU64 internally for niche optimization (Option<SemanticsNodeId> = 8 bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SemanticsNodeId(NonZeroU64);

impl SemanticsNodeId {
    /// The root node ID (always 0 in the protocol, but we use 1 internally).
    pub const ROOT: Self = Self(unsafe { NonZeroU64::new_unchecked(1) });

    /// Creates a new node ID from a u64.
    ///
    /// Returns None if the value is 0.
    pub fn new(value: u64) -> Option<Self> {
        NonZeroU64::new(value).map(Self)
    }

    /// Creates a new node ID, adding 1 to support 0-based external IDs.
    pub fn from_index(index: u64) -> Self {
        Self(NonZeroU64::new(index + 1).expect("index + 1 should not overflow"))
    }

    /// Returns the raw value.
    pub fn get(&self) -> u64 {
        self.0.get()
    }

    /// Returns the 0-based index (for external APIs).
    pub fn to_index(&self) -> u64 {
        self.0.get() - 1
    }
}

impl std::fmt::Display for SemanticsNodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SemanticsNode({})", self.to_index())
    }
}

// ============================================================================
// SemanticsNode
// ============================================================================

/// A node in the semantics tree representing an accessible element.
///
/// Semantics nodes form a tree structure that mirrors the render tree
/// but with semantic information for assistive technologies.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `SemanticsNode` class.
#[derive(Debug)]
pub struct SemanticsNode {
    /// Unique identifier for this node.
    id: SemanticsNodeId,

    /// The depth in the tree (used for dirty management).
    depth: u32,

    /// Parent node ID.
    parent: Option<SemanticsNodeId>,

    /// Child node IDs.
    children: Vec<SemanticsNodeId>,

    /// Whether this node has been merged from multiple render objects.
    is_merged_into_parent: bool,

    /// Whether this node represents a semantic boundary.
    is_part_of_node_merging: bool,

    /// The configuration for this node.
    config: SemanticsConfiguration,

    /// The bounding rectangle in local coordinates.
    rect: Rect,

    /// Transform from local to parent coordinates.
    transform: Matrix4,

    /// Tags attached to this node.
    tags: HashSet<SemanticsTag>,

    /// Whether this node is dirty and needs update.
    is_dirty: bool,

    /// Whether the node's children have changed.
    children_dirty: bool,
}

impl SemanticsNode {
    /// Creates a new semantics node with the given ID.
    pub fn new(id: SemanticsNodeId) -> Self {
        Self {
            id,
            depth: 0,
            parent: None,
            children: Vec::new(),
            is_merged_into_parent: false,
            is_part_of_node_merging: false,
            config: SemanticsConfiguration::new(),
            rect: Rect::ZERO,
            transform: Matrix4::IDENTITY,
            tags: HashSet::new(),
            is_dirty: true,
            children_dirty: true,
        }
    }

    /// Creates a root semantics node.
    pub fn root() -> Self {
        Self::new(SemanticsNodeId::ROOT)
    }

    // ========================================================================
    // Identity
    // ========================================================================

    /// Returns the node ID.
    pub fn id(&self) -> SemanticsNodeId {
        self.id
    }

    /// Returns the depth in the tree.
    pub fn depth(&self) -> u32 {
        self.depth
    }

    /// Sets the depth.
    pub fn set_depth(&mut self, depth: u32) {
        self.depth = depth;
    }

    // ========================================================================
    // Tree Structure
    // ========================================================================

    /// Returns the parent node ID.
    pub fn parent(&self) -> Option<SemanticsNodeId> {
        self.parent
    }

    /// Sets the parent node ID.
    pub fn set_parent(&mut self, parent: Option<SemanticsNodeId>) {
        self.parent = parent;
    }

    /// Returns the child node IDs.
    pub fn children(&self) -> &[SemanticsNodeId] {
        &self.children
    }

    /// Returns whether this node has children.
    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    /// Returns the number of children.
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Adds a child node.
    pub fn add_child(&mut self, child_id: SemanticsNodeId) {
        self.children.push(child_id);
        self.children_dirty = true;
    }

    /// Removes a child node.
    pub fn remove_child(&mut self, child_id: SemanticsNodeId) -> bool {
        if let Some(pos) = self.children.iter().position(|id| *id == child_id) {
            self.children.remove(pos);
            self.children_dirty = true;
            true
        } else {
            false
        }
    }

    /// Clears all children.
    pub fn clear_children(&mut self) {
        if !self.children.is_empty() {
            self.children.clear();
            self.children_dirty = true;
        }
    }

    /// Replaces all children with the given list.
    pub fn set_children(&mut self, children: Vec<SemanticsNodeId>) {
        self.children = children;
        self.children_dirty = true;
    }

    // ========================================================================
    // Merging
    // ========================================================================

    /// Returns whether this node is merged into its parent.
    pub fn is_merged_into_parent(&self) -> bool {
        self.is_merged_into_parent
    }

    /// Sets whether this node is merged into its parent.
    pub fn set_merged_into_parent(&mut self, value: bool) {
        self.is_merged_into_parent = value;
    }

    /// Returns whether this node participates in merging.
    pub fn is_part_of_node_merging(&self) -> bool {
        self.is_part_of_node_merging
    }

    /// Sets whether this node participates in merging.
    pub fn set_part_of_node_merging(&mut self, value: bool) {
        self.is_part_of_node_merging = value;
    }

    // ========================================================================
    // Configuration
    // ========================================================================

    /// Returns the configuration.
    pub fn config(&self) -> &SemanticsConfiguration {
        &self.config
    }

    /// Returns a mutable reference to the configuration.
    pub fn config_mut(&mut self) -> &mut SemanticsConfiguration {
        self.is_dirty = true;
        &mut self.config
    }

    /// Replaces the configuration.
    pub fn set_config(&mut self, config: SemanticsConfiguration) {
        self.config = config;
        self.is_dirty = true;
    }

    /// Updates the configuration from a closure.
    pub fn update_config<F>(&mut self, f: F)
    where
        F: FnOnce(&mut SemanticsConfiguration),
    {
        f(&mut self.config);
        self.is_dirty = true;
    }

    // ========================================================================
    // Geometry
    // ========================================================================

    /// Returns the local bounding rectangle.
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// Sets the local bounding rectangle.
    pub fn set_rect(&mut self, rect: Rect) {
        if self.rect != rect {
            self.rect = rect;
            self.is_dirty = true;
        }
    }

    /// Returns the transform from local to parent coordinates.
    pub fn transform(&self) -> &Matrix4 {
        &self.transform
    }

    /// Sets the transform.
    pub fn set_transform(&mut self, transform: Matrix4) {
        if self.transform != transform {
            self.transform = transform;
            self.is_dirty = true;
        }
    }

    /// Returns the semantic bounds in the parent coordinate system.
    pub fn semantic_bounds(&self) -> Rect {
        // Transform the rect by the matrix
        let (min_x, min_y) = self
            .transform
            .transform_point(self.rect.min.x, self.rect.min.y);
        let (max_x, max_y) = self
            .transform
            .transform_point(self.rect.max.x, self.rect.max.y);
        Rect::new(min_x, min_y, max_x, max_y)
    }

    // ========================================================================
    // Tags
    // ========================================================================

    /// Returns the tags.
    pub fn tags(&self) -> &HashSet<SemanticsTag> {
        &self.tags
    }

    /// Adds a tag.
    pub fn add_tag(&mut self, tag: SemanticsTag) {
        self.tags.insert(tag);
        self.is_dirty = true;
    }

    /// Removes a tag.
    pub fn remove_tag(&mut self, tag: &SemanticsTag) -> bool {
        let removed = self.tags.remove(tag);
        if removed {
            self.is_dirty = true;
        }
        removed
    }

    /// Returns whether a tag is present.
    pub fn has_tag(&self, tag: &SemanticsTag) -> bool {
        self.tags.contains(tag)
    }

    /// Replaces all tags.
    pub fn set_tags(&mut self, tags: HashSet<SemanticsTag>) {
        self.tags = tags;
        self.is_dirty = true;
    }

    // ========================================================================
    // Dirty State
    // ========================================================================

    /// Returns whether this node is dirty.
    pub fn is_dirty(&self) -> bool {
        self.is_dirty
    }

    /// Marks this node as dirty.
    pub fn mark_dirty(&mut self) {
        self.is_dirty = true;
    }

    /// Clears the dirty flag.
    pub fn clear_dirty(&mut self) {
        self.is_dirty = false;
    }

    /// Returns whether children are dirty.
    pub fn children_dirty(&self) -> bool {
        self.children_dirty
    }

    /// Clears the children dirty flag.
    pub fn clear_children_dirty(&mut self) {
        self.children_dirty = false;
    }

    // ========================================================================
    // Convenience Accessors (delegating to config)
    // ========================================================================

    /// Returns the label.
    pub fn label(&self) -> Option<&AttributedString> {
        self.config.label()
    }

    /// Returns the value.
    pub fn value(&self) -> Option<&AttributedString> {
        self.config.value()
    }

    /// Returns the hint.
    pub fn hint(&self) -> Option<&AttributedString> {
        self.config.hint()
    }

    /// Returns whether this is a button.
    pub fn is_button(&self) -> bool {
        self.config.is_button()
    }

    /// Returns whether this has actions.
    pub fn has_actions(&self) -> bool {
        self.config.actions_as_bits() != 0
    }

    /// Returns whether this has a specific action.
    pub fn has_action(&self, action: SemanticsAction) -> bool {
        self.config.has_action(action)
    }

    /// Returns the text direction.
    pub fn text_direction(&self) -> Option<TextDirection> {
        self.config.text_direction()
    }

    /// Returns the sort key.
    pub fn sort_key(&self) -> Option<&SemanticsSortKey> {
        self.config.sort_key()
    }

    // ========================================================================
    // Serialization
    // ========================================================================

    /// Converts this node to a data structure suitable for platform API.
    pub fn to_platform_data(&self) -> SemanticsNodeData {
        SemanticsNodeData {
            id: self.id.to_index(),
            flags: self.config.flags().bits(),
            actions: self.config.actions_as_bits(),
            label: self.config.label().map(|l| l.string.to_string()),
            value: self.config.value().map(|v| v.string.to_string()),
            increased_value: self.config.increased_value().map(|v| v.string.to_string()),
            decreased_value: self.config.decreased_value().map(|v| v.string.to_string()),
            hint: self.config.hint().map(|h| h.string.to_string()),
            tooltip: self.config.tooltip().map(|t| t.to_string()),
            text_direction: self.config.text_direction(),
            rect: self.rect,
            transform: self.transform,
            children: self.children.iter().map(|c| c.to_index()).collect(),
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
}

// ============================================================================
// SemanticsNodeData
// ============================================================================

/// Serialized data for a semantics node, suitable for sending to the platform.
///
/// This is the format used when communicating with the platform's accessibility API.
#[derive(Debug, Clone)]
pub struct SemanticsNodeData {
    /// Node identifier.
    pub id: u64,
    /// Flags bitmask.
    pub flags: u64,
    /// Actions bitmask.
    pub actions: u64,
    /// Label text.
    pub label: Option<String>,
    /// Value text.
    pub value: Option<String>,
    /// Increased value text.
    pub increased_value: Option<String>,
    /// Decreased value text.
    pub decreased_value: Option<String>,
    /// Hint text.
    pub hint: Option<String>,
    /// Tooltip text.
    pub tooltip: Option<String>,
    /// Text direction.
    pub text_direction: Option<TextDirection>,
    /// Bounding rectangle.
    pub rect: Rect,
    /// Transform matrix.
    pub transform: Matrix4,
    /// Child node IDs.
    pub children: Vec<u64>,
    /// Elevation (z-order).
    pub elevation: f64,
    /// Thickness.
    pub thickness: f64,
    /// Platform view ID.
    pub platform_view_id: Option<i32>,
    /// Maximum value length for text fields.
    pub max_value_length: Option<i32>,
    /// Current value length for text fields.
    pub current_value_length: Option<i32>,
    /// Scroll position.
    pub scroll_position: Option<f64>,
    /// Maximum scroll extent.
    pub scroll_extent_max: Option<f64>,
    /// Minimum scroll extent.
    pub scroll_extent_min: Option<f64>,
    /// Scroll index.
    pub scroll_index: Option<i32>,
    /// Scroll child count.
    pub scroll_child_count: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantics::SemanticsFlag;

    #[test]
    fn test_node_id() {
        let id = SemanticsNodeId::from_index(0);
        assert_eq!(id.get(), 1);
        assert_eq!(id.to_index(), 0);

        let id2 = SemanticsNodeId::from_index(42);
        assert_eq!(id2.get(), 43);
        assert_eq!(id2.to_index(), 42);
    }

    #[test]
    fn test_node_id_root() {
        assert_eq!(SemanticsNodeId::ROOT.get(), 1);
        assert_eq!(SemanticsNodeId::ROOT.to_index(), 0);
    }

    #[test]
    fn test_node_creation() {
        let node = SemanticsNode::new(SemanticsNodeId::from_index(5));
        assert_eq!(node.id().to_index(), 5);
        assert!(node.is_dirty());
        assert!(!node.has_children());
    }

    #[test]
    fn test_node_children() {
        let mut node = SemanticsNode::new(SemanticsNodeId::from_index(0));

        node.add_child(SemanticsNodeId::from_index(1));
        node.add_child(SemanticsNodeId::from_index(2));

        assert_eq!(node.child_count(), 2);
        assert!(node.has_children());
        assert!(node.children_dirty());

        node.remove_child(SemanticsNodeId::from_index(1));
        assert_eq!(node.child_count(), 1);
    }

    #[test]
    fn test_node_configuration() {
        let mut node = SemanticsNode::new(SemanticsNodeId::from_index(0));

        node.config_mut().set_label("Test Button");
        node.config_mut().set_button(true);

        assert_eq!(node.label().map(|l| l.string.as_str()), Some("Test Button"));
        assert!(node.is_button());
    }

    #[test]
    fn test_node_geometry() {
        let mut node = SemanticsNode::new(SemanticsNodeId::from_index(0));

        let rect = Rect::from_ltwh(10.0, 20.0, 100.0, 50.0);
        node.set_rect(rect);

        assert_eq!(node.rect(), rect);
    }

    #[test]
    fn test_node_tags() {
        let mut node = SemanticsNode::new(SemanticsNodeId::from_index(0));

        let tag = SemanticsTag::new("test_tag");
        node.add_tag(tag.clone());

        assert!(node.has_tag(&tag));
        assert!(node.remove_tag(&tag));
        assert!(!node.has_tag(&tag));
    }

    #[test]
    fn test_node_to_platform_data() {
        let mut node = SemanticsNode::new(SemanticsNodeId::from_index(5));
        node.config_mut().set_label("Submit");
        node.config_mut().set_button(true);
        node.set_rect(Rect::from_ltwh(0.0, 0.0, 100.0, 50.0));
        node.add_child(SemanticsNodeId::from_index(6));

        let data = node.to_platform_data();

        assert_eq!(data.id, 5);
        assert_eq!(data.label, Some("Submit".to_string()));
        assert!((data.flags & SemanticsFlag::IsButton.value()) != 0);
        assert_eq!(data.children, vec![6]);
    }

    #[test]
    fn test_node_dirty_tracking() {
        let mut node = SemanticsNode::new(SemanticsNodeId::from_index(0));
        assert!(node.is_dirty());

        node.clear_dirty();
        assert!(!node.is_dirty());

        node.set_rect(Rect::from_ltwh(0.0, 0.0, 50.0, 50.0));
        assert!(node.is_dirty());
    }
}
