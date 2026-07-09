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

// Lint levels come from `[workspace.lints]`. Ship bar (wave 3): every public
// item is documented; keep it that way.
#![deny(missing_docs)]
// `flex/flex.rs`, `text/text.rs`: a one-type family module named after its
// type is the catalog's house style (matches `flui-view`/`flui-objects`).
#![allow(clippy::module_inception)]

// ============================================================================
// Modules
// ============================================================================

mod support;

pub mod animated;
pub mod app;
mod async_builders;
pub mod clip;
mod container;
pub mod flex;
pub mod icon;
pub mod image;
pub mod interaction;
pub mod layout;
// The route stack — ADR-0019 U2. Private and pure data: no widget, no
// `Navigator`, no public API. U3 adds the `Navigator` view on top of it.
mod navigator;
// `Overlay` / `OverlayEntry` — ADR-0019 U1, the first `Navigator` prerequisite.
// Deliberately private: nothing here is exported from the crate root or the
// prelude until ADR-0019 U4's parity + sign-off gate. `Navigator` (U3) is the
// intended in-crate consumer. (A `///` doc here would be concatenated with the
// module's own `//!` docs and resolve its intra-doc links in the crate root.)
mod overlay;
pub mod paint;
pub mod scroll;
pub mod semantics;
pub mod stack;
pub mod text;
pub mod transitions;
pub mod wrap;

// ============================================================================
// Flat re-exports — `flui_widgets::Padding`, identical depth to Flutter's
// single-import surface.
// ============================================================================

// Application-scoped inherited widgets: ambient screen data and theming.
pub use app::{MediaQuery, MediaQueryData, SafeArea, Theme, ThemeData};
// `Brightness` is the value type shared by `MediaQueryData` and `ThemeData`;
// re-exported here so callers need only `use flui_widgets::Brightness`.
pub use flui_types::platform::Brightness;

pub use animated::{
    AnimatedAlign, AnimatedAlignState, AnimatedContainer, AnimatedContainerState, AnimatedOpacity,
    AnimatedOpacityState, AnimatedPadding, AnimatedPaddingState, AnimatedSize, AnimatedSizeState,
    VsyncScope,
};
pub use clip::{ClipOval, ClipPath, ClipRRect, ClipRect};
// `Image` widget over `RenderImage`; provider types live in the same module.
// `ImageFit`/`ImageAlignment` are re-exported from `flui-objects` so consumers
// need only import from `flui-widgets`, not from lower-level crates.
pub use async_builders::{
    BoxedResultFuture, BoxedResultStream, FutureBuilder, FutureFactory, InitialDataFactory,
    SnapshotBuilder, Stream, StreamBuilder, StreamFactory,
};
pub use container::Container;
pub use flex::{Column, Expanded, Flex, Flexible, Row, Spacer};
pub use flui_objects::{ImageAlignment, ImageFit};
pub use icon::{Icon, IconData, IconTheme, IconThemeData};
#[cfg(feature = "network-images")]
pub use image::NetworkImage;
pub use image::{
    DirectImageProvider, FileImage, Image, ImageProvider, ImageProviderError, MemoryImage,
};
pub use interaction::{
    AbsorbPointer, GestureArenaScope, GestureDetector, GestureDetectorState, IgnorePointer,
    Listener, MouseRegion, Offstage, Visibility,
};
pub use layout::{
    Align, AspectRatio, Baseline, Center, ConstrainedBox, CustomMultiChildLayout,
    CustomSingleChildLayout, FittedBox, Flow, FractionalTranslation, FractionallySizedBox,
    IntrinsicHeight, IntrinsicWidth, LayoutBuilder, LayoutId, LimitedBox, ListBody, OverflowBox,
    Padding, RotatedBox, SizedBox, SizedOverflowBox, Table, TableCell, TableRow, Transform,
};
// `OverflowBoxFit` configures `OverflowBox`'s size policy; exposed at crate root
// so consumers don't need to reach into `flui_objects`.
pub use flui_objects::OverflowBoxFit;
// `TableColumnWidth`/`TableCellVerticalAlignment` configure `Table`/`TableCell`;
// `TableBorder` configures `Table::border`. Re-exported here so widget authors
// need only import from `flui_widgets`.
pub use flui_types::layout::{TableCellVerticalAlignment, TableColumnWidth};
pub use flui_types::styling::TableBorder;
pub use paint::{ColoredBox, CustomPaint, DecoratedBox, Opacity, RepaintBoundary};
pub use scroll::{
    BouncingScrollPhysics, ClampingScrollPhysics, CustomScrollView, GridView, ListView,
    RefreshController, RefreshIndicator, RefreshIndicatorState, ScrollController, ScrollPhysics,
    Scrollable, Scrollbar, SharedScrollPhysics, ShrinkWrappingViewport, SingleChildScrollView,
    SliverChildBuilderDelegate, SliverFillRemaining, SliverFillRemainingAndOverscroll,
    SliverFillRemainingWithScrollable, SliverFillViewport, SliverFixedExtentList, SliverGrid,
    SliverIgnorePointer, SliverList, SliverOffstage, SliverOpacity, SliverPadding,
    SliverToBoxAdapter, Viewport,
};
pub use semantics::{ExcludeSemantics, MergeSemantics, Semantics};
pub use stack::{IndexedStack, Positioned, Stack};
pub use text::{EditableText, EditableTextState, RichText, Text, TextEditingController, TextField};
pub use transitions::{
    AnimatedBuilder, AnimatedBuilderState, FadeTransition, FadeTransitionState, RotationTransition,
    RotationTransitionState, ScaleTransition, ScaleTransitionState,
};
pub use wrap::Wrap;

// The heterogeneous-children macros (contract C2's static tuple path). Kept out
// of the prelude glob: their names collide with `std`'s `column!`/`row!`, so
// they must be imported explicitly (`use flui_widgets::{column, row};`), which
// shadows the std macros — a glob import would be ambiguous instead.
pub use flui_view::{column, row};

// Flex/stack configuration enums consumed by `Row`/`Column`/`Flex`/`Stack`
// (re-exported from the `flui-objects` catalog, whose canonical home is
// `flui-types::layout`).
pub use flui_objects::{CrossAxisAlignment, MainAxisAlignment, MainAxisSize, StackFit};
// `WrapAlignment`/`WrapCrossAlignment` configure `Wrap`'s main-axis distribution
// and per-child cross-axis positioning.
pub use flui_objects::{WrapAlignment, WrapCrossAlignment};
// `FlexFit` (the `Flexible` fit mode) lives with the parent-data it configures.
pub use flui_rendering::parent_data::FlexFit;
// Grid, custom-paint, flow, and custom layout delegates — always
// available (un-gated since their companion render objects ship in the
// default build). Re-exported here so widget authors need only import from
// `flui_widgets`, matching Flutter's single-import surface.
pub use flui_rendering::delegates::{
    AspectRatioDelegate, CenterLayoutDelegate, CustomPainter, FlowDelegate, FlowPaintingContext,
    MultiChildLayoutContext, MultiChildLayoutDelegate, SingleChildLayoutDelegate,
    SliverGridDelegate, SliverGridDelegateWithFixedCrossAxisCount,
    SliverGridDelegateWithMaxCrossAxisExtent, SliverGridLayout,
};
// Pointer-routing surface for `Listener`: the `HitTestBehavior` knob and the
// pointer event types its callbacks receive.
pub use flui_rendering::hit_testing::{
    CursorIcon, DeviceId, EventPropagation, HitTestBehavior, PointerEvent,
};
// Drag details surfaced by `GestureDetector`'s `on_pan_*` callbacks.
pub use flui_interaction::{
    DragEndDetails, DragStartDetails, DragUpdateDetails, PointerPanZoomEvent,
};
pub use flui_rendering::semantics::{
    SemanticsConfiguration, SemanticsProperties, SemanticsRole,
    TextDirection as SemanticsTextDirection,
};

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
        AbsorbPointer, Align, AspectRatio, Baseline, Brightness, Center, ClipOval, ClipPath,
        ClipRRect, ClipRect, ColoredBox, Column, ConstrainedBox, Container, CustomMultiChildLayout,
        CustomPaint, CustomScrollView, CustomSingleChildLayout, DecoratedBox, EditableText,
        EditableTextState, ExcludeSemantics, Expanded, FittedBox, Flex, FlexFit, Flexible, Flow,
        FractionalTranslation, FractionallySizedBox, FutureBuilder, GestureArenaScope,
        GestureDetector, GridView, Icon, IconData, IconTheme, IconThemeData, IgnorePointer, Image,
        ImageAlignment, ImageFit, ImageProvider, IndexedStack, IntrinsicHeight, IntrinsicWidth,
        LayoutBuilder, LayoutId, LimitedBox, ListBody, ListView, Listener, MediaQuery,
        MediaQueryData, MergeSemantics, MouseRegion, Offstage, Opacity, OverflowBox,
        OverflowBoxFit, Padding, Positioned, RepaintBoundary, RichText, RotatedBox, Row, SafeArea,
        ScrollController, Scrollable, Scrollbar, Semantics, ShrinkWrappingViewport,
        SingleChildScrollView, SizedBox, SizedOverflowBox, SliverChildBuilderDelegate,
        SliverFillRemaining, SliverFillRemainingAndOverscroll, SliverFillRemainingWithScrollable,
        SliverFillViewport, SliverFixedExtentList, SliverGrid, SliverIgnorePointer, SliverList,
        SliverOffstage, SliverOpacity, SliverPadding, SliverToBoxAdapter, Spacer, Stack,
        StreamBuilder, Table, TableCell, TableRow, Text, TextEditingController, TextField, Theme,
        ThemeData, Transform, Viewport, Visibility, Wrap,
    };

    // Common configuration value types, so an app author needs only this import.
    pub use crate::{
        AspectRatioDelegate, CenterLayoutDelegate, CustomPainter, FlowDelegate,
        FlowPaintingContext, MultiChildLayoutContext, MultiChildLayoutDelegate,
        SemanticsConfiguration, SemanticsProperties, SemanticsRole, SemanticsTextDirection,
        SingleChildLayoutDelegate, SliverGridDelegate, SliverGridDelegateWithFixedCrossAxisCount,
        SliverGridDelegateWithMaxCrossAxisExtent, SliverGridLayout, TableBorder,
        TableCellVerticalAlignment, TableColumnWidth,
    };
    pub use flui_geometry::{EdgeInsets, Matrix4, Pixels, px};
    pub use flui_interaction::{
        DragEndDetails, DragStartDetails, DragUpdateDetails, PointerPanZoomEvent,
    };
    pub use flui_objects::{CrossAxisAlignment, MainAxisAlignment, MainAxisSize, StackFit};
    pub use flui_objects::{WrapAlignment, WrapCrossAlignment};
    pub use flui_rendering::constraints::BoxConstraints;
    pub use flui_rendering::hit_testing::{
        CursorIcon, DeviceId, EventPropagation, HitTestBehavior, PointerEvent,
    };
    pub use flui_types::layout::{Axis, AxisDirection, BoxFit};
    pub use flui_types::painting::Clip;
    pub use flui_types::typography::TextBaseline;
    pub use flui_types::{Alignment, Color};
}
