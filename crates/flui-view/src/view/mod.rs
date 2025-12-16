//! View traits - immutable UI configuration.
//!
//! This module contains the core View traits that define how UI is declared.
//! Views are the "what" of UI - they describe what should be rendered,
//! while Elements handle the "how" of managing lifecycle and state.
//!
//! # View Types
//!
//! - [`View`] - Base trait for all Views
//! - [`StatelessView`] - Views without internal state
//! - [`StatefulView`] - Views with persistent mutable state
//! - [`InheritedView`] - Views that provide data to descendants
//! - [`RenderView`] - Views that create RenderObjects
//! - [`ProxyView`] - Single-child wrapper Views

mod inherited;
mod into_view;
mod proxy;
mod render;
mod stateful;
mod stateless;
mod view;

pub use inherited::{InheritedElement, InheritedView};
pub use into_view::{BoxedView, IntoView, ViewExt};
pub use proxy::{ProxyElement, ProxyView};
pub use render::{RenderElement, RenderView};
pub use stateful::{StatefulElement, StatefulView, ViewState};
pub use stateless::{StatelessElement, StatelessView};
pub use view::{ElementBase, View, ViewKey};
