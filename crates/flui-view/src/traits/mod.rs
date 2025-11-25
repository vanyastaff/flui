//! View traits module.
//!
//! Contains the core view traits that define different view types.

mod proxy;
mod render;
mod stateful;
mod stateless;

pub use proxy::ProxyView;
pub use render::RenderView;
pub use stateful::StatefulView;
pub use stateless::StatelessView;
