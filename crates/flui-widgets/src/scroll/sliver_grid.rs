//! [`SliverGrid`] — lazy (element-owned) 2-D grid sliver, re-exported from
//! `flui-view` where its element lifecycle lives.
//!
//! `SliverGrid` is a type alias for `SliverMultiBoxAdaptor<RenderSliverGrid>`,
//! the generic lazy adaptor instantiated for the grid's render object. An
//! alias adds no `TypeId` of its own: the element's identity is that
//! instantiation's, which is distinct from the list's
//! (`SliverMultiBoxAdaptor<RenderSliverList>`) because the render-object
//! parameter differs. Re-exported here for the widgets API.

// The `SliverGrid` type lives in `flui-view` (co-located with its element
// implementation). Re-exporting it here keeps the widgets-crate API surface
// consistent with how `SliverList` re-exports from `sliver_list.rs`.
pub use flui_view::element::SliverGrid;
