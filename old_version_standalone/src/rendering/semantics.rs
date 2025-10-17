//! Semantic annotations for accessibility
//!
//! This module provides semantic information about UI elements
//! for screen readers and other accessibility tools.
//!
//! Similar to Flutter's SemanticsNode system.

use crate::types::core::Rect;
use std::collections::HashMap;

/// Text direction for semantic labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextDirection {
    /// Left-to-right text
    Ltr,
    /// Right-to-left text
    Rtl,
}

impl Default for TextDirection {
    fn default() -> Self {
        TextDirection::Ltr
    }
}

/// Semantic actions that can be performed on a UI element.
///
/// Similar to Flutter's `SemanticsAction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SemanticsAction {
    /// Tap/click action
    Tap,
    /// Long press action
    LongPress,
    /// Scroll left
    ScrollLeft,
    /// Scroll right
    ScrollRight,
    /// Scroll up
    ScrollUp,
    /// Scroll down
    ScrollDown,
    /// Increase value (for sliders, etc.)
    Increase,
    /// Decrease value (for sliders, etc.)
    Decrease,
    /// Show tooltip
    ShowTooltip,
    /// Move cursor to previous
    MoveCursorBackward,
    /// Move cursor to next
    MoveCursorForward,
    /// Set selection
    SetSelection,
    /// Copy
    Copy,
    /// Cut
    Cut,
    /// Paste
    Paste,
    /// Dismiss
    Dismiss,
}

/// Semantic flags that describe properties of a UI element.
///
/// Similar to Flutter's `SemanticsFlag`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SemanticsFlag {
    /// Element has been checked (checkbox, radio button)
    IsChecked,
    /// Element is selected (in a list, etc.)
    IsSelected,
    /// Element is a button
    IsButton,
    /// Element is a text field
    IsTextField,
    /// Element is focused
    IsFocused,
    /// Element is in a mutually exclusive group
    IsInMutuallyExclusiveGroup,
    /// Element is a header
    IsHeader,
    /// Element is obscured (password field)
    IsObscured,
    /// Element is a link
    IsLink,
    /// Element is an image
    IsImage,
    /// Element is live region (announces changes)
    IsLiveRegion,
    /// Element is read-only
    IsReadOnly,
    /// Element is disabled
    IsDisabled,
    /// Element is hidden from accessibility
    IsHidden,
}

/// Data associated with a semantic node.
///
/// Similar to Flutter's `SemanticsData`.
#[derive(Debug, Clone, PartialEq)]
pub struct SemanticsData {
    /// Semantic flags for this node
    pub flags: Vec<SemanticsFlag>,

    /// Supported actions for this node
    pub actions: Vec<SemanticsAction>,

    /// Label describing this node (read by screen readers)
    pub label: Option<String>,

    /// Current value (for inputs, sliders, etc.)
    pub value: Option<String>,

    /// Hint for how to interact with this node
    pub hint: Option<String>,

    /// Error message if validation failed
    pub error: Option<String>,

    /// Tooltip text
    pub tooltip: Option<String>,

    /// Text direction for label/value
    pub text_direction: TextDirection,

    /// Rectangle describing the node's position
    pub rect: Option<Rect>,

    /// Unique identifier for this node
    pub id: usize,

    /// Parent node ID
    pub parent_id: Option<usize>,

    /// Child node IDs
    pub children_ids: Vec<usize>,
}

impl Default for SemanticsData {
    fn default() -> Self {
        Self {
            flags: Vec::new(),
            actions: Vec::new(),
            label: None,
            value: None,
            hint: None,
            error: None,
            tooltip: None,
            text_direction: TextDirection::default(),
            rect: None,
            id: 0,
            parent_id: None,
            children_ids: Vec::new(),
        }
    }
}

impl SemanticsData {
    /// Create a new semantic node with a label.
    pub fn labeled(label: impl Into<String>) -> Self {
        Self {
            label: Some(label.into()),
            ..Default::default()
        }
    }

    /// Create a new semantic node for a button.
    pub fn button(label: impl Into<String>) -> Self {
        Self {
            label: Some(label.into()),
            flags: vec![SemanticsFlag::IsButton],
            actions: vec![SemanticsAction::Tap],
            ..Default::default()
        }
    }

    /// Create a new semantic node for a text field.
    pub fn text_field(label: Option<String>, value: Option<String>) -> Self {
        Self {
            label,
            value,
            flags: vec![SemanticsFlag::IsTextField],
            actions: vec![
                SemanticsAction::Tap,
                SemanticsAction::MoveCursorForward,
                SemanticsAction::MoveCursorBackward,
            ],
            ..Default::default()
        }
    }

    /// Add a flag to this node.
    pub fn with_flag(mut self, flag: SemanticsFlag) -> Self {
        if !self.flags.contains(&flag) {
            self.flags.push(flag);
        }
        self
    }

    /// Add an action to this node.
    pub fn with_action(mut self, action: SemanticsAction) -> Self {
        if !self.actions.contains(&action) {
            self.actions.push(action);
        }
        self
    }

    /// Set the value for this node.
    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// Set the hint for this node.
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Set the tooltip for this node.
    pub fn with_tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// Check if this node has a specific flag.
    pub fn has_flag(&self, flag: SemanticsFlag) -> bool {
        self.flags.contains(&flag)
    }

    /// Check if this node supports a specific action.
    pub fn supports_action(&self, action: SemanticsAction) -> bool {
        self.actions.contains(&action)
    }

    /// Get the full announcement string for screen readers.
    ///
    /// This combines label, value, hint, and error into a single string.
    pub fn announcement(&self) -> String {
        let mut parts = Vec::new();

        if let Some(label) = &self.label {
            parts.push(label.clone());
        }

        if let Some(value) = &self.value {
            parts.push(value.clone());
        }

        if let Some(hint) = &self.hint {
            parts.push(hint.clone());
        }

        if let Some(error) = &self.error {
            parts.push(format!("Error: {}", error));
        }

        parts.join(", ")
    }
}

/// A semantic node representing a UI element in the semantic tree.
///
/// Similar to Flutter's `SemanticsNode`.
#[derive(Debug, Clone)]
pub struct SemanticsNode {
    /// The semantic data for this node
    pub data: SemanticsData,

    /// Child nodes
    pub children: Vec<SemanticsNode>,
}

impl SemanticsNode {
    /// Create a new semantic node with the given data.
    pub fn new(data: SemanticsData) -> Self {
        Self {
            data,
            children: Vec::new(),
        }
    }

    /// Add a child node.
    pub fn add_child(&mut self, child: SemanticsNode) {
        self.children.push(child);
    }

    /// Visit all nodes in the tree (depth-first).
    pub fn visit<F>(&self, f: &mut F)
    where
        F: FnMut(&SemanticsNode),
    {
        f(self);
        for child in &self.children {
            child.visit(f);
        }
    }

    /// Find a node by ID.
    pub fn find_by_id(&self, id: usize) -> Option<&SemanticsNode> {
        if self.data.id == id {
            return Some(self);
        }

        for child in &self.children {
            if let Some(node) = child.find_by_id(id) {
                return Some(node);
            }
        }

        None
    }

    /// Get all nodes that support a specific action.
    pub fn find_by_action(&self, action: SemanticsAction) -> Vec<usize> {
        let mut result = Vec::new();

        self.visit(&mut |node| {
            if node.data.supports_action(action) {
                result.push(node.data.id);
            }
        });

        result
    }
}

/// A manager for the semantic tree.
///
/// This keeps track of all semantic nodes and their relationships.
pub struct SemanticsManager {
    /// Root semantic nodes
    pub roots: Vec<SemanticsNode>,

    /// Mapping from ID to node path (for quick lookup)
    node_map: HashMap<usize, Vec<usize>>,

    /// Next available node ID
    next_id: usize,
}

impl Default for SemanticsManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SemanticsManager {
    /// Create a new semantics manager.
    pub fn new() -> Self {
        Self {
            roots: Vec::new(),
            node_map: HashMap::new(),
            next_id: 1,
        }
    }

    /// Generate a new unique node ID.
    pub fn next_id(&mut self) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Add a root semantic node.
    pub fn add_root(&mut self, node: SemanticsNode) {
        self.roots.push(node);
        self.rebuild_map();
    }

    /// Clear all semantic nodes.
    pub fn clear(&mut self) {
        self.roots.clear();
        self.node_map.clear();
    }

    /// Rebuild the internal node map for quick lookups.
    fn rebuild_map(&mut self) {
        self.node_map.clear();
        // TODO: Implement path-based lookup
    }

    /// Get announcement for all root nodes.
    pub fn announce_all(&self) -> Vec<String> {
        self.roots
            .iter()
            .map(|node| node.data.announcement())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semantics_data_button() {
        let button = SemanticsData::button("Click me");

        assert_eq!(button.label, Some("Click me".to_string()));
        assert!(button.has_flag(SemanticsFlag::IsButton));
        assert!(button.supports_action(SemanticsAction::Tap));
    }

    #[test]
    fn test_semantics_data_text_field() {
        let field = SemanticsData::text_field(
            Some("Name".to_string()),
            Some("John".to_string()),
        );

        assert_eq!(field.label, Some("Name".to_string()));
        assert_eq!(field.value, Some("John".to_string()));
        assert!(field.has_flag(SemanticsFlag::IsTextField));
    }

    #[test]
    fn test_semantics_data_builder() {
        let data = SemanticsData::labeled("Test")
            .with_flag(SemanticsFlag::IsFocused)
            .with_action(SemanticsAction::Tap)
            .with_hint("Tap to activate");

        assert_eq!(data.label, Some("Test".to_string()));
        assert!(data.has_flag(SemanticsFlag::IsFocused));
        assert!(data.supports_action(SemanticsAction::Tap));
        assert_eq!(data.hint, Some("Tap to activate".to_string()));
    }

    #[test]
    fn test_semantics_announcement() {
        let data = SemanticsData::labeled("Submit")
            .with_value("Ready")
            .with_hint("Press to submit form");

        let announcement = data.announcement();
        assert!(announcement.contains("Submit"));
        assert!(announcement.contains("Ready"));
        assert!(announcement.contains("Press to submit form"));
    }

    #[test]
    fn test_semantics_node_tree() {
        let mut root = SemanticsNode::new(SemanticsData::labeled("Root"));
        let child1 = SemanticsNode::new(SemanticsData::button("Button 1"));
        let child2 = SemanticsNode::new(SemanticsData::button("Button 2"));

        root.add_child(child1);
        root.add_child(child2);

        assert_eq!(root.children.len(), 2);

        let mut count = 0;
        root.visit(&mut |_| count += 1);
        assert_eq!(count, 3); // root + 2 children
    }

    #[test]
    fn test_semantics_manager() {
        let mut manager = SemanticsManager::new();

        let id1 = manager.next_id();
        let id2 = manager.next_id();

        assert_eq!(id1, 1);
        assert_eq!(id2, 2);

        let node = SemanticsNode::new(SemanticsData::button("Test"));
        manager.add_root(node);

        assert_eq!(manager.roots.len(), 1);
    }
}
