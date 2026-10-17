//! The set of receivers one drag has entered, and the rules that keep their
//! enter, leave and drop honest (ADR-0211 §5, §8).
//!
//! Receivers are user code and may reenter the session that drives them, so no
//! borrow of the tracker is held across a call to one. State is committed first,
//! calls follow from owned clones, and a receiver whose `drag_entered` call was
//! still running when the tracker ended is sent its `drag_left` by the call that
//! entered it.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use flui_platform_api::EventTime;
use flui_platform_api::data_transfer::{DropFeedback, TransferActions};

use super::receiver::{DragPoint, DropReceiver, ResolvedReceiver, SpringLoading};
use super::view::DragView;
use crate::close::ClosePanic;
use crate::pointer::LocalPayloadTarget;

/// The receivers one drag is over. Cloning shares the state.
#[derive(Clone, Default)]
pub(crate) struct DropTracker(Rc<RefCell<TrackerState>>);

#[derive(Default)]
struct TrackerState {
    entered: Vec<Entered>,
    /// The receiver that accepted the last move, and the action it answered.
    accepting: Option<Accepting>,
    ended: bool,
}

struct Entered {
    key: LocalPayloadTarget,
    receiver: Rc<dyn DropReceiver>,
    /// `drag_entered` has returned. Until then a leave is owed by the call that
    /// is entering it, not by whoever removes the entry.
    announced: bool,
    point: DragPoint,
    spring: Option<Spring>,
}

#[derive(Clone, Copy)]
struct Spring {
    delay: Duration,
    since: Option<EventTime>,
    fired: bool,
}

#[derive(Clone, Copy)]
struct Accepting {
    key: LocalPayloadTarget,
    action: TransferActions,
}

/// What a finished tracker did with a drop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Dropped {
    /// A receiver took the drop.
    Delivered,
    /// No receiver had accepted the last move, or the drop carried no action.
    NoRecipient,
}

enum Step {
    Enter(Rc<dyn DropReceiver>, LocalPayloadTarget, DragPoint),
    Move(Rc<dyn DropReceiver>, LocalPayloadTarget, DragPoint),
}

/// The single action a hover answer names: when a target answers several the
/// platform convention order picks one (copy, then move, then link).
pub(crate) fn single_action(actions: TransferActions) -> Option<TransferActions> {
    [
        TransferActions::COPY,
        TransferActions::MOVE,
        TransferActions::LINK,
    ]
    .into_iter()
    .find(|action| actions.contains(*action))
}

impl DropTracker {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Whether the tracker has finished: no further update has any effect.
    pub(crate) fn is_ended(&self) -> bool {
        self.0.borrow().ended
    }

    /// Move the drag to `hits` (innermost first) at `global`. Receivers no longer
    /// under the pointer get their one leave, new ones their enter, the rest a
    /// move. Returns the action the innermost accepting receiver answered, which
    /// the drop would take, or `None` when nothing accepts.
    pub(crate) fn update(
        &self,
        hits: Vec<ResolvedReceiver>,
        view: &DragView<'_>,
        global: flui_foundation::geometry::Offset<f64>,
        time: Option<EventTime>,
    ) -> Option<TransferActions> {
        let mut failure = ClosePanic::new();
        let mut leaving = Vec::new();
        let mut steps = Vec::new();
        {
            let mut state = self.0.borrow_mut();
            if state.ended {
                return None;
            }
            state.entered.retain(|entered| {
                if hits.iter().any(|hit| hit.key == entered.key) {
                    return true;
                }
                if entered.announced {
                    leaving.push(Rc::clone(&entered.receiver));
                }
                false
            });
            for hit in &hits {
                let point = DragPoint {
                    global,
                    local: hit.local,
                    time,
                };
                if let Some(entered) = state.entered.iter_mut().find(|e| e.key == hit.key) {
                    entered.point = point;
                    steps.push(Step::Move(Rc::clone(&hit.receiver), hit.key, point));
                } else {
                    state.entered.push(Entered {
                        key: hit.key,
                        receiver: Rc::clone(&hit.receiver),
                        announced: false,
                        point,
                        spring: None,
                    });
                    steps.push(Step::Enter(Rc::clone(&hit.receiver), hit.key, point));
                }
            }
        }
        for receiver in leaving {
            failure.invoke(|| receiver.drag_left(view));
        }
        let mut answers: Vec<(LocalPayloadTarget, DropFeedback)> = Vec::new();
        for step in steps {
            if self.is_ended() {
                break;
            }
            match step {
                Step::Enter(receiver, key, point) => {
                    let answer = failure.invoke(|| receiver.drag_entered(view, point));
                    let spring = failure.invoke(|| receiver.spring_loading()).flatten();
                    let removed = {
                        let mut state = self.0.borrow_mut();
                        match state.entered.iter_mut().find(|e| e.key == key) {
                            Some(entered) => {
                                entered.announced = true;
                                entered.spring = spring.map(|SpringLoading { delay }| Spring {
                                    delay,
                                    since: point.time,
                                    fired: false,
                                });
                                false
                            }
                            None => true,
                        }
                    };
                    if removed {
                        // Removed while its enter ran: the remover left it
                        // unannounced, so the leave is owed here.
                        failure.invoke(|| receiver.drag_left(view));
                    } else if let Some(answer) = answer {
                        answers.push((key, answer));
                    }
                }
                Step::Move(receiver, key, point) => {
                    if let Some(answer) = failure.invoke(|| receiver.drag_moved(view, point)) {
                        answers.push((key, answer));
                    }
                }
            }
        }
        let accepted = answers.iter().find_map(|(key, answer)| {
            let action = answer
                .accept
                .map(|accept| accept & view.allowed())
                .and_then(single_action)?;
            Some(Accepting { key: *key, action })
        });
        {
            let mut state = self.0.borrow_mut();
            if state.ended {
                drop(state);
                failure.finish();
                return None;
            }
            state.accepting = accepted;
        }
        failure.finish();
        accepted.map(|accepting| accepting.action)
    }

    /// End the drag. With `drop` naming the action the drop resolved to, the
    /// receiver that accepted the last move takes it; every other receiver that
    /// was entered, a declined one included, gets its one leave. Idempotent.
    pub(crate) fn finish(&self, view: &DragView<'_>, drop: Option<TransferActions>) -> Dropped {
        let mut failure = ClosePanic::new();
        let (recipient, others) = {
            let mut state = self.0.borrow_mut();
            if state.ended {
                return Dropped::NoRecipient;
            }
            state.ended = true;
            let accepting = state.accepting.take();
            let mut entered = std::mem::take(&mut state.entered);
            let recipient = match (drop, accepting) {
                (Some(action), Some(accepting)) if !action.is_empty() => entered
                    .iter()
                    .position(|e| e.key == accepting.key && e.announced)
                    .map(|index| (entered.remove(index), action)),
                _ => None,
            };
            (recipient, entered)
        };
        for other in others {
            if other.announced {
                failure.invoke(|| other.receiver.drag_left(view));
            }
        }
        let delivered = recipient.is_some();
        if let Some((entered, action)) = recipient {
            failure.invoke(|| entered.receiver.dropped(view, entered.point, action));
        }
        failure.finish();
        if delivered {
            Dropped::Delivered
        } else {
            Dropped::NoRecipient
        }
    }

    /// Run the spring-loading of every receiver hovered past its delay at `now`,
    /// once per hover.
    pub(crate) fn tick(&self, view: &DragView<'_>, now: EventTime) {
        let due: Vec<Rc<dyn DropReceiver>> = {
            let mut state = self.0.borrow_mut();
            if state.ended {
                return;
            }
            state
                .entered
                .iter_mut()
                .filter_map(|entered| {
                    let spring = entered.spring.as_mut()?;
                    let since = *spring.since.get_or_insert(now);
                    if spring.fired || now.saturating_duration_since(since) < spring.delay {
                        return None;
                    }
                    spring.fired = true;
                    Some(Rc::clone(&entered.receiver))
                })
                .collect()
        };
        let mut failure = ClosePanic::new();
        for receiver in due {
            if self.is_ended() {
                break;
            }
            failure.invoke(|| receiver.spring_loaded(view));
        }
        failure.finish();
    }

    /// When the earliest pending spring-loading comes due, if any is hovering.
    pub(crate) fn next_deadline(&self) -> Option<EventTime> {
        let state = self.0.borrow();
        if state.ended {
            return None;
        }
        state
            .entered
            .iter()
            .filter_map(|entered| {
                let spring = entered.spring.as_ref()?;
                if spring.fired {
                    return None;
                }
                spring.since?.checked_add(spring.delay)
            })
            .min()
    }
}
