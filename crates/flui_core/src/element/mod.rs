//! Element system - Unified View lifecycle and tree management
//!
//! This module provides the Element layer of the three-tree architecture:
//! - **View** → Immutable configuration (recreated each rebuild)
//! - **Element** → Unified mutable state holder (persists across rebuilds)
//! - **Render** → Layout and painting (delegated to ViewObject wrappers)
//!
//! # Unified Element Architecture (v0.7.0)
//!
//! Single `Element` struct with `ViewObject` delegation for all view types:
//! - **StatelessViewWrapper** - For stateless component views
//! - **StatefulViewWrapper** - For stateful component views
//! - **ProviderViewWrapper** - For provider views (data propagation + dependency tracking)
//! - **RenderViewWrapper** - For render objects (layout/paint)
//! - **AnimatedViewWrapper** - For animated views
//! - **ProxyViewWrapper** - For proxy views
//!
//! # Architecture
//!
//! ```text
//! View → Element (unified) → ViewObject (type-specific behavior)
//!
//! Component View  → Element + StatelessViewWrapper  → build() → child views
//! Provider View   → Element + ProviderViewWrapper   → (data + dependents) → child view
//! Render Object   → Element + RenderViewWrapper     → layout/paint
//! ```
//!
//! # ElementTree
//!
//! The ElementTree stores unified `Element` structs that delegate behavior
//! to `ViewObject` implementations. This eliminates enum dispatch overhead
//! and provides extensible architecture.
//!
//! # Performance
//!
//! - **O(1) access** by ElementId (direct slab indexing)
//! - **Cache-friendly** layout (contiguous memory in slab)
//! - **Lock-free reads** for RenderState flags via atomic operations
//
// Modules
pub mod dependency;
#[allow(clippy::module_inception)] // element/element.rs is intentional for main Element struct
pub mod element;
pub mod element_base;
pub mod element_tree;
pub mod hit_test;
pub mod hit_test_entry;
pub mod into_element;
pub mod lifecycle;

// Re-exports
pub use dependency::{DependencyInfo, DependencyTracker};
// Unified Element struct with ViewObject delegation
pub use element::Element;
// ElementBase is internal - used by framework only
pub(crate) use element_base::ElementBase;
pub use element_tree::ElementTree;
pub use hit_test::{
    BoxHitTestResult, ElementHitTestEntry, ElementHitTestResult, SliverHitTestResult,
};
pub use lifecycle::ElementLifecycle;

// Re-export ElementId from foundation (moved to break circular dependencies)
// ElementId is now defined in foundation::element_id to allow:
// - element depends on foundation (OK)
// - render depends on foundation (OK)
// - pipeline depends on foundation + element (OK)
// Previously: element → render → pipeline → element (CIRCULAR!)
pub use crate::foundation::ElementId;

// IntoElement trait for converting views/renders to elements
pub use into_element::IntoElement;
