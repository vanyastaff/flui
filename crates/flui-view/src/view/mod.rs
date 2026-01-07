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
mod parent_data;
mod proxy;
mod render;
mod root;
mod stateful;
mod stateless;
mod view;

pub use animated::AnimatedView;
pub use error::{
    clear_error_view_builder, set_error_view_builder, ErrorElement, ErrorView, ErrorViewBuilder,
    FlutterError,
};
pub use inherited::InheritedView;
pub use into_view::{BoxedElement, BoxedView, ElementExt, IntoElement, IntoView, ViewExt};
pub use parent_data::{ParentData, ParentDataElement, ParentDataView};
pub use proxy::ProxyView;
pub use render::RenderView;
pub use root::{RootRenderElement, RootRenderView};
pub use stateful::{StatefulView, ViewState};
pub use stateless::StatelessView;
pub use view::{ElementBase, View, ViewKey};

// Re-export unified element types from element module
pub use crate::element::{
    AnimatedElement, InheritedElement, ProxyElement, RenderElement, StatefulElement,
    StatelessElement,
};
