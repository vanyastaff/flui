//! View layer for declarative UI composition.
//!
//! Views are immutable descriptions of UI that the framework converts into
//! mutable elements for lifecycle management.
//!
//! # Architecture
//!
//! ```text
//! View (immutable) → Element (mutable) → RenderObject (layout/paint)
//! ```
//!
//! # View Types
//!
//! - [`StatelessView`] - Simple views without state
//! - [`StatefulView<S>`] - Views with persistent state
//! - [`AnimatedView<L>`] - Views driven by animations
//! - [`ProviderView<T>`] - Views that provide data to descendants
//! - [`ProxyView`] - Views that wrap single child
//! - [`RenderView<P, A>`] - Views that create render objects
//!
//! # Examples
//!
//! ## Simple widget
//!
//! ```rust,ignore
//! #[derive(Clone)]
//! struct Greeting {
//!     name: String,
//! }
//!
//! impl StatelessView for Greeting {
//!     fn build(self, _ctx: &BuildContext) -> impl IntoElement {
//!         Text::new(format!("Hello, {}!", self.name))
//!     }
//! }
//! ```
//!
//! ## Stateful widget
//!
//! ```rust,ignore
//! #[derive(Clone)]
//! struct Counter {
//!     initial: i32,
//! }
//!
//! struct CounterState {
//!     count: i32,
//! }
//!
//! impl StatefulView<CounterState> for Counter {
//!     fn create_state(&self) -> CounterState {
//!         CounterState { count: self.initial }
//!     }
//!
//!     fn build(&mut self, state: &mut CounterState, ctx: &BuildContext) -> impl IntoElement {
//!         Column::new()
//!             .child(Text::new(format!("Count: {}", state.count)))
//!             .child(Button::new("+").on_click(move || {
//!                 state.count += 1;
//!                 ctx.mark_dirty();
//!             }))
//!     }
//! }
//! ```

// Core modules
pub mod build_context;
// children moved to flui-view
pub mod empty_view;
pub mod protocol;
pub mod root_view;
pub mod update_result;
#[allow(clippy::module_inception)]
pub mod view;
// view_element module removed - functionality moved to unified Element + ViewObject wrappers
pub mod view_object;
pub mod view_state;

// View type modules
pub mod view_animated;
pub mod view_provider;
pub mod view_proxy;
pub mod view_render;
pub mod view_stateful;
pub mod view_stateless;

// Wrappers for ViewObject
pub mod wrappers;

// Re-exports
pub use build_context::{
    current_build_context, with_build_context, BuildContext, BuildContextGuard,
};
// Re-export from flui-view
pub use flui_view::children::{Child, Children};
pub use protocol::{
    Animated, Provider, Proxy, RenderBox, RenderSliver, Stateful, Stateless, ViewMode, ViewProtocol,
};
pub use update_result::UpdateResult;
// View trait is internal - users should use StatelessView, StatefulView, etc.
pub(crate) use view::View;
// BuildFn and ViewElement removed - functionality moved to unified Element + ViewObject wrappers
pub use view_object::ViewObject;
pub use view_state::ViewState;

// View traits
pub use view_animated::AnimatedView;
pub use view_provider::ProviderView;

// Empty view for no-content cases
pub use empty_view::EmptyView;
pub use view_proxy::ProxyView;

// Root view for application bootstrap
pub use root_view::{RootView, RootViewError};
pub use view_render::{RenderView, RenderViewExt};
pub use view_stateful::StatefulView;
pub use view_stateless::StatelessView;

// Wrappers
pub use wrappers::{
    AnimatedViewWrapper, ProviderViewWrapper, ProxyViewWrapper, RenderViewWrapper,
    StatefulViewWrapper, StatelessViewWrapper,
};

pub use crate::element::IntoElement;
