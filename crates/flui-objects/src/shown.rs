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

use flui_foundation::geometry::{Point, Rect, Size};
use flui_painting::paint::Clip;
use flui_rendering::RenderResult;

/// Whether a box must clip its children to its size: `true` until a layout that returned `Ok`
/// says no child lies outside it.
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

    /// Ends a layout. `Ok((size, overflows))` stores `overflows` and hands `size` on; `Err`
    /// stores nothing, so the box keeps clipping as [`Self::begin`] left it.
    pub(crate) fn commit(&mut self, laid_out: RenderResult<(Size, bool)>) -> RenderResult<Size> {
        let (size, overflows) = laid_out?;
        self.clips = overflows;
        Ok(size)
    }

    /// The box's rectangle when it must clip and `behavior` clips, else `None`.
    pub(crate) fn clip(&self, behavior: Clip, size: Size) -> Option<Rect> {
        (self.clips && behavior != Clip::None).then(|| Rect::from_origin_size(Point::ZERO, size))
    }

    /// Whether the box must clip: a child of the last layout that returned `Ok` lies outside
    /// it, or no layout has returned `Ok` since the last one began.
    pub(crate) const fn clips(&self) -> bool {
        self.clips
    }
}

impl Default for OverflowClip {
    fn default() -> Self {
        Self::new()
    }
}
