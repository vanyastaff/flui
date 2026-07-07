//! # FLUI Semantics - Accessibility Tree
//!
//! This crate provides the Semantics tree - the fifth tree in FLUI's 5-tree
//! architecture: View → Element → Render → Layer → **Semantics**
//!
//! ## Purpose
//!
//! The semantics tree provides accessibility information for assistive
//! technologies:
//! - Screen readers (VoiceOver, TalkBack, NVDA, JAWS)
//! - Switch control
//! - Voice control
//! - Braille displays
//!
//! ## Architecture
//!
//! ```text
//! RenderObject (flui_rendering)
//!     │
//!     │ assembleSemanticsNode() during paint phase
//!     ▼
//! SemanticsNode (this crate)
//!     │
//!     │ flush() sends to platform
//!     ▼
//! Platform Accessibility API (iOS/Android/Windows/macOS)
//! ```
//!
//! ## Key Types
//!
//! - [`SemanticsNode`] - Node with accessibility properties (label, role,
//!   actions)
//! - [`SemanticsConfiguration`] - Builder for semantic properties
//! - [`SemanticsOwner`] - Manages tree lifecycle and platform updates
//! - [`SemanticsAction`] - Actions that assistive tech can perform
//! - [`SemanticsEvent`] - Notifications to assistive technologies
//!
//! ## Optimizations
//!
//! This crate uses several optimizations for performance:
//! - [`SmolStr`](smol_str::SmolStr) for labels/hints (O(1) clone, inline
//!   storage)
//! - [`SmallVec`](smallvec::SmallVec) for children/actions (stack allocation)
//! - [`FxHashMap`](rustc_hash::FxHashMap) for fast lookups
//!
//! ## Flutter Compatibility
//!
//! This follows Flutter's semantics protocol closely:
//! - `SemanticsNode` ≈ Flutter's `SemanticsNode`
//! - `SemanticsConfiguration` ≈ Flutter's `SemanticsConfiguration`
//! - `SemanticsOwner` ≈ Flutter's `SemanticsOwner`
//! - `SemanticsAction` ≈ Flutter's `SemanticsAction`

// Lint levels come from `[workspace.lints]` (Cargo.toml `[lints] workspace = true`).
// Ship bar (wave 2): every public item is documented; keep it that way.
#![deny(missing_docs)]

// ============================================================================
// MODULES
// ============================================================================

pub mod action;
pub mod binding;
pub mod configuration;
pub mod event;
pub mod flags;
pub mod node;
pub mod owner;
pub mod properties;
pub mod role;
pub mod tree;
pub mod update;

// ============================================================================
// RE-EXPORTS - Action Types
// ============================================================================

pub use action::{ActionArgs, SemanticsAction, SemanticsActionHandler};
// ============================================================================
// RE-EXPORTS - Binding Types
// ============================================================================
pub use binding::{
    AccessibilityFeatures, SemanticsActionEvent, SemanticsBinding, SemanticsHandle,
    SemanticsService,
};
// ============================================================================
// RE-EXPORTS - Configuration
// ============================================================================
pub use configuration::SemanticsConfiguration;
// ============================================================================
// RE-EXPORTS - Event Types
// ============================================================================
pub use event::{SemanticsEvent, SemanticsEventData, SemanticsEventType};
// ============================================================================
// RE-EXPORTS - Flag Types
// ============================================================================
pub use flags::{SemanticsFlag, SemanticsFlags};
// ============================================================================
// RE-EXPORTS - Foundation Types
// ============================================================================
pub use flui_foundation::SemanticsId;
// ============================================================================
// RE-EXPORTS - Node Types
// ============================================================================
pub use node::SemanticsNode;
// ============================================================================
// RE-EXPORTS - Owner Types
// ============================================================================
pub use owner::{SemanticsNodeUpdate, SemanticsOwner, SemanticsUpdateCallback};
// ============================================================================
// RE-EXPORTS - Property Types
// ============================================================================
pub use properties::{
    AttributedString, CustomSemanticsAction, SemanticsHintOverrides, SemanticsProperties,
    SemanticsSortKey, SemanticsTag, StringAttribute, StringAttributeType, TextDirection,
};
// ============================================================================
// RE-EXPORTS - Role Types
// ============================================================================
pub use role::{
    AccessibilityFocusBlockType, Assertiveness, DebugSemanticsDumpOrder, SemanticsRole,
};
// ============================================================================
// RE-EXPORTS - Tree Types
// ============================================================================
pub use tree::SemanticsTree;
// ============================================================================
// RE-EXPORTS - Update Types
// ============================================================================
pub use update::{SemanticsNodeData, SemanticsTreeUpdate, SemanticsTreeUpdateBuilder};

// ============================================================================
// PRELUDE
// ============================================================================

/// The semantics prelude - commonly used types and traits.
///
/// ```rust,ignore
/// use flui_semantics::prelude::*;
/// ```
pub mod prelude {
    // Core types
    // Re-export tree traits for convenience
    pub use flui_tree::{TreeNav, TreeRead};
    // Re-export optimized types
    pub use rustc_hash::{FxHashMap, FxHashSet};
    pub use smallvec::SmallVec;
    pub use smol_str::SmolStr;

    pub use crate::{
        AccessibilityFeatures, AccessibilityFocusBlockType, ActionArgs, Assertiveness,
        AttributedString, DebugSemanticsDumpOrder, SemanticsAction, SemanticsActionEvent,
        SemanticsActionHandler, SemanticsBinding, SemanticsConfiguration, SemanticsEvent,
        SemanticsEventType, SemanticsFlag, SemanticsFlags, SemanticsHandle, SemanticsId,
        SemanticsNode, SemanticsNodeData, SemanticsNodeUpdate, SemanticsOwner, SemanticsProperties,
        SemanticsRole, SemanticsService, SemanticsTag, SemanticsTree, SemanticsTreeUpdate,
        SemanticsTreeUpdateBuilder, TextDirection,
    };
}

// ============================================================================
// VERSION INFO
// ============================================================================

/// The version of the flui-semantics crate.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        // `VERSION` is wired from the package version (`env!("CARGO_PKG_VERSION")`);
        // assert its shape, not a pinned literal — a hardcoded value breaks on
        // every workspace version bump (it broke at the 0.1.0 -> 0.2.0 bump).
        let parts: Vec<&str> = VERSION.split('.').collect();
        assert_eq!(
            parts.len(),
            3,
            "VERSION should be semver `major.minor.patch`, got {VERSION:?}",
        );
        assert!(
            parts.iter().all(|part| part.parse::<u64>().is_ok()),
            "VERSION components should be numeric, got {VERSION:?}",
        );
    }

    #[test]
    fn test_semantics_tree_basic() {
        let mut tree = SemanticsTree::new();
        assert!(tree.is_empty());

        let node = SemanticsNode::new();
        let id = tree.insert(node);

        assert!(!tree.is_empty());
        assert_eq!(tree.len(), 1);
        assert!(tree.contains(id));
    }

    #[test]
    fn test_configuration_basic() {
        let mut config = SemanticsConfiguration::new();
        config.set_label("Test Button");
        config.set_button(true);

        assert!(config.is_button());
        assert_eq!(
            config
                .label()
                .map(super::properties::AttributedString::as_str),
            Some("Test Button")
        );
    }

    #[test]
    fn test_action_bitmask() {
        let tap = SemanticsAction::Tap;
        let long_press = SemanticsAction::LongPress;

        let combined = tap.value() | long_press.value();
        assert_eq!(combined, 3);
    }

    #[test]
    fn test_event_creation() {
        let event = SemanticsEvent::announce("Item selected");
        assert_eq!(event.event_type(), SemanticsEventType::Announce);
        assert_eq!(event.get_string("message"), Some("Item selected"));
    }
}
