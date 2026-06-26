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
//! ```rust
//! use flui_widgets::prelude::*;
//! use flui_widgets::{column, row}; // ViewSeq macros (shadow std's same-named)
//!
//! let _tree = Container::new()
//!     .padding(EdgeInsets::all(px(8.0)))
//!     .color(Color::rgb(26, 102, 230))
//!     .child(Column::new(column![
//!         Text::new("Hello"),
//!         Padding::all(4.0).child(Text::new("World")),
//!     ]));
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

pub mod clip;
mod container;
pub mod flex;
pub mod interaction;
pub mod layout;
pub mod paint;
pub mod scroll;
pub mod stack;
pub mod text;

// ============================================================================
// Flat re-exports — `flui_widgets::Padding`, identical depth to Flutter's
// single-import surface.
// ============================================================================

pub use clip::{ClipOval, ClipRect};
pub use container::Container;
pub use flex::{Column, Expanded, Flex, Flexible, Row};
pub use interaction::{AbsorbPointer, IgnorePointer, Offstage};
pub use layout::{
    Align, AspectRatio, Baseline, Center, ConstrainedBox, FittedBox, FractionalTranslation,
    FractionallySizedBox, LimitedBox, Padding, SizedBox, Transform,
};
pub use paint::{ColoredBox, DecoratedBox, Opacity, RepaintBoundary};
pub use scroll::{
    ListView, SingleChildScrollView, SliverFixedExtentList, SliverOpacity, SliverPadding,
    SliverToBoxAdapter, Viewport,
};
pub use stack::{Positioned, Stack};
pub use text::Text;

// The heterogeneous-children macros (contract C2's static tuple path). Kept out
// of the prelude glob: their names collide with `std`'s `column!`/`row!`, so
// they must be imported explicitly (`use flui_widgets::{column, row};`), which
// shadows the std macros — a glob import would be ambiguous instead.
pub use flui_view::{column, row};

// Flex/stack configuration enums consumed by `Row`/`Column`/`Flex`/`Stack`
// (re-exported from the `flui-objects` catalog, whose canonical home is
// `flui-types::layout`).
pub use flui_objects::{CrossAxisAlignment, MainAxisAlignment, MainAxisSize, StackFit};
// `FlexFit` (the `Flexible` fit mode) lives with the parent-data it configures.
pub use flui_rendering::parent_data::FlexFit;

// ============================================================================
// Prelude
// ============================================================================

/// Commonly used widgets and supporting types for `use flui_widgets::prelude::*;`.
pub mod prelude {
    // Authoring spine re-exported so a single prelude import is enough to write
    // a widget tree (View traits, BuildContext, ViewSeq, derives). The
    // `column!`/`row!` macros are intentionally NOT globbed here (they collide
    // with `std`'s same-named macros) — import them explicitly from the crate
    // root: `use flui_widgets::{column, row};`.
    pub use flui_view::prelude::*;

    // The widget catalog.
    pub use crate::{
        AbsorbPointer, Align, AspectRatio, Baseline, Center, ClipOval, ClipRect, ColoredBox,
        Column, ConstrainedBox, Container, DecoratedBox, Expanded, FittedBox, Flex, FlexFit,
        Flexible, FractionalTranslation, FractionallySizedBox, IgnorePointer, LimitedBox, ListView,
        Offstage, Opacity, Padding, Positioned, RepaintBoundary, Row, SingleChildScrollView,
        SizedBox, SliverFixedExtentList, SliverOpacity, SliverPadding, SliverToBoxAdapter, Stack,
        Text, Transform, Viewport,
    };

    // Common configuration value types, so an app author needs only this import.
    pub use flui_geometry::{EdgeInsets, Matrix4, Pixels, px};
    pub use flui_objects::{CrossAxisAlignment, MainAxisAlignment, MainAxisSize, StackFit};
    pub use flui_rendering::constraints::BoxConstraints;
    pub use flui_types::layout::{Axis, AxisDirection, BoxFit};
    pub use flui_types::painting::Clip;
    pub use flui_types::typography::TextBaseline;
    pub use flui_types::{Alignment, Color};
}
