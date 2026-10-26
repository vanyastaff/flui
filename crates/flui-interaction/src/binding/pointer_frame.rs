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
    /// Dispatch the pointer events `surface` reported as one frame: the
    /// contacts a touch screen sampled at the same instant, or the motion of
    /// one display frame. Each event is dispatched as
    /// [`dispatch_pointer_on`](Self::dispatch_pointer_on) dispatches it, in
    /// order, and `outcomes` has the outcome of each. What differs is when the
    /// arena settles and when recognizers that measure several contacts do:
    /// once, after the last event, so a pinch never sees one finger moved and
    /// the other not. A frame of one is an ordinary event.
    ///
    /// An event that panics does not drop the rest of the frame: every event
    /// is delivered and the frame settles before the first failure resumes.
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
        if self.close.is_closed() {
            // Each event is refused by its own dispatch, which owns the
            // rejection of a closed binding's callbacks.
            for event in frame {
                outcomes.push(self.dispatch_pointer_on(surface, event, &mut hit_test_fn));
            }
            return outcomes;
        }
        self.dispatch(|| {
            let arena = self.arena.arena();
            arena.begin_pointer_frame();
            let mut first = None;
            for event in frame {
                let delivered = RoutePanic::capture(|| {
                    outcomes.push(self.dispatch_pointer_on(surface, event, &mut hit_test_fn));
                });
                RoutePanic::preserve_first(&mut first, delivered, "pointer frame event");
            }
            let ended = arena.end_pointer_frame();
            RoutePanic::preserve_first(&mut first, ended, "pointer frame end");
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
