//! What a layout decided to show of its children, kept as one answer that paint, hit testing,
//! the reported clip and the semantics visit read instead of computing their own.
//!
//! [`OverflowClip`] is the answer of a box that clips its children to its own size only while
//! one of them lies outside it (`RenderStack`, `RenderIndexedStack`, `RenderWrap`,
//! `RenderFittedBox`, `RenderAnimatedSize`, `RenderConstraintsTransformBox`).
//!
//! # Contract
//!
//! - **One writer, on every exit.** `perform_layout` is three statements: [`OverflowClip::begin`],
//!   the object's own `lay_out`, which returns its size together with whether a child lies
//!   outside it, and [`OverflowClip::commit`] of that result. An early `return Ok(..)` inside
//!   `lay_out` (no children, a hidden index) gives an answer as the last line does. A layout that
//!   returns `Err`, or panics and is caught by the pipeline, leaves what `begin` wrote: the box
//!   clips to the size it keeps, because the failed pass may already have moved children and no
//!   earlier answer is true of them.
//! - **No answer from a stand-in.** A descendant whose layout failed hands the box a stand-in
//!   size, but it is drawn, hit and published at the geometry it last committed, which the
//!   stand-in does not describe. A pass that the walk reports as degraded therefore commits no
//!   answer either: the box keeps clipping as `begin` left it.
//! - **The answer is the geometry.** A box that places children by offset answers whether a
//!   child's rectangle leaves its own ([`OverflowClip::leaves`]), not a proxy such as a target
//!   size, which disagrees with where the child lies on the exits that do not reach it.
//! - **Readers delegate.** `paint` pushes a clip only when [`OverflowClip::clip`] is `Some`, and
//!   `describe_approximate_paint_clip` returns that same value for every child, so the semantics
//!   walk and the input-region walk cut a child where paint cuts it. Hit testing needs no read:
//!   a box hit test rejects a point outside the box's own size, clipped or not.
//! - **Pure.** The readers are functions of the record and the size handed to them and run no
//!   user code, as the pipeline's walks require of these hooks.
//!
//! The record is a bool, not a rectangle: the rectangle is the box's size, which the hooks are
//! handed, and a stored copy would go stale on a size change committed without the record
//! (ADR-0224).

use flui_foundation::Arity;
use flui_foundation::geometry::{Offset, Point, Rect, Size};
use flui_painting::paint::Clip;
use flui_rendering::{
    BoxLayoutContext, BoxProtocol, LayoutContextApi, ParentData, Protocol, RenderResult,
};

/// Whether a box must clip its children to its size: `true` until a layout that returned `Ok`
/// from a pass with no stand-in says no child lies outside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OverflowClip {
    clips: bool,
}

impl OverflowClip {
    /// A record no layout has written: the box clips.
    pub(crate) const fn new() -> Self {
        Self { clips: true }
    }

    /// Starts a layout: the box clips until [`Self::commit`] says otherwise.
    pub(crate) fn begin(&mut self) {
        self.clips = true;
    }

    /// Ends a layout. `Ok((size, overflows))` stores `overflows` and hands `size` on, unless a
    /// descendant's layout in this pass was degraded to a stand-in; `Err` stores nothing. In both
    /// of those cases the box keeps clipping as [`Self::begin`] left it.
    pub(crate) fn commit<'ctx, A, PD>(
        &mut self,
        ctx: &BoxLayoutContext<'ctx, A, PD>,
        laid_out: RenderResult<(Size, bool)>,
    ) -> RenderResult<Size>
    where
        A: Arity,
        PD: ParentData + Default,
        <BoxProtocol as Protocol>::LayoutContext<'ctx, A, PD>:
            LayoutContextApi<'ctx, BoxProtocol, A, PD>,
    {
        let (size, overflows) = laid_out?;
        self.clips = overflows || ctx.descendant_layout_degraded();
        Ok(size)
    }

    /// Whether a child of `child_size` at `offset` leaves a box of `size` on any side.
    pub(crate) fn leaves(size: Size, offset: Offset, child_size: Size) -> bool {
        offset.dx < 0.0
            || offset.dy < 0.0
            || offset.dx + child_size.width > size.width
            || offset.dy + child_size.height > size.height
    }

    /// The box's rectangle when it must clip and `behavior` clips, else `None`.
    pub(crate) fn clip(&self, behavior: Clip, size: Size) -> Option<Rect> {
        (self.clips && behavior != Clip::None).then(|| Rect::from_origin_size(Point::ZERO, size))
    }

    /// Whether the box must clip: a child of the last committed layout lies outside it, or no
    /// layout has committed an answer since the last one began.
    pub(crate) const fn clips(&self) -> bool {
        self.clips
    }
}

impl Default for OverflowClip {
    fn default() -> Self {
        Self::new()
    }
}
