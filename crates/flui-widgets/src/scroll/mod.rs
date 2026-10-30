//! Scroll widgets — show a scrollable window into content larger than the
//! available space, over `flui-objects`' viewport/sliver render objects.
//!
//! The low-level primitives ([`Viewport`], [`SliverToBoxAdapter`],
//! [`SliverFixedExtentList`]) compose the cross-protocol box→sliver→box layout
//! path; [`SingleChildScrollView`] and [`ListView`] are the common user-facing
//! widgets.
//!
//! For gesture-driven interactive scrolling use [`Scrollable`] + a
//! [`ScrollController`], and optionally wrap them in a [`Scrollbar`] for a
//! visual position indicator.

mod anchored_drag;
mod carousel;
#[cfg(test)]
pub(crate) use anchored_drag::anchor_keys;
#[cfg(test)]
pub(crate) use interactive_viewer::zoom_keys;
#[cfg(test)]
pub(crate) use refresh_indicator::refresh_keys;
mod animated_list;
mod collapsing_header;
mod custom_scroll_view;
mod drag_autoscroll;
mod focusable_sliver;
mod grid_view;
mod interactive_viewer;
mod list_focus;
mod list_view;
pub mod nested;
mod page_indicator;
mod page_view;
mod refresh_indicator;
mod scroll_controller;
mod scroll_owner;
mod scroll_physics;
mod scroll_position_scope;
mod scrollable;
mod scrollbar;
mod single_child_scroll_view;
mod sliver_constrained_cross_axis;
mod sliver_cross_axis_group;
mod sliver_fill_remaining;
mod sliver_fill_viewport;
mod sliver_fixed_extent_list;
mod sliver_grid;
mod sliver_ignore_pointer;
mod sliver_list;
mod sliver_main_axis_group;
mod sliver_offstage;
mod sliver_opacity;
mod sliver_padding;
mod sliver_persistent_header;
mod sliver_to_box_adapter;
mod viewport;

pub use anchored_drag::{
    AnchorIntent, AnchoredDrag, AnchoredDragController, AnchoredDragState, AnchorsError,
    DraggableAnchors, DraggableAnchorsBuilder,
};
pub use animated_list::{AnimatedGrid, AnimatedList, SliverAnimatedList, SliverAnimatedListState};
pub use carousel::{
    Carousel, CarouselAutoplay, CarouselAutoplayError, CarouselController, CarouselItemScope,
    CarouselLayout, CarouselLayoutError, CarouselLayoutKind, CarouselState, KeylineSnap,
    MAX_ITEM_EXTENT, MIN_AUTOPLAY_PERIOD,
};
pub use collapsing_header::{
    CollapsingHeader, CollapsingHeaderState, HeaderExtents, HeaderExtentsError, HeaderOrder,
};
pub use custom_scroll_view::CustomScrollView;
pub use flui_objects::InvalidSliverCrossAxisExtent;
pub use focusable_sliver::{FocusableSliver, FocusableSliverParts, IntoFocusableSliver};
pub use grid_view::GridView;
pub use interactive_viewer::{
    InteractionEndDetails, InteractionStartDetails, InteractionUpdateDetails, InteractiveViewer,
    InteractiveViewerState, PanAxis, PanIntent, WheelScaleGate, ZoomIntent,
};
pub use list_view::ListView;
pub use nested::{
    AnchorSnap, AnchorSnapError, FlingHandoff, FlingStep, InvalidViewportFraction, NestedScroll,
    NestedScrollConnection, NestedScrollState, OverscrollBehavior, PageSnap, ScrollCause,
    ScrollDevice, ScrollEndReason, ScrollStep, SnapPolicy, ViewportFraction,
};
pub use page_indicator::{
    PageIndicator, PageIndicatorState, PageIndicatorStyle, PageIndicatorStyleError,
};
pub use page_view::{PageController, PageIntent, PageView, PageViewState};
pub use refresh_indicator::{
    RefreshController, RefreshIndicator, RefreshIndicatorPhase, RefreshIndicatorState,
    RefreshIndicatorStyle, RefreshIntent,
};
pub use scroll_controller::{InvalidExtents, ScrollController};
pub use scroll_owner::SharedSnapPolicy;
pub use scroll_physics::{
    BouncingScrollPhysics, ClampingScrollPhysics, ScrollMetrics, ScrollPhysics, SharedScrollPhysics,
};
pub use scroll_position_scope::ScrollPositionScope;
use scrollable::wheel_axis_delta;
pub use scrollable::{InvalidWheelScrollDistance, Scrollable, WheelScrollDistances};
pub use scrollbar::Scrollbar;
pub use single_child_scroll_view::SingleChildScrollView;
pub use sliver_constrained_cross_axis::SliverConstrainedCrossAxis;
pub use sliver_cross_axis_group::{SliverCrossAxisGroup, SliverCrossAxisItem};
pub use sliver_fill_remaining::{
    SliverFillRemaining, SliverFillRemainingAndOverscroll, SliverFillRemainingWithScrollable,
};
pub use sliver_fill_viewport::SliverFillViewport;
pub use sliver_fixed_extent_list::SliverFixedExtentList;
pub use sliver_grid::SliverGrid;
pub use sliver_ignore_pointer::SliverIgnorePointer;
pub use sliver_list::{SliverChildBuilderDelegate, SliverList};
pub use sliver_main_axis_group::SliverMainAxisGroup;
pub use sliver_offstage::SliverOffstage;
pub use sliver_opacity::SliverOpacity;
pub use sliver_padding::SliverPadding;
pub use sliver_persistent_header::SliverPersistentHeader;
// The trait a header's content author implements; lives beside the element
// half in flui-view, surfaced here so `use flui_widgets::...` is sufficient.
pub use flui_view::element::SliverPersistentHeaderDelegate;
// What a delegate's `stretch_configuration` returns — surfaced with the
// trait for the same reason.
pub use flui_objects::{
    FloatingHeaderSnapConfiguration, OverScrollHeaderStretchConfiguration, StretchTriggerSignal,
};
pub use sliver_to_box_adapter::SliverToBoxAdapter;
pub use viewport::{ShrinkWrappingViewport, Viewport};
