//! Per-run participation in presentation motion policy.

use super::{AnimationController, WalkProbe};

impl WalkProbe {
    /// Initial observation, settlement and parked resumption each need one
    /// sample even when they create no continuous animation demand.
    pub(crate) fn needs_sample(&self, tick: Option<crate::FrameTick>) -> bool {
        if !self.has_run {
            return false;
        }
        if self.live_running {
            return true;
        }
        let Some(tick) = tick else {
            return true;
        };
        let exhausted = tick.time(self.behavior).as_duration() == std::time::Duration::MAX;
        if self.parked {
            tick.policy() == crate::MotionPolicy::Full && !exhausted
        } else {
            exhausted
                || (self.behavior == crate::AnimationBehavior::Normal
                    && tick.policy() == crate::MotionPolicy::Reduce)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MotionRunState {
    Active,
    Parked,
}

pub(crate) enum SettleReason {
    Clock,
    ReducedMotion,
    ExhaustedClock,
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
