//! A slot's surface state: whether it has a surface to present to, and the
//! newest size it was sent with the backend's answer (ADR-0305 §6, §8).
//!
//! The executor and the owner's sends change it only through
//! [`SlotSurface::on`], and a slot's record publishes it, so the owner reads
//! where the slot stands instead of inferring it from the failures it took.
//! A failure the slot holds says what went wrong, for a report; the state
//! says what the owner does next.
//!
//! | Event | [`SurfaceHold`] after | [`SlotSize`] after |
//! |-------|-----------------------|--------------------|
//! | the owner sends size `s` | unchanged | `Pending(s)` |
//! | the backend applies `s` | unchanged | `Applied(s)` if it was `Pending(s)` and no size was sent since |
//! | the backend refuses `s` for good (past the device's limit) | unchanged | `Refused(s)` if it was `Pending(s)` and no size was sent since |
//! | any other resize error, a resize that panics | unchanged | unchanged: `Pending(s)` is tried on the next turn |
//! | the owner sends a restore | `Restoring` | unchanged |
//! | the restore's send fails | what it was before, if still `Restoring` | unchanged |
//! | the window's surface is dropped (a render that needs a new one, a rebuilt device that cannot present to it) | `Wanted`, or `Restoring` while a restore is on its way | unchanged |
//! | a restore the backend takes | `Held` | unchanged |
//! | a restore the backend refuses | `Refused` | unchanged |
//! | a restore that panics | `Wanted` | unchanged |
//! | a restore or a rebuild that cannot configure at `s` (over the device's limit) | `Held`: the surface is kept, unconfigured | `Refused(s)` if it was `Applied(s)` |
//! | the device is rebuilt | unchanged | `Pending(s)` if it was `Refused(s)`: the new device answers it |
//! | the owner sends a release | `Released` | unchanged |
//! | the release's send fails | what it was before, if still `Released` | unchanged |
//! | while `Released`: a dropped surface, a restore taken, refused or panicked | `Released`: a release sent after the restore is still to come | unchanged, or `Refused(s)` for a restore over the limit as above |
//!
//! A frame renders only on a held surface configured at the size its stamp
//! was laid out for: a new surface generation authorises frames only while
//! the size is [settled](SlotSize::is_settled).

use flui_foundation::geometry::DeviceSize;

/// Whether a window slot has a surface to present to. An offscreen slot's
/// target is always held.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceHold {
    /// The slot holds a surface.
    Held,
    /// The slot dropped its surface: the owner makes a new one with its
    /// [`SurfaceFactory`](crate::SurfaceFactory) and sends it in a
    /// [`RasterCommand::Restore`](super::RasterCommand::Restore). Until one
    /// is restored the slot's frames settle not shown.
    Wanted,
    /// The owner sent a surface the executor has not answered yet: the
    /// owner sends no other until it does.
    Restoring,
    /// The newest surface the owner sent was refused, and the slot has
    /// none: a frame has nowhere to go until the owner sends another.
    Refused,
    /// The owner released the surface (the platform took the window's
    /// surface away): the slot wants none until the owner restores one when
    /// the platform gives the window back, and its frames settle not shown.
    Released,
}

/// The newest size a slot was sent, and the backend's answer to it.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotSize {
    /// No size yet: a window attached before it had one.
    Unsized,
    /// Sent and not applied yet. The executor applies it on its next turn,
    /// and again on each turn after a refusal for now or a panic.
    Pending(DeviceSize),
    /// The backend is configured at it.
    Applied(DeviceSize),
    /// The backend refuses it for good (past the device's limit), and the
    /// executor does not try it again: a frame laid out for it settles
    /// stale until the owner sends another size or a rebuilt device takes
    /// it.
    Refused(DeviceSize),
}

impl SlotSize {
    /// The newest size the slot was sent, whatever became of it.
    #[must_use]
    pub fn newest(self) -> Option<DeviceSize> {
        match self {
            Self::Unsized => None,
            Self::Pending(size) | Self::Applied(size) | Self::Refused(size) => Some(size),
        }
    }

    /// Whether the backend has answered the newest size by taking it: a
    /// new surface generation then authorises frames at once, where a size
    /// pending or refused keeps its barrier until the backend takes one.
    #[must_use]
    pub fn is_settled(self) -> bool {
        matches!(self, Self::Unsized | Self::Applied(_))
    }
}

/// A slot's surface state, as its [`SlotRecord`](super::SlotRecord)
/// publishes it.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotSurface {
    /// Whether the slot has a surface.
    pub hold: SurfaceHold,
    /// Its newest size and the backend's answer.
    pub size: SlotSize,
}

/// What changes a slot's surface state; the module's table lists each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfaceEvent {
    /// The owner sent a size.
    SizeSent(DeviceSize),
    /// The backend applied a size.
    Resized(DeviceSize),
    /// The backend refused a size for good.
    SizeRefused(DeviceSize),
    /// The owner sent a restore.
    RestoreSent,
    /// The restore's send failed: the hold goes back to what it was.
    RestoreWithdrawn(SurfaceHold),
    /// The owner sent a release.
    ReleaseSent,
    /// The release's send failed: the hold goes back to what it was.
    ReleaseWithdrawn(SurfaceHold),
    /// The backend no longer presents to the window's surface.
    SurfaceDropped,
    /// The backend took a restored surface.
    Restored,
    /// The backend refused a restored surface.
    RestoreRefused,
    /// A restore panicked.
    RestorePanicked,
    /// The backend keeps the surface but cannot configure it at this size.
    OverLimit(DeviceSize),
    /// The device was rebuilt.
    DeviceRebuilt,
}

impl SlotSurface {
    /// A slot attached at `size`, or unsized.
    pub(crate) fn attached(size: Option<DeviceSize>) -> Self {
        Self {
            hold: SurfaceHold::Held,
            size: size.map_or(SlotSize::Unsized, SlotSize::Applied),
        }
    }

    /// The state after `event`.
    #[must_use]
    pub(crate) fn on(self, event: SurfaceEvent) -> Self {
        let Self { hold, size } = self;
        match event {
            SurfaceEvent::SizeSent(sent) => Self {
                hold,
                size: SlotSize::Pending(sent),
            },
            SurfaceEvent::Resized(applied) => Self {
                hold,
                size: if size == SlotSize::Pending(applied) {
                    SlotSize::Applied(applied)
                } else {
                    size
                },
            },
            SurfaceEvent::SizeRefused(refused) => Self {
                hold,
                size: if size == SlotSize::Pending(refused) {
                    SlotSize::Refused(refused)
                } else {
                    size
                },
            },
            SurfaceEvent::RestoreSent => Self {
                hold: SurfaceHold::Restoring,
                size,
            },
            SurfaceEvent::RestoreWithdrawn(before) if hold == SurfaceHold::Restoring => {
                Self { hold: before, size }
            }
            SurfaceEvent::ReleaseSent => Self {
                hold: SurfaceHold::Released,
                size,
            },
            SurfaceEvent::ReleaseWithdrawn(before) if hold == SurfaceHold::Released => {
                Self { hold: before, size }
            }
            // A withdrawal the record has already moved past changes nothing.
            SurfaceEvent::RestoreWithdrawn(_) | SurfaceEvent::ReleaseWithdrawn(_) => self,
            // A release on its way, or done, outlasts what the backend says
            // of the surface before it: the owner wants none until it
            // restores one.
            SurfaceEvent::SurfaceDropped
            | SurfaceEvent::Restored
            | SurfaceEvent::RestoreRefused
            | SurfaceEvent::RestorePanicked
                if hold == SurfaceHold::Released =>
            {
                self
            }
            SurfaceEvent::OverLimit(over) if hold == SurfaceHold::Released => Self {
                hold,
                size: if size == SlotSize::Applied(over) {
                    SlotSize::Refused(over)
                } else {
                    size
                },
            },
            // The restore on its way replaces the dropped surface.
            SurfaceEvent::SurfaceDropped if hold == SurfaceHold::Restoring => self,
            SurfaceEvent::SurfaceDropped | SurfaceEvent::RestorePanicked => Self {
                hold: SurfaceHold::Wanted,
                size,
            },
            SurfaceEvent::Restored => Self {
                hold: SurfaceHold::Held,
                size,
            },
            SurfaceEvent::RestoreRefused => Self {
                hold: SurfaceHold::Refused,
                size,
            },
            SurfaceEvent::OverLimit(over) => Self {
                hold: SurfaceHold::Held,
                size: if size == SlotSize::Applied(over) {
                    SlotSize::Refused(over)
                } else {
                    size
                },
            },
            SurfaceEvent::DeviceRebuilt => Self {
                hold,
                size: match size {
                    SlotSize::Refused(refused) => SlotSize::Pending(refused),
                    other => other,
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SlotSize, SlotSurface, SurfaceEvent, SurfaceHold};
    use flui_foundation::geometry::DeviceSize;

    const S: DeviceSize = DeviceSize::new(32, 16);
    const T: DeviceSize = DeviceSize::new(64, 32);

    fn state(hold: SurfaceHold, size: SlotSize) -> SlotSurface {
        SlotSurface { hold, size }
    }

    /// Every row of the module's table, from each state it can meet.
    #[test]
    fn every_transition_of_the_table() {
        use SlotSize::{Applied, Pending, Refused, Unsized};
        use SurfaceEvent as E;
        use SurfaceHold::{Held, Refused as Gone, Released, Restoring, Wanted};
        let rows: [(SlotSurface, SurfaceEvent, SlotSurface); 38] = [
            // A size sent replaces whatever the newest was.
            (
                state(Held, Unsized),
                E::SizeSent(S),
                state(Held, Pending(S)),
            ),
            (
                state(Held, Refused(S)),
                E::SizeSent(T),
                state(Held, Pending(T)),
            ),
            (
                state(Wanted, Applied(S)),
                E::SizeSent(T),
                state(Wanted, Pending(T)),
            ),
            // The backend's answer counts only for the size still pending.
            (
                state(Held, Pending(S)),
                E::Resized(S),
                state(Held, Applied(S)),
            ),
            (
                state(Held, Pending(T)),
                E::Resized(S),
                state(Held, Pending(T)),
            ),
            (
                state(Held, Pending(S)),
                E::SizeRefused(S),
                state(Held, Refused(S)),
            ),
            (
                state(Held, Pending(T)),
                E::SizeRefused(S),
                state(Held, Pending(T)),
            ),
            (
                state(Held, Applied(S)),
                E::SizeRefused(S),
                state(Held, Applied(S)),
            ),
            // The surface's own events leave the size alone.
            (
                state(Wanted, Applied(S)),
                E::RestoreSent,
                state(Restoring, Applied(S)),
            ),
            (
                state(Gone, Applied(S)),
                E::RestoreSent,
                state(Restoring, Applied(S)),
            ),
            (
                state(Restoring, Applied(S)),
                E::SurfaceDropped,
                state(Restoring, Applied(S)),
            ),
            (
                state(Restoring, Pending(T)),
                E::Restored,
                state(Held, Pending(T)),
            ),
            (
                state(Restoring, Applied(S)),
                E::RestoreWithdrawn(Wanted),
                state(Wanted, Applied(S)),
            ),
            (
                state(Held, Applied(S)),
                E::SurfaceDropped,
                state(Wanted, Applied(S)),
            ),
            (
                state(Gone, Refused(S)),
                E::SurfaceDropped,
                state(Wanted, Refused(S)),
            ),
            (
                state(Wanted, Applied(S)),
                E::Restored,
                state(Held, Applied(S)),
            ),
            (
                state(Gone, Pending(S)),
                E::Restored,
                state(Held, Pending(S)),
            ),
            (
                state(Wanted, Applied(S)),
                E::RestoreRefused,
                state(Gone, Applied(S)),
            ),
            (
                state(Gone, Applied(S)),
                E::RestorePanicked,
                state(Wanted, Applied(S)),
            ),
            // A surface kept unconfigured refuses the size it was configured
            // at, not a newer one still pending.
            (
                state(Wanted, Applied(S)),
                E::OverLimit(S),
                state(Held, Refused(S)),
            ),
            (
                state(Held, Applied(S)),
                E::OverLimit(S),
                state(Held, Refused(S)),
            ),
            (
                state(Wanted, Pending(T)),
                E::OverLimit(S),
                state(Held, Pending(T)),
            ),
            (
                state(Held, Applied(T)),
                E::OverLimit(S),
                state(Held, Applied(T)),
            ),
            // A rebuilt device answers a refused size anew.
            (
                state(Held, Refused(S)),
                E::DeviceRebuilt,
                state(Held, Pending(S)),
            ),
            (
                state(Wanted, Refused(S)),
                E::DeviceRebuilt,
                state(Wanted, Pending(S)),
            ),
            (
                state(Held, Applied(S)),
                E::DeviceRebuilt,
                state(Held, Applied(S)),
            ),
            (state(Held, Unsized), E::DeviceRebuilt, state(Held, Unsized)),
            // A release holds until the owner restores, whatever the backend
            // answers of a restore sent before it.
            (
                state(Held, Applied(S)),
                E::ReleaseSent,
                state(Released, Applied(S)),
            ),
            (
                state(Restoring, Pending(T)),
                E::ReleaseSent,
                state(Released, Pending(T)),
            ),
            (
                state(Released, Applied(S)),
                E::ReleaseWithdrawn(Held),
                state(Held, Applied(S)),
            ),
            (
                state(Restoring, Applied(S)),
                E::ReleaseWithdrawn(Held),
                state(Restoring, Applied(S)),
            ),
            (
                state(Released, Applied(S)),
                E::Restored,
                state(Released, Applied(S)),
            ),
            (
                state(Released, Applied(S)),
                E::RestoreRefused,
                state(Released, Applied(S)),
            ),
            (
                state(Released, Applied(S)),
                E::RestorePanicked,
                state(Released, Applied(S)),
            ),
            (
                state(Released, Applied(S)),
                E::SurfaceDropped,
                state(Released, Applied(S)),
            ),
            (
                state(Released, Applied(S)),
                E::OverLimit(S),
                state(Released, Refused(S)),
            ),
            (
                state(Released, Applied(S)),
                E::RestoreSent,
                state(Restoring, Applied(S)),
            ),
            (
                state(Released, Refused(S)),
                E::DeviceRebuilt,
                state(Released, Pending(S)),
            ),
        ];
        for (before, event, after) in rows {
            assert_eq!(before.on(event), after, "{before:?} on {event:?}");
        }
    }

    #[test]
    fn a_size_is_settled_only_once_the_backend_took_it() {
        assert!(SlotSize::Unsized.is_settled());
        assert!(SlotSize::Applied(S).is_settled());
        assert!(!SlotSize::Pending(S).is_settled());
        assert!(!SlotSize::Refused(S).is_settled());
        assert_eq!(SlotSize::Refused(S).newest(), Some(S));
        assert_eq!(SlotSize::Unsized.newest(), None);
    }
}
