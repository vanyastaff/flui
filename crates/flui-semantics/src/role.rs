//! Semantics roles for accessibility.
//!
//! This module provides role types for accessibility semantics.
//! Roles describe the structural type of UI element and help
//! assistive technologies understand how to interact with elements.
//!
//! A node has one role, interactive kinds (button, checkbox, slider)
//! included (ADR-0206); states are typed separately
//! ([`States`](crate::States)).
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DebugSemanticsDumpOrder {
    /// Inverse hit test order (visual, bottom-to-top).
    #[default]
    InverseHitTest,

    /// Traversal order (accessibility navigation order).
    TraversalOrder,
}

// ============================================================================
// LiveSetting, Urgency
// ============================================================================

/// How a live region's changes reach a screen reader (`aria-live`).
///
/// [`Self::Off`] is a setting, not an absence: an undeclared setting inherits
/// the enclosing region's, so a ticking timer inside a polite region declares
/// `Off` to stay silent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LiveSetting {
    /// Changes are not announced.
    Off,
    /// Changes are announced after what is being read.
    Polite,
    /// Changes interrupt what is being read.
    Assertive,
}

/// How urgently an announcement is spoken.
///
/// Apart from [`LiveSetting`]: an announcement is always spoken, so it has no
/// `Off`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Urgency {
    /// Spoken after what is being read.
    Polite,
    /// Interrupts what is being read.
    Assertive,
}

impl Urgency {
    /// The `author_id` the announcer node of this urgency publishes under,
    /// which tests and agents read announcements by (ADR-0240). It is in a
    /// namespace no widget identifier uses.
    #[must_use]
    pub const fn announcer_identifier(self) -> &'static str {
        match self {
            Self::Polite => "flui:announcer:polite",
            Self::Assertive => "flui:announcer:assertive",
        }
    }
}
