//! A phased touchpad pan as an arena contact (ADR-0251).
//!
//! The binding turns the packets of a precise, phased scroll sequence into the
//! `Down`, `Move`, `Up` and `Cancel` of a virtual `Trackpad` contact and
//! delivers them to the pointer targets on the hit path that declared they
//! feed a drag-family recognizer. Everything after the first sample is the
//! pointer path's own: slop, the ownership table, velocity, terminal reasons.
//!
//! A scroll target that declared its axis competes for the contact through a
//! drag recognizer of its own, registered as one more pointer target of the
//! contact's path. While the contest is open the sequence's packets are
//! buffered; the verdict either drops them (a feed won) or replays them to the
//! scroll route (a scroll target won).

use std::{
    cell::{Cell, RefCell},
    num::NonZeroU64,
    rc::Rc,
};

use flui_foundation::geometry::Point;
use flui_platform::pointer::{
    PointerButton, PointerButtons, PointerId, PointerMove, PointerPosition, PointerPress,
    PointerRelease, PointerRole, PointerSample, ScrollEvent, ScrollPhase, ScrollPrecision,
    ScrollUnit,
};

use super::{
    GestureBinding,
    pointer_sequences::MAX_SIMULTANEOUS_POINTERS,
    scroll_sequences::{ScrollAdmission, ScrollContact, ScrollSequence},
};
use crate::{
    DragAxis, DragGestureRecognizer, GestureRecognizer,
    containment::RoutePanic,
    events::{CancelReason, PointerCancel, PointerEvent, PointerInfo, PointerKind},
    ids::InputSurface,
    pointer::{HitTestEntry, HitTestResult, PointerTarget, active_dispatch_handle},
};

/// The top bit of a virtual contact's pointer id. Platform pointer ids are
/// opaque but small in practice (a device's contact index, a handle); this bit
/// is reserved for the contacts the binding mints itself, so the two never meet
/// in the contact table.
const VIRTUAL_POINTER_BIT: u64 = 1 << 63;

/// Who owns a contact that feeds and scroll targets compete for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Pending,
    /// A scroll target won: the sequence belongs to the scroll route.
    Scroll,
    /// A feed won: the sequence belongs to its recognizer.
    Recognizer,
}

/// The contest between the feeds and the scroll targets of one sequence.
pub(super) struct Arbitration {
    verdict: Rc<Cell<Verdict>>,
    /// The packets since `Began`, kept while the verdict is pending.
    buffered: RefCell<Vec<ScrollEvent>>,
    /// The pointer targets standing in the contact's path for the scroll
    /// targets.
    proxies: Vec<PointerTarget>,
}

/// What a scroll packet did to the sequence's contact.
pub(super) enum ContactOutcome {
    /// The sequence has no contact: the packet belongs to the scroll route.
    None,
    /// The contact took the packet; the scroll route must not see it.
    Consumed,
    /// The contest ended in a scroll target's favour (or was never decided):
    /// the scroll route first receives these earlier packets, then the packet
    /// itself.
    Replay(Vec<ScrollEvent>),
}

impl GestureBinding {
    /// Open, move or end the contact of the scroll sequence `event` belongs to.
    pub(super) fn drive_scroll_contact(
        &self,
        surface: InputSurface,
        event: &ScrollEvent,
        admission: &ScrollAdmission,
        fresh: &HitTestResult,
        first: &mut Option<RoutePanic>,
    ) -> ContactOutcome {
        match admission {
            ScrollAdmission::Active(latch) => {
                // A handler that saw the raw packet may have begun another
                // sequence: this latch is gone, and a contact stored in it
                // would never be advanced or ended.
                if !self.scrolls.is_current(surface, event.pointer, latch) {
                    return ContactOutcome::None;
                }
                let current = latch.contact.borrow().clone();
                if let Some(contact) = current {
                    // The sequence is a contact already; whatever arrives is its.
                    return self.advance_scroll_contact(surface, latch, contact, event, first);
                }
                if !opens_contact(event) {
                    return ContactOutcome::None;
                }
                let (feeds, arbitration) = self.scroll_contact_path(fresh);
                if feeds.path().is_empty() {
                    return ContactOutcome::None;
                }
                let Some(opened) =
                    open_scroll_contact(surface, latch, event, arbitration.clone(), |pointer| {
                        self.contacts.contains(pointer)
                    })
                else {
                    if let Some(arbitration) = arbitration {
                        retire_proxies(&arbitration);
                    }
                    return ContactOutcome::None;
                };
                if let Some(arbitration) = &opened.contact.arbitration {
                    arbitration.buffered.borrow_mut().push(*event);
                }
                *latch.contact.borrow_mut() = Some(opened.contact);
                self.send_contact_event(surface, opened.event, Some(feeds), first);
                ContactOutcome::Consumed
            }
            ScrollAdmission::Terminal(Some(latch)) => {
                let Some(contact) = latch.contact.take() else {
                    return ContactOutcome::None;
                };
                if event.phase != Some(ScrollPhase::Ended) {
                    self.cancel_scroll_contact(surface, &contact, event.time, first);
                    return ContactOutcome::Consumed;
                }
                let moved = step_scroll_contact(&contact, event);
                let last = moved.map_or_else(|| contact.clone(), |moved| moved.contact);
                let Some(release) = release_event(&last, event) else {
                    self.cancel_scroll_contact(surface, &last, event.time, first);
                    return ContactOutcome::Consumed;
                };
                // The verdict is what the moves reached before the release: a
                // contest still open when the fingers lift (nobody crossed
                // slop) is the scroll targets', whichever member the sweep of
                // the release then picks.
                if last.arbitration.is_some() {
                    // A move still waiting for its frame may be the one that
                    // crosses slop.
                    let flushed = RoutePanic::capture(|| {
                        self.flush_pending_moves();
                    });
                    RoutePanic::preserve_first(first, flushed, "scroll-fed contact flush");
                }
                let before_release = last
                    .arbitration
                    .as_ref()
                    .map(|arbitration| arbitration.verdict.get());
                self.send_contact_event(surface, release, None, first);
                retire_contact(&last);
                match (&last.arbitration, before_release) {
                    (Some(arbitration), Some(verdict)) if verdict != Verdict::Recognizer => {
                        ContactOutcome::Replay(arbitration.buffered.take())
                    }
                    _ => {
                        self.scrolls.drop_momentum_of(surface, event.pointer);
                        ContactOutcome::Consumed
                    }
                }
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
            self.cancel_scroll_contact(contact.surface, &contact, time, first);
        }
    }

    /// Move the live `contact` by `event` and apply what the pointer path
    /// decided about it.
    fn advance_scroll_contact(
        &self,
        surface: InputSurface,
        latch: &ScrollSequence,
        contact: ScrollContact,
        event: &ScrollEvent,
        first: &mut Option<RoutePanic>,
    ) -> ContactOutcome {
        let Some(moved) = step_scroll_contact(&contact, event) else {
            latch.contact.take();
            self.cancel_scroll_contact(surface, &contact, event.time, first);
            return ContactOutcome::Consumed;
        };
        *latch.contact.borrow_mut() = Some(moved.contact.clone());
        self.send_contact_event(surface, moved.event, None, first);
        let Some(arbitration) = &moved.contact.arbitration else {
            return ContactOutcome::Consumed;
        };
        match arbitration.verdict.get() {
            Verdict::Pending => {
                arbitration.buffered.borrow_mut().push(*event);
                ContactOutcome::Consumed
            }
            Verdict::Recognizer => ContactOutcome::Consumed,
            Verdict::Scroll => {
                // The feeds were rejected when the proxy won; the contact has
                // nothing left to carry.
                latch.contact.take();
                self.cancel_scroll_contact(surface, &moved.contact, event.time, first);
                ContactOutcome::Replay(arbitration.buffered.take())
            }
        }
    }

    /// End `contact` with a cancel and release what it holds.
    fn cancel_scroll_contact(
        &self,
        surface: InputSurface,
        contact: &ScrollContact,
        time: flui_platform::EventTime,
        first: &mut Option<RoutePanic>,
    ) {
        let cancel = PointerEvent::Cancel(PointerCancel::new(
            contact.pointer,
            time,
            CancelReason::Platform,
        ));
        self.send_contact_event(surface, cancel, None, first);
        retire_contact(contact);
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

    /// The entries of `fresh` that take part in a scroll-fed contact, and the
    /// contest between them.
    ///
    /// Only pointer targets that declared they feed a recognizer, and scroll
    /// targets that declared their axis, are kept; each scroll target stands in
    /// the path as a drag recognizer's pointer target. A path with no feed has
    /// no contact (ADR-0251 §3).
    fn scroll_contact_path(
        &self,
        fresh: &HitTestResult,
    ) -> (HitTestResult, Option<Rc<Arbitration>>) {
        let mut path = HitTestResult::new();
        let Ok(handle) = active_dispatch_handle() else {
            return (path, None);
        };
        let feeds = |entry: &HitTestEntry| {
            entry
                .pointer_target
                .filter(|target| handle.feeds_scroll_contacts(*target))
        };
        if !fresh.path().iter().any(|entry| feeds(entry).is_some()) {
            return (path, None);
        }
        let verdict = Rc::new(Cell::new(Verdict::Pending));
        let mut proxies = Vec::new();
        for entry in fresh.path() {
            if let Some(target) = feeds(entry) {
                let mut kept = HitTestEntry::new(entry.target).pointer_target(target);
                kept.transform = entry.transform;
                path.path_mut().push(kept);
            }
            let Some(scroll) = entry.scroll_target else {
                continue;
            };
            let Some(axis) = handle.scroll_target_axis(scroll) else {
                continue;
            };
            let Ok(proxy) = self.register_scroll_proxy(axis, &verdict) else {
                continue;
            };
            proxies.push(proxy);
            let mut stand_in = HitTestEntry::new(entry.target).pointer_target(proxy);
            stand_in.transform = entry.transform;
            path.path_mut().push(stand_in);
        }
        let arbitration = (!proxies.is_empty()).then(|| {
            Rc::new(Arbitration {
                verdict,
                buffered: RefCell::new(Vec::new()),
                proxies,
            })
        });
        (path, arbitration)
    }

    /// A pointer target feeding the drag recognizer that stands for a scroll
    /// target on `axis`: its win is the scroll target's.
    fn register_scroll_proxy(
        &self,
        axis: DragAxis,
        verdict: &Rc<Cell<Verdict>>,
    ) -> Result<PointerTarget, crate::pointer::InteractionDispatchError> {
        let handle = active_dispatch_handle()?;
        let (won, lost) = (Rc::clone(verdict), Rc::clone(verdict));
        let recognizer = DragGestureRecognizer::builder(self.arena.arena().clone(), axis)
            .settings(self.default_profiles.provider())
            .on_start(move |_| {
                if won.get() == Verdict::Pending {
                    won.set(Verdict::Scroll);
                }
            })
            .on_cancel(move || {
                if lost.get() == Verdict::Pending {
                    lost.set(Verdict::Recognizer);
                }
            })
            .build();
        handle.register_pointer(move |dispatch| {
            if matches!(dispatch.local, PointerEvent::Down(_)) {
                recognizer.add_pointer(dispatch);
            } else {
                recognizer.handle_event(dispatch);
            }
        })
    }
}

/// Release the proxy targets a contact registered.
fn retire_contact(contact: &ScrollContact) {
    if let Some(arbitration) = &contact.arbitration {
        retire_proxies(arbitration);
    }
}

fn retire_proxies(arbitration: &Arbitration) {
    let Ok(handle) = active_dispatch_handle() else {
        return;
    };
    for proxy in &arbitration.proxies {
        // A proxy already gone with its lane has nothing left to release.
        let _ = handle.unregister_pointer(*proxy);
    }
}

struct OpenedContact {
    contact: ScrollContact,
    event: PointerEvent,
}

/// Whether `event` starts a pan a drag recognizer may take: the fingers
/// touched down on a precise surface.
fn opens_contact(event: &ScrollEvent) -> bool {
    event.phase == Some(ScrollPhase::Began) && event.precision == ScrollPrecision::Precise
}

fn open_scroll_contact(
    surface: InputSurface,
    latch: &ScrollSequence,
    event: &ScrollEvent,
    arbitration: Option<Rc<Arbitration>>,
    in_use: impl Fn(PointerId) -> bool,
) -> Option<OpenedContact> {
    // The latch's own id, stepped past any live contact that already holds it.
    let id = (0..=MAX_SIMULTANEOUS_POINTERS as u64).find_map(|step| {
        let candidate = NonZeroU64::new(latch.id.get().wrapping_add(step) | VIRTUAL_POINTER_BIT)?;
        let candidate = PointerId::new(candidate);
        (!in_use(candidate)).then_some(candidate)
    })?;
    let pointer = PointerInfo::new(id, PointerKind::Trackpad).with_role(PointerRole::Primary);
    let at = event.position.get();
    let contact = ScrollContact {
        surface,
        pointer,
        position: at,
        arbitration,
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

fn release_event(contact: &ScrollContact, event: &ScrollEvent) -> Option<PointerEvent> {
    let position = PointerPosition::try_new(contact.position).ok()?;
    Some(PointerEvent::Up(PointerRelease::new(
        contact.pointer,
        PointerButton::PRIMARY,
        PointerButtons::only(PointerButton::PRIMARY),
        PointerSample::new(event.time, position),
    )))
}

/// The contact after `event`'s displacement, and the `Move` that says so.
fn step_scroll_contact(contact: &ScrollContact, event: &ScrollEvent) -> Option<OpenedContact> {
    // The fingers move opposite to the scroll offset's growth.
    let (dx, dy) = if event.delta.unit() == ScrollUnit::Pixels {
        (-event.delta.x(), -event.delta.y())
    } else {
        (0.0, 0.0)
    };
    let at = Point::new(contact.position.x + dx, contact.position.y + dy);
    let position = PointerPosition::try_new(at).ok()?;
    let moved = ScrollContact {
        position: at,
        ..contact.clone()
    };
    Some(OpenedContact {
        event: PointerEvent::Move(PointerMove::new(
            moved.pointer,
            PointerButtons::only(PointerButton::PRIMARY),
            PointerSample::new(event.time, position),
        )),
        contact: moved,
    })
}
