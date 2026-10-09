//! Per-run participation in presentation motion policy.

use super::AnimationController;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MotionRunState {
    Active,
    Parked,
}

pub(crate) enum SettleReason {
    Clock,
    ReducedMotion,
}

impl AnimationController {
    /// Resume only the parked run whose identity the registry just observed.
    /// This prepares state without invoking observers or frame hooks.
    pub(crate) fn resume_motion_run(&self, generation: u64) -> bool {
        let mut inner = self.inner.borrow_mut();
        if inner.disposed
            || inner.active_run.is_none()
            || inner.run_generation != generation
            || inner.motion_state != MotionRunState::Parked
        {
            return false;
        }
        inner.motion_state = MotionRunState::Active;
        true
    }
}
