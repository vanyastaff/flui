//! What [`Dismissible`](super::Dismissible) and
//! [`SwipeActions`](super::SwipeActions) share: the stops a swiped row rests
//! on, its committed extent, the contacts its drag leaves to the operating
//! system, what lies behind the content at the edge it uncovers, and the
//! return a cancelled contact makes.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use flui_animation::{ArcCurve, MotionPolicy};
use flui_foundation::geometry::{Axis, Size};
use flui_foundation::{Listenable, ListenerId};
use flui_objects::SubtreeAnchor;
use flui_painting::typography::TextDirection;
use flui_rendering::pipeline::PipelineCell;
use flui_scheduler::PostFrameHandle;
use flui_view::prelude::*;
use flui_view::{BoxedView, RebuildHandle, RebuildReason};

use crate::anchored_box::AnchoredBox;
use crate::scroll::AnchoredDragController;
use crate::{Positioned, SizedBox};

/// How long a row takes to return to a stop it did not settle on by itself:
/// after a cancelled contact, a chosen action or a full swipe.
const RETURN: Duration = Duration::from_millis(250);

/// Where a swiped row rests. Offsets run toward the reading end, as an
/// [`AnchoredDrag`](crate::AnchoredDrag)'s do: a positive offset moves the
/// content toward the reading end and reveals the row's leading edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Stop {
    /// The content covers the row.
    Closed,
    /// The leading edge's actions are shown.
    Leading,
    /// The trailing edge's actions are shown.
    Trailing,
    /// The content went all the way toward the reading end: a full swipe of
    /// the leading edge, a dismissal toward the reading end or down.
    LeadingOut,
    /// The content went all the way toward the reading start: a full swipe
    /// of the trailing edge, a dismissal toward the reading start or up.
    TrailingOut,
}

impl Stop {
    /// Whether the content went all the way out.
    pub(crate) const fn is_out(self) -> bool {
        matches!(self, Self::LeadingOut | Self::TrailingOut)
    }
}

/// The committed size of a row, read through the presentation's pipeline
/// outside layout.
#[derive(Default)]
pub(crate) struct RowGeometry {
    anchor: SubtreeAnchor,
    pipeline: RefCell<Option<PipelineCell>>,
}

impl RowGeometry {
    /// Take the presentation's pipeline; called from the lifecycle hooks of
    /// the state that builds the row.
    pub(crate) fn acquire(&self, ctx: &dyn LifecycleContext) {
        let outgoing = self.pipeline.replace(ctx.pipeline_owner());
        drop(outgoing);
    }

    pub(crate) fn release(&self) {
        let outgoing = self.pipeline.borrow_mut().take();
        drop(outgoing);
    }

    /// `row`, measured: its committed size is what [`size`](Self::size)
    /// reads.
    pub(crate) fn measure(&self, row: impl IntoView) -> AnchoredBox {
        AnchoredBox::new(self.anchor.clone(), row)
    }

    /// The row's size as last laid out; `None` before the first layout, or
    /// while a layout runs.
    pub(crate) fn size(&self) -> Option<Size> {
        let node = self.anchor.get()?;
        let pipeline = self.pipeline.borrow().clone()?;
        pipeline.try_with(|owner| owner.box_size(node)).flatten()
    }

    /// The row's extent along `axis`, when it is finite and positive.
    pub(crate) fn extent(&self, axis: Axis) -> Option<f64> {
        let size = self.size()?;
        let extent = match axis {
            Axis::Horizontal => size.width,
            Axis::Vertical => size.height,
        };
        (extent.is_finite() && extent > 0.0).then_some(extent)
    }
}

/// The edge of a row its content uncovers: the leading one when the content
/// moved toward the reading end (or down), the trailing one the other way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Uncovered {
    Leading,
    Trailing,
}

impl Uncovered {
    /// The edge a row whose content sits at `offset` uncovers; `None` at
    /// rest.
    pub(crate) fn at(offset: f64) -> Option<Self> {
        if offset > 0.0 {
            Some(Self::Leading)
        } else if offset < 0.0 {
            Some(Self::Trailing)
        } else {
            None
        }
    }
}

/// `child` laid along `edge` of a row behind its content: `length` deep into
/// the row, or across all of it.
pub(crate) fn edge_strip(
    axis: Axis,
    direction: TextDirection,
    edge: Uncovered,
    length: Option<f64>,
    child: BoxedView,
) -> BoxedView {
    let strip = Positioned::new(child);
    let leading = edge == Uncovered::Leading;
    let strip = match axis {
        Axis::Vertical => {
            let strip = strip.left(0.0).right(0.0);
            let strip = match length {
                Some(length) => strip.height(length),
                None => strip.top(0.0).bottom(0.0),
            };
            if leading {
                strip.top(0.0)
            } else {
                strip.bottom(0.0)
            }
        }
        Axis::Horizontal => {
            let strip = strip.top(0.0).bottom(0.0);
            let strip = match length {
                Some(length) => strip.width(length),
                None => strip.left(0.0).right(0.0),
            };
            // The reading start is the left under left-to-right text.
            if leading == (direction == TextDirection::Ltr) {
                strip.left(0.0)
            } else {
                strip.right(0.0)
            }
        }
    };
    strip.boxed()
}

/// What lies behind a row's content at rest: nothing, in the place a strip
/// takes while an edge is uncovered, so the content keeps its own.
pub(crate) fn nothing_behind() -> BoxedView {
    Positioned::new(SizedBox::shrink())
        .left(0.0)
        .top(0.0)
        .boxed()
}

/// Rebuilds a row when the edge its content uncovers changes, and only then:
/// the content's motion patches a transform layer, so a drag or a settle
/// rebuilds nothing in between.
pub(crate) struct UncoverWatch {
    offsets: Rc<dyn Listenable>,
    listener: Cell<Option<ListenerId>>,
}

impl UncoverWatch {
    pub(crate) fn new(controller: &AnchoredDragController<Stop>, rebuild: RebuildHandle) -> Self {
        let offsets = controller.offset_listenable();
        let watched = controller.clone();
        let shown = Cell::new(Uncovered::at(controller.offset()));
        let listener = offsets.add_listener(Rc::new(move || {
            let edge = Uncovered::at(watched.offset());
            if shown.replace(edge) != edge {
                rebuild.schedule(RebuildReason::AnimationTick);
            }
        }));
        Self {
            offsets,
            listener: Cell::new(Some(listener)),
        }
    }

    pub(crate) fn retire(&self) {
        if let Some(id) = self.listener.take() {
            self.offsets.remove_listener(id);
        }
    }
}

/// Return `controller` to `stop`, at once when the motion policy reduces
/// motion. A stop the anchors lack leaves it where it is.
pub(crate) fn return_to(
    controller: &AnchoredDragController<Stop>,
    stop: Stop,
    motion: Option<MotionPolicy>,
) {
    let result = if motion == Some(MotionPolicy::Reduce) {
        controller.jump_to(stop)
    } else {
        controller.animate_to(
            stop,
            RETURN,
            ArcCurve::new(flui_animation::curves::EASE_OUT),
        )
    };
    if result.is_err() {
        tracing::debug!(?stop, "a swiped row returns to a stop it no longer has");
    }
}

/// Acts on each rest of a row at an outer stop, once, after the frame that
/// shows it: a full swipe performs its action, a dismissal collapses the row.
/// The rest is checked again when the frame is over, so a cancellation or a
/// new drag in between undoes it, and a row that is gone acts on nothing.
pub(crate) struct OutWatch {
    controller: AnchoredDragController<Stop>,
    listener: Cell<Option<ListenerId>>,
    mounted: Rc<Cell<bool>>,
}

impl OutWatch {
    /// Watch `controller`; `act` runs, with the stop, after each frame that
    /// shows the row resting out.
    pub(crate) fn new(
        controller: &AnchoredDragController<Stop>,
        post_frame: Option<PostFrameHandle>,
        act: Rc<dyn Fn(Stop)>,
    ) -> Self {
        let mounted = Rc::new(Cell::new(true));
        let pending = Rc::new(Cell::new(false));
        let watched = controller.clone();
        let alive = Rc::clone(&mounted);
        let listener = controller.listenable().add_listener(Rc::new(move || {
            let stop = watched.current();
            if watched.is_moving() || !stop.is_out() || pending.replace(true) {
                return;
            }
            let Some(post_frame) = &post_frame else {
                pending.set(false);
                tracing::warn!(
                    "a swiped row has no post-frame lane, so its rest out is not acted on"
                );
                return;
            };
            let (controller, pending, alive, act) = (
                watched.clone(),
                Rc::clone(&pending),
                Rc::clone(&alive),
                Rc::clone(&act),
            );
            let scheduled = post_frame.schedule(move |_| {
                pending.set(false);
                if alive.get() && !controller.is_moving() && controller.current() == stop {
                    act(stop);
                }
            });
            if let Err(error) = scheduled {
                tracing::warn!(?error, "a swiped row's post-frame lane is closed");
            }
        }));
        Self {
            controller: controller.clone(),
            listener: Cell::new(Some(listener)),
            mounted,
        }
    }

    /// Stop watching: a rest not yet acted on is dropped.
    pub(crate) fn retire(&self) {
        self.mounted.set(false);
        if let Some(id) = self.listener.take() {
            self.controller.listenable().remove_listener(id);
        }
    }
}

/// Where a cancelled contact returns a row: the stop it rested on when the
/// contact went down. A cancelled contact undoes its swipe, so it never
/// performs a full swipe or a dismissal.
#[derive(Default)]
pub(crate) struct ContactOrigin {
    stop: Cell<Option<Stop>>,
}

impl ContactOrigin {
    /// A contact went down on the row resting at `stop`.
    pub(crate) fn down(&self, stop: Stop) {
        self.stop.set(Some(stop));
    }

    /// The contact was cancelled: return a row it moved to where it began.
    /// Called after the drag's own cancellation settled or started its
    /// settle, which the return replaces. A rest on an outer stop that the
    /// cancellation reached is undone before it is acted on, since a row
    /// acts on a rest only after the frame.
    pub(crate) fn cancelled(
        &self,
        controller: &AnchoredDragController<Stop>,
        motion: Option<MotionPolicy>,
    ) {
        let Some(stop) = self.stop.take() else {
            return;
        };
        let home = controller.anchors().offset(stop);
        if controller.is_moving() || home != Some(controller.offset()) {
            return_to(controller, stop, motion);
        }
    }
}
