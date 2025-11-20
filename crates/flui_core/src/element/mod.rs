//! Element system - View lifecycle and tree management
//!
//! This module provides the Element layer of the three-tree architecture:
//! - **View** → Immutable configuration (recreated each rebuild)
//! - **Element** → Mutable state holder (persists across rebuilds)
//! - **Render** → Layout and painting (optional, for render objects)
//!
//! # Element Types
//!
//! 1. **ComponentElement** - For component views (calls build())
//! 2. **ProviderElement** - For provider views (data propagation + dependency tracking)
//! 3. **RenderElement** - For render objects (owns Render)
//!
//! # Architecture
//!
//! ```text
//! View → Element → Render (optional)
//!
//! Component View  → ComponentElement  → build() → child views
//! Provider View   → ProviderElement   → (data + dependents) → child view
//! Render Object   → RenderElement     → Render (type-erased)
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
pub mod dependency;
#[allow(clippy::module_inception)] // element/element.rs is intentional for main Element enum
pub mod element;
pub mod element_base;
pub mod element_tree;
pub mod hit_test;
pub mod hit_test_entry;
pub mod into_element;
pub mod lifecycle;
pub mod provider;
// TODO: Re-enable sliver support after completing box render migration
// pub mod sliver;

// Re-exports
// ViewElement (formerly ComponentElement) is in view module
pub use crate::view::ViewElement;
// Keep ComponentElement as alias for backwards compatibility
pub use crate::view::ViewElement as ComponentElement;
pub use dependency::{DependencyInfo, DependencyTracker};
pub use element::Element;
// ElementBase is internal - used by framework only
pub(crate) use element_base::ElementBase;
pub use element_tree::ElementTree; // Moved from pipeline to break circular dependency
pub use hit_test::{
    BoxHitTestResult, ElementHitTestEntry, ElementHitTestResult, SliverHitTestResult,
};
pub use lifecycle::ElementLifecycle;
pub use provider::ProviderElement;
// RenderElement is now in render module
pub use crate::render::RenderElement;
// TODO: Re-enable sliver support after completing box render migration
// pub use sliver::SliverElement;

// Moved to other modules (Phase 1):
// - BuildContext moved to view::BuildContext
// - PipelineOwner moved to pipeline::PipelineOwner
//
// Moved back from pipeline (Phase 2 - Issue #21):
// - ElementTree moved back to element module (logical home, breaks pipeline ↔ render cycle)

// Re-export ElementId from foundation (moved to break circular dependencies)
// ElementId is now defined in foundation::element_id to allow:
// - element depends on foundation (OK)
// - render depends on foundation (OK)
// - pipeline depends on foundation + element (OK)
// Previously: element → render → pipeline → element (CIRCULAR!)
pub use crate::foundation::ElementId;

// IntoElement trait for converting views/renders to elements
pub use into_element::IntoElement;
