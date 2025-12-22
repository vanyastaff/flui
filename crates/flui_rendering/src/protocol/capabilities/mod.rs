//! Capability traits for the composition-based protocol system.
//!
//! This module provides the three core capabilities that protocols compose:
//!
//! - [`LayoutCapability`]: Constraints, geometry, and layout context
//! - [`HitTestCapability`]: Position, result, and hit test context
//! - [`PaintCapability`]: Painting traits composition (Painter, Layering, Effects, Caching)
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                    CAPABILITY TRAITS                            │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐ │
//! │  │LayoutCapability │  │HitTestCapability│  │ PaintCapability │ │
//! │  ├─────────────────┤  ├─────────────────┤  ├─────────────────┤ │
//! │  │ Constraints     │  │ Position        │  │ Painter         │ │
//! │  │ Geometry        │  │ Result          │  │ Layering        │ │
//! │  │ Context<GAT>    │  │ Entry           │  │ Effects         │ │
//! │  │                 │  │ Context<GAT>    │  │ Caching         │ │
//! │  │                 │  │                 │  │ Context<GAT>    │ │
//! │  └─────────────────┘  └─────────────────┘  └─────────────────┘ │
//! └─────────────────────────────────────────────────────────────────┘
//!                               │
//!                               ▼
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  trait Protocol {                                               │
//! │      type Layout: LayoutCapability;                             │
//! │      type HitTest: HitTestCapability;                           │
//! │      type Paint: PaintCapability;                               │
//! │      type DefaultParentData;                                    │
//! │  }                                                              │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Benefits
//!
//! 1. **Composition over inheritance** - Protocol composes capabilities
//! 2. **Clear type grouping** - Each capability groups related types
//! 3. **Paint decomposition** - 4 orthogonal components instead of monolith
//! 4. **Easy extension** - New protocol = new capability combination
//! 5. **Backend swap** - Replace only Canvas component

mod hit_test;
mod layout;
mod paint;

pub use hit_test::{
    BoxHitTest, BoxHitTestCtx, BoxHitTestEntry, BoxHitTestResult, HitTestCapability,
    HitTestContextApi, MainAxisPosition, SliverHitTest, SliverHitTestCtx, SliverHitTestEntry,
    SliverHitTestResult,
};
pub use layout::{
    BoxLayout, BoxLayoutCtx, LayoutCapability, LayoutContextApi, SliverLayout, SliverLayoutCtx,
};
pub use paint::{
    DynCaching, DynEffects, DynLayering, DynPainter, PaintCapability, PaintContextApi,
    StandardPaint, StandardPaintCtx,
};

/// Prelude for capability traits.
pub mod prelude {
    pub use super::{
        HitTestCapability, HitTestContextApi, LayoutCapability, LayoutContextApi, PaintCapability,
        PaintContextApi,
    };
}
