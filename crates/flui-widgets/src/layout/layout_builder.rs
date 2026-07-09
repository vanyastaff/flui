//! [`LayoutBuilder`] — build a child from the constraints the parent imposes.
//!
//! `LayoutBuilder` is defined in `flui-view`, co-located with its element (the
//! same arrangement as [`SliverList`](crate::SliverList)) so the element's
//! `view_type_id` is `TypeId::of::<LayoutBuilder>()` rather than an internal
//! adaptor type's. Re-exported here so it reads as part of the widget catalog.
//!
//! The builder runs during layout with the real incoming `BoxConstraints`, and
//! the child it returns is laid out **and painted in the same frame** — see
//! [`ADR-0017`](../../../../docs/adr/ADR-0017-build-during-layout-callback-seam.md)
//! for how that is achieved without Flutter's mid-pass `invokeLayoutCallback`.

pub use flui_view::element::LayoutBuilder;
