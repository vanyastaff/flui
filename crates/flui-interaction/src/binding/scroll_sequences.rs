//! Binding-owned contactless admission, withdrawal and completion delivery.

use std::{cell::Cell, rc::Rc};

use flui_foundation::InputSequenceId;
use flui_platform_api::pointer::{PointerInfo, ScrollEvent, ScrollPhase};
use web_time::Instant;

use super::{GestureBinding, MAX_SIMULTANEOUS_POINTERS, SignalSource};
use crate::routing::{InteractionDispatchError, RoutePanic, ScrollRoute};
use crate::{ScrollEndReason, ScrollSequenceEnd};

/// Rc identity protects admission against replacement during user callbacks.
/// These records hold data only; callback ownership stays in the selected lane.
pub(super) struct ScrollSequence {
    pub(super) id: InputSequenceId,
    pub(super) route: Cell<Option<ScrollRoute>>,
    deadline: Cell<Instant>,
    admitted_at: Instant,
}

pub(super) struct PendingScrollEnd {
    route: ScrollRoute,
    end: ScrollSequenceEnd,
    retry_due: Instant,
}

pub(super) enum ScrollAdmission {
    Active(Rc<ScrollSequence>),
    Terminal(Rc<ScrollSequence>, ScrollEndReason),
    Refused,
}

impl GestureBinding {
    pub(super) fn scroll_deadline(&self) -> Option<Instant> {
        self.scroll_sequences
            .borrow()
            .values()
            .map(|sequence| sequence.deadline.get())
            .chain(
                self.pending_scroll_ends
                    .borrow()
                    .iter()
                    .map(|pending| pending.retry_due),
            )
            .min()
    }

    pub(crate) fn next_deadline_snapshot(&self) -> Option<Instant> {
        self.arena
            .next_deadline_snapshot()
            .into_iter()
            .chain(self.scroll_deadline())
            .min()
    }

    pub(crate) fn has_pending_deadlines_snapshot(&self) -> bool {
        self.arena.has_pending_deadlines_snapshot() || self.scroll_deadline().is_some()
    }

    pub(super) fn end_scroll_sequences(
        &self,
        mut sequences: Vec<Rc<ScrollSequence>>,
        reason: ScrollEndReason,
        first: &mut Option<RoutePanic>,
    ) {
        sequences.sort_unstable_by_key(|sequence| sequence.id);
        for sequence in sequences {
            if let Some(route) = sequence.route.get() {
                self.deliver_scroll_end(
                    PendingScrollEnd {
                        route,
                        end: ScrollSequenceEnd {
                            sequence: sequence.id,
                            reason,
                        },
                        retry_due: sequence.admitted_at,
                    },
                    first,
                );
            }
        }
    }

    fn deliver_scroll_end(&self, pending: PendingScrollEnd, first: &mut Option<RoutePanic>) {
        if self.closed.get() {
            return;
        }
        let mut retry = false;
        let ended = RoutePanic::capture(|| {
            retry = matches!(
                pending.route.end(pending.end),
                Err(InteractionDispatchError::InactiveRuntime
                    | InteractionDispatchError::WrongRuntime
                    | InteractionDispatchError::WrongThread)
            );
        });
        // A callback panic means invocation happened. Only failure to enter the
        // exact lane retains delivery debt, before any reentrant diagnostics.
        if retry && !self.closed.get() {
            self.pending_scroll_ends.borrow_mut().push(pending);
        }
        RoutePanic::preserve_first(first, ended, "scroll sequence completion");
    }

    pub(super) fn expire_scroll_sequences(&self, now: Instant) -> Option<RoutePanic> {
        let pending = std::mem::take(&mut *self.pending_scroll_ends.borrow_mut());
        let mut first = None;
        for end in pending {
            self.deliver_scroll_end(end, &mut first);
        }
        let mut expired = Vec::new();
        self.scroll_sequences.borrow_mut().retain(|_, sequence| {
            if sequence.deadline.get() <= now {
                expired.push(Rc::clone(sequence));
                false
            } else {
                true
            }
        });
        self.end_scroll_sequences(expired, ScrollEndReason::Released, &mut first);
        first
    }

    pub(super) fn prepare_scroll_sequence(
        &self,
        event: &ScrollEvent,
    ) -> (ScrollAdmission, Option<Rc<ScrollSequence>>) {
        let now = self.clock.now();
        let source = SignalSource::from(event.pointer);
        let mut sequences = self.scroll_sequences.borrow_mut();
        let deadline = now
            .checked_add(
                self.default_settings
                    .scroll_sequence_timeout(event.phase.is_some()),
            )
            .unwrap_or(now);
        if matches!(
            event.phase,
            Some(ScrollPhase::Ended | ScrollPhase::Cancelled | ScrollPhase::MomentumEnded)
        ) {
            // Withdrawal precedes terminal observers and consumer callbacks.
            let reason = if matches!(event.phase, Some(ScrollPhase::Cancelled)) {
                ScrollEndReason::Cancelled
            } else {
                ScrollEndReason::Released
            };
            if let Some(sequence) = sequences.remove(&source) {
                return (ScrollAdmission::Terminal(sequence, reason), None);
            }
            return (
                match self.input_sequence_ids.next() {
                    Ok(id) => ScrollAdmission::Terminal(
                        Rc::new(ScrollSequence {
                            id,
                            route: Cell::new(None),
                            deadline: Cell::new(deadline),
                            admitted_at: now,
                        }),
                        reason,
                    ),
                    Err(_) => ScrollAdmission::Refused,
                },
                None,
            );
        }
        let replaced = if matches!(
            event.phase,
            Some(ScrollPhase::Began | ScrollPhase::MomentumBegan)
        ) {
            sequences.remove(&source)
        } else {
            None
        };
        if let Some(sequence) = sequences.get(&source) {
            sequence.deadline.set(deadline);
            return (ScrollAdmission::Active(Rc::clone(sequence)), replaced);
        }
        if sequences.len()
            + self.pending_scroll_ends.borrow().len()
            + usize::from(
                replaced
                    .as_ref()
                    .is_some_and(|sequence| sequence.route.get().is_some()),
            )
            >= MAX_SIMULTANEOUS_POINTERS
        {
            // Refuse consumption rather than erase accepted completion debt.
            // Fresh observers still receive the native packet.
            return (ScrollAdmission::Refused, replaced);
        }
        let Ok(id) = self.input_sequence_ids.next() else {
            return (ScrollAdmission::Refused, replaced);
        };
        let sequence = Rc::new(ScrollSequence {
            id,
            route: Cell::new(None),
            deadline: Cell::new(deadline),
            admitted_at: now,
        });
        sequences.insert(source, Rc::clone(&sequence));
        (ScrollAdmission::Active(sequence), replaced)
    }

    pub(super) fn is_current_scroll_sequence(
        &self,
        pointer: PointerInfo,
        sequence: &Rc<ScrollSequence>,
    ) -> bool {
        self.scroll_sequences
            .borrow()
            .get(&SignalSource::from(pointer))
            .is_some_and(|current| Rc::ptr_eq(current, sequence))
    }
}
