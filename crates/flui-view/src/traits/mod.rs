//! View traits module
//!
//! Defines the core view traits for different view types:
//! - `StatelessView` - Simple views without state
//! - `StatefulView` - Views with persistent mutable state
//! - `AnimatedView` - Views driven by animations
//! - `ProviderView` - Views that provide data to descendants
//! - `ProxyView` - Views that wrap single child
//! - `RenderView` - Views that create render objects

mod animated;
mod provider;
mod proxy;
mod render;
mod stateful;
mod stateless;
mod update_result;

pub use animated::{AnimatedView, Listenable};
pub use provider::ProviderView;
pub use proxy::ProxyView;
pub use render::{
    RenderObjectFor, RenderView, RenderViewConfig, RenderViewExt, RenderViewLeaf,
    RenderViewWithChild, RenderViewWithChildren, RenderViewWithOptionalChild,
};
pub use stateful::StatefulView;
pub use stateless::StatelessView;
pub use update_result::UpdateResult;
