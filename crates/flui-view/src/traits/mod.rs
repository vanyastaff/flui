//! View traits module
//!
//! Defines the core view traits for different view types:
//! - `StatelessView` - Simple views without state
//! - `StatefulView` - Views with persistent mutable state
//! - `AnimatedView` - Views driven by animations
//! - `ProviderView` - Views that provide data to descendants
//! - `ProxyView` - Views that wrap single child
//!
//! Note: `RenderView` is in `flui_rendering::view` to avoid circular deps.

mod animated;
mod provider;
mod proxy;
mod stateful;
mod stateless;

pub use animated::{AnimatedView, Listenable};
pub use provider::ProviderView;
pub use proxy::ProxyView;
pub use stateful::StatefulView;
pub use stateless::StatelessView;
