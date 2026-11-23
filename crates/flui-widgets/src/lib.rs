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
//! The render *machine* (layout/paint/compositing) lives in [`flui_rendering`] and
//! [`flui_objects`]; this crate is the declarative surface over it.
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
//! Widgets favour a constructor + chainable-config surface. Single
//! children are taken as `impl IntoView`; a list of children is a
//! [`ViewSeq`](flui_view::ViewSeq): a tuple (the static path), an array
//! of one type, or `Vec<BoxedView>` (the dynamic path).
//!
//! ```rust
//! use flui_widgets::prelude::*;
//!
//! let _tree = Container::new()
//!     .padding(EdgeInsets::all(8.0))
//!     .color(Color::rgb(26, 102, 230))
//!     .child(Column::new((
//!         Text::new("Hello"),
//!         Padding::all(4.0).child(Text::new("World")),
//!     )));
//! ```
//!
//! [`RenderView`]: flui_view::prelude::RenderView
//! [`StatelessView`]: flui_view::prelude::StatelessView
//! [`ParentDataView`]: flui_view::prelude::ParentDataView

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "test code catches a panic to assert on it (ADR-0303 §5)"
    )
)]
// Lint levels come from `[workspace.lints]`. Every public
// item is documented; keep it that way.
#![deny(missing_docs)]
// Ids the process mints hash with `rustc_hash`; std maps need an `#[expect]` (AGENTS.md "Current forms").
#![warn(clippy::disallowed_types)]
#![expect(
    clippy::module_inception,
    reason = "a one-type family module named after its type (`flex/flex.rs`, `text/text.rs`) is the catalog's house style"
)]

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
//
// Every widget module is private: an item has one public path, the crate
// root (`flui_widgets::Padding`, `flui::widgets::Padding`), as Flutter's
// `widgets.dart` is one flat library. A module's `pub use` list is what the
// root re-exports from it. The exceptions are namespaces the root keeps whole
// rather than flattening (`particles`), and the doc-hidden seams below.
// A type another crate owns (a semantics role, a pointer event, a render
// delegate, a layout enum) is reached from its owner's module
// (`flui::semantics`, `flui::interaction`, `flui::rendering`, …); the
// prelude imports the ones an app author needs.

mod support;

// Framework seams for the sibling `flui-*` widget crates: no semver
// guarantee, workspace-only.
#[doc(hidden)]
pub mod __private;
// Private items this crate's own integration tests name (ADR-0083 §4). Built
// only with `testing`, which the crate's dev-dependency on itself turns on, so
// no application build carries it.
#[cfg(feature = "testing")]
#[doc(hidden)]
pub mod __test_access;
mod anchored_box;

mod animated;
mod app;
mod async_builders;
mod clip;
mod container;
mod context_menu;
mod controls;
mod flex;
mod form;
mod icon;
mod image;
mod interaction;
mod layout;
mod localization;
mod magnifier;
mod motion;
// The route stack beneath `Navigator` (ADR-0019, ADR-0330 §4): the pages,
// routes and heroes an application names are re-exported below, and the stack,
// its lifecycle and the result channel are not public.
mod navigator;
// `Overlay` / `OverlayEntry` (ADR-0076): the types and the lookup contract are
// re-exported below; `OverlayScope` and the `Theater`/`OverlayState`
// machinery are not.
mod overlay;
mod paint;
mod physical_model;
mod popup_menu;
mod reorderable;
// The typed `Router` (ADR-0330).
mod router;
mod scroll;
mod semantics;
mod stack;
mod swipe;
mod text;
mod transitions;
mod value_listenable_builder;
mod widget_state;
mod wrap;

// ============================================================================
// Re-exports: the crate root is each widget item's one public path.
// ============================================================================

// Application composition and the `InheritedTheme` trait a theme widget
// (e.g. `flui_material::Theme`) implements.
pub use app::{AppBuilder, InheritedTheme, SafeArea, WidgetsApp, WidgetsAppState, app_root};
pub use magnifier::{
    InvalidMagnifierScale, MagnifierDecoration, MagnifierScale, MagnifierShape, RawMagnifier,
};
// Ambient direction + localized-resource infrastructure — see
// `localization`'s module docs for the sync-only-v1 limits.
pub use localization::{
    BoxedLocalizationsDelegate, BoxedWidgetsLocalizations, DefaultWidgetsLocalizations,
    DefaultWidgetsLocalizationsDelegate, Directionality, GlobalWidgetsLocalizations,
    GlobalWidgetsLocalizationsDelegate, Localizations, LocalizationsDelegate, RTL_LANGUAGES,
    WidgetsLocalizations, basic_locale_list_resolution,
};

pub use animated::{
    Animated, AnimatedAlign, AnimatedAlignState, AnimatedContainer, AnimatedContainerState,
    AnimatedLerp, AnimatedLerpState, AnimatedOpacity, AnimatedOpacityState, AnimatedPadding,
    AnimatedPaddingState, AnimatedPositioned, AnimatedPositionedState, AnimatedRotation,
    AnimatedRotationState, AnimatedScale, AnimatedScaleState, AnimatedSize, AnimatedSizeState,
    AnimatedSlide, AnimatedSlideState, AnimatedState, MotionBuilder, RotationPath, TickerMode,
    TickerModeState,
};
// `FutureBuilder`/`StreamBuilder` and their snapshot vocabulary are defined in
// `flui-view` beside their element state; the catalog is their path.
pub use async_builders::{
    AsyncSnapshot, BoxedResultFuture, BoxedResultStream, ConnectionState, FutureBuilder,
    FutureFactory, InitialDataFactory, SnapshotBuilder, Stream, StreamBuilder, StreamFactory,
};
pub use clip::{ClipOval, ClipPath, ClipRRect, ClipRSuperellipse, ClipRect};
pub use container::Container;
pub use context_menu::{ContextMenuArea, ContextMenuAreaState};
pub use controls::{
    ActivityArc, ActivityIndicator, ActivityIndicatorState, Disclosure, DisclosureState,
    ExpansionState, Slider, SliderState,
};
pub use flex::{Column, Expanded, Flex, Flexible, Row, Spacer};
pub use icon::{
    AnimatedIcon, AnimatedIconData, AnimatedIconState, DiscreteEffect, Icon, IconCommand,
    IconCommandBuf, IconData, IconDataError, IconPath, IconPathBuf, IconTheme, IconThemeData,
    IndefiniteEffect, PathEffect, RotateEffect, Speed, SymbolEffect,
};
#[cfg(feature = "asset-images")]
pub use image::AssetImage;
#[cfg(feature = "asset-images")]
pub use image::NetworkImage;
pub use image::{
    DecodeJob, DirectImageProvider, FileImage, Image, ImageCacheKey, ImageProvider,
    ImageProviderError, ImageServices, ImageState, MemoryImage, Texture,
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
    FirstClick, Focus, FocusChangeHandler, FocusRing, FocusScope, FocusScopeState, FocusState,
    FocusTraversalGroup, FocusTraversalGroupState, GestureAlternatives, GestureAlternativesError,
    GestureAlternativesState, GestureCommand, GestureDetector, GestureDetectorState,
    GestureRecognizerFactory, HitSlop, Intent, Listener, MenuCause, MenuItem, MenuPresenter,
    MenuRequest, MenuSpec, MetaData, MouseRegion, NextFocusAction, NextFocusIntent, Offstage,
    PanClaimRegion, PasteTextIntent, PreviousFocusAction, PreviousFocusIntent, RawButton,
    RawGestureDetector, RawGestureDetectorState, RecognizerContext, SelectAllTextIntent,
    ShortcutCallback, Shortcuts, ShortcutsState, ShowContextMenuIntent, SingleActivator,
    SystemGestureExclusion, TransformationController, Visibility, VisibilityGate,
    WindowCaptionButton, WindowChromeState, WindowMovePassThrough, WindowResizeArea,
    WindowTitleBar,
};
pub use interaction::{DragSlot, ForcePressSlot, TransformSlot};
pub use layout::{
    Align, AspectRatio, Baseline, Center, ConstrainedBox, CustomMultiChildLayout,
    CustomSingleChildLayout, FittedBox, Flow, FractionalTranslation, FractionallySizedBox,
    IgnoreBaseline, IntrinsicHeight, IntrinsicWidth, LayoutBuilder, LayoutId, LimitedBox, ListBody,
    OverflowBox, Padding, PreferredSize, PreferredSizeView, RotatedBox, SizedBox, SizedOverflowBox,
    Table, TableCell, TableRow, Transform, UnconstrainedBox,
};
pub use motion::{
    Animate, AnimateExt, AnimateState, AnimatedSwitcher, AnimatedSwitcherLayoutBuilder,
    AnimatedSwitcherState, AnimatedSwitcherTransitionBuilder, Effect, Effects, Shimmer,
    ShimmerState, Skeleton, SkeletonState, Transition, Trigger,
};
// Particle effects: `ParticleEmitter`, its checked `ParticleConfig` and the
// `ParticleController` that fires bursts — a namespace of its own, so its
// configuration vocabulary (`Sprite`, `Coverage`, `MAX_CAPACITY`, …) does not
// crowd the catalog.
pub use form::{
    AutovalidateMode, Form, FormField, FormFieldHandle, FormFieldHandleAlreadyAttached,
    FormFieldSetter, FormFieldState, FormFieldValidator, FormHandle, FormHandleAlreadyAttached,
    FormState, RawTextFormField, RawTextFormFieldState,
};
pub use motion::particles;
pub use navigator::{
    FlightDirection, Hero, HeroController, HeroControllerScope, HeroMode, PageError, PageHandle,
    PageRoute, PageTransitionBuilder, PageTransitions, ParallaxPageTransition, PopInvokedCallback,
    PopScope, PopupRoute, PushCompletion, Route, RouteAnimation, RouteBindingSlot,
    RouteContentBuilder, RouteId, RoutePageBuilder, RouteResult, RouteResultError, RouteSettings,
    RouteTransitionsBuilder,
};
pub use popup_menu::PlainSelectionMenu;
// The `Overlay::of`/`maybe_of` lookup contract (ADR-0076) and the types it
// resolves. The mutation surface (`insert`/`rearrange`/…) stays private to
// the crate — `Navigator` and `Draggable`'s feedback layer are its callers.
pub use overlay::{InsertPosition, Overlay, OverlayEntry, OverlayEntryId, OverlayHandle};
pub use paint::{
    AnnotatedRegion, BackdropFilter, BackdropGroup, BackdropGroupState, ColoredBox, CustomPaint,
    DecoratedBox, IgnorePointer, Opacity, RepaintBoundary,
};
pub use physical_model::{PhysicalModel, PhysicalShape};
pub use reorderable::{
    Reorder, ReorderCause, ReorderHandles, ReorderIntent, ReorderPreview, ReorderableGrid,
    ReorderableGridState, ReorderableItem, ReorderableList, ReorderableListState,
};
pub use router::{Routable, Router, RouterError, RouterHandle, RouterState};
// The helpers `#[derive(Routable)]` expands to calls of: no semver guarantee,
// and no other caller.
#[doc(hidden)]
pub use router::__derive;
pub use scroll::ScrollPositionScope;
pub use scroll::{AnchorIntent, PageIntent};
pub use scroll::{
    AnchorSnap, AnchorSnapError, AnchoredDrag, AnchoredDragController, AnchoredDragState,
    AnchorsError, AnimatedGrid, AnimatedList, BouncingScrollPhysics, ClampingScrollPhysics,
    CollapsingHeader, CollapsingHeaderState, CustomScrollView, DefaultScrollBehavior,
    DraggableAnchors, DraggableAnchorsBuilder, FlingHandoff, FlingStep, FocusableSliver,
    FocusableSliverParts, GridView, HeaderExtents, HeaderExtentsError, HeaderOrder,
    IntoFocusableSliver, InvalidExtents, InvalidViewportFraction, InvalidWheelScrollDistance,
    ListView, NestedScroll, NestedScrollConnection, NestedScrollState, OverscrollBehavior,
    PageController, PageIndicator, PageIndicatorState, PageIndicatorStyle, PageIndicatorStyleError,
    PageSnap, PageView, PageViewState, RefreshController, RefreshIndicator, RefreshIndicatorPhase,
    RefreshIndicatorState, RefreshIndicatorStyle, RefreshIntent, ScrollBehavior,
    ScrollBehaviorDetails, ScrollCause, ScrollConfiguration, ScrollController, ScrollDevice,
    ScrollEndReason, ScrollMetrics, ScrollPhysics, ScrollPhysicsError, ScrollPhysicsParameter,
    ScrollStep, ScrollView, ScrollViewConfig, Scrollable, ScrollableDetails, Scrollbar,
    ScrollbarOrientation, ScrollbarState, SharedScrollPhysics, SharedSnapPolicy,
    ShrinkWrappingViewport, SingleChildScrollView, SliverAnimatedList, SliverAnimatedListState,
    SliverChildBuilderDelegate, SliverConstrainedCrossAxis, SliverCrossAxisGroup,
    SliverCrossAxisItem, SliverFillRemaining, SliverFillRemainingAndOverscroll,
    SliverFillRemainingWithScrollable, SliverFillViewport, SliverFixedExtentList, SliverGrid,
    SliverIgnorePointer, SliverList, SliverMainAxisGroup, SliverOffstage, SliverOpacity,
    SliverPadding, SliverPersistentHeader, SliverToBoxAdapter, SnapPolicy, Viewport,
    ViewportFraction, WheelScrollDistances,
};
pub use scroll::{
    Carousel, CarouselAutoplay, CarouselAutoplayError, CarouselController, CarouselItemScope,
    CarouselState, KeylineSnap, MIN_AUTOPLAY_PERIOD,
};
pub use scroll::{
    InteractionEndDetails, InteractionStartDetails, InteractionUpdateDetails, InteractiveViewer,
    InteractiveViewerState, PanAxis, PanIntent, WheelScaleGate, ZoomIntent,
};
pub use semantics::{
    ExcludeSemantics, IndexedSemantics, MergeSemantics, Semantics, SliverSemantics,
};
pub use stack::{DirectionalPositioned, IndexedStack, Positioned, Stack};
pub use swipe::{
    DismissDirection, DismissSide, DismissSideCallback, DismissThreshold, DismissThresholdError,
    DismissUpdateCallback, DismissUpdateDetails, Dismissible, DismissibleState, SwipeAction,
    SwipeActions, SwipeActionsError, SwipeActionsState, SwipeAlternative, SwipeEdge,
};
pub use text::{
    AnimatedDefaultTextStyle, CaretHandle, DefaultSelectionControls, DefaultSelectionStyle,
    DefaultTextStyle, DotHandles, EditableText, EditableTextState, HandleInfo, HandleSide,
    LongPressSelection, MagnifierInfo, PlainMagnifier, PopupOrigin, RawTextField,
    RawTextFieldState, RichText, SelectError, SelectedText, SelectionArea, SelectionChange,
    SelectionChangeCause, SelectionContainer, SelectionController, SelectionHandles,
    SelectionInput, SelectionMagnifier, SelectionMenu, SelectionMenuPolicy, SelectionPopup,
    SelectionStyle, SubmitCallback, Text, TextEditingController, ToolbarItems,
    ToolbarItemsController, ToolbarItemsState,
};
pub use transitions::{
    AlignTransition, AlignTransitionState, AnimatedBuilder, AnimatedBuilderState,
    DecoratedBoxTransition, DecoratedBoxTransitionState, FadeTransition, FadeTransitionState,
    PositionedTransition, PositionedTransitionState, RotationTransition, RotationTransitionState,
    ScaleTransition, ScaleTransitionState, SizeTransition, SizeTransitionState, SlideTransition,
    SlideTransitionState, SliverFadeTransition, SliverFadeTransitionState,
};
pub use value_listenable_builder::{ValueListenableBuilder, ValueListenableBuilderState};
// The interactive-state vocabulary a widget's visual properties can vary
// over (hover/focus/press/…) — see the module's own docs for named
// deferrals.
pub use widget_state::{
    WidgetState, WidgetStateConstraint, WidgetStateProperty, WidgetStates, WidgetStatesController,
};
// An activation's hold on `Pressed`, for the official catalogs (ADR-0322 §3).
#[doc(hidden)]
pub use widget_state::PressHold;
pub use wrap::Wrap;

// ============================================================================
// Prelude
// ============================================================================

/// Commonly used widgets and supporting types for `use flui_widgets::prelude::*;`.
pub mod prelude {
    // Authoring spine re-exported so a single prelude import is enough to write
    // a widget tree (View traits, BuildContext, ViewSeq, derives).
    pub use flui_view::prelude::*;

    // The widget catalog.
    pub use crate::{
        AbsorbPointer, Action, ActionOutcome, Actions, ActivateIntent, Align, AspectRatio,
        AutovalidateMode, BackdropFilter, BackdropGroup, Baseline, ButtonActivateIntent,
        CallbackAction, CallbackShortcuts, Center, ClipOval, ClipPath, ClipRRect,
        ClipRSuperellipse, ClipRect, ColoredBox, Column, ConstrainedBox, Container, ContentShape,
        CopySelectionTextIntent, CustomMultiChildLayout, CustomPaint, CustomScrollView,
        CustomSingleChildLayout, DecoratedBox, DefaultFocusTraversal, DefaultFocusTraversalState,
        DefaultSelectionControls, DefaultSelectionStyle, DefaultTextStyle,
        DefaultWidgetsLocalizations, Directionality, Disclosure, DisclosureState, DotHandles,
        DragSlot, Draggable, DropTarget, EditableText, EditableTextState, ExcludeFocus,
        ExcludeSemantics, Expanded, ExpansionState, FittedBox, Flex, Flexible, FlightDirection,
        Flow, Focus, FocusScope, FocusTraversalGroup, Form, FormField, FormFieldHandle, FormHandle,
        FractionalTranslation, FractionallySizedBox, FutureBuilder, GestureDetector, GridView,
        HandleInfo, HandleSide, Hero, HeroController, HeroMode, HitSlop, Icon, IconData, IconTheme,
        IconThemeData, IgnoreBaseline, IgnorePointer, Image, ImageProvider, IndexedSemantics,
        IndexedStack, InheritedTheme, Intent, IntrinsicHeight, IntrinsicWidth, LayoutBuilder,
        LayoutId, LimitedBox, ListBody, ListView, Listener, Localizations, LocalizationsDelegate,
        MergeSemantics, MouseRegion, NextFocusAction, NextFocusIntent, Offstage, Opacity,
        OverflowBox, Overlay, OverlayEntry, OverlayEntryId, OverlayHandle, Padding, PageController,
        PageHandle, PageRoute, PageView, PanClaimRegion, PasteTextIntent, PhysicalModel,
        PhysicalShape, PopScope, PopupRoute, Positioned, PreferredSize, PreferredSizeView,
        PreviousFocusAction, PreviousFocusIntent, RawButton, RawTextField, RawTextFieldState,
        RawTextFormField, RepaintBoundary, RichText, RotatedBox, Routable, Router, RouterHandle,
        Row, SafeArea, ScrollController, ScrollView, Scrollable, Scrollbar, SelectAllTextIntent,
        SelectionArea, SelectionContainer, SelectionController, SelectionHandles, SelectionStyle,
        Semantics, Shortcuts, ShrinkWrappingViewport, SingleActivator, SingleChildScrollView,
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

    // Localization configuration.
    pub use crate::basic_locale_list_resolution;
    pub use crate::{
        BoxedLocalizationsDelegate, BoxedWidgetsLocalizations, DefaultWidgetsLocalizationsDelegate,
    };

    // The value types the widgets above take, from the crates that own them, so
    // an app author needs only this import.
    pub use flui_foundation::geometry::Axis;
    pub use flui_foundation::geometry::{EdgeInsets, Matrix4};
    pub use flui_interaction::gesture::{
        DragDownDetails, DragEndDetails, DragStartDetails, DragUpdateDetails, GestureEndReason,
        TapDetails,
    };
    pub use flui_interaction::{
        events::{CursorIcon, PointerEvent},
        ids::DeviceId,
        pointer::{EventPropagation, HitTestBehavior, PointerDispatch},
    };
    pub use flui_objects::{
        CrossAxisAlignment, HitShape, MainAxisAlignment, MainAxisSize, OverflowBoxFit, StackFit,
        TableColumnWidth, VerticalDirection, WrapAlignment, WrapCrossAlignment,
    };
    pub use flui_painting::paint::{Clip, ImageRepeat};
    pub use flui_painting::styling::{Color, TableBorder};
    pub use flui_painting::typography::TextBaseline;
    pub use flui_painting::{Alignment, BoxFit, ImagePlacement};
    pub use flui_platform::input::pointer::PanZoomEvent;
    pub use flui_platform::{Brightness, InvalidLocale, Locale};
    pub use flui_rendering::constraints::{AxisDirection, BoxConstraints};
    pub use flui_rendering::delegates::{
        AspectRatioDelegate, CenterLayoutDelegate, CustomPainter, FlowDelegate,
        FlowPaintingContext, MultiChildLayoutContext, MultiChildLayoutDelegate,
        SingleChildLayoutDelegate, SliverGridDelegate, SliverGridDelegateWithFixedCrossAxisCount,
        SliverGridDelegateWithMaxCrossAxisExtent, SliverGridLayout,
    };
    pub use flui_rendering::parent_data::{FlexFit, TableCellVerticalAlignment};
    pub use flui_rendering::view::ScrollPosition;
    pub use flui_semantics::{
        HeadingLevel, LiveSetting, NumericRange, NumericRangeError, SemanticsAnchor,
        SemanticsConfiguration, SemanticsRole, Toggled, Urgency,
    };
    pub use flui_view::navigation::RoutePath;
}
