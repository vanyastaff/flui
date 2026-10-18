//! The refresh rate a presentation asks its display for (ADR-0236).
//!
//! A presentation is [`Active`](FrameRateDemand::Active) while it animates or a
//! pointer is down, and [`Idle`](FrameRateDemand::Idle) when neither has held
//! for [`FRAME_RATE_HOLD`]. The runtime only decides and publishes the change
//! through [`OwnerEffects::frame_rate_demand_changed`]; it has no platform edge,
//! so the host turns the change into a window request (ADR-0083).
//!
//! The release is delayed so that the demand does not flutter between the
//! frames of an animation. A still screen produces no frames, so the delay
//! is also a wake deadline the runtime reports through its ordinary
//! wall-clock deadline sources: without it the release would wait for an
//! unrelated frame. A hidden or closing presentation draws nothing and is
//! released at once, with no hold.
//!
//! Only a host that asks for it ([`OwnerEffects::tracks_frame_rate_demand`])
//! gets any of this: elsewhere no demand is observed, no hold is armed and no
//! deadline joins the loop's wake sources.
//!
//! The owner keeps, per presentation, the demand it last delivered, and each
//! work item delivers whatever differs from it. A hook that fails or is absent
//! therefore does not lose the change: it is owed until a delivery succeeds.
//!
//! [`OwnerEffects::frame_rate_demand_changed`]: crate::owner::OwnerEffects::frame_rate_demand_changed
//! [`OwnerEffects::tracks_frame_rate_demand`]: crate::owner::OwnerEffects::tracks_frame_rate_demand

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
    /// Fold in what the presentation wants at `now`. With `hold` false a demand
    /// that is no longer wanted is released at once instead of after the hold.
    pub(crate) fn observe(&mut self, wanted: bool, hold: bool, now: Instant) {
        if wanted {
            self.release_at = None;
            self.decided = FrameRateDemand::Active;
        } else if self.decided == FrameRateDemand::Active {
            if !hold {
                self.release_at = None;
                self.decided = FrameRateDemand::Idle;
                return;
            }
            match self.release_at {
                None => self.release_at = Some(now + FRAME_RATE_HOLD),
                Some(due) if now >= due => {
                    self.release_at = None;
                    self.decided = FrameRateDemand::Idle;
                }
                Some(_) => {}
            }
        }
    }

    /// The demand as last decided.
    pub(crate) fn decided(&self) -> FrameRateDemand {
        self.decided
    }

    /// The instant the pending release is due.
    pub(crate) fn release_at(&self) -> Option<Instant> {
        self.release_at
    }
}
