//! # FLUI Widgets
//!
//! The user-facing, Flutter-style widget catalog for FLUI — the layer an app
//! author composes. Every widget here is a small, immutable **configuration
//! object** that either:
//!
//! - wraps a render object from [`flui_objects`] (a [`RenderView`]), or
//! - composes other widgets (a [`StatelessView`]), or
//! - configures parent-layout data on its single child (a [`ParentDataView`]).
//!
//! This mirrors Flutter's `widgets/` package: a widget is "a thin configuration
//! object over a render object." The render *machine* (layout/paint/compositing)
//! lives in [`flui_rendering`] and [`flui_objects`]; this crate is the
//! declarative surface over it.
//!
//! ## Architecture
//!
//! ```text
//! flui-widgets  ← you are here (declarative config)
//!     │  View → Element → RenderObject
//!     ▼
//! flui-view     ← View/Element lifecycle + reconciliation
//!     ▼
//! flui-objects  ← concrete RenderBox catalog
//!     ▼
//! flui-rendering ← layout/paint/composite engine
//! ```
//!
//! ## Authoring style
//!
//! Widgets favour a Flutter-like constructor + chainable-config surface (with
//! `bon` builders reserved for the widest future configuration objects). Single
//! children are taken as `impl IntoView`; heterogeneous child lists use the
//! [`ViewSeq`](flui_view::seq::ViewSeq)-backed `column!`/`row!` macros (the
//! static tuple path) or `Vec<BoxedView>` (the dynamic path).
//!
//! ```rust,ignore
//! use flui_widgets::prelude::*;
//!
//! Container::new()
//!     .padding(EdgeInsets::all(8.0))
//!     .color(Color::from_rgb(0.1, 0.4, 0.9))
//!     .child(Column::new(column![
//!         Text::new("Hello"),
//!         Padding::all(4.0).child(Text::new("World")),
//!     ]))
//! # ;
//! ```
//!
//! [`RenderView`]: flui_view::prelude::RenderView
//! [`StatelessView`]: flui_view::prelude::StatelessView
//! [`ParentDataView`]: flui_view::prelude::ParentDataView

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
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::doc_markdown,
    // `flex/flex.rs`, `text/text.rs`: a one-type family module named after its
    // type is the catalog's house style (matches `flui-view`/`flui-objects`).
    clippy::module_inception
)]

// ============================================================================
// Modules
// ============================================================================

mod support;

mod container;
pub mod flex;
pub mod layout;
pub mod paint;
pub mod text;

// ============================================================================
// Flat re-exports — `flui_widgets::Padding`, identical depth to Flutter's
// single-import surface.
// ============================================================================

pub use container::Container;
pub use flex::{Column, Flex, Row};
pub use layout::{Align, Center, ConstrainedBox, LimitedBox, Padding, SizedBox, Transform};
pub use paint::{ColoredBox, DecoratedBox, Opacity};
pub use text::Text;

// Flex configuration enums consumed by `Row`/`Column`/`Flex` (re-exported from
// the `flui-objects` catalog, whose canonical home is `flui-types::layout`).
pub use flui_objects::{CrossAxisAlignment, MainAxisAlignment, MainAxisSize};

// ============================================================================
// Prelude
// ============================================================================

/// Commonly used widgets and supporting types for `use flui_widgets::prelude::*;`.
pub mod prelude {
    // Authoring spine re-exported so a single prelude import is enough to write
    // a widget tree (View traits, BuildContext, ViewSeq, derives).
    pub use flui_view::prelude::*;
    // The heterogeneous-children macros (contract C2's static tuple path).
    pub use flui_view::{column, row};

    // The widget catalog.
    pub use crate::{
        Align, Center, ColoredBox, Column, ConstrainedBox, Container, DecoratedBox, Flex,
        LimitedBox, Opacity, Padding, Row, SizedBox, Text, Transform,
    };

    // Common configuration value types, so an app author needs only this import.
    pub use flui_geometry::{EdgeInsets, Matrix4};
    pub use flui_objects::{CrossAxisAlignment, MainAxisAlignment, MainAxisSize};
    pub use flui_rendering::constraints::BoxConstraints;
    pub use flui_types::{Alignment, Color};
}
