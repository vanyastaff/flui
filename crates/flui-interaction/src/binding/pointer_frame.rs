//! The pointer frame at the binding: input a platform reported at one
//! instant, or one flush of coalesced motion, delivered so that recognizers
//! measuring several contacts settle once, after all of them moved.

use flui_foundation::geometry::Offset;
use smallvec::SmallVec;

use super::{DispatchOutcome, GestureBinding, MotionFlush};
use crate::{
    containment::RoutePanic, events::PointerEvent, ids::InputSurface, pointer::HitTestResult,
};

impl GestureBinding {
    /// Run `body`, which dispatches pointer events, as one pointer frame: the
    /// contacts a touch screen sampled at the same instant, or the motion of
    /// one display frame. The arena settles once, after `body`, and so do the
    /// recognizers that measure several contacts, which join the frame while
    /// `body` runs: a pinch never sees one finger moved and the other not.
    ///
    /// A panic of `body` does not skip the frame's end: it runs, and the first
    /// failure resumes after it. On a closed binding `body` just runs, each
    /// event it dispatches refused by its own dispatch.
    pub fn pointer_frame<R>(&self, body: impl FnOnce() -> R) -> R {
        if self.close.is_closed() {
            return body();
        }
        self.dispatch(|| {
            let arena = self.arena.arena();
            arena.begin_pointer_frame();
            let ran = RoutePanic::try_run(body);
            let ended = arena.end_pointer_frame();
            match ran {
                Ok(value) => {
                    if let Some(failure) = ended {
                        failure.resume();
                    }
                    value
                }
                Err(failure) => {
                    let mut first = Some(failure);
                    RoutePanic::preserve_first(&mut first, ended, "pointer frame end");
                    first
                        .expect("BUG: the failure of the frame was stored just above")
                        .resume()
                }
            }
        })
    }

    /// Dispatch the pointer events `surface` reported as one frame, in order;
    /// `outcomes` has the outcome of each. See
    /// [`pointer_frame`](Self::pointer_frame) for what makes them a frame. An
    /// event that panics does not drop the rest of the frame: every event is
    /// delivered before the first failure resumes. A frame of one is an
    /// ordinary event.
    pub fn dispatch_pointer_frame_on<F>(
        &self,
        surface: InputSurface,
        frame: &[PointerEvent],
        mut hit_test_fn: F,
    ) -> SmallVec<[DispatchOutcome; 4]>
    where
        F: FnMut(Offset<f64>) -> HitTestResult,
    {
        let mut outcomes = SmallVec::new();
        self.pointer_frame(|| {
            let mut first = None;
            for event in frame {
                let delivered = RoutePanic::capture(|| {
                    outcomes.push(self.dispatch_pointer_on(surface, event, &mut hit_test_fn));
                });
                RoutePanic::preserve_first(&mut first, delivered, "pointer frame event");
            }
            if let Some(failure) = first {
                failure.resume();
            }
        });
        outcomes
    }

    /// Deliver a flush's motion as one pointer frame: a recognizer that
    /// measures several contacts settles once, after all of them moved.
    pub(super) fn flush_pending_moves_kernel(&self, mode: MotionFlush) -> usize {
        let arena = self.arena.arena();
        arena.begin_pointer_frame();
        let flushed = RoutePanic::try_run(|| self.deliver_pending_moves(mode));
        let ended = arena.end_pointer_frame();
        match flushed {
            Ok(count) => {
                if let Some(failure) = ended {
                    failure.resume();
                }
                count
            }
            Err(failure) => {
                let mut first = Some(failure);
                RoutePanic::preserve_first(&mut first, ended, "pointer frame end");
                first
                    .expect("BUG: the failure of the flush was stored just above")
                    .resume()
            }
        }
    }
}
