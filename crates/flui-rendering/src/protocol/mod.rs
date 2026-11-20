//! Layout protocols: the sealed [`Protocol`] trait and its two implementors.
//!
//! - [`BoxProtocol`]: 2D layout, `BoxConstraints` in and `Size` out, hit-tested
//!   at an `Offset`. Most render objects use it.
//! - [`SliverProtocol`]: scrollable content, `SliverConstraints` in and
//!   `SliverGeometry` out, hit-tested at a [`MainAxisPosition`].
//!
//! A protocol names its types as associated types (`P::Constraints`,
//! `P::Geometry`, `P::Position`, …) and its typed contexts as
//! [`Protocol::LayoutContext`] ([`RawBoxLayoutContext`],
//! [`RawSliverLayoutContext`]) and [`Protocol::HitTestContext`]
//! ([`RawBoxHitTestContext`], [`RawSliverHitTestContext`]), whose operations are
//! [`LayoutContextApi`] and [`HitTestContextApi`]. The pipeline drives a render
//! object through [`Protocol::ErasedLayoutContext`] ([`ErasedBoxLayoutContext`],
//! [`ErasedSliverLayoutContext`]), the arity- and parent-data-erased form of
//! the layout context, which the [`RenderObject`] blanket impls turn back into
//! a typed one.
//!
//! The erased traits share method names with [`LayoutContextApi`]
//! (`constraints`, `layout_child`, `position_child`), so a scope that imports
//! both by a glob of this module gets ambiguous method calls: import the
//! traits you call by name. Nothing re-exports this module by glob.
//!
//! ```
//! use flui_rendering::protocol::Protocol;
//!
//! fn is_cacheable<P: Protocol>(constraints: &P::Constraints) -> bool {
//!     P::cache_key(constraints).is_some()
//! }
//! ```

mod box_protocol;
mod context_api;
mod into_render_object;
#[expect(
    clippy::module_inception,
    reason = "`protocol.rs` inside `protocol/` holds the core `Protocol` trait"
)]
mod protocol;
mod sliver_protocol;

pub use box_protocol::{
    ActualBaselineChildCallback, BoxChildIntrinsicCallback, BoxConstraintsCacheKey,
    BoxHitTestEntry, BoxHitTestResult, BoxProtocol, ChildState, DegradationProbe,
    DriverBoxLayoutContext, ErasedBoxLayoutContext, ErasedChildState, HitTestChildCallback,
    LayoutChildCallback, RawBoxHitTestContext, RawBoxLayoutContext, SliverLayoutChildCallback,
};
pub use context_api::{HitTestContextApi, LayoutContextApi};
pub use into_render_object::IntoRenderObject;
pub use protocol::{Protocol, UsageByParent};
pub(crate) use sliver_protocol::ErasedBoxChildLayoutCallback;
pub use sliver_protocol::{
    BoxChildLayoutCallback, BoxChildSize, ChildLayout, DriverSliverLayoutContext,
    ErasedSliverChildState, ErasedSliverLayoutContext, MainAxisPosition, RawSliverHitTestContext,
    RawSliverLayoutContext, SliverChildLayoutCallback, SliverChildState, SliverConstraintsCacheKey,
    SliverHitTestChildCallback, SliverHitTestEntry, SliverHitTestResult, SliverProtocol,
};

pub use crate::traits::RenderObject;
