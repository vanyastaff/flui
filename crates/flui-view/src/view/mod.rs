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
//! - [`AnimatedView`] - Views that automatically rebuild when animations change
//! - [`InheritedView`] - Views that provide data to descendants
//! - [`RenderView`] - Views that create RenderObjects
//! - [`ProxyView`] - Single-child wrapper Views
//! - [`ParentDataView`] - Views that configure parent data on RenderObjects
//! - [`ErrorView`] - View displayed when build fails

mod animated;
mod error;
mod inherited;
mod into_view;
pub mod memo;
mod parent_data;
mod proxy;
mod render;
mod root;
mod stateful;
mod stateless;
mod view;

pub use animated::AnimatedView;
pub use error::{
    ErrorView, ErrorViewBuilder, FlutterError, clear_error_view_builder, set_error_view_builder,
};
// The containment substitute-view factory (issue #561): `ErrorView`'s own
// concept, so it lives beside `ErrorView::build_error_view` rather than in
// `tree::element_tree`. Crate-internal — `ElementTree::mount_or_substitute`
// / `update_or_substitute` and the lazy-sliver item builder are its only
// callers.
pub(crate) use error::recovery_view_for;
pub use inherited::InheritedView;
pub use into_view::{BoxedElement, BoxedView, ElementExt, IntoElement, IntoView, ViewExt};
pub use memo::Memo;
pub use parent_data::{ParentDataConfig, ParentDataView};
pub use proxy::ProxyView;
pub use render::{RenderObjectContext, RenderObjectContextError, RenderView};
pub use root::{RootRenderElement, RootRenderView};
pub use stateful::{StatefulView, ViewState};
pub use stateless::StatelessView;
pub use view::{ElementBase, View};

// Re-export unified element types from element module
pub use crate::element::{
    AnimatedElement, InheritedElement, ParentDataElement, ProxyElement, RenderElement,
    StatefulElement, StatelessElement,
};
