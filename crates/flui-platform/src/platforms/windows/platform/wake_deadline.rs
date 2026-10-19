//! The owner's wake deadline as the message loop arms and delivers it.

use std::sync::{Arc, Weak};

pub(super) type WakeDeadlineHook = dyn Fn() -> Option<web_time::Instant> + Send + Sync;

// The query may replace its own registration before panicking. Keep its
// outgoing ownership in custody before invoking it, so captures cannot
// double-panic during the incoming unwind. Healthy retirement remains normal;
// this cannot rescue an aggregate that double-panics during healthy Drop.
pub(super) struct DeadlineQuery(pub(super) Option<Arc<WakeDeadlineHook>>);

impl Drop for DeadlineQuery {
    fn drop(&mut self) {
        if std::thread::panicking()
            && let Some(hook) = self.0.take()
        {
            std::mem::forget(hook);
        }
    }
}

/// The last deadline the loop delivered. The hook answers with an instant
/// only, so the same `(hook, instant)` pair is either work no frame has had
/// a chance to service, which must not spin, or work a frame left or
/// accepted at the instant it just serviced (a queue of obligations all due
/// at one instant answers the same pair once per item), which must get its
/// frame. The frame count is the delivery generation that tells them apart.
pub(super) struct DeadlineDelivery {
    pub(super) arm: DeadlineArm,
    /// The platform's frame count when the delivery was made.
    pub(super) frames: u64,
}

impl DeadlineDelivery {
    /// Whether `deadline` from `hook` may be armed after this delivery: a
    /// different pair always, the same pair whenever a frame callback ran
    /// since. An answer no frame could have serviced stays delivered, so a
    /// delivery that reaches no frame never spins the loop.
    pub(super) fn admits(
        &self,
        deadline: web_time::Instant,
        hook: &Weak<WakeDeadlineHook>,
        frames: u64,
    ) -> bool {
        !self.arm.matches(deadline, hook) || frames != self.frames
    }
}

/// How many frame callbacks this platform's windows have run. The message
/// loop compares it across a deadline delivery to tell an answer repeated
/// after a serviced frame from one nothing has serviced yet. Each window's
/// [`WindowCallbacks`](crate::shared::WindowCallbacks) counts into it when its frame callback runs, so a
/// frame request no callback answered (a window with nothing registered,
/// or one already cleared) leaves it unchanged.
#[derive(Default)]
pub(in crate::platforms::windows) struct FrameCount(Arc<std::sync::atomic::AtomicU64>);

impl FrameCount {
    pub(super) fn get(&self) -> u64 {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The counter a window's callbacks increment.
    pub(in crate::platforms::windows) fn counter(&self) -> Arc<std::sync::atomic::AtomicU64> {
        Arc::clone(&self.0)
    }
}

pub(super) struct DeadlineArm {
    pub(super) deadline: web_time::Instant,
    pub(super) hook: Weak<WakeDeadlineHook>,
}

impl DeadlineArm {
    fn matches(&self, deadline: web_time::Instant, hook: &Weak<WakeDeadlineHook>) -> bool {
        self.deadline == deadline && Weak::ptr_eq(&self.hook, hook)
    }
}
