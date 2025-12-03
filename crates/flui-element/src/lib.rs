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
//! - [`Element`] - Unified element enum (View | Render variants)
//! - [`ViewElement`] - Element for component views (Stateless, Stateful, Provider)
//! - [`RenderElement`] - Element for render views (RenderBox, RenderSliver)
//! - [`ElementBase`] - Common base with lifecycle, flags, parent/slot
//! - [`ElementTree`] - Slab-based storage with O(1) access
//! - [`ElementLifecycle`] - Lifecycle states (Initial, Active, Inactive, Defunct)
//! - [`IntoElement`] - Trait for converting types to elements
//!
//! ## Design Principles
//!
//! ### Two Element Variants
//!
//! Element is an enum with two variants:
//! - `View(ViewElement)`: Component views that build children
//! - `Render(RenderElement)`: Render views that handle layout/paint
//!
//! This mirrors Flutter's distinction between ComponentElement and RenderObjectElement.
//!
//! ### Type Erasure
//!
//! ViewElement stores `Box<dyn ViewObject>` for component behavior.
//! RenderElement stores `Box<dyn Any>` for both render object and render state,
//! allowing flui-element to remain independent of flui_rendering types.
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
//! use flui_element::{Element, ViewElement, RenderElement, ElementTree, ElementLifecycle};
//! use flui_foundation::{ElementId, ViewMode};
//!
//! // Create a tree
//! let mut tree = ElementTree::new();
//!
//! // Insert a view element
//! let view_elem = Element::empty();
//! let root_id = tree.insert(view_elem);
//!
//! // Insert another element as child
//! let child_elem = Element::empty();
//! let child_id = tree.insert(child_elem);
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
//! flui-foundation (ElementId, Slot, ViewMode, Flags)
//!        ↓
//! flui-tree (TreeRead, TreeNav, TreeWrite, RenderTreeAccess)
//!        ↓
//! flui-view (ViewObject, BuildContext, IntoView)
//!        ↓
//! flui-element (Element, ViewElement, RenderElement, ElementTree)
//!        ↓
//! flui_rendering (RenderObject, RenderState, layout/paint)
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
    clippy::return_self_not_must_use,
    clippy::doc_markdown,
    clippy::redundant_closure_for_method_calls,
    clippy::map_unwrap_or,
    clippy::missing_fields_in_debug,
    clippy::needless_pass_by_value,
    clippy::explicit_iter_loop,
    clippy::module_inception
)]

// ============================================================================
// MODULES
// ============================================================================

pub mod element;
pub mod into_element;
pub mod tree;

// ============================================================================
// RE-EXPORTS FROM flui-view
// ============================================================================

// ViewObject and BuildContext are now defined in flui-view
pub use flui_view::{BuildContext, ViewObject};

// IntoView for convenience
pub use flui_view::IntoView;

// ============================================================================
// RE-EXPORTS
// ============================================================================

// Element types - the new architecture
pub use element::{
    AtomicElementFlags, Element, ElementBase, ElementFlags, ElementLifecycle, RenderElement,
    RenderObjectTrait, ViewElement,
};

// Tree types
pub use tree::ElementTree;

// IntoElement trait
pub use into_element::IntoElement;

// Re-export from flui-foundation for convenience
pub use flui_foundation::{ElementId, Slot};
pub use flui_view::ViewMode;

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
    // Core element types
    pub use crate::element::{Element, ElementLifecycle, RenderElement, ViewElement};

    // Conversion trait
    pub use crate::into_element::IntoElement;

    // Tree types
    pub use crate::tree::ElementTree;

    // Foundation types
    pub use flui_foundation::ElementId;
    pub use flui_view::ViewMode;

    // Tree traits
    pub use flui_tree::{RenderTreeAccess, TreeNav, TreeRead, TreeWrite};

    // From flui-view
    pub use flui_view::{BuildContext, ViewObject};
}
