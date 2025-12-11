//! Mixin-based render object architecture
//!
//! This module provides the foundational mixin patterns for render objects,
//! combining **Ambassador** (automatic trait delegation) with **Deref** (direct field access).
//!
//! # Available Mixins
//!
//! | Mixin | Purpose | Flutter Equivalent |
//! |-------|---------|-------------------|
//! | `ProxyBox<T>` | Delegates all to child | `RenderProxyBox` |
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

// Re-export key types and traits
pub use proxy::{
    ProxyBox, ProxyBase, ProxyData,
    HasChild, HasBoxGeometry,
    RenderProxyBoxMixin,
};
