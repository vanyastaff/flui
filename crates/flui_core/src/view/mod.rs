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
//! # Components
//!
//! - [`View`] - Core trait for UI components
//! - [`BuildContext`] - Context for hooks and tree queries
//! - [`IntoElement`] - Conversion trait for element tree insertion
//! - [`Child`] / [`Children`] - Ergonomic child wrappers
//! - [`ViewElement`] - Element managing view lifecycle
//!
//! # Examples
//!
//! ## Simple widget
//!
//! ```rust,ignore
//! #[derive(Debug)]
//! struct Greeting {
//!     name: String,
//! }
//!
//! impl View for Greeting {
//!     fn build(&self, _ctx: &BuildContext) -> impl IntoElement {
//!         Text::new(format!("Hello, {}!", self.name))
//!     }
//! }
//! ```
//!
//! ## Stateful widget
//!
//! ```rust,ignore
//! #[derive(Debug)]
//! struct Counter;
//!
//! impl View for Counter {
//!     fn build(&self, ctx: &BuildContext) -> impl IntoElement {
//!         let count = use_signal(ctx, 0);
//!
//!         Column::new()
//!             .child(Text::new(format!("Count: {}", count.get())))
//!             .child(Button::new("+").on_click(move || count.update(|n| n + 1)))
//!     }
//! }
//! ```

pub mod build_context;
pub mod children;
#[allow(clippy::module_inception)]
pub mod view;
pub mod view_element;

pub use build_context::{
    current_build_context, with_build_context, BuildContext, BuildContextGuard,
};
pub use children::{Child, Children};
pub use view::View;
pub use view_element::{BuildFn, ViewElement};

pub use crate::element::IntoElement;
