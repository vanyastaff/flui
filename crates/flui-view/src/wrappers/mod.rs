//! ViewObject wrappers module
//!
//! Contains wrapper types that implement ViewObject for each view trait.
//!
//! # Wrapper Types
//!
//! | View Trait | Wrapper | IntoElement Helper |
//! |------------|---------|-------------------|
//! | `StatelessView` | `StatelessViewWrapper` | `Stateless(view)` |
//! | `StatefulView` | `StatefulViewWrapper` | `Stateful(view)` |
//! | `ProxyView` | `ProxyViewWrapper` | `Proxy(view)` |
//! | `ProviderView<T>` | `ProviderViewWrapper` | `Provider(view)` |
//! | `AnimatedView<L>` | `AnimatedViewWrapper` | `Animated(view)` |
//!
//! Note: `RenderViewWrapper` is in `flui_rendering::view` to avoid circular deps.
//!
//! # Architecture
//!
//! ```text
//! View trait (user implements)
//!     ↓
//! ViewWrapper (framework provides)
//!     ↓ implements ViewObject
//! Element (stores Box<dyn Any + Send>)
//!     ↓ downcast to ViewObject
//! Framework operations (build, layout, paint)
//! ```

mod animated;
mod provider;
mod proxy;
mod stateful;
mod stateless;

// Wrappers
pub use animated::AnimatedViewWrapper;
pub use provider::ProviderViewWrapper;
pub use proxy::ProxyViewWrapper;
pub use stateful::StatefulViewWrapper;
pub use stateless::StatelessViewWrapper;

// IntoElement helpers
pub use animated::Animated;
pub use provider::Provider;
pub use proxy::Proxy;
pub use stateful::Stateful;
pub use stateless::Stateless;
