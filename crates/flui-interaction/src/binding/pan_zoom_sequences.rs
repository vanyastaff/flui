//! Native pan/zoom source leases: an independent bounded lease per source and
//! surface, selected on first consumption.
//!
//! Repeated Start for the same contact replaces its admission authority but
//! preserves its consumer through old-session cancellation; only a terminal
//! edge permits the next fresh source to choose a different target.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

use flui_foundation::{IdMint, InputSequenceId};
use flui_platform_api::pointer::{PanZoomEvent, PanZoomPhase};

use super::{SignalSource, pointer_sequences::MAX_SIMULTANEOUS_POINTERS};
use crate::{
    containment::RoutePanic,
    events::PointerInfo,
    ids::{InputSurface, PointerId},
    pointer::{HitTestResult, PanZoomRetirement, PanZoomRoute},
};

/// A staged consumer and the retirement its admission owes.
pub(super) type StagedTicket = (PanZoomRoute, PanZoomRetirement);

pub(super) struct PanZoomSequence {
    /// The trackpad sequence this session serves (ADR-0207 §4).
    pub(super) id: InputSequenceId,
    pub(super) route: Cell<Option<PanZoomRoute>>,
    pointer: PointerId,
    staged: RefCell<Vec<StagedTicket>>,
    /// The hit path at the session's first packet (its Start focal point):
    /// every later packet is observed and claimed along it, like a captured
    /// pointer, wherever the focal point moves.
    path: RefCell<Option<HitTestResult>>,
}

impl PanZoomSequence {
    /// The session's hit path, hit-testing it with `hit_test` on first use.
    pub(super) fn path_or_hit_test(
        &self,
        hit_test: impl FnOnce() -> HitTestResult,
    ) -> HitTestResult {
        if let Some(path) = self.path.borrow().as_ref() {
            return path.clone();
        }
        let path = hit_test();
        *self.path.borrow_mut() = Some(path.clone());
        path
    }

    /// The session's hit path, if its first packet resolved one.
    pub(super) fn path(&self) -> Option<HitTestResult> {
        self.path.borrow().clone()
    }

    /// Take the retirements still staged on this session.
    pub(super) fn take_staged(&self) -> Vec<StagedTicket> {
        self.staged.take()
    }

    /// Whether `route` holds this session's newest actor generation.
    pub(super) fn terminal_is_current(&self, route: PanZoomRoute) -> bool {
        // Repeated Start appends the newest actor generation to its retained route.
        let current = self
            .staged
            .borrow()
            .iter()
            .rev()
            .find_map(|(target, ticket)| {
                target
                    .same_target(route)
                    .then(|| ticket.1.clone())
                    .flatten()
            });
        current.is_some_and(|current| current())
    }
}

pub(super) enum PanZoomAdmission {
    Active(Rc<PanZoomSequence>),
    Terminal(Option<Rc<PanZoomSequence>>),
    Refused,
}

#[derive(Default)]
pub(super) struct PanZoomSequences {
    sessions: RefCell<HashMap<(InputSurface, SignalSource), Rc<PanZoomSequence>>>,
}

impl PanZoomSequences {
    /// Admit a packet, returning the admission and the retirements a repeated
    /// Start owes its replaced session. The caller runs those after this
    /// borrow is released.
    pub(super) fn prepare(
        &self,
        surface: InputSurface,
        event: &PanZoomEvent,
        mint: &IdMint<InputSequenceId>,
    ) -> (PanZoomAdmission, Vec<StagedTicket>) {
        let pointer = *event.pointer();
        let key = (surface, SignalSource::from(pointer));
        let mut sequences = self.sessions.borrow_mut();
        if let Some(sequence) = sequences.get(&key)
            && sequence.pointer != pointer.id
            && !matches!(event.phase, PanZoomPhase::Start)
        {
            return (PanZoomAdmission::Refused, Vec::new());
        }
        if matches!(event.phase, PanZoomPhase::End | PanZoomPhase::Cancelled) {
            return (
                PanZoomAdmission::Terminal(sequences.remove(&key)),
                Vec::new(),
            );
        }
        let prior = if matches!(event.phase, PanZoomPhase::Start) {
            // Repeated exact Start replaces the cumulative session, while its
            // admitted consumer receives retirement and the new staged pair.
            sequences.remove(&key)
        } else if let Some(sequence) = sequences.get(&key) {
            return (PanZoomAdmission::Active(Rc::clone(sequence)), Vec::new());
        } else {
            None
        };
        if sequences.len() >= MAX_SIMULTANEOUS_POINTERS {
            return (PanZoomAdmission::Refused, Vec::new());
        }
        let Ok(id) = mint.next() else {
            // An exhausted binding refuses new sessions permanently; a
            // replaced Start's prior session still owes its retirements.
            let retired = prior.map(|prior| prior.take_staged()).unwrap_or_default();
            return (PanZoomAdmission::Refused, retired);
        };
        let sequence = Rc::new(PanZoomSequence {
            id,
            route: Cell::new(
                prior
                    .as_ref()
                    .filter(|prior| prior.pointer == pointer.id)
                    .and_then(|prior| prior.route.get()),
            ),
            pointer: pointer.id,
            staged: RefCell::new(Vec::new()),
            path: RefCell::new(None),
        });
        sequences.insert(key, Rc::clone(&sequence));
        drop(sequences);
        let mut retired = Vec::new();
        if let Some(prior) = prior {
            let tickets = prior.take_staged();
            let kept;
            (kept, retired) = tickets.into_iter().partition(|(route, _)| {
                sequence
                    .route
                    .get()
                    .is_some_and(|winner| winner.same_target(*route))
            });
            *sequence.staged.borrow_mut() = kept;
        }
        (PanZoomAdmission::Active(sequence), retired)
    }

    pub(super) fn is_current(
        &self,
        surface: InputSurface,
        pointer: PointerInfo,
        sequence: &Rc<PanZoomSequence>,
    ) -> bool {
        self.sessions
            .borrow()
            .get(&(surface, SignalSource::from(pointer)))
            .is_some_and(|current| Rc::ptr_eq(current, sequence))
    }

    /// Stage a consumer's retirement on a current session, or retire it now.
    pub(super) fn stage(
        &self,
        surface: InputSurface,
        pointer: PointerInfo,
        sequence: &Rc<PanZoomSequence>,
        ticket: StagedTicket,
        preserving_failure: &Cell<bool>,
    ) {
        if self.is_current(surface, pointer, sequence) {
            sequence.staged.borrow_mut().push(ticket);
        } else {
            retire_tickets(vec![ticket], preserving_failure);
        }
    }

    /// Select the session's consumer on first consumption and retire every
    /// other staged admission.
    pub(super) fn claim(
        &self,
        surface: InputSurface,
        pointer: PointerInfo,
        sequence: &Rc<PanZoomSequence>,
        route: PanZoomRoute,
        preserving_failure: &Cell<bool>,
    ) {
        if !self.is_current(surface, pointer, sequence) {
            return;
        }
        if sequence
            .route
            .get()
            .is_some_and(|winner| !winner.same_target(route))
        {
            return;
        }
        sequence.route.set(Some(route));
        let tickets = sequence.take_staged();
        let (kept, retired): (Vec<_>, Vec<_>) = tickets
            .into_iter()
            .partition(|(staged, _)| staged.same_target(route));
        *sequence.staged.borrow_mut() = kept;
        retire_tickets(retired, preserving_failure);
    }

    /// End `source`'s sessions on every surface: the source cancelled or left.
    pub(super) fn withdraw_source(&self, source: SignalSource) -> Vec<Rc<PanZoomSequence>> {
        let mut withdrawn: Vec<_> = self
            .sessions
            .borrow_mut()
            .extract_if(|(_, leased), _| *leased == source)
            .collect();
        // Retire in surface order, independent of hash layout.
        withdrawn.sort_unstable_by_key(|(key, _)| *key);
        withdrawn.into_iter().map(|(_, session)| session).collect()
    }

    /// Yield every session; the map is empty before any retirement runs.
    pub(super) fn drain(&self) -> Vec<Rc<PanZoomSequence>> {
        self.sessions.take().into_values().collect()
    }
}

/// Retire `tickets`, recording a caught failure in `preserving_failure` and
/// resuming it after every ticket ran.
pub(super) fn retire_tickets(tickets: Vec<StagedTicket>, preserving_failure: &Cell<bool>) {
    let mut first = None;
    retire_tickets_capturing_panic(tickets, &mut first, preserving_failure.get());
    if let Some(failure) = first {
        preserving_failure.set(true);
        failure.resume();
    }
}

pub(super) fn retire_tickets_capturing_panic(
    tickets: Vec<StagedTicket>,
    first: &mut Option<RoutePanic>,
    preserving_failure: bool,
) {
    for (_, ticket) in tickets {
        if std::thread::panicking() {
            crate::retain::Retain::retain(crate::retain::Owned(ticket.0));
            continue;
        }
        // Required quiescence still runs after a caught earlier failure;
        // its opaque captures inherit that failure's retirement policy.
        RoutePanic::preserve_first(
            first,
            RoutePanic::capture(|| (ticket.0)()),
            "native admission retirement",
        );
        if preserving_failure || first.is_some() {
            crate::retain::Retain::retain(crate::retain::Owned(ticket.0));
        } else {
            RoutePanic::preserve_first(
                first,
                RoutePanic::capture(|| drop(ticket.0)),
                "native admission capture retirement",
            );
        }
    }
}

/// Retire whatever `sequences` still stage.
pub(super) fn retire_sequences(
    sequences: Vec<Rc<PanZoomSequence>>,
    first: &mut Option<RoutePanic>,
) {
    let tickets = sequences
        .into_iter()
        .flat_map(|sequence| sequence.take_staged())
        .collect();
    retire_tickets_capturing_panic(tickets, first, false);
}
