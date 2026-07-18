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

mod custom_scroll_view;
mod grid_view;
mod list_view;
mod refresh_indicator;
mod scroll_controller;
mod scroll_physics;
mod scrollable;
mod scrollbar;
mod single_child_scroll_view;
mod sliver_fill_remaining;
mod sliver_fill_viewport;
mod sliver_fixed_extent_list;
mod sliver_grid;
mod sliver_ignore_pointer;
mod sliver_list;
mod sliver_offstage;
mod sliver_opacity;
mod sliver_padding;
mod sliver_to_box_adapter;
mod viewport;

pub use custom_scroll_view::CustomScrollView;
pub use grid_view::GridView;
pub use list_view::ListView;
pub use refresh_indicator::{RefreshController, RefreshIndicator, RefreshIndicatorState};
pub use scroll_controller::ScrollController;
pub use scroll_physics::{
    BouncingScrollPhysics, ClampingScrollPhysics, ScrollMetrics, ScrollPhysics, SharedScrollPhysics,
};
pub use scrollable::Scrollable;
pub use scrollbar::Scrollbar;
pub use single_child_scroll_view::SingleChildScrollView;
pub use sliver_fill_remaining::{
    SliverFillRemaining, SliverFillRemainingAndOverscroll, SliverFillRemainingWithScrollable,
};
pub use sliver_fill_viewport::SliverFillViewport;
pub use sliver_fixed_extent_list::SliverFixedExtentList;
pub use sliver_grid::{SliverGrid, SliverGridLazy};
pub use sliver_ignore_pointer::SliverIgnorePointer;
pub use sliver_list::{SliverChildBuilderDelegate, SliverList};
pub use sliver_offstage::SliverOffstage;
pub use sliver_opacity::SliverOpacity;
pub use sliver_padding::SliverPadding;
pub use sliver_to_box_adapter::SliverToBoxAdapter;
pub use viewport::{ShrinkWrappingViewport, Viewport};
