//! View layer
//!
//! The view layer provides the BuildContext for widget building and manages
//! the view tree during the build phase.
//!
//! # Overview
//!
//! The view layer sits between widgets and elements, providing context and
//! utilities for building the element tree from widget descriptions.
//!
//! ## Key Components
//!
//! - [`BuildContext`]: Context provided to views during build (read-only, with hooks)
//! - [`View`]: Simplified trait for reactive UI (no GATs, returns `impl IntoElement`)
//!
//! # Example
//!
//! ```rust,ignore
//! impl View for MyWidget {
//!     fn build(self, ctx: &BuildContext) -> impl IntoElement {
//!         // Use BuildContext for hooks
//!         let count = use_signal(ctx, 0);
//!         Text::new("Hello, World!")
//!     }
//! }
//! ```

pub mod any_view;
pub mod build_context;
pub mod into_element;
pub mod sealed;
#[allow(clippy::module_inception)] // view/view.rs is intentional for main View trait
pub mod view;

// BuildContext and thread-local helpers
pub use build_context::{
    current_build_context, with_build_context, BuildContext, BuildContextGuard,
};

// View trait and related types
pub use any_view::AnyView;
pub use view::{ChangeFlags, View, ViewElement};

// Simplified API exports (IntoElement, tuple syntax)
pub use into_element::{AnyElement, IntoAnyElement, IntoElement, RenderExt};

// TODO: Add view tree management for tracking widget-to-element mappings and
// efficient lookup during rebuild.

