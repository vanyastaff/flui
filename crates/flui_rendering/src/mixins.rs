//! Mixin-based render object architecture
//!
//! This module provides the foundational mixin patterns for render objects,
//! combining **Ambassador** (automatic trait delegation) with **Deref** (direct field access).
//!
//! # Available Mixins
//!
//! ## Box Protocol
//!
//! | Mixin | Purpose | Flutter Equivalent |
//! |-------|---------|-------------------|
//! | `ProxyBox<T>` | Delegates all to child | `RenderProxyBox` |
//! | `ShiftedBox<T>` | Applies offset transform | `RenderShiftedBox` |
//! | `AligningShiftedBox<T>` | Adds alignment support | `RenderAligningShiftedBox` |
//! | `ContainerBox<T, PD>` | Manages multiple children | `ContainerRenderObjectMixin` |
//! | `LeafBox<T>` | No children (leaf node) | Leaf render objects |
//!
//! ## Sliver Protocol
//!
//! | Mixin | Purpose | Flutter Equivalent |
//! |-------|---------|-------------------|
//! | `ProxySliver<T>` | Delegates all to child | `RenderProxySliver` |
//! | `ShiftedSliver<T>` | Applies offset transform | `RenderShiftedSliver` |
//! | `ContainerSliver<T, PD>` | Manages multiple children | `SliverMultiBoxAdaptorMixin` |
//! | `LeafSliver<T>` | No children (leaf sliver) | Leaf sliver objects |
//!
//! # Pattern Summary
//!
//! ```text
//! User writes 3 things:
//! ┌─────────────────────────────────────┐
//! │ 1. Data struct                       │  #[derive(Default, Clone, Debug)]
//! │    pub struct OpacityData {          │  pub struct OpacityData {
//! │        pub alpha: f32,               │      pub alpha: f32,
//! │    }                                 │  }
//! └─────────────────────────────────────┘
//!
//! ┌─────────────────────────────────────┐
//! │ 2. Type alias                        │  pub type RenderOpacity = ProxyBox<OpacityData>;
//! └─────────────────────────────────────┘
//!
//! ┌─────────────────────────────────────┐
//! │ 3. Override methods (optional)       │  impl RenderProxyBoxMixin for RenderOpacity {
//! │                                      │      fn paint(&self, ctx, offset) {
//! │                                      │          // self.alpha via Deref
//! │                                      │          // self.child() via Ambassador
//! │                                      │      }
//! │                                      │  }
//! └─────────────────────────────────────┘
//! ```
//!
//! # Automatic Features
//!
//! - **Ambassador delegation**: `child()`, `size()` methods
//! - **Deref to data**: `self.alpha` instead of `self.data.alpha`
//! - **Default implementations**: `paint()`, `layout()`, `hit_test()`
//! - **Minimal boilerplate**: ~30 lines instead of 200+

pub mod proxy;
pub mod shifted;
pub mod aligning;
pub mod container;
pub mod leaf;
pub mod integration;

// Re-export key types and traits

// Box Protocol
pub use proxy::{
    ProxyBox, ProxyBase, ProxyData,
    HasChild, HasBoxGeometry,
    RenderProxyBoxMixin,
    // Sliver Protocol
    ProxySliver,
    HasSliverGeometry,
    RenderProxySliverMixin,
};

pub use shifted::{
    ShiftedBox, ShiftedBase,
    HasOffset,
    RenderShiftedBox,
    // Sliver Protocol
    ShiftedSliver,
    RenderShiftedSliver,
};

pub use aligning::{
    AligningShiftedBox, AligningBase,
    HasAlignment,
    RenderAligningShiftedBox,
};

pub use container::{
    ContainerBox, ContainerBase,
    HasChildren,
    RenderContainerBox,
    // Sliver Protocol
    ContainerSliver,
    RenderContainerSliver,
};

pub use leaf::{
    LeafBox, LeafBase,
    RenderLeafBox,
    // Sliver Protocol
    LeafSliver,
    RenderLeafSliver,
};
