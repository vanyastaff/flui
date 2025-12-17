//! Trait definitions for render objects.
//!
//! This module defines the trait hierarchy for render objects:
//!
//! ```text
//! RenderObject (base)
//!     ├── RenderBox (2D layout)
//!     │   ├── SingleChildRenderBox
//!     │   │   ├── RenderProxyBox
//!     │   │   └── RenderShiftedBox
//!     │   │       └── RenderAligningShiftedBox
//!     │   └── MultiChildRenderBox
//!     └── RenderSliver (scrollable)
//!         ├── RenderProxySliver
//!         ├── RenderSliverSingleBoxAdapter
//!         ├── RenderSliverMultiBoxAdaptor
//!         └── RenderSliverPersistentHeader
//! ```

pub mod r#box;
mod render_object;
pub mod sliver;

// Re-export RenderObject at traits level
pub use render_object::*;

// Re-export box traits
// Note: BoxHitTestEntry, BoxHitTestResult, HitTestBehavior are also available from hit_testing module
// The versions here are kept for backward compatibility with existing code
pub use r#box::{
    // Helper functions for child offset management (Flutter-style)
    get_child_offset,
    set_child_offset,
    BoxHitTestEntry,
    BoxHitTestResult,
    HitTestBehavior,
    MultiChildRenderBox,
    RenderAligningShiftedBox,
    RenderBox,
    RenderProxyBox,
    RenderShiftedBox,
    SingleChildRenderBox,
    TextBaseline,
    TextDirection,
};

// Re-export sliver traits
// Note: SliverHitTestEntry, SliverHitTestResult are also available from hit_testing module
pub use sliver::{
    RenderProxySliver, RenderSliver, RenderSliverMultiBoxAdaptor, RenderSliverPersistentHeader,
    RenderSliverSingleBoxAdapter, SliverHitTestEntry, SliverHitTestResult,
};
