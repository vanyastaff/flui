//! Element system - Widget lifecycle and tree management
//!
//! This module provides the Element layer of the three-tree architecture:
//! - **Widget** → Immutable configuration (recreated each rebuild)
//! - **Element** → Mutable state holder (persists across rebuilds)
//! - **RenderObject** → Layout and painting (optional, for render widgets)
//!
//! # Element Types
//!
//! 1. **ComponentElement** - For StatelessWidget (calls build())
//! 2. **StatefulElement** - For StatefulWidget (manages State object)
//! 3. **InheritedElement** - For InheritedWidget (data propagation + dependency tracking)
//! 4. **ParentDataElement** - For ParentDataWidget (attaches metadata to children)
//! 5. **RenderObjectElement** - For RenderObjectWidget (owns RenderObject)
//!
//! # Architecture
//!
//! ```text
//! Widget → Element → RenderObject (optional)
//!
//! StatelessWidget     → ComponentElement     → build() → child widget
//! StatefulWidget      → StatefulElement      → State.build() → child widget
//! InheritedWidget     → InheritedElement     → (data + dependents) → child widget
//! ParentDataWidget    → ParentDataElement    → (attach data) → child widget
//! RenderObjectWidget  → RenderObjectElement<Arity> → RenderObject<Arity>
//! ```
//!
//! # ElementTree
//!
//! The ElementTree currently stores RenderObjects directly (will be refactored to store Elements):
//! - **RenderObjects** for rendering (temporary, will become part of RenderObjectElement)
//! - **RenderState** per RenderObject (size, constraints, dirty flags)
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
pub mod dyn_element;
pub mod element_tree;
pub mod inherited;
pub mod parent_data_element;
pub mod render_object_element;
pub mod stateful;






// Re-exports
pub use dyn_element::{DynElement, BoxedElement, ElementLifecycle};
pub use component::ComponentElement;
pub use stateful::StatefulElement;
pub use inherited::InheritedElement;
pub use parent_data_element::ParentDataElement;
pub use render_object_element::RenderObjectElement;
pub use element_tree::ElementTree;
pub use build_context::BuildContext;

/// Element ID - stable index into the ElementTree slab
///
/// This is a handle to an element that remains valid until the element is removed.
/// ElementIds are reused after removal (slab behavior), so don't store them long-term
/// without verifying the element still exists.
pub type ElementId = usize;





