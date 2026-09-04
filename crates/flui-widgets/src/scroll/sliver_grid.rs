//! [`SliverGrid`] — lazy (element-owned) 2-D grid sliver, re-exported from
//! `flui-view` where its element lifecycle lives.
//!
//! `SliverGrid` is the canonical lazy-sliver view type, defined in `flui-view`
//! so the element's identity (`view_type_id`) is `TypeId::of::<SliverGrid>()`
//! rather than an internal adaptor type. Re-exported here for the widgets API.

// The `SliverGrid` type lives in `flui-view` (co-located with its element
// implementation). Re-exporting it here keeps the widgets-crate API surface
// consistent with how `SliverList` re-exports from `sliver_list.rs`.
pub use flui_view::element::SliverGrid;
