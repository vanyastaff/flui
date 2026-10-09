//! # FLUI Widgets
//!
//! The user-facing widget catalog for FLUI — the layer an app
//! author composes. Every widget here is a small, immutable **configuration
//! object** that either:
//!
//! - wraps a render object from [`flui_objects`] (a [`RenderView`]), or
//! - composes other widgets (a [`StatelessView`]), or
//! - configures parent-layout data on its single child (a [`ParentDataView`]).
//!
//! A widget is a thin configuration
//! object over a render object. The render *machine* (layout/paint/compositing)
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
//! Widgets favour a constructor + chainable-config surface (with
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
//!     .padding(EdgeInsets::all(8.0))
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
#![expect(clippy::module_inception)]
// ADR-0027: navigator/overlay/hero/focus widget state is owner-local, but the
// current handle graph still uses `Arc` at many internal seams. Do not restore
// `Send + Sync` to UI callbacks or route/page builders to satisfy this lint; a
// focused owner-local handle migration can replace these with `Rc` later.
#![expect(clippy::arc_with_non_send_sync)]

// `#[derive(Routable)]` names this crate by its absolute path, which also
// resolves inside the crate and its doctests through this alias.
#[allow(
    unused_extern_crates,
    reason = "derive expansions resolve the owner by its absolute crate name"
)]
extern crate self as flui_widgets;

// ============================================================================
// Modules
// ============================================================================

mod support;

// Framework seams for the sibling `flui-*` widget crates: no semver
// guarantee, workspace-only.
#[doc(hidden)]
pub mod __private;
// Temporary access to private items for this crate's own integration tests
// (ADR-0083 §4): no semver guarantee, `crates/flui-widgets/tests` only.
#[doc(hidden)]
pub mod __test_access;
mod anchored_box;

pub mod animated;
pub mod app;
mod async_builders;
pub mod clip;
mod container;
pub mod controls;
pub mod flex;
pub mod form;
pub mod icon;
pub mod image;
pub mod interaction;
pub mod layout;
pub mod localization;
mod media_query;

/// `Navigator` and routing — see `docs/adr/ADR-0019-navigator-routing-seam.md`. The
/// route stack, its lifecycle, the flush algorithm and the result channel are
/// not public API (some are nameable only through the doc-hidden, temporary
/// `__test_access`, ADR-0083 §4); the signed-off surface is re-exported from
/// the crate root below.
pub mod navigator;
// `Overlay` / `OverlayEntry`, the first `Navigator` prerequisite. The module
// stays private: the types (ADR-0076) and the mutation surface
// (`insert`/`rearrange`/`InsertPosition`/the entry lifecycle, ADR-0076) are
// re-exported from the crate root, and nothing else is nameable, so
// `OverlayScope` and the `Theater`/`OverlayState` machinery stay private. (A
// `///` doc here would be concatenated with the module's own `//!` docs and
// resolve its intra-doc links in the crate root.)
mod overlay;
pub mod paint;
pub mod physical_model;
// The typed `Router` (ADR-0093); its items are re-exported from the crate root.
pub mod router;
pub mod scroll;
pub mod semantics;
pub mod stack;
pub mod text;
pub mod transitions;
mod value_listenable_builder;
pub mod widget_state;
pub mod wrap;

// ============================================================================
// Flat re-exports — `flui_widgets::Padding`, one import path for every widget.
// ============================================================================

// Application composition and the
// `InheritedTheme` trait a theme widget (e.g. `flui_material::Theme`)
// implements. The Material `Theme`/`ThemeData` widget itself lives in
// `flui-material` — see `app` module docs.
pub use app::{
    AppBuilder, AppForm, InheritedTheme, NavigatorForm, RouterForm, SafeArea, WidgetsApp,
    WidgetsAppState,
};
pub use media_query::{MediaQuery, MediaQueryData};
// `Brightness` is the value type `MediaQueryData` (and any theme's
// brightness field) uses; re-exported here so callers need only
// `use flui_widgets::Brightness`.
pub use flui_platform_api::Brightness;
// Ambient direction + localized-resource infrastructure — see
// `localization`'s module docs for the sync-only-v1 limits.
pub use localization::{
    BoxedLocalizationsDelegate, BoxedWidgetsLocalizations, DefaultWidgetsLocalizations,
    DefaultWidgetsLocalizationsDelegate, Directionality, GlobalWidgetsLocalizations,
    GlobalWidgetsLocalizationsDelegate, Localizations, LocalizationsDelegate, RTL_LANGUAGES,
    WidgetsLocalizations, basic_locale_list_resolution, resolve_alignment,
};

pub use animated::{
    AnimatedAlign, AnimatedAlignState, AnimatedContainer, AnimatedContainerState, AnimatedOpacity,
    AnimatedOpacityState, AnimatedPadding, AnimatedPaddingState, AnimatedRotation,
    AnimatedRotationState, AnimatedSize, AnimatedSizeState, AnimatedSwitcher,
    AnimatedSwitcherLayoutBuilder, AnimatedSwitcherState, AnimatedSwitcherTransitionBuilder,
    RotationPath, TickerMode, TickerModeState, VsyncScope,
};
pub use clip::{ClipOval, ClipPath, ClipRRect, ClipRect, Oval};
// `Image` widget over `RenderImage`; provider types live in the same module.
// `ImageFit`/`ImageAlignment` are re-exported from `flui-objects` so consumers
// need only import from `flui-widgets`, not from lower-level crates.
pub use async_builders::{
    BoxedResultFuture, BoxedResultStream, FutureBuilder, FutureFactory, InitialDataFactory,
    SnapshotBuilder, Stream, StreamBuilder, StreamFactory,
};
pub use container::Container;
pub use controls::{
    ActivityIndicator, ActivityIndicatorState, Disclosure, DisclosureState, ExpansionState, Slider,
    SliderState,
};
pub use flex::{Column, Expanded, Flex, Flexible, Row, Spacer};
pub use flui_objects::{ImageAlignment, ImageFit};
pub use icon::{Icon, IconData, IconTheme, IconThemeData};
#[cfg(feature = "asset-images")]
pub use image::AssetImage;
#[cfg(feature = "asset-images")]
pub use image::ImageState;
pub use image::{
    DirectImageProvider, FileImage, Image, ImageCacheKey, ImageProvider, ImageProviderError,
    MemoryImage,
};
#[cfg(feature = "network-images")]
pub use image::{NetworkImage, NetworkImageKey};
pub use interaction::{
    AbsorbPointer, Action, ActionOutcome, Actions, ActivateIntent, ButtonActivateIntent,
    CallbackAction, CallbackShortcuts, CopySelectionTextIntent, DefaultFocusTraversal,
    DefaultFocusTraversalState, DirectionalFocusAction, DirectionalFocusIntent, DismissDirection,
    DismissDirectionCallback, DismissUpdateCallback, DismissUpdateDetails, Dismissible,
    DismissibleState, DragPosition, DragTarget, DragTargetAccept, DragTargetBuilder,
    DragTargetDetails, DragTargetLeave, DragTargetMove, DragTargetSlot, DragTargetState,
    DragTargetWillAccept, Draggable, DraggableCanceledDetails, DraggableDetails, DraggableState,
    ErasedDragData, ExcludeFocus, Focus, FocusChangeHandler, FocusRoot, FocusRootState, FocusScope,
    FocusScopeState, FocusState, FocusTraversalGroup, FocusTraversalGroupState, GestureArenaScope,
    GestureDetector, GestureDetectorState, IgnorePointer, Intent, InteractionEndDetails,
    InteractionStartDetails, InteractionUpdateDetails, InteractiveViewer, InteractiveViewerState,
    Listener, MetaData, MouseRegion, NextFocusAction, NextFocusIntent, Offstage, PanAxis,
    PasteTextIntent, PreviousFocusAction, PreviousFocusIntent, RawButton, SelectAllTextIntent,
    ShortcutCallback, Shortcuts, ShortcutsState, SingleActivator, TransformationController,
    Visibility, VisibilityGate, WheelScaleGate,
};
pub use layout::{
    Align, AspectRatio, Baseline, Center, ConstrainedBox, CustomMultiChildLayout,
    CustomSingleChildLayout, FittedBox, Flow, FractionalTranslation, FractionallySizedBox,
    IgnoreBaseline, IntrinsicHeight, IntrinsicWidth, LayoutBuilder, LayoutId, LimitedBox, ListBody,
    OverflowBox, Padding, PreferredSize, PreferredSizeView, RotatedBox, SizedBox, SizedOverflowBox,
    Table, TableCell, TableRow, Transform, UnconstrainedBox,
};
// `OverflowBoxFit` configures `OverflowBox`'s size policy; exposed at crate root
// so consumers don't need to reach into `flui_objects`.
pub use flui_objects::OverflowBoxFit;
// `TableColumnWidth`/`TableCellVerticalAlignment` configure `Table`/`TableCell`;
// `TableBorder` configures `Table::border`. Re-exported here so widget authors
// need only import from `flui_widgets`.
pub use flui_objects::TableColumnWidth;
pub use flui_painting::styling::TableBorder;
pub use flui_rendering::parent_data::TableCellVerticalAlignment;
pub use form::{
    AutovalidateMode, Form, FormField, FormFieldHandle, FormFieldHandleAlreadyAttached,
    FormFieldSetter, FormFieldState, FormFieldValidator, FormHandle, FormHandleAlreadyAttached,
    FormState, RawTextFormField, RawTextFormFieldState,
};
pub use navigator::{
    FlightDirection, GeneratedRoute, Hero, HeroController, HeroControllerScope, HeroMode,
    KeyedSettings, NamedRouteError, Navigator, NavigatorCommand, NavigatorCommandError,
    NavigatorCommandOutcome, NavigatorCommandTarget, NavigatorHandle, NavigatorObserver,
    NavigatorRoute, NavigatorState, PageRoute, PopInvokedCallback, PopScope, PopupRoute,
    PushCompletion, Route, RouteAnimation, RouteArguments, RouteBindingSlot, RouteContentBuilder,
    RouteId, RouteKey, RoutePageBuilder, RouteRequest, RouteResult, RouteSettings,
    RouteTransitionsBuilder, SimpleRoute, TickerCanceled, TickerFuture,
};
// The `Overlay::of`/`maybe_of` lookup contract (ADR-0076) and the types it
// resolves. The mutation surface (`insert`/`rearrange`/…) stays private to
// the crate — `Navigator` and `Draggable`'s feedback layer are its callers.
pub use overlay::{InsertPosition, Overlay, OverlayEntry, OverlayEntryId, OverlayHandle};
pub use paint::{ColoredBox, CustomPaint, DecoratedBox, Opacity, RepaintBoundary};
pub use physical_model::{PhysicalModel, PhysicalShape};
pub use router::{
    Routable, RouteParseError, RoutePath, Router, RouterError, RouterHandle, RouterState,
};
pub use scroll::{
    BouncingScrollPhysics, ClampingScrollPhysics, CustomScrollView, GridView, ListView,
    OverScrollHeaderStretchConfiguration, PageController, PageScrollPhysics, PageView,
    PageViewState, RefreshController, RefreshIndicator, RefreshIndicatorState, ScrollController,
    ScrollMetrics, ScrollPhysics, Scrollable, Scrollbar, SharedScrollPhysics,
    ShrinkWrappingViewport, SingleChildScrollView, SliverChildBuilderDelegate, SliverFillRemaining,
    SliverFillRemainingAndOverscroll, SliverFillRemainingWithScrollable, SliverFillViewport,
    SliverFixedExtentList, SliverGrid, SliverIgnorePointer, SliverList, SliverMainAxisGroup,
    SliverOffstage, SliverOpacity, SliverPadding, SliverPersistentHeader,
    SliverPersistentHeaderDelegate, SliverToBoxAdapter, StretchTriggerSignal, Viewport,
};
pub use scroll::{FloatingHeaderSnapConfiguration, ScrollPositionScope};
pub use semantics::{ExcludeSemantics, IndexedSemantics, MergeSemantics, Semantics};
pub use stack::{IndexedStack, Positioned, Stack};
pub use text::{
    DefaultTextStyle, EditableText, EditableTextState, RawTextField, RawTextFieldState, RichText,
    SubmitCallback, Text, TextEditingController,
};
pub use transitions::{
    AnimatedBuilder, AnimatedBuilderState, FadeTransition, FadeTransitionState, RotationTransition,
    RotationTransitionState, ScaleTransition, ScaleTransitionState, SlideTransition,
    SlideTransitionState,
};
pub use value_listenable_builder::{
    ValueListenableBuilder, ValueListenableBuilderState, ValueWidgetBuilder,
};
// The interactive-state vocabulary a widget's visual properties can vary
// over (hover/focus/press/…) — see the module's own docs for named
// deferrals.
pub use widget_state::{
    WidgetState, WidgetStateConstraint, WidgetStateProperty, WidgetStates, WidgetStatesController,
};
pub use wrap::Wrap;

// The heterogeneous-children macros (contract C2's static tuple path). Kept out
// of the prelude glob: their names collide with `std`'s `column!`/`row!`, so
// they must be imported explicitly (`use flui_widgets::{column, row};`), which
// shadows the std macros — a glob import would be ambiguous instead.
pub use flui_view::{column, row};

// Flex/stack configuration enums consumed by `Row`/`Column`/`Flex`/`Stack`
// (re-exported from the `flui-objects` catalog, their home beside
// the render objects that read them).
pub use flui_objects::{CrossAxisAlignment, MainAxisAlignment, MainAxisSize, StackFit};
// `WrapAlignment`/`WrapCrossAlignment` configure `Wrap`'s main-axis distribution
// and per-child cross-axis positioning.
pub use flui_objects::{WrapAlignment, WrapCrossAlignment};
// `FlexFit` (the `Flexible` fit mode) lives with the parent-data it configures.
pub use flui_rendering::parent_data::FlexFit;
// Grid, custom-paint, flow, and custom layout delegates — always
// available (un-gated since their companion render objects ship in the
// default build). Re-exported here so widget authors need only import from
// `flui_widgets`.
pub use flui_rendering::delegates::{
    AspectRatioDelegate, CenterLayoutDelegate, CustomPainter, FlowDelegate, FlowPaintingContext,
    MultiChildLayoutContext, MultiChildLayoutDelegate, SingleChildLayoutDelegate,
    SliverGridDelegate, SliverGridDelegateWithFixedCrossAxisCount,
    SliverGridDelegateWithMaxCrossAxisExtent, SliverGridLayout,
};
// Pointer-routing surface for `Listener`: the `HitTestBehavior` knob and the
// pointer event types its callbacks receive.
pub use flui_rendering::hit_testing::{
    CursorIcon, DeviceId, EventPropagation, HitTestBehavior, PointerDispatch, PointerEvent,
};
// The shared scroll state `ScrollController::position()` returns and
// `Viewport`/`SingleChildScrollView::position()` accept — a widget author
// composing a custom scrollable directly on `Viewport` needs to name this
// type without reaching past `flui-widgets` into `flui-rendering`.
pub use flui_rendering::view::ScrollPosition;
// Drag details surfaced by `GestureDetector`'s `on_pan_*` callbacks.
pub use flui_interaction::{
    DragDownDetails, DragEndDetails, DragStartDetails, DragUpdateDetails, GestureEndReason,
    PanZoomEvent,
};
pub use flui_rendering::semantics::{
    NumericRange, NumericRangeError, SemanticsConfiguration, SemanticsProperties, SemanticsRole,
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
    // Ergonomic local-state cells (fold the `Rc<Cell<_>>`/`Option<RebuildHandle>`
    // pattern into one bindable, cloneable value). Already covered by the glob
    // above; named here too, next to `StatefulView`/`ViewState`/`RebuildHandle`,
    // so they show up in a symbol search of this module.
    pub use flui_view::{StateCell, StateHandle};

    // The widget catalog.
    pub use crate::{
        AbsorbPointer, Action, ActionOutcome, Actions, ActivateIntent, Align, AspectRatio,
        AutovalidateMode, Baseline, Brightness, ButtonActivateIntent, CallbackAction,
        CallbackShortcuts, Center, ClipOval, ClipPath, ClipRRect, ClipRect, ColoredBox, Column,
        ConstrainedBox, Container, CopySelectionTextIntent, CustomMultiChildLayout, CustomPaint,
        CustomScrollView, CustomSingleChildLayout, DecoratedBox, DefaultFocusTraversal,
        DefaultFocusTraversalState, DefaultTextStyle, DefaultWidgetsLocalizations, Directionality,
        Disclosure, DisclosureState, DragTarget, Draggable, EditableText, EditableTextState,
        ExcludeFocus, ExcludeSemantics, Expanded, ExpansionState, FittedBox, Flex, FlexFit,
        Flexible, FlightDirection, Flow, Focus, FocusRoot, FocusScope, FocusTraversalGroup, Form,
        FormField, FormFieldHandle, FormHandle, FractionalTranslation, FractionallySizedBox,
        FutureBuilder, GestureArenaScope, GestureDetector, GridView, Hero, HeroController,
        HeroMode, Icon, IconData, IconTheme, IconThemeData, IgnoreBaseline, IgnorePointer, Image,
        ImageAlignment, ImageFit, ImageProvider, IndexedSemantics, IndexedStack, InheritedTheme,
        Intent, IntrinsicHeight, IntrinsicWidth, LayoutBuilder, LayoutId, LimitedBox, ListBody,
        ListView, Listener, Localizations, LocalizationsDelegate, MediaQuery, MediaQueryData,
        MergeSemantics, MouseRegion, Navigator, NavigatorHandle, NextFocusAction, NextFocusIntent,
        Offstage, Opacity, OverflowBox, OverflowBoxFit, Overlay, OverlayEntry, OverlayEntryId,
        OverlayHandle, Padding, PageController, PageRoute, PageScrollPhysics, PageView,
        PasteTextIntent, PhysicalModel, PhysicalShape, PopScope, PopupRoute, Positioned,
        PreferredSize, PreferredSizeView, PreviousFocusAction, PreviousFocusIntent, RawButton,
        RawTextField, RawTextFieldState, RawTextFormField, RepaintBoundary, RichText, RotatedBox,
        Routable, RoutePath, Router, RouterHandle, Row, SafeArea, ScrollController, Scrollable,
        Scrollbar, SelectAllTextIntent, Semantics, Shortcuts, ShrinkWrappingViewport, SimpleRoute,
        SingleActivator, SingleChildScrollView, SizedBox, SizedOverflowBox, Slider, SliderState,
        SliverChildBuilderDelegate, SliverFillRemaining, SliverFillRemainingAndOverscroll,
        SliverFillRemainingWithScrollable, SliverFillViewport, SliverFixedExtentList, SliverGrid,
        SliverIgnorePointer, SliverList, SliverOffstage, SliverOpacity, SliverPadding,
        SliverToBoxAdapter, Spacer, Stack, StreamBuilder, SubmitCallback, Table, TableCell,
        TableRow, Text, TextEditingController, TickerMode, Transform, UnconstrainedBox,
        ValueListenableBuilder, Viewport, Visibility, VisibilityGate, WidgetState,
        WidgetStateConstraint, WidgetStateProperty, WidgetStates, WidgetStatesController,
        WidgetsApp, WidgetsLocalizations, Wrap,
    };

    // Common configuration value types, so an app author needs only this import.
    pub use crate::basic_locale_list_resolution;
    pub use crate::{
        AspectRatioDelegate, BoxedLocalizationsDelegate, BoxedWidgetsLocalizations,
        CenterLayoutDelegate, CustomPainter, DefaultWidgetsLocalizationsDelegate, FlowDelegate,
        FlowPaintingContext, MultiChildLayoutContext, MultiChildLayoutDelegate, NumericRange,
        NumericRangeError, SemanticsConfiguration, SemanticsProperties, SemanticsRole,
        SemanticsTextDirection, SingleChildLayoutDelegate, SliverGridDelegate,
        SliverGridDelegateWithFixedCrossAxisCount, SliverGridDelegateWithMaxCrossAxisExtent,
        SliverGridLayout, TableBorder, TableCellVerticalAlignment, TableColumnWidth,
    };
    pub use flui_foundation::geometry::Axis;
    pub use flui_foundation::geometry::{EdgeInsets, Matrix4};
    pub use flui_interaction::{
        DragDownDetails, DragEndDetails, DragStartDetails, DragUpdateDetails, GestureEndReason,
        PanZoomEvent,
    };
    pub use flui_objects::{CrossAxisAlignment, MainAxisAlignment, MainAxisSize, StackFit};
    pub use flui_objects::{WrapAlignment, WrapCrossAlignment};
    pub use flui_painting::Alignment;
    pub use flui_painting::BoxFit;
    pub use flui_painting::paint::Clip;
    pub use flui_painting::styling::Color;
    pub use flui_painting::typography::TextBaseline;
    pub use flui_platform_api::{InvalidLocale, Locale};
    pub use flui_rendering::constraints::AxisDirection;
    pub use flui_rendering::constraints::BoxConstraints;
    pub use flui_rendering::hit_testing::{
        CursorIcon, DeviceId, EventPropagation, HitTestBehavior, PointerDispatch, PointerEvent,
    };
    pub use flui_rendering::view::ScrollPosition;
}
