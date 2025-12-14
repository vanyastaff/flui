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
pub use r#box::{
    BoxHitTestEntry, BoxHitTestResult, HitTestBehavior, MultiChildRenderBox,
    RenderAligningShiftedBox, RenderBox, RenderProxyBox, RenderShiftedBox, SingleChildRenderBox,
    TextBaseline,
};

// Re-export sliver traits
pub use sliver::{
    RenderProxySliver, RenderSliver, RenderSliverMultiBoxAdaptor, RenderSliverPersistentHeader,
    RenderSliverSingleBoxAdapter, SliverHitTestEntry, SliverHitTestResult,
};
