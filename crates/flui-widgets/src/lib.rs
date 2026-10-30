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
// `ContextMenuArea` (ADR-0216); re-exported from the crate root.
mod context_menu;
pub mod controls;
pub mod flex;
pub mod form;
pub mod icon;
pub mod image;
pub mod interaction;
pub mod layout;
pub mod localization;
mod media_query;
// Effects, reversible transitions and `Animate`; flat re-exports below.
pub mod motion;

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
mod swipe;
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
pub use flui_platform::Brightness;
// Ambient direction + localized-resource infrastructure — see
// `localization`'s module docs for the sync-only-v1 limits.
pub use localization::{
    BoxedLocalizationsDelegate, BoxedWidgetsLocalizations, DefaultWidgetsLocalizations,
    DefaultWidgetsLocalizationsDelegate, Directionality, GlobalWidgetsLocalizations,
    GlobalWidgetsLocalizationsDelegate, Localizations, LocalizationsDelegate, RTL_LANGUAGES,
    WidgetsLocalizations, basic_locale_list_resolution, resolve_alignment,
};

pub use animated::{
    Animated, AnimatedAlign, AnimatedAlignState, AnimatedContainer, AnimatedContainerState,
    AnimatedLerp, AnimatedLerpState, AnimatedOpacity, AnimatedOpacityState, AnimatedPadding,
    AnimatedPaddingState, AnimatedPositioned, AnimatedPositionedState, AnimatedRotation,
    AnimatedRotationState, AnimatedScale, AnimatedScaleState, AnimatedSize, AnimatedSizeState,
    AnimatedSlide, AnimatedSlideState, AnimatedState, MotionBuilder, RotationPath, TickerMode,
    TickerModeState, VsyncScope,
};
pub use clip::{ClipOval, ClipPath, ClipRRect, ClipRSuperellipse, ClipRect, Oval};
// `Image` widget over `RenderImage`; provider types live in the same module.
pub use async_builders::{
    BoxedResultFuture, BoxedResultStream, FutureBuilder, FutureFactory, InitialDataFactory,
    SnapshotBuilder, Stream, StreamBuilder, StreamFactory,
};
pub use container::Container;
pub use context_menu::{ContextMenuArea, ContextMenuAreaState};
pub use controls::{
    ActivityArc, ActivityIndicator, ActivityIndicatorState, Disclosure, DisclosureState,
    ExpansionState, Slider, SliderState,
};
pub use flex::{Column, Expanded, Flex, Flexible, Row, Spacer};
pub use flui_painting::ImagePlacement;
pub use flui_painting::paint::ImageRepeat;
pub use icon::{
    AnimatedIcon, AnimatedIconData, AnimatedIconState, Icon, IconCommand, IconCommandBuf, IconData,
    IconDataError, IconPath, IconPathBuf, IconTheme, IconThemeData,
};
#[cfg(feature = "asset-images")]
pub use image::AssetImage;
#[cfg(feature = "asset-images")]
pub use image::ImageState;
#[cfg(feature = "network-images")]
pub use image::NetworkImage;
pub use image::{
    DirectImageProvider, FileImage, Image, ImageCacheKey, ImageProvider, ImageProviderError,
    MemoryImage, Texture, TextureState,
};
pub use interaction::{
    AbsorbPointer, Action, ActionOutcome, Actions, ActivateIntent, ActivateLinkIntent,
    ActivationPressIntent, Alternative, AlternativeKind, AnimatedCrossFade, AnimatedCrossFadeState,
    BackIntent, ButtonActivateIntent, CallbackAction, CallbackShortcuts, CommandAppearance,
    CommandHandle, CompanionGap, CompanionLink, CompanionRegistration, CompanionSpec,
    ContactAdmission, ContentShape, ControlPlacement, ControlSpec, CopySelectionTextIntent,
    CrossFadeState, DefaultFocusRingStyle, DefaultFocusTraversal, DefaultFocusTraversalState,
    DirectionalFocusAction, DirectionalFocusIntent, DismissIntent, DragAnchor, DragCommands,
    Draggable, DraggableState, DropCommands, DropHover, DropTarget, DropTargetState, ExcludeFocus,
    Focus, FocusChangeHandler, FocusRing, FocusRingStyle, FocusRoot, FocusRootState, FocusScope,
    FocusScopeState, FocusState, FocusTraversalGroup, FocusTraversalGroupState,
    GestureAlternatives, GestureAlternativesError, GestureAlternativesState, GestureArenaScope,
    GestureCommand, GestureDetector, GestureDetectorState, GestureRecognizerFactory, HitSlop,
    Intent, Listener, MenuCause, MenuItem, MenuPresenter, MenuRequest, MenuSpec, MetaData,
    MouseRegion, NextFocusAction, NextFocusIntent, Offstage, PanClaimRegion, PasteTextIntent,
    PreviousFocusAction, PreviousFocusIntent, RawButton, RawGestureDetector,
    RawGestureDetectorState, RecognizerContext, SelectAllTextIntent, ShortcutCallback, Shortcuts,
    ShortcutsState, ShowContextMenuIntent, SingleActivator, SystemGestureExclusion,
    TransformationController, Visibility, VisibilityGate,
};
pub use interaction::{DragSlot, ForcePressSlot, MultiTapCount, MultiTapCountError, TransformSlot};
pub use layout::{
    Align, AspectRatio, Baseline, Center, ConstrainedBox, CustomMultiChildLayout,
    CustomSingleChildLayout, FittedBox, Flow, FractionalTranslation, FractionallySizedBox,
    IgnoreBaseline, IntrinsicHeight, IntrinsicWidth, LayoutBuilder, LayoutId, LimitedBox, ListBody,
    OverflowBox, Padding, PreferredSize, PreferredSizeView, RotatedBox, SizedBox, SizedOverflowBox,
    Table, TableCell, TableRow, Transform, UnconstrainedBox,
};
pub use motion::{
    Animate, AnimateExt, AnimateState, AnimatedSwitcher, AnimatedSwitcherLayoutBuilder,
    AnimatedSwitcherState, AnimatedSwitcherTransitionBuilder, Effect, Effects, ParticleController,
    ParticleEmitter, ParticleEmitterState, Shimmer, ShimmerBand, ShimmerState, Skeleton,
    SkeletonState, Transition, Trigger,
};
// `OverflowBoxFit` configures `OverflowBox`'s size policy; exposed at crate root
// so consumers don't need to reach into `flui_objects`.
pub use flui_objects::OverflowBoxFit;
// `HitShape` configures `ContentShape`.
pub use flui_objects::HitShape;
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
    AnimationRunFuture, FlightDirection, GeneratedRoute, Hero, HeroController, HeroControllerScope,
    HeroMode, KeyedSettings, NamedRouteError, Navigator, NavigatorCommand, NavigatorCommandError,
    NavigatorCommandOutcome, NavigatorCommandTarget, NavigatorHandle, NavigatorObserver,
    NavigatorRoute, NavigatorState, PageRoute, PageTransitionBuilder, PageTransitions,
    ParallaxPageTransition, PopInvokedCallback, PopScope, PopupRoute, PushCompletion, Route,
    RouteAnimation, RouteArguments, RouteBindingSlot, RouteContentBuilder, RouteId, RouteKey,
    RoutePageBuilder, RouteRequest, RouteResult, RouteSettings, RouteTransitionsBuilder,
    RunCanceled, SimpleRoute,
};
// The `Overlay::of`/`maybe_of` lookup contract (ADR-0076) and the types it
// resolves. The mutation surface (`insert`/`rearrange`/…) stays private to
// the crate — `Navigator` and `Draggable`'s feedback layer are its callers.
/// The offset [`SlideTransition`] and [`AnimatedSlide`] take: a fraction of
/// the child's own size, so an app can build one without naming the render
/// object crate.
pub use flui_objects::TranslationFraction;
pub use overlay::{InsertPosition, Overlay, OverlayEntry, OverlayEntryId, OverlayHandle};
pub use paint::{
    AnnotatedRegion, BackdropFilter, BackdropGroup, BackdropGroupState, ColoredBox, CustomPaint,
    DecoratedBox, IgnorePointer, Opacity, RepaintBoundary,
};
pub use physical_model::{PhysicalModel, PhysicalShape};
pub use router::{
    Routable, RouteParseError, RoutePath, Router, RouterError, RouterHandle, RouterState,
};
pub use scroll::{AnchorIntent, PageIntent};
pub use scroll::{
    AnchorSnap, AnchorSnapError, AnchoredDrag, AnchoredDragController, AnchoredDragState,
    AnchorsError, AnimatedGrid, AnimatedList, BouncingScrollPhysics, ClampingScrollPhysics,
    CollapsingHeader, CollapsingHeaderState, CustomScrollView, DraggableAnchors,
    DraggableAnchorsBuilder, FlingHandoff, FlingStep, FocusableSliver, GridView, HeaderExtents,
    HeaderExtentsError, HeaderOrder, InvalidExtents, InvalidSliverCrossAxisExtent,
    InvalidViewportFraction, InvalidWheelScrollDistance, ListView, NestedScroll,
    NestedScrollConnection, NestedScrollState, OverScrollHeaderStretchConfiguration,
    OverscrollBehavior, PageController, PageIndicator, PageIndicatorState, PageIndicatorStyle,
    PageIndicatorStyleError, PageSnap, PageView, PageViewState, RefreshController,
    RefreshIndicator, RefreshIndicatorPhase, RefreshIndicatorState, RefreshIndicatorStyle,
    RefreshIntent, ScrollCause, ScrollController, ScrollDevice, ScrollEndReason, ScrollMetrics,
    ScrollPhysics, ScrollStep, Scrollable, Scrollbar, SharedScrollPhysics, SharedSnapPolicy,
    ShrinkWrappingViewport, SingleChildScrollView, SliverAnimatedList, SliverAnimatedListState,
    SliverChildBuilderDelegate, SliverConstrainedCrossAxis, SliverCrossAxisGroup,
    SliverCrossAxisItem, SliverFillRemaining, SliverFillRemainingAndOverscroll,
    SliverFillRemainingWithScrollable, SliverFillViewport, SliverFixedExtentList, SliverGrid,
    SliverIgnorePointer, SliverList, SliverMainAxisGroup, SliverOffstage, SliverOpacity,
    SliverPadding, SliverPersistentHeader, SliverPersistentHeaderDelegate, SliverToBoxAdapter,
    SnapPolicy, StretchTriggerSignal, Viewport, ViewportFraction, WheelScrollDistances,
};
pub use scroll::{
    Carousel, CarouselAutoplay, CarouselAutoplayError, CarouselController, CarouselItemScope,
    CarouselLayout, CarouselLayoutError, CarouselLayoutKind, CarouselState, KeylineSnap,
    MAX_ITEM_EXTENT, MIN_AUTOPLAY_PERIOD,
};
pub use scroll::{FloatingHeaderSnapConfiguration, ScrollPositionScope};
pub use scroll::{
    InteractionEndDetails, InteractionStartDetails, InteractionUpdateDetails, InteractiveViewer,
    InteractiveViewerState, PanAxis, PanIntent, WheelScaleGate, ZoomIntent,
};
pub use semantics::{
    ExcludeSemantics, IndexedSemantics, MergeSemantics, Semantics, SliverSemantics,
};
pub use stack::{IndexedStack, Positioned, Stack};
pub use swipe::{
    DismissDirection, DismissSide, DismissSideCallback, DismissThreshold, DismissThresholdError,
    DismissUpdateCallback, DismissUpdateDetails, Dismissible, DismissibleState, SwipeAction,
    SwipeActions, SwipeActionsError, SwipeActionsState, SwipeAlternative, SwipeEdge,
};
pub use text::{
    AnimatedDefaultTextStyle, DefaultSelectionStyle, DefaultTextStyle, EditableText,
    EditableTextState, RawTextField, RawTextFieldState, RichText, SelectedText, SelectionArea,
    SelectionContainer, SelectionStyle, SubmitCallback, Text, TextEditingController,
};
pub use transitions::{
    AlignTransition, AlignTransitionState, AnimatedBuilder, AnimatedBuilderState,
    DecoratedBoxTransition, DecoratedBoxTransitionState, FadeTransition, FadeTransitionState,
    PositionedTransition, PositionedTransitionState, RotationTransition, RotationTransitionState,
    ScaleTransition, ScaleTransitionState, SizeTransition, SizeTransitionState, SlideTransition,
    SlideTransitionState, SliverFadeTransition, SliverFadeTransitionState,
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
pub use flui_view::{HapticsMode, column, row};

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
// pointer event types its callbacks receive (`on_pointer_move` also receives
// `PointerEvent::ButtonChange`).
pub use flui_interaction::events::ButtonChange;
pub use flui_rendering::hit_testing::{
    CursorIcon, DeviceId, EventPropagation, HitTestBehavior, PointerDispatch, PointerEvent,
};
// The shared scroll state `ScrollController::position()` returns and
// `Viewport`/`SingleChildScrollView::position()` accept — a widget author
// composing a custom scrollable directly on `Viewport` needs to name this
// type without reaching past `flui-widgets` into `flui-rendering`.
pub use flui_rendering::view::ScrollPosition;
// Details a `GestureDetector` drag slot hands its callbacks.
pub use flui_interaction::{
    DragDownDetails, DragEndDetails, DragStartDetails, DragUpdateDetails, GestureEndReason,
    PanZoomEvent, TapDetails,
};
pub use flui_rendering::semantics::{
    ActionCall, AnnounceError, Announcer, AutoComplete, CellPosition, Current, HasPopup,
    HeadingLevel, Invalid, LiveSetting, NumericRange, NumericRangeError, Reveal,
    ScrollUnit as SemanticsScrollUnit, SemanticsAnchor, SemanticsConfiguration, SemanticsRole,
    SetSize, SortDirection, TableShape, TextDirection as SemanticsTextDirection, TextSelection,
    Toggled, Urgency,
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
        AutovalidateMode, BackdropFilter, BackdropGroup, Baseline, Brightness,
        ButtonActivateIntent, CallbackAction, CallbackShortcuts, Center, ClipOval, ClipPath,
        ClipRRect, ClipRSuperellipse, ClipRect, ColoredBox, Column, ConstrainedBox, Container,
        ContentShape, CopySelectionTextIntent, CustomMultiChildLayout, CustomPaint,
        CustomScrollView, CustomSingleChildLayout, DecoratedBox, DefaultFocusTraversal,
        DefaultFocusTraversalState, DefaultSelectionStyle, DefaultTextStyle,
        DefaultWidgetsLocalizations, Directionality, Disclosure, DisclosureState, DragSlot,
        Draggable, DropTarget, EditableText, EditableTextState, ExcludeFocus, ExcludeSemantics,
        Expanded, ExpansionState, FittedBox, Flex, FlexFit, Flexible, FlightDirection, Flow, Focus,
        FocusRoot, FocusScope, FocusTraversalGroup, Form, FormField, FormFieldHandle, FormHandle,
        FractionalTranslation, FractionallySizedBox, FutureBuilder, GestureArenaScope,
        GestureDetector, GridView, Hero, HeroController, HeroMode, HitShape, HitSlop, Icon,
        IconData, IconTheme, IconThemeData, IgnoreBaseline, IgnorePointer, Image, ImagePlacement,
        ImageProvider, ImageRepeat, IndexedSemantics, IndexedStack, InheritedTheme, Intent,
        IntrinsicHeight, IntrinsicWidth, LayoutBuilder, LayoutId, LimitedBox, ListBody, ListView,
        Listener, Localizations, LocalizationsDelegate, MediaQuery, MediaQueryData, MergeSemantics,
        MouseRegion, Navigator, NavigatorHandle, NextFocusAction, NextFocusIntent, Offstage,
        Opacity, OverflowBox, OverflowBoxFit, Overlay, OverlayEntry, OverlayEntryId, OverlayHandle,
        Padding, PageController, PageRoute, PageView, PanClaimRegion, PasteTextIntent,
        PhysicalModel, PhysicalShape, PopScope, PopupRoute, Positioned, PreferredSize,
        PreferredSizeView, PreviousFocusAction, PreviousFocusIntent, RawButton, RawTextField,
        RawTextFieldState, RawTextFormField, RepaintBoundary, RichText, RotatedBox, Routable,
        RoutePath, Router, RouterHandle, Row, SafeArea, ScrollController, Scrollable, Scrollbar,
        SelectAllTextIntent, SelectionArea, SelectionContainer, SelectionStyle, Semantics,
        Shortcuts, ShrinkWrappingViewport, SimpleRoute, SingleActivator, SingleChildScrollView,
        SizedBox, SizedOverflowBox, Slider, SliderState, SliverChildBuilderDelegate,
        SliverFadeTransition, SliverFillRemaining, SliverFillRemainingAndOverscroll,
        SliverFillRemainingWithScrollable, SliverFillViewport, SliverFixedExtentList, SliverGrid,
        SliverIgnorePointer, SliverList, SliverOffstage, SliverOpacity, SliverPadding,
        SliverToBoxAdapter, Spacer, Stack, StreamBuilder, SubmitCallback, Table, TableCell,
        TableRow, Text, TextEditingController, TickerMode, Transform, UnconstrainedBox,
        ValueListenableBuilder, Viewport, Visibility, VisibilityGate, WidgetState,
        WidgetStateConstraint, WidgetStateProperty, WidgetStates, WidgetStatesController,
        WidgetsApp, WidgetsLocalizations, Wrap,
    };

    // The implicit widgets' duration/curve/spring builders.
    pub use crate::MotionBuilder;

    // Effects on any view (`view.animate(effects)`) and the vocabulary they take.
    pub use crate::{Animate, AnimateExt, Effects, Transition};

    // Common configuration value types, so an app author needs only this import.
    pub use crate::basic_locale_list_resolution;
    pub use crate::{
        AspectRatioDelegate, BoxedLocalizationsDelegate, BoxedWidgetsLocalizations,
        CenterLayoutDelegate, CustomPainter, DefaultWidgetsLocalizationsDelegate, FlowDelegate,
        FlowPaintingContext, HeadingLevel, LiveSetting, MultiChildLayoutContext,
        MultiChildLayoutDelegate, NumericRange, NumericRangeError, SemanticsAnchor,
        SemanticsConfiguration, SemanticsRole, SemanticsTextDirection, SingleChildLayoutDelegate,
        SliverGridDelegate, SliverGridDelegateWithFixedCrossAxisCount,
        SliverGridDelegateWithMaxCrossAxisExtent, SliverGridLayout, TableBorder,
        TableCellVerticalAlignment, TableColumnWidth, Toggled, Urgency,
    };
    pub use flui_foundation::geometry::Axis;
    pub use flui_foundation::geometry::{EdgeInsets, Matrix4};
    pub use flui_interaction::{
        DragDownDetails, DragEndDetails, DragStartDetails, DragUpdateDetails, GestureEndReason,
        PanZoomEvent, TapDetails,
    };
    pub use flui_objects::{CrossAxisAlignment, MainAxisAlignment, MainAxisSize, StackFit};
    pub use flui_objects::{WrapAlignment, WrapCrossAlignment};
    pub use flui_painting::Alignment;
    pub use flui_painting::BoxFit;
    pub use flui_painting::paint::Clip;
    pub use flui_painting::styling::Color;
    pub use flui_painting::typography::TextBaseline;
    pub use flui_platform::{InvalidLocale, Locale};
    pub use flui_rendering::constraints::AxisDirection;
    pub use flui_rendering::constraints::BoxConstraints;
    pub use flui_rendering::hit_testing::{
        CursorIcon, DeviceId, EventPropagation, HitTestBehavior, PointerDispatch, PointerEvent,
    };
    pub use flui_rendering::view::ScrollPosition;
}
