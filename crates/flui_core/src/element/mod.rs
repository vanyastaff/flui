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
pub mod component;
pub mod dependency;
#[allow(clippy::module_inception)] // element/element.rs is intentional for main Element enum
pub mod element;
pub mod element_base;
pub mod element_tree;
pub mod hit_test;
pub mod lifecycle;
pub mod provider;
pub mod render;


// Re-exports
pub use component::ComponentElement;
pub use dependency::{DependencyInfo, DependencyTracker};
pub use element::Element;
pub use element_base::ElementBase;
pub use element_tree::ElementTree; // Moved from pipeline to break circular dependency
pub use hit_test::{ElementHitTestEntry, ElementHitTestResult};
pub use lifecycle::ElementLifecycle;
pub use provider::ProviderElement;
pub use render::RenderElement;

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

