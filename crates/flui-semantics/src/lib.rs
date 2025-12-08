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
//! - [`SemanticsTree`] - Tree storage implementing `TreeRead`/`TreeNav`
//! - [`SemanticsOwner`] - Manages tree lifecycle and platform updates
//!
//! ## Tree Integration
//!
//! SemanticsTree implements `TreeRead<SemanticsId>` and `TreeNav<SemanticsId>` from `flui-tree`,
//! enabling generic tree algorithms and visitors.
//!
//! ```rust,ignore
//! use flui_semantics::{SemanticsTree, SemanticsNode};
//! use flui_foundation::SemanticsId;
//! use flui_tree::{TreeRead, TreeNav};
//!
//! let mut tree = SemanticsTree::new();
//! let id = tree.insert(SemanticsNode::new());
//!
//! // Use generic tree operations
//! assert!(tree.contains(id));
//! ```
//!
//! ## Flutter Compatibility
//!
//! This follows Flutter's semantics protocol closely:
//! - `SemanticsNode` ≈ Flutter's `SemanticsNode`
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

pub mod node;
pub mod owner;
pub mod tree;

// ============================================================================
// RE-EXPORTS - Core Types
// ============================================================================

pub use node::SemanticsNode;
pub use owner::{SemanticsOwner, SemanticsUpdateCallback};
pub use tree::SemanticsTree;

// ============================================================================
// RE-EXPORTS - Foundation Types
// ============================================================================

pub use flui_foundation::SemanticsId;

// Re-export semantics types from flui_types
pub use flui_types::semantics::{
    SemanticsAction, SemanticsData, SemanticsFlags, SemanticsProperties, SemanticsRole,
};

// ============================================================================
// PRELUDE
// ============================================================================

/// The semantics prelude - commonly used types and traits.
///
/// ```rust,ignore
/// use flui_semantics::prelude::*;
/// ```
pub mod prelude {
    pub use crate::{
        SemanticsAction, SemanticsData, SemanticsId, SemanticsNode, SemanticsOwner,
        SemanticsProperties, SemanticsRole, SemanticsTree,
    };

    // Re-export tree traits for convenience
    pub use flui_tree::{TreeNav, TreeRead};
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
}
