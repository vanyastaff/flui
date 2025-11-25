//! # FLUI Element
//!
//! Element tree and lifecycle management for the FLUI UI framework.
//!
//! This crate provides the core Element type and ElementTree data structure
//! for managing the element layer of FLUI's three-tree architecture.
//!
//! ## Architecture
//!
//! ```text
//! View (immutable) → Element (mutable) → RenderObject (layout/paint)
//!                    ^^^^^^^^^^^^^^^^
//!                    This crate!
//! ```
//!
//! ## Key Types
//!
//! - [`Element`] - Unified element struct with type-erased view object
//! - [`ElementTree`] - Slab-based storage with O(1) access
//! - [`ElementLifecycle`] - Lifecycle states (Initial, Active, Inactive, Defunct)
//! - [`IntoElement`] - Trait for converting types to elements
//!
//! ## Design Principles
//!
//! ### Type Erasure
//!
//! Element stores `Box<dyn Any + Send>` instead of `Box<dyn ViewObject>`.
//! This breaks the dependency on ViewObject trait, allowing flui-element
//! to be independent of flui-view. The actual ViewObject is stored inside
//! and can be accessed via downcasting.
//!
//! ### Slab-Based Storage
//!
//! ElementTree uses a slab for O(1) element access by ElementId.
//! ElementId is 1-based (NonZeroUsize), while slab uses 0-based indexing.
//!
//! ### Thread Safety
//!
//! - Element is `Send` (can be moved between threads)
//! - ElementBase uses atomic flags for lock-free dirty tracking
//! - `mark_dirty()` can be called from any thread
//!
//! ## Example
//!
//! ```rust
//! use flui_element::{Element, ElementTree, ElementLifecycle, IntoElement};
//! use flui_foundation::ElementId;
//!
//! // Create a tree
//! let mut tree = ElementTree::new();
//!
//! // Insert elements
//! let root_id = tree.insert(Element::empty());
//! let child_id = tree.insert(Element::empty());
//!
//! // Set up parent-child relationship
//! if let Some(child) = tree.get_mut(child_id) {
//!     child.base_mut().set_parent(Some(root_id));
//! }
//! if let Some(root) = tree.get_mut(root_id) {
//!     root.add_child(child_id);
//! }
//!
//! // Check lifecycle
//! if let Some(element) = tree.get(root_id) {
//!     assert_eq!(element.lifecycle(), ElementLifecycle::Initial);
//! }
//! ```
//!
//! ## Crate Dependencies
//!
//! ```text
//! flui-foundation (ElementId, Slot, Flags)
//!        ↓
//! flui-tree (TreeRead, TreeNav, TreeWrite)
//!        ↓
//! flui-element (Element, ElementTree, IntoElement)  ← This crate
//!        ↓
//! flui-view (ViewObject, BuildContext, View traits)
//! ```

#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    clippy::all,
    clippy::pedantic
)]
#![allow(
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use
)]

// ============================================================================
// MODULES
// ============================================================================

pub mod element;
pub mod into_element;
pub mod tree;

// ============================================================================
// RE-EXPORTS
// ============================================================================

// Element types
pub use element::{Element, ElementBase, ElementLifecycle};

// Tree types
pub use tree::ElementTree;

// IntoElement trait
pub use into_element::IntoElement;

// Re-export from flui-foundation for convenience
pub use flui_foundation::{ElementId, Slot, ViewMode};

// Re-export tree traits for convenience
pub use flui_tree::{RenderTreeAccess, TreeNav, TreeRead, TreeWrite, TreeWriteNav};

// ============================================================================
// PRELUDE
// ============================================================================

/// Commonly used types for convenient importing.
///
/// ```rust
/// use flui_element::prelude::*;
/// ```
pub mod prelude {
    pub use crate::element::{Element, ElementLifecycle};
    pub use crate::into_element::IntoElement;
    pub use crate::tree::ElementTree;
    pub use flui_foundation::ElementId;
    pub use flui_tree::{RenderTreeAccess, TreeNav, TreeRead, TreeWrite};
}
