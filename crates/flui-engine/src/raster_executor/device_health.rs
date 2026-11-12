//! The context's device health: the pacing of its rebuilds after a loss, and
//! the point where a rebuild that keeps failing is given up (ADR-0305 §8,
//! ADR-0267).
//!
//! The executor keeps one [`DeviceRebuilds`] for its context and publishes
//! the [`DeviceHealth`] it derives into the record every slot's owner reads.
//! Time comes from the clock the executor was built with, so a manual clock
//! walks the backoff without sleeping.

use web_time::{Duration, Instant};

/// Where the context's device stands, as the owner reads it from
/// [`RasterSender::device_health`](super::RasterSender::device_health).
///
/// While the device is lost every frame settles
/// [`RasterVerdict::DeviceLost`](super::RasterVerdict::DeviceLost).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DeviceHealth {
    /// The device renders.
    #[default]
    Healthy,
    /// The device was lost and the executor rebuilds it on a backoff:
    /// `attempts` rebuilds failed in a row so far, none when the loss was
    /// just seen.
    Rebuilding {
        /// Consecutive failed rebuilds.
        attempts: u32,
    },
    /// The rebuild was given up after `attempts` consecutive failures. No
    /// rebuild runs until the owner sends
    /// [`RasterCommand::RetryDevice`](super::RasterCommand::RetryDevice).
    GivenUp {
        /// Consecutive failed rebuilds when the rebuild was given up.
        attempts: u32,
    },
}

impl DeviceHealth {
    /// Whether the device is lost: rebuilding or given up.
    #[must_use]
    pub fn is_lost(self) -> bool {
        match self {
            Self::Healthy => false,
            Self::Rebuilding { .. } | Self::GivenUp { .. } => true,
        }
    }
}

/// The first wait after a failed rebuild: about one frame at 60 Hz.
pub(crate) const REBUILD_BASE: Duration = Duration::from_millis(16);
/// The longest wait between two rebuilds.
pub(crate) const REBUILD_CAP: Duration = Duration::from_secs(1);
/// How many consecutive rebuilds may fail at [`REBUILD_CAP`] before the
/// rebuild is given up. The backoff reaches its ceiling after seven failures,
/// about two seconds after the loss, so ten more give the driver about
/// twelve seconds in all: longer than a Windows driver reset (TDR, two
/// seconds by default) or a GPU switch takes.
pub(crate) const REBUILD_ATTEMPTS_AT_CAP: u32 = 10;
/// `REBUILD_BASE << 6` is past the cap, so the shift stops there and never
/// overflows on a long streak.
const SHIFT_CAP: u32 = 6;

/// What one failed rebuild left the streak at, for the executor to log once
/// its state is committed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FailedRebuild {
    /// The first failure of the streak.
    First,
    /// The first failure with the backoff at its ceiling.
    ReachedCap,
    /// Another failure; the backoff waits `wait`.
    Again { wait: Duration },
    /// The rebuild was given up.
    GivenUp,
}

/// The rebuild streak of one context's device.
#[derive(Debug, Default)]
pub(crate) struct DeviceRebuilds {
    lost: bool,
    /// Consecutive failed rebuilds.
    attempts: u32,
    /// How many of them failed with the backoff at its ceiling.
    at_cap: u32,
    given_up: bool,
    /// The earliest instant the next rebuild may run; `None` for now.
    next_at: Option<Instant>,
}

impl DeviceRebuilds {
    /// The health this streak publishes.
    pub(crate) fn health(&self) -> DeviceHealth {
        match (self.lost, self.given_up) {
            (false, _) => DeviceHealth::Healthy,
            (true, false) => DeviceHealth::Rebuilding {
                attempts: self.attempts,
            },
            (true, true) => DeviceHealth::GivenUp {
                attempts: self.attempts,
            },
        }
    }

    /// When the next rebuild may run, for the owner's wake: `now` while one
    /// is due at once, `None` while the device is healthy or the rebuild is
    /// given up, which need no wake.
    pub(crate) fn next_attempt_at(&self, now: Instant) -> Option<Instant> {
        if self.lost && !self.given_up {
            Some(self.next_at.unwrap_or(now))
        } else {
            None
        }
    }

    /// No rebuild can succeed where the executor runs: give up now, until
    /// the owner asks again.
    pub(crate) fn give_up(&mut self) {
        self.lost = true;
        self.given_up = true;
        self.next_at = None;
    }

    /// The device was seen lost.
    pub(crate) fn note_lost(&mut self) {
        self.lost = true;
    }

    /// Whether a rebuild may run at `now`.
    pub(crate) fn due(&self, now: Instant) -> bool {
        self.lost && !self.given_up && self.next_at.is_none_or(|at| now >= at)
    }

    /// A rebuild failed at `now`: arm the next one, or give the rebuild up
    /// once [`REBUILD_ATTEMPTS_AT_CAP`] failures ran at the ceiling.
    pub(crate) fn record_failure(&mut self, now: Instant) -> FailedRebuild {
        let shift = self.attempts.min(SHIFT_CAP);
        let wait = REBUILD_BASE.saturating_mul(1 << shift).min(REBUILD_CAP);
        let first = self.attempts == 0;
        let at_cap = wait >= REBUILD_CAP;
        let reached_cap = at_cap && self.at_cap == 0;
        self.attempts = self.attempts.saturating_add(1);
        if at_cap {
            self.at_cap = self.at_cap.saturating_add(1);
        }
        self.next_at = now.checked_add(wait);
        if self.at_cap >= REBUILD_ATTEMPTS_AT_CAP {
            self.given_up = true;
            return FailedRebuild::GivenUp;
        }
        if first {
            FailedRebuild::First
        } else if reached_cap {
            FailedRebuild::ReachedCap
        } else {
            FailedRebuild::Again { wait }
        }
    }

    /// The device renders again: the streak ends.
    pub(crate) fn recovered(&mut self) {
        *self = Self::default();
    }

    /// The owner asked again after the rebuild was given up: the next turn
    /// rebuilds at once. Nothing changes while the device renders or a
    /// rebuild is still paced.
    pub(crate) fn retry(&mut self) -> bool {
        if !self.given_up {
            return false;
        }
        *self = Self {
            lost: true,
            ..Self::default()
        };
        true
    }
}

#[cfg(test)]
mod tests {
    use web_time::Instant;

    use super::{
        DeviceHealth, DeviceRebuilds, FailedRebuild, REBUILD_ATTEMPTS_AT_CAP, REBUILD_BASE,
        REBUILD_CAP,
    };

    #[test]
    fn the_rebuild_backoff_climbs_to_its_ceiling_and_gives_up_after_ten_at_it() {
        let mut rebuilds = DeviceRebuilds::default();
        let now = Instant::now();
        rebuilds.note_lost();
        assert!(rebuilds.due(now), "the first rebuild runs at once");
        assert_eq!(
            rebuilds.next_attempt_at(now),
            Some(now),
            "a rebuild due at once is due now"
        );
        let mut outcomes = Vec::new();
        while outcomes.last() != Some(&FailedRebuild::GivenUp) {
            assert!(outcomes.len() < 100, "the rebuild is given up");
            outcomes.push(rebuilds.record_failure(now));
            if let Some(at) = rebuilds.next_attempt_at(now) {
                assert!(!rebuilds.due(now), "the next rebuild waits out the backoff");
                assert!(
                    at.duration_since(now) <= REBUILD_CAP,
                    "never past the ceiling"
                );
            }
        }
        // 16, 32, … 512 ms below the ceiling, then ten failures at it.
        let below_cap = 6;
        assert_eq!(outcomes.len(), below_cap + REBUILD_ATTEMPTS_AT_CAP as usize);
        assert_eq!(outcomes[0], FailedRebuild::First);
        assert_eq!(
            outcomes[1],
            FailedRebuild::Again {
                wait: REBUILD_BASE * 2
            }
        );
        assert_eq!(outcomes[below_cap], FailedRebuild::ReachedCap);
        let failures = u32::try_from(outcomes.len()).expect("a short streak");
        assert_eq!(
            rebuilds.health(),
            DeviceHealth::GivenUp { attempts: failures }
        );
        assert_eq!(
            rebuilds.next_attempt_at(now),
            None,
            "a given-up rebuild needs no wake"
        );
        assert!(!rebuilds.due(now + REBUILD_CAP * 100));

        assert!(rebuilds.retry(), "the owner asks again");
        assert_eq!(rebuilds.health(), DeviceHealth::Rebuilding { attempts: 0 });
        assert!(rebuilds.due(now), "the asked-for rebuild runs at once");
        rebuilds.recovered();
        assert_eq!(rebuilds.health(), DeviceHealth::Healthy);
        assert!(!rebuilds.retry(), "a healthy device has nothing to retry");
    }
}
