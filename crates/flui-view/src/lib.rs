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
//! use flui_view::{StatelessView, BuildContext, IntoElement};
//!
//! struct Greeting {
//!     name: String,
//! }
//!
//! impl StatelessView for Greeting {
//!     fn build(self, ctx: &BuildContext) -> impl IntoElement {
//!         Text::new(format!("Hello, {}!", self.name))
//!     }
//! }
//!
//! // Use the view
//! let element = Greeting { name: "World".into() }.into_element();
//! ```
//!
//! ## Crate Dependencies
//!
//! ```text
//! flui-foundation → flui-tree → flui-element → flui-view
//!                                     ↓
//!                               (Element, IntoElement)
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
pub mod protocol;
pub mod state;
pub mod traits;
pub mod wrappers;

mod empty;

// ============================================================================
// RE-EXPORTS
// ============================================================================

// Context (re-exported from flui-element)
pub use flui_element::BuildContext;

// Protocol
pub use protocol::ViewMode;

// State
pub use state::ViewState;

// View traits
pub use traits::{AnimatedView, Listenable, ProviderView, ProxyView, StatefulView, StatelessView};

// ViewObject (re-exported from flui-element)
pub use flui_element::{ElementViewObjectExt, ProviderViewObject, ViewObject};

// Wrappers
pub use wrappers::{
    Animated, AnimatedViewWrapper, Provider, ProviderViewWrapper, Proxy, ProxyViewWrapper,
    Stateful, StatefulViewWrapper, Stateless, StatelessViewWrapper,
};

// Empty view
pub use empty::EmptyView;

// Children
pub use children::{Child, Children};

// Re-export from flui-element for convenience
pub use flui_element::{Element, ElementTree, IntoElement};
pub use flui_foundation::ElementId;

// ============================================================================
// PRELUDE
// ============================================================================

/// Commonly used types for convenient importing.
///
/// ```rust,ignore
/// use flui_view::prelude::*;
/// ```
pub mod prelude {
    pub use crate::empty::EmptyView;
    pub use crate::protocol::ViewMode;
    pub use crate::traits::{
        AnimatedView, Listenable, ProviderView, ProxyView, StatefulView, StatelessView,
    };
    pub use crate::wrappers::{Animated, Provider, Proxy, Stateful, Stateless};
    pub use flui_element::{BuildContext, Element, IntoElement};
}
