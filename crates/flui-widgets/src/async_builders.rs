//! [`FutureBuilder`] and [`StreamBuilder`] — build from the latest state of an
//! asynchronous computation.
//!
//! Both are defined in `flui-view`, co-located with their element state (the same
//! arrangement as [`SliverList`](crate::SliverList) and
//! [`LayoutBuilder`](crate::LayoutBuilder)). Re-exported here so they read as part
//! of the widget catalog.
//!
//! # Identity is an explicit key
//!
//! Flutter compares `oldWidget.future == widget.future`. A Rust `Future`/`Stream`
//! is move-only, not `Clone`, not `Eq`, and cannot live in a view that is cloned
//! on every rebuild. So both take an explicit `key: Option<K>` plus a factory:
//! the subscription is recreated exactly when the key changes, and `None` means
//! "no future / no stream". This also makes Flutter's worst `FutureBuilder`
//! footgun — constructing the future inside `build` — unrepresentable.
//!
//! See [`ADR-0018`](../../../../docs/adr/ADR-0018-async-builder-seam.md) for the
//! seam, the parity findings, and the documented divergences.

pub use flui_view::element::{
    BoxedResultFuture, BoxedResultStream, FutureBuilder, FutureFactory, InitialDataFactory,
    SnapshotBuilder, StreamBuilder, StreamFactory,
};
/// The `Stream` trait a [`StreamBuilder`] consumes.
///
/// Re-exported from `futures-core` (trait-only: no executor, no combinators) so a
/// consumer can implement or name a stream without taking that dependency
/// directly.
pub use futures_core::Stream;
