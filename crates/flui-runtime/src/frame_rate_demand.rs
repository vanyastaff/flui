//! The refresh rate a presentation asks its display for (ADR-0236).
//!
//! A presentation is [`Active`](FrameRateDemand::Active) while it animates or a
//! pointer is down, and [`Idle`](FrameRateDemand::Idle) when neither has held
//! for [`FRAME_RATE_HOLD`]. The runtime only decides and publishes the edge
//! through [`OwnerEffects::frame_rate_demand_changed`]; it has no platform edge,
//! so the host turns the edge into a window request (ADR-0083).
//!
//! The release is delayed so that the demand does not flutter between the
//! frames of an animation. A still screen produces no frames, so the delay
//! is also a wake deadline the runtime reports through its ordinary
//! wall-clock deadline sources: without it the release would wait for an
//! unrelated frame.
//!
//! [`OwnerEffects::frame_rate_demand_changed`]: crate::owner::OwnerEffects::frame_rate_demand_changed

use std::time::Duration;

use web_time::Instant;

/// How long a presentation keeps asking for the high rate after its last
/// animation or pointer contact ended.
pub const FRAME_RATE_HOLD: Duration = Duration::from_millis(250);

/// Whether a presentation currently wants its display's highest refresh rate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum FrameRateDemand {
    /// Nothing animates and no pointer is down, or the hold has run out: the
    /// display may pick its own rate.
    #[default]
    Idle,
    /// An animation runs or a pointer is down: ask for the highest rate.
    Active,
}

/// One presentation's demand state: the demand last decided and the instant the
/// pending release becomes due.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DemandGovernor {
    decided: FrameRateDemand,
    release_at: Option<Instant>,
}

impl DemandGovernor {
    /// Fold in what the presentation wants at `now`; the new demand when it
    /// changed.
    pub(crate) fn observe(&mut self, wanted: bool, now: Instant) -> Option<FrameRateDemand> {
        let before = self.decided;
        if wanted {
            self.release_at = None;
            self.decided = FrameRateDemand::Active;
        } else if before == FrameRateDemand::Active {
            match self.release_at {
                None => self.release_at = Some(now + FRAME_RATE_HOLD),
                Some(due) if now >= due => {
                    self.release_at = None;
                    self.decided = FrameRateDemand::Idle;
                }
                Some(_) => {}
            }
        }
        (self.decided != before).then_some(self.decided)
    }

    /// The instant the pending release is due.
    pub(crate) fn release_at(&self) -> Option<Instant> {
        self.release_at
    }

    /// Whether the pending release is due at `now`.
    pub(crate) fn release_due(&self, now: Instant) -> bool {
        self.release_at.is_some_and(|due| now >= due)
    }
}
