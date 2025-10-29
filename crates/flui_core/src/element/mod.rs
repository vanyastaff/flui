//! Element system - Widget lifecycle and tree management
//!
//! This module provides the Element layer of the three-tree architecture:
//! - **Widget** → Immutable configuration (recreated each rebuild)
//! - **Element** → Mutable state holder (persists across rebuilds)
//! - **Render** → Layout and painting (optional, for render widgets)
//!
//! # Element Types
//!
//! 1. **ComponentElement** - For StatelessWidget (calls build())
//! 2. **StatefulElement** - For StatefulWidget (manages State object)
//! 3. **InheritedElement** - For InheritedWidget (data propagation + dependency tracking)
//! 4. **ParentDataElement** - For ParentDataWidget (attaches metadata to children)
//! 5. **RenderElement** - For RenderWidget (owns Render)
//!
//! # Architecture
//!
//! ```text
//! Widget → Element → Render (optional)
//!
//! StatelessWidget     → ComponentElement  → build() → child widget
//! StatefulWidget      → StatefulElement   → State.build() → child widget
//! InheritedWidget     → InheritedElement  → (data + dependents) → child widget
//! ParentDataWidget    → ParentDataElement → (attach data) → child widget
//! RenderWidget  → RenderElement     → Render (type-erased)
//! ```
//!
//! # ElementTree
//!
//! The ElementTree currently stores Renders directly (will be refactored to store Elements):
//! - **Renders** for rendering (temporary, will become part of RenderElement)
//! - **RenderState** per Render (size, constraints, dirty flags)
//! - **Tree relationships** (parent/children) via ElementId indices
//!
//! # Performance
//!
//! - **O(1) access** by ElementId (direct slab indexing)
//! - **Cache-friendly** layout (contiguous memory in slab)
//! - **Lock-free reads** for RenderState flags via atomic operations

// Modules
pub mod build_context;
pub mod component;
pub mod dependency;
pub mod element;
pub mod element_tree;
pub mod inherited;
pub mod lifecycle;
pub mod parent_data_element;
pub mod pipeline_owner;
pub mod render_object_element;
pub mod stateful;


// Re-exports
pub use build_context::BuildContext;
pub use component::ComponentElement;
pub use dependency::{DependencyInfo, DependencyTracker};
pub use element::Element;
pub use element_tree::ElementTree;
pub use inherited::InheritedElement;
pub use parent_data_element::ParentDataElement;
pub use pipeline_owner::PipelineOwner;
pub use render_object_element::RenderElement;
pub use stateful::{BoxedState, DynState, StatefulElement};

/// Element lifecycle states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementLifecycle {
    /// Element created but not yet mounted
    Initial,
    /// Element is active in the tree
    Active,
    /// Element removed from tree but might be reinserted
    Inactive,
    /// Element permanently removed
    Defunct,
}

/// Element ID - stable index into the ElementTree slab
///
/// This is a handle to an element that remains valid until the element is removed.
/// ElementIds are reused after removal (slab behavior), so don't store them long-term
/// without verifying the element still exists.
pub type ElementId = usize;


