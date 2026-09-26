//! Semantics roles for accessibility.
//!
//! This module provides role types for accessibility semantics.
//! Roles describe the structural type of UI element and help
//! assistive technologies understand how to interact with elements.
//!
//! Note: In Flutter, interactive element types like Button, Checkbox,
//! Slider are represented as flags (`SemanticsFlag`), not roles.
//! Roles are for structural elements (tables, menus, regions, etc.).
//!
//! [`SemanticsRole`] itself lives in `flui-protocol`, the vocabulary FLUI
//! shares with tests, devtools and agents; it is re-exported here so its path
//! through this crate is unchanged.

pub use flui_protocol::SemanticsRole;

// ============================================================================
// AccessibilityFocusBlockType
// ============================================================================

/// Controls how accessibility focus is blocked.
///
/// This is typically used to prevent screen readers from focusing
/// on parts of the UI.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `AccessibilityFocusBlockType` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AccessibilityFocusBlockType {
    /// Accessibility focus is **not blocked**.
    #[default]
    None,

    /// Blocks accessibility focus for the entire subtree.
    BlockSubtree,

    /// Blocks accessibility focus for the **current node only**.
    /// Its descendants may still be focusable.
    BlockNode,
}

impl AccessibilityFocusBlockType {
    /// Merges two focus block types.
    ///
    /// The result follows these rules:
    /// 1. If either is `BlockSubtree`, the result is `BlockSubtree`.
    /// 2. If either is `BlockNode`, the result is `BlockNode`.
    /// 3. Otherwise, the result is `None`.
    #[must_use]
    pub fn merge(self, other: Self) -> Self {
        // If either is blockSubtree, the result is blockSubtree
        if self == Self::BlockSubtree || other == Self::BlockSubtree {
            return Self::BlockSubtree;
        }

        // If either is blockNode, the result is blockNode
        if self == Self::BlockNode || other == Self::BlockNode {
            return Self::BlockNode;
        }

        // Otherwise both are none
        Self::None
    }

    /// Returns whether focus is blocked in some way.
    pub fn is_blocked(self) -> bool {
        self != Self::None
    }
}

// ============================================================================
// DebugSemanticsDumpOrder
// ============================================================================

/// Order for dumping the semantics tree in debug output.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `DebugSemanticsDumpOrder` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DebugSemanticsDumpOrder {
    /// Inverse hit test order (visual, bottom-to-top).
    #[default]
    InverseHitTest,

    /// Traversal order (accessibility navigation order).
    TraversalOrder,
}

// ============================================================================
// Assertiveness
// ============================================================================

/// The assertiveness level for accessibility announcements.
///
/// This controls how urgently screen readers announce content.
///
/// # Flutter Equivalence
///
/// Corresponds to Flutter's `Assertiveness` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Assertiveness {
    /// Polite announcements wait for the user to finish.
    #[default]
    Polite,

    /// Assertive announcements interrupt the user immediately.
    Assertive,
}

impl Assertiveness {
    /// Returns the string name of this assertiveness level.
    pub fn name(self) -> &'static str {
        match self {
            Self::Polite => "polite",
            Self::Assertive => "assertive",
        }
    }
}

impl std::fmt::Display for Assertiveness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_focus_block_merge() {
        use AccessibilityFocusBlockType::*;

        // BlockSubtree takes precedence
        assert_eq!(BlockSubtree.merge(None), BlockSubtree);
        assert_eq!(None.merge(BlockSubtree), BlockSubtree);
        assert_eq!(BlockSubtree.merge(BlockNode), BlockSubtree);

        // BlockNode next
        assert_eq!(BlockNode.merge(None), BlockNode);
        assert_eq!(None.merge(BlockNode), BlockNode);

        // None + None = None
        assert_eq!(None.merge(None), None);
    }

    #[test]
    fn test_focus_block_is_blocked() {
        assert!(!AccessibilityFocusBlockType::None.is_blocked());
        assert!(AccessibilityFocusBlockType::BlockNode.is_blocked());
        assert!(AccessibilityFocusBlockType::BlockSubtree.is_blocked());
    }

    #[test]
    fn test_assertiveness() {
        assert_eq!(Assertiveness::Polite.name(), "polite");
        assert_eq!(Assertiveness::Assertive.name(), "assertive");
    }
}
