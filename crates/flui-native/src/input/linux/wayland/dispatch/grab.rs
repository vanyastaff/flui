//! A window's hold of the pointer (`CursorGrab`): the compositor's pointer
//! constraints (`zwp_pointer_constraints_v1`, oneshot) on the pointer of the
//! seat whose keyboard is in the window, and a locked pointer's relative
//! motion (`zwp_relative_pointer_v1`) as the window's raw motion. A hold
//! lasts until the window lets go, the keyboard leaves it, its pointer goes,
//! or the compositor ends the constraint; the window's snapshot shows a grab
//! once the compositor carries it out.

use std::sync::{Arc, Weak};

use flui_platform::{CursorGrab, PlatformFailure, WindowId};
use wayland_client::backend::ObjectId;
use wayland_client::protocol::wl_pointer::WlPointer;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::wp::pointer_constraints::zv1::client::zwp_confined_pointer_v1::{
    self, ZwpConfinedPointerV1,
};
use wayland_protocols::wp::pointer_constraints::zv1::client::zwp_locked_pointer_v1::{
    self, ZwpLockedPointerV1,
};
use wayland_protocols::wp::pointer_constraints::zv1::client::zwp_pointer_constraints_v1::{
    self, ZwpPointerConstraintsV1,
};
use wayland_protocols::wp::relative_pointer::zv1::client::zwp_relative_pointer_manager_v1::{
    self, ZwpRelativePointerManagerV1,
};
use wayland_protocols::wp::relative_pointer::zv1::client::zwp_relative_pointer_v1::{
    self, ZwpRelativePointerV1,
};

use super::cursors::show_cursor;
use super::{PointerWire, SeatEvent, SeatKey, Shared, Wire, arrival, surface_key};
use crate::input::assembly::NativeDevice;
use crate::owner::linux::wayland::WaylandState;

/// Constrain the pointer of the seat whose keyboard is in the window `id`
/// as `grab` asks, after ending what the window held
/// ([`WaylandInput::hold_pointer`](crate::owner::linux::wayland::WaylandInput::hold_pointer)).
pub(super) fn hold(
    shared: &Arc<Shared>,
    id: WindowId,
    grab: CursorGrab,
) -> Result<(), PlatformFailure> {
    let surface = shared
        .windows
        .with(|windows| {
            let window = windows.iter_mut().find(|held| held.id == id)?;
            Some((window.surface.clone(), window.hold.take()))
        })
        .flatten();
    let Some((surface, held)) = surface else {
        return Err(PlatformFailure::Failed {
            message: "the window is not open on this Wayland loop".to_owned(),
        });
    };
    if let Some(held) = held {
        end_hold(shared, id, held);
    }
    let locked = match grab {
        CursorGrab::None => return Ok(()),
        CursorGrab::Confined => false,
        CursorGrab::Locked => true,
    };
    // The seat whose keyboard is in the window: a window in none holds
    // nothing, as one without the focus.
    let found = shared
        .objects
        .with(|objects| {
            let seat = objects
                .seats
                .iter()
                .find(|seat| seat.keyboard_focus.as_ref() == Some(&surface))?;
            let pointer = seat.pointer.as_ref()?;
            Some((
                seat.key,
                pointer.pointer.clone(),
                pointer.device,
                objects.constraints.clone(),
                objects
                    .relative
                    .as_ref()
                    .map(|(_, manager)| manager.clone()),
            ))
        })
        .flatten();
    let Some((seat, pointer, device, constraints, relative)) = found else {
        return Ok(());
    };
    let refused = |what: &str| PlatformFailure::Failed {
        message: format!("the compositor offers no {what}"),
    };
    let (_, constraints, queue) = constraints.ok_or_else(|| refused("pointer constraints"))?;
    let data = || ConstraintData {
        shared: Arc::downgrade(shared),
        window: id,
    };
    // Oneshot: the constraint holds while the window has the keyboard,
    // and a new grab is asked for again.
    let lifetime = zwp_pointer_constraints_v1::Lifetime::Oneshot;
    let constraint = if locked {
        let relative = relative.ok_or_else(|| refused("relative pointer motion"))?;
        Constraint::Locked {
            lock: constraints.lock_pointer(&surface, &pointer, None, lifetime, &queue, data()),
            relative: relative.get_relative_pointer(
                &pointer,
                &queue,
                RelativeData {
                    shared: Arc::downgrade(shared),
                    window: id,
                    device,
                },
            ),
        }
    } else {
        Constraint::Confined(constraints.confine_pointer(
            &surface,
            &pointer,
            None,
            lifetime,
            &queue,
            data(),
        ))
    };
    let hold = Hold {
        seat,
        pointer,
        constraint,
        active: false,
    };
    let unheld = shared
        .windows
        .with(
            |windows| match windows.iter_mut().find(|held| held.id == id) {
                Some(window) => {
                    window.hold = Some(hold);
                    None
                }
                None => Some(hold),
            },
        )
        .flatten();
    if let Some(hold) = unheld {
        drop(hold.destroy());
    }
    Ok(())
}

/// A constraint a window asked the compositor for on one seat's pointer
/// (`zwp_pointer_constraints_v1`, for its keyboard's focus alone: oneshot).
pub(super) struct Hold {
    /// The seat whose keyboard was in the window.
    seat: SeatKey,
    /// That seat's pointer, which it constrains.
    pointer: WlPointer,
    constraint: Constraint,
    /// The compositor said it carries it out (`confined`, `locked`).
    active: bool,
}

/// The objects of a [`Hold`].
enum Constraint {
    /// The pointer stays inside the window's surface.
    Confined(ZwpConfinedPointerV1),
    /// The pointer stays put, its motion reported as relative motion.
    Locked {
        lock: ZwpLockedPointerV1,
        relative: ZwpRelativePointerV1,
    },
}

impl Hold {
    /// What the window holds of the pointer: the grab the compositor carries
    /// out, none before it said so.
    pub(super) fn grab(&self) -> CursorGrab {
        match (&self.constraint, self.active) {
            (_, false) => CursorGrab::None,
            (Constraint::Confined(_), true) => CursorGrab::Confined,
            (Constraint::Locked { .. }, true) => CursorGrab::Locked,
        }
    }

    /// Whether `object` is the constraint of this hold, not one it replaced.
    fn constrains_by(&self, object: &ObjectId) -> bool {
        match &self.constraint {
            Constraint::Confined(confined) => confined.id() == *object,
            Constraint::Locked { lock, .. } => lock.id() == *object,
        }
    }

    /// The pointer the compositor holds in place for it, once it said so.
    fn locked_pointer(&self) -> Option<&WlPointer> {
        (self.active && matches!(self.constraint, Constraint::Locked { .. }))
            .then_some(&self.pointer)
    }

    /// Destroy its objects; the pointer it locked, which shows the window's
    /// cursor again, `None` for a confinement.
    fn destroy(self) -> Option<WlPointer> {
        match self.constraint {
            Constraint::Confined(confined) => {
                confined.destroy();
                None
            }
            Constraint::Locked { lock, relative } => {
                lock.destroy();
                relative.destroy();
                Some(self.pointer)
            }
        }
    }
}

/// End `hold`, the window `id`'s: its objects destroyed, a pointer it locked
/// no longer held and showing the window's cursor again, and the window's
/// snapshot read again at the next delivery. Called with no cell of the
/// input borrowed.
pub(super) fn end_hold(shared: &Shared, id: WindowId, hold: Hold) {
    if let Some(pointer) = hold.destroy() {
        let shown = shared
            .windows
            .with(|windows| {
                windows
                    .iter()
                    .find(|window| window.id == id)
                    .map(|window| (window.cursor.clone(), (window.scale)()))
            })
            .flatten();
        shared.objects.with(|objects| {
            if let Some(held) = objects.pointer_mut(&pointer) {
                held.locked = false;
            }
            if let Some((cursor, scale)) = shown {
                show_cursor(shared, objects, surface_key(id), &cursor, scale);
            }
        });
    }
    shared.record(Wire::Grab(id));
}

/// `seat`'s keyboard left `surface`: the window there lets go of the
/// pointer it held on that seat. Called with no cell of the input borrowed.
pub(super) fn let_go_on_leave(shared: &Shared, seat: SeatKey, surface: &WlSurface) {
    let ended: Vec<(WindowId, Hold)> = shared
        .windows
        .with(|windows| {
            windows
                .iter_mut()
                .filter(|window| {
                    window.surface == *surface
                        && window.hold.as_ref().is_some_and(|hold| hold.seat == seat)
                })
                .filter_map(|window| Some((window.id, window.hold.take()?)))
                .collect()
        })
        .unwrap_or_default();
    for (id, hold) in ended {
        end_hold(shared, id, hold);
    }
}

/// `pointer` is gone: every window lets go of its hold of it. Called with no
/// cell of the input borrowed.
pub(super) fn let_go_of_pointer(shared: &Shared, pointer: &WlPointer) {
    let ended: Vec<(WindowId, Hold)> = shared
        .windows
        .with(|windows| {
            windows
                .iter_mut()
                .filter(|window| {
                    window
                        .hold
                        .as_ref()
                        .is_some_and(|hold| hold.pointer == *pointer)
                })
                .filter_map(|window| Some((window.id, window.hold.take()?)))
                .collect()
        })
        .unwrap_or_default();
    for (id, hold) in ended {
        end_hold(shared, id, hold);
    }
}

/// The compositor carries out (`active`) or ended the constraint `object`
/// of the window `id`. One it carries out shows in the window's snapshot,
/// and a lock hides the pointer's cursor; one it ended is let go of, as a
/// oneshot constraint does not come back. A constraint the window no longer
/// holds says nothing.
fn constraint_said(shared: &Shared, id: WindowId, object: &ObjectId, active: bool) {
    enum Said {
        Carried(Option<WlPointer>),
        Ended(Hold),
    }
    let said = shared
        .windows
        .with(|windows| {
            let window = windows.iter_mut().find(|window| window.id == id)?;
            let hold = window
                .hold
                .as_mut()
                .filter(|hold| hold.constrains_by(object))?;
            if active {
                hold.active = true;
                Some(Said::Carried(hold.locked_pointer().cloned()))
            } else {
                window.hold.take().map(Said::Ended)
            }
        })
        .flatten();
    match said {
        Some(Said::Carried(locked)) => {
            if let Some(locked) = locked {
                shared.objects.with(|objects| {
                    if let Some(held) = objects.pointer_mut(&locked) {
                        held.locked = true;
                        if let Some((_, serial)) = held.focus {
                            held.pointer.set_cursor(serial, None, 0, 0);
                        }
                    }
                });
            }
            shared.record(Wire::Grab(id));
        }
        Some(Said::Ended(hold)) => end_hold(shared, id, hold),
        None => {}
    }
}

/// A grab's user data: the window it is for.
pub(crate) struct ConstraintData {
    shared: Weak<Shared>,
    window: WindowId,
}

/// A locked pointer's relative pointer's user data: the window that holds
/// it, and the pointer's device label.
pub(crate) struct RelativeData {
    shared: Weak<Shared>,
    window: WindowId,
    device: NativeDevice,
}

impl Dispatch<ZwpPointerConstraintsV1, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &ZwpPointerConstraintsV1,
        _: zwp_pointer_constraints_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The constraints manager has no events.
    }
}

impl Dispatch<ZwpRelativePointerManagerV1, ()> for WaylandState {
    fn event(
        _: &mut Self,
        _: &ZwpRelativePointerManagerV1,
        _: zwp_relative_pointer_manager_v1::Event,
        (): &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // The relative pointer manager has no events.
    }
}

impl Dispatch<ZwpConfinedPointerV1, ConstraintData> for WaylandState {
    fn event(
        _: &mut Self,
        confined: &ZwpConfinedPointerV1,
        event: zwp_confined_pointer_v1::Event,
        data: &ConstraintData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(shared) = data.shared.upgrade() else {
            return;
        };
        let active = match event {
            zwp_confined_pointer_v1::Event::Confined => true,
            zwp_confined_pointer_v1::Event::Unconfined => false,
            _ => return,
        };
        constraint_said(&shared, data.window, &confined.id(), active);
    }
}

impl Dispatch<ZwpLockedPointerV1, ConstraintData> for WaylandState {
    fn event(
        _: &mut Self,
        locked: &ZwpLockedPointerV1,
        event: zwp_locked_pointer_v1::Event,
        data: &ConstraintData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(shared) = data.shared.upgrade() else {
            return;
        };
        let active = match event {
            zwp_locked_pointer_v1::Event::Locked => true,
            zwp_locked_pointer_v1::Event::Unlocked => false,
            _ => return,
        };
        constraint_said(&shared, data.window, &locked.id(), active);
    }
}

impl Dispatch<ZwpRelativePointerV1, RelativeData> for WaylandState {
    fn event(
        _: &mut Self,
        _: &ZwpRelativePointerV1,
        event: zwp_relative_pointer_v1::Event,
        data: &RelativeData,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let zwp_relative_pointer_v1::Event::RelativeMotion {
            utime_hi,
            utime_lo,
            dx_unaccel,
            dy_unaccel,
            ..
        } = event
        else {
            return;
        };
        let Some(shared) = data.shared.upgrade() else {
            return;
        };
        // Relative motion comes whether or not the pointer is locked: it is
        // the window's raw motion only while the compositor holds it.
        if !shared.holds_locked(data.window) {
            return;
        }
        let micros = (u64::from(utime_hi) << 32) | u64::from(utime_lo);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the stamp is the low 32 bits of the millisecond clock, as `wl_pointer`'s"
        )]
        let stamp = (micros / 1_000) as u32;
        shared.record_seat(
            arrival(Some(stamp)),
            SeatEvent::Pointer {
                device: data.device,
                event: PointerWire::RawMotion {
                    surface: surface_key(data.window),
                    dx: dx_unaccel,
                    dy: dy_unaccel,
                },
            },
        );
    }
}
