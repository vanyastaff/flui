//! Scroll widgets — show a scrollable window into content larger than the
//! available space, over `flui-objects`' viewport/sliver render objects.
//!
//! The low-level primitives ([`Viewport`], [`SliverToBoxAdapter`],
//! [`SliverFixedExtentList`]) compose the cross-protocol box→sliver→box layout
//! path; [`SingleChildScrollView`] and [`ListView`] are the common user-facing
//! widgets. Scroll offset is programmatic for now — gesture-driven scrolling
//! lands with the `Scrollable`/`ScrollController` layer.

mod list_view;
mod single_child_scroll_view;
mod sliver_fixed_extent_list;
mod sliver_to_box_adapter;
mod viewport;

pub use list_view::ListView;
pub use single_child_scroll_view::SingleChildScrollView;
pub use sliver_fixed_extent_list::SliverFixedExtentList;
pub use sliver_to_box_adapter::SliverToBoxAdapter;
pub use viewport::Viewport;
