//! # FLUI View
//!
//! View traits and abstractions for the FLUI UI framework.
//!
//! This crate provides the view layer of FLUI's three-tree architecture,
//! defining how declarative UI components are structured and built.
//!
//! ## Architecture
//!
//! ```text
//! View (immutable config) → Element (mutable state) → RenderObject (layout/paint)
//! ^^^^^^^^^^^^^^^^^^^^
//! This crate!
//! ```
//!
//! ## Key Types
//!
//! - [`StatelessView`] - Views without internal state
//! - [`StatefulView`] - Views with persistent mutable state
//! - [`ViewObject`] - Dynamic dispatch interface for all views
//! - [`BuildContext`] - Context passed during view building
//!
//! ## Example
//!
//! ```rust,ignore
//! use flui_view::{StatelessView, BuildContext, IntoView};
//!
//! struct Greeting {
//!     name: String,
//! }
//!
//! impl StatelessView for Greeting {
//!     fn build(self, ctx: &dyn BuildContext) -> impl IntoView {
//!         Text::new(format!("Hello, {}!", self.name))
//!     }
//! }
//!
//! // Use the view
//! let view_obj = Greeting { name: "World".into() }.into_view_wrapped();
//! ```
//!
//! ## Crate Dependencies
//!
//! ```text
//! flui-foundation → flui-tree → flui-view
//!                                   ↓
//!                             flui-element (depends on flui-view)
//! ```

#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    clippy::all,
    clippy::pedantic
)]
#![allow(
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use,
    clippy::module_inception,
    clippy::missing_fields_in_debug,
    clippy::no_effect
)]

// ============================================================================
// MODULES
// ============================================================================

pub mod children;
pub mod context;
pub mod element;
pub mod into_view;
pub mod state;
pub mod traits;
pub mod tree;
pub mod view_mode;
pub mod view_object;
pub mod wrappers;

mod empty;

// ============================================================================
// RE-EXPORTS
// ============================================================================

// Context (defined in this crate)
pub use context::BuildContext;
#[cfg(any(test, feature = "test-utils"))]
pub use context::MockBuildContext;

// ViewMode (defined in this crate)
pub use view_mode::ViewMode;

// State
pub use state::ViewState;

// View traits
pub use traits::{
    AnimatedView, Listenable, ProviderView, ProxyView, RenderObjectFor, RenderView,
    RenderViewConfig, RenderViewExt, RenderViewLeaf, RenderViewWithChild, RenderViewWithChildren,
    RenderViewWithOptionalChild, StatefulView, StatelessView, UpdateResult,
};

// ViewObject (defined in this crate)
pub use view_object::ViewObject;

// Wrappers
pub use wrappers::{
    Animated, AnimatedViewWrapper, Provider, ProviderViewWrapper, Proxy, ProxyViewWrapper,
    Stateful, StatefulViewWrapper, Stateless, StatelessViewWrapper,
};

// Empty view
pub use empty::EmptyView;

// IntoView trait
pub use into_view::IntoView;

// Children
pub use children::{Child, Children};

// Re-export from flui-foundation for convenience
pub use flui_foundation::ElementId;

// Re-export key types from flui-foundation
pub use flui_foundation::{
    GlobalKey, Key, KeyRef, Keyed, ObjectKey, UniqueKey, ValueKey, ViewKey, WithKey,
};

// Element types (ViewElement, ViewLifecycle, ViewFlags)
pub use element::{AtomicViewFlags, PendingChildren, ViewElement, ViewFlags, ViewLifecycle};

// Tree types (ViewTree, ViewNode, ViewId)
pub use tree::{ViewId, ViewNode, ViewTree};

// ============================================================================
// PRELUDE
// ============================================================================

/// Commonly used types for convenient importing.
///
/// ```rust,ignore
/// use flui_view::prelude::*;
/// ```
pub mod prelude {
    pub use crate::context::BuildContext;
    pub use crate::element::{ViewElement, ViewLifecycle};
    pub use crate::empty::EmptyView;
    pub use crate::into_view::IntoView;
    pub use crate::traits::{
        AnimatedView, Listenable, ProviderView, ProxyView, StatefulView, StatelessView,
    };
    pub use crate::view_mode::ViewMode;
    pub use crate::view_object::ViewObject;
    pub use crate::wrappers::{Animated, Provider, Proxy, Stateful, Stateless};

    // Re-export key types from flui-foundation
    pub use flui_foundation::{
        GlobalKey, Key, KeyRef, Keyed, ObjectKey, UniqueKey, ValueKey, ViewKey, WithKey,
    };
}
