//! # FLUI Semantics - Accessibility Tree
//!
//! This crate provides the Semantics tree - the fifth tree in FLUI's 5-tree architecture:
//! View → Element → Render → Layer → **Semantics**
//!
//! ## Purpose
//!
//! The semantics tree provides accessibility information for assistive technologies:
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
//! - [`SemanticsNode`] - Node with accessibility properties (label, role, actions)
//! - [`SemanticsConfiguration`] - Builder for semantic properties
//! - [`SemanticsOwner`] - Manages tree lifecycle and platform updates
//! - [`SemanticsAction`] - Actions that assistive tech can perform
//! - [`SemanticsEvent`] - Notifications to assistive technologies
//!
//! ## Optimizations
//!
//! This crate uses several optimizations for performance:
//! - [`SmolStr`](smol_str::SmolStr) for labels/hints (O(1) clone, inline storage)
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

#![warn(rust_2018_idioms, clippy::all, clippy::pedantic)]
#![allow(
    dead_code,
    unused_variables,
    missing_docs,
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use,
    clippy::doc_markdown,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

// ============================================================================
// MODULES
// ============================================================================

pub mod action;
pub mod configuration;
pub mod event;
pub mod flags;
pub mod node;
pub mod owner;
pub mod properties;
pub mod tree;
pub mod update;

// ============================================================================
// RE-EXPORTS - Action Types
// ============================================================================

pub use action::{ActionArgs, SemanticsAction, SemanticsActionHandler};

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
// RE-EXPORTS - Tree Types
// ============================================================================

pub use tree::SemanticsTree;

// ============================================================================
// RE-EXPORTS - Update Types
// ============================================================================

pub use update::{SemanticsNodeData, SemanticsTreeUpdate, SemanticsTreeUpdateBuilder};

// ============================================================================
// RE-EXPORTS - Foundation Types
// ============================================================================

pub use flui_foundation::SemanticsId;

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
    pub use crate::{
        ActionArgs, AttributedString, SemanticsAction, SemanticsActionHandler,
        SemanticsConfiguration, SemanticsEvent, SemanticsEventType, SemanticsFlag, SemanticsFlags,
        SemanticsId, SemanticsNode, SemanticsNodeData, SemanticsNodeUpdate, SemanticsOwner,
        SemanticsProperties, SemanticsTag, SemanticsTree, SemanticsTreeUpdate,
        SemanticsTreeUpdateBuilder, TextDirection,
    };

    // Re-export tree traits for convenience
    pub use flui_tree::{TreeNav, TreeRead};

    // Re-export optimized types
    pub use rustc_hash::{FxHashMap, FxHashSet};
    pub use smallvec::SmallVec;
    pub use smol_str::SmolStr;
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
        assert_eq!(VERSION, "0.1.0");
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
        assert_eq!(config.label().map(|l| l.as_str()), Some("Test Button"));
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
