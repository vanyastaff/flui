//! A phased touchpad pan as an arena contact (ADR-0251).
//!
//! The binding turns the packets of a precise, phased scroll sequence into the
//! `Down`, `Move`, `Up` and `Cancel` of a virtual `Trackpad` contact and
//! delivers them to the pointer targets on the hit path that declared they
//! feed a drag-family recognizer. Everything after the first sample is the
//! pointer path's own: slop, the ownership table, velocity, terminal reasons.

use std::num::NonZeroU64;

use flui_foundation::geometry::Point;
use flui_platform::pointer::{
    PointerButton, PointerButtons, PointerId, PointerMove, PointerPosition, PointerPress,
    PointerRelease, PointerRole, PointerSample, ScrollPhase, ScrollPrecision, ScrollUnit,
};

use super::{
    GestureBinding,
    scroll_sequences::{ScrollAdmission, ScrollContact, ScrollSequence},
};
use crate::{
    containment::RoutePanic,
    events::{CancelReason, PointerCancel, PointerEvent, PointerInfo, PointerKind},
    ids::InputSurface,
    pointer::{HitTestEntry, HitTestResult, active_dispatch_handle},
};

/// The top bit of a virtual contact's pointer id. Platform pointer ids are
/// opaque but small in practice (a device's contact index, a handle); this bit
/// is reserved for the contacts the binding mints itself, so the two never meet
/// in the contact table.
const VIRTUAL_POINTER_BIT: u64 = 1 << 63;

/// What a scroll packet did to the sequence's contact.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ContactOutcome {
    /// The sequence has no contact: the packet belongs to the scroll route.
    None,
    /// The contact took the packet; the scroll route must not see it.
    Consumed,
}

impl GestureBinding {
    /// Open, move or end the contact of the scroll sequence `event` belongs to.
    pub(super) fn drive_scroll_contact(
        &self,
        surface: InputSurface,
        event: &flui_platform::pointer::ScrollEvent,
        admission: &ScrollAdmission,
        fresh: &HitTestResult,
        first: &mut Option<RoutePanic>,
    ) -> ContactOutcome {
        match admission {
            ScrollAdmission::Active(latch) => {
                if let Some(contact) = latch.contact.get() {
                    // The sequence is a contact already; whatever arrives is its.
                    if let Some(moved) = step_scroll_contact(contact, event) {
                        latch.contact.set(Some(moved.contact));
                        self.send_contact_event(surface, moved.event, None, first);
                    } else {
                        latch.contact.set(None);
                        self.cancel_scroll_contact(surface, contact, event, first);
                    }
                    return ContactOutcome::Consumed;
                }
                if !opens_contact(event) {
                    return ContactOutcome::None;
                }
                let feeds = scroll_contact_path(fresh);
                if feeds.path().is_empty() {
                    return ContactOutcome::None;
                }
                let Some(opened) = open_scroll_contact(surface, latch, event) else {
                    return ContactOutcome::None;
                };
                latch.contact.set(Some(opened.contact));
                self.send_contact_event(surface, opened.event, Some(feeds), first);
                ContactOutcome::Consumed
            }
            ScrollAdmission::Terminal(Some(latch)) => {
                let Some(contact) = latch.contact.take() else {
                    return ContactOutcome::None;
                };
                match event.phase {
                    Some(ScrollPhase::Ended) => {
                        let moved = step_scroll_contact(contact, event);
                        let last = moved.as_ref().map_or(contact, |moved| moved.contact);
                        if let Some(release) = release_event(last, event) {
                            self.send_contact_event(surface, release, None, first);
                            self.scrolls.drop_momentum_of(surface, event.pointer);
                        } else {
                            self.cancel_scroll_contact(surface, last, event, first);
                        }
                    }
                    _ => self.cancel_scroll_contact(surface, contact, event, first),
                }
                ContactOutcome::Consumed
            }
            ScrollAdmission::Terminal(None) | ScrollAdmission::Refused => ContactOutcome::None,
        }
    }

    /// Cancel `contacts`, whose sequences ended without a terminal packet: a
    /// lapsed or replaced latch, or a source that left.
    pub(super) fn cancel_orphaned_scroll_contacts(
        &self,
        contacts: Vec<ScrollContact>,
        time: flui_platform::EventTime,
        first: &mut Option<RoutePanic>,
    ) {
        for contact in contacts {
            let surface = contact.surface;
            let cancel = PointerEvent::Cancel(PointerCancel::new(
                contact.pointer,
                time,
                CancelReason::Platform,
            ));
            self.send_contact_event(surface, cancel, None, first);
        }
    }

    /// The contact after `event`'s displacement, and the `Move` that says so.
    fn cancel_scroll_contact(
        &self,
        surface: InputSurface,
        contact: ScrollContact,
        event: &flui_platform::pointer::ScrollEvent,
        first: &mut Option<RoutePanic>,
    ) {
        let cancel = PointerEvent::Cancel(PointerCancel::new(
            contact.pointer,
            event.time,
            CancelReason::Platform,
        ));
        self.send_contact_event(surface, cancel, None, first);
    }

    /// Deliver one event of a scroll-fed contact through the pointer path.
    ///
    /// `route` is the hit path the `Down` is delivered along; the contact's
    /// later events ride the route that `Down` cached.
    fn send_contact_event(
        &self,
        surface: InputSurface,
        event: PointerEvent,
        route: Option<HitTestResult>,
        first: &mut Option<RoutePanic>,
    ) {
        let delivered = RoutePanic::capture(|| {
            let mut incoming = None;
            self.handle_pointer_event_after_signal_withdrawal(
                surface,
                &event,
                |_| route.unwrap_or_default(),
                &mut incoming,
            );
            if let Some(panic) = incoming {
                panic.resume();
            }
        });
        RoutePanic::preserve_first(first, delivered, "scroll-fed contact dispatch");
    }
}

struct OpenedContact {
    contact: ScrollContact,
    event: PointerEvent,
}

/// Whether `event` starts a pan a drag recognizer may take: the fingers
/// touched down on a precise surface.
fn opens_contact(event: &flui_platform::pointer::ScrollEvent) -> bool {
    event.phase == Some(ScrollPhase::Began) && event.precision == ScrollPrecision::Precise
}

fn open_scroll_contact(
    surface: InputSurface,
    latch: &ScrollSequence,
    event: &flui_platform::pointer::ScrollEvent,
) -> Option<OpenedContact> {
    let id = PointerId::new(NonZeroU64::new(latch.id.get() | VIRTUAL_POINTER_BIT)?);
    let pointer = PointerInfo::new(id, PointerKind::Trackpad).with_role(PointerRole::Primary);
    let at = event.position.get();
    let contact = ScrollContact {
        surface,
        pointer,
        position: at,
    };
    let sample = PointerSample::new(event.time, event.position);
    Some(OpenedContact {
        contact,
        event: PointerEvent::Down(PointerPress::new(
            pointer,
            PointerButton::PRIMARY,
            PointerButtons::NONE,
            sample,
        )),
    })
}

fn release_event(
    contact: ScrollContact,
    event: &flui_platform::pointer::ScrollEvent,
) -> Option<PointerEvent> {
    let position = PointerPosition::try_new(contact.position).ok()?;
    Some(PointerEvent::Up(PointerRelease::new(
        contact.pointer,
        PointerButton::PRIMARY,
        PointerButtons::only(PointerButton::PRIMARY),
        PointerSample::new(event.time, position),
    )))
}

/// The entries of `fresh` whose pointer targets declared they feed a
/// recognizer, stripped of every other target: only feeds see the contact.
fn scroll_contact_path(fresh: &HitTestResult) -> HitTestResult {
    let mut feeds = HitTestResult::new();
    let Ok(handle) = active_dispatch_handle() else {
        return feeds;
    };
    for entry in fresh.path() {
        let Some(target) = entry.pointer_target else {
            continue;
        };
        if !handle.feeds_scroll_contacts(target) {
            continue;
        }
        let mut kept = HitTestEntry::new(entry.target).pointer_target(target);
        kept.transform = entry.transform;
        feeds.path_mut().push(kept);
    }
    feeds
}

/// The contact after `event`'s displacement, and the `Move` that says so.
fn step_scroll_contact(
    contact: ScrollContact,
    event: &flui_platform::pointer::ScrollEvent,
) -> Option<OpenedContact> {
    // The fingers move opposite to the scroll offset's growth.
    let (dx, dy) = if event.delta.unit() == ScrollUnit::Pixels {
        (-event.delta.x(), -event.delta.y())
    } else {
        (0.0, 0.0)
    };
    let at = Point::new(contact.position.x + dx, contact.position.y + dy);
    let position = PointerPosition::try_new(at).ok()?;
    let contact = ScrollContact {
        position: at,
        ..contact
    };
    Some(OpenedContact {
        contact,
        event: PointerEvent::Move(PointerMove::new(
            contact.pointer,
            PointerButtons::only(PointerButton::PRIMARY),
            PointerSample::new(event.time, position),
        )),
    })
}
