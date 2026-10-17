//! The set of receivers one drag has entered, and the rules that keep their
//! enter, leave and drop honest (ADR-0211 §5, §8).
//!
//! Receivers are user code and may reenter the session that drives them, so no
//! borrow of the tracker is held across a call to one. State is committed first,
//! calls follow from owned clones, and a receiver whose `drag_entered` call was
//! still running when the tracker ended is sent its `drag_left` by the call that
//! entered it.
//!
//! A receiver is one registration: the lane ticket its node carries, holding
//! that receiver. A ticket its widget unregistered, or whose payload now holds
//! another receiver, no longer names it: the tracker leaves it at the next
//! move or tick and never drops on it.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use flui_platform_api::EventTime;
use flui_platform_api::data_transfer::{DropFeedback, TransferActions};

use super::receiver::{
    Autoscroll, DragPoint, DropReceiver, ResolvedReceiver, SpringLoading, still_registered,
};
use super::view::DragView;
use crate::close::ClosePanic;
use crate::pointer::{InteractionDispatchHandle, LocalPayloadTarget};

/// How often an autoscroll that no frame drives asks for a turn: one frame at
/// 60 Hz. Frames tick sooner on faster displays.
const AUTOSCROLL_FRAME: Duration = Duration::from_nanos(16_666_667);

/// The receivers one drag is over. Cloning shares the state.
#[derive(Clone, Default)]
pub(crate) struct DropTracker(Rc<RefCell<TrackerState>>);

#[derive(Default)]
struct TrackerState {
    /// Innermost first, as of the last move.
    entered: Vec<Entered>,
    /// The receiver that accepted the last move, and the action it answered.
    accepting: Option<Accepting>,
    /// Autoscroll frames are due: the drag moved, or a receiver still scrolls.
    autoscroll: Option<AutoscrollClock>,
    /// Bumped by every move, so an autoscroll pass a move reentered keeps the
    /// frames that move asked for.
    moves: u64,
    ended: bool,
}

struct Entered {
    key: LocalPayloadTarget,
    receiver: Rc<dyn DropReceiver>,
    lane: InteractionDispatchHandle,
    /// `drag_entered` has returned. Until then a leave is owed by the call that
    /// is entering it, not by whoever removes the entry.
    announced: bool,
    point: DragPoint,
    spring: Option<Spring>,
}

impl Entered {
    fn is(&self, key: LocalPayloadTarget, receiver: &Rc<dyn DropReceiver>) -> bool {
        self.key == key && Rc::ptr_eq(&self.receiver, receiver)
    }

    fn is_registered(&self) -> bool {
        still_registered(&self.lane, self.key, &self.receiver)
    }
}

#[derive(Clone, Copy)]
struct Spring {
    delay: Duration,
    since: Option<EventTime>,
    fired: bool,
}

#[derive(Clone, Copy)]
struct AutoscrollClock {
    /// When the previous frame ran, or the hover began; `None` until a time
    /// is known.
    since: Option<EventTime>,
    /// The last frame scrolled, so the next one is due a frame later. The
    /// first frame after a move rides the frame that move requests.
    scrolling: bool,
}

#[derive(Clone)]
struct Accepting {
    key: LocalPayloadTarget,
    receiver: Rc<dyn DropReceiver>,
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

/// What a tick changed that the drag's owner must pass on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ticked {
    /// The accepting receiver is still the one the last move found.
    Unchanged,
    /// The receiver that accepted the last move went away: nothing accepts
    /// until the next move.
    AcceptingGone,
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
            let mut previous = std::mem::take(&mut state.entered);
            let mut current = Vec::with_capacity(hits.len());
            for hit in &hits {
                let point = DragPoint {
                    global,
                    local: hit.local,
                    time,
                };
                if let Some(index) = previous
                    .iter()
                    .position(|entered| hit.is_same(entered.key, &entered.receiver))
                {
                    let mut entered = previous.swap_remove(index);
                    entered.point = point;
                    steps.push(Step::Move(Rc::clone(&hit.receiver), hit.key, point));
                    current.push(entered);
                } else {
                    current.push(Entered {
                        key: hit.key,
                        receiver: Rc::clone(&hit.receiver),
                        lane: hit.lane.clone(),
                        announced: false,
                        point,
                        spring: None,
                    });
                    steps.push(Step::Enter(Rc::clone(&hit.receiver), hit.key, point));
                }
            }
            leaving.extend(
                previous
                    .into_iter()
                    .filter(|entered| entered.announced)
                    .map(|entered| entered.receiver),
            );
            state.entered = current;
            state.moves = state.moves.wrapping_add(1);
            if state.entered.is_empty() {
                state.autoscroll = None;
            } else {
                // A move keeps the clock of frames already running, so a drag
                // moving every frame does not lose the time between them.
                let clock = state.autoscroll.get_or_insert(AutoscrollClock {
                    since: time,
                    scrolling: false,
                });
                if clock.since.is_none() {
                    clock.since = time;
                }
            }
        }
        for receiver in leaving {
            failure.invoke(|| receiver.drag_left(view));
        }
        let mut answers: Vec<(LocalPayloadTarget, Rc<dyn DropReceiver>, DropFeedback)> = Vec::new();
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
                        match state.entered.iter_mut().find(|e| e.is(key, &receiver)) {
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
                        answers.push((key, receiver, answer));
                    }
                }
                Step::Move(receiver, key, point) => {
                    if let Some(answer) = failure.invoke(|| receiver.drag_moved(view, point)) {
                        answers.push((key, receiver, answer));
                    }
                }
            }
        }
        let accepted = answers.into_iter().find_map(|(key, receiver, answer)| {
            let action = answer
                .accept
                .map(|accept| accept & view.allowed())
                .and_then(single_action)?;
            Some(Accepting {
                key,
                receiver,
                action,
            })
        });
        let action = accepted.as_ref().map(|accepting| accepting.action);
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
        action
    }

    /// End the drag. With `drop` naming the action the drop resolved to, the
    /// receiver that accepted the last move takes it, if it is still
    /// registered; every other receiver that was entered, a declined one
    /// included, gets its one leave. Idempotent.
    pub(crate) fn finish(&self, view: &DragView<'_>, drop: Option<TransferActions>) -> Dropped {
        let mut failure = ClosePanic::new();
        let (recipient, others) = {
            let mut state = self.0.borrow_mut();
            if state.ended {
                return Dropped::NoRecipient;
            }
            state.ended = true;
            state.autoscroll = None;
            let accepting = state.accepting.take();
            let mut entered = std::mem::take(&mut state.entered);
            let recipient = match (drop, accepting) {
                (Some(action), Some(accepting)) if !action.is_empty() => entered
                    .iter()
                    .position(|e| {
                        e.is(accepting.key, &accepting.receiver) && e.announced && e.is_registered()
                    })
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

    /// Advance the hover to `now`: receivers whose registration went away
    /// leave, the spring-loading of every receiver hovered past its delay
    /// runs once per hover, and one autoscroll frame runs.
    pub(crate) fn tick(&self, view: &DragView<'_>, now: EventTime) -> Ticked {
        let mut failure = ClosePanic::new();
        let ticked = self.leave_unregistered(view, &mut failure);
        self.spring_load(view, now, &mut failure);
        self.autoscroll_frame(view, now, &mut failure);
        failure.finish();
        ticked
    }

    /// Leave every entered receiver whose ticket no longer holds it.
    fn leave_unregistered(&self, view: &DragView<'_>, failure: &mut ClosePanic) -> Ticked {
        let (gone, ticked) = {
            let mut state = self.0.borrow_mut();
            if state.ended {
                return Ticked::Unchanged;
            }
            let (kept, gone): (Vec<Entered>, Vec<Entered>) = std::mem::take(&mut state.entered)
                .into_iter()
                .partition(Entered::is_registered);
            state.entered = kept;
            let lost = state.accepting.as_ref().is_some_and(|accepting| {
                gone.iter()
                    .any(|entered| entered.is(accepting.key, &accepting.receiver))
            });
            let ticked = if lost {
                state.accepting = None;
                Ticked::AcceptingGone
            } else {
                Ticked::Unchanged
            };
            (gone, ticked)
        };
        for entered in gone {
            if entered.announced && !self.is_ended() {
                failure.invoke(|| entered.receiver.drag_left(view));
            }
        }
        ticked
    }

    fn spring_load(&self, view: &DragView<'_>, now: EventTime, failure: &mut ClosePanic) {
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
        for receiver in due {
            if self.is_ended() {
                break;
            }
            failure.invoke(|| receiver.spring_loaded(view));
        }
    }

    /// Ask the receivers under the drag, innermost first, for one autoscroll
    /// frame, until one scrolls. When none does, frames stop until the next
    /// move.
    fn autoscroll_frame(&self, view: &DragView<'_>, now: EventTime, failure: &mut ClosePanic) {
        let (elapsed, moves, candidates) = {
            let mut state = self.0.borrow_mut();
            if state.ended {
                return;
            }
            let Some(clock) = state.autoscroll.as_mut() else {
                return;
            };
            let elapsed = clock
                .since
                .map_or(Duration::ZERO, |since| now.saturating_duration_since(since));
            clock.since = Some(now);
            let candidates: Vec<(Rc<dyn DropReceiver>, DragPoint)> = state
                .entered
                .iter()
                .filter(|entered| entered.announced)
                .map(|entered| (Rc::clone(&entered.receiver), entered.point))
                .collect();
            (elapsed, state.moves, candidates)
        };
        let mut scrolling = false;
        for (receiver, point) in candidates {
            if self.is_ended() {
                return;
            }
            // A receiver that panicked scrolls nothing; the next one out is asked.
            if failure.invoke(|| receiver.autoscroll(view, point, elapsed))
                == Some(Autoscroll::Scrolling)
            {
                scrolling = true;
                break;
            }
        }
        let mut state = self.0.borrow_mut();
        if scrolling {
            if let Some(clock) = state.autoscroll.as_mut() {
                clock.scrolling = true;
            }
        } else if state.moves == moves {
            state.autoscroll = None;
        }
    }

    /// When the tracker next has work: the earliest pending spring-loading or
    /// autoscroll frame. A hover that carries no time yet asks for a turn at
    /// once, which stamps it.
    pub(crate) fn next_deadline(&self) -> Option<EventTime> {
        let state = self.0.borrow();
        if state.ended {
            return None;
        }
        let at_once = EventTime::from_nanos(0);
        let springs = state.entered.iter().filter_map(|entered| {
            let spring = entered.spring.as_ref()?;
            if spring.fired {
                return None;
            }
            Some(
                spring
                    .since
                    .map_or(Some(at_once), |since| since.checked_add(spring.delay)),
            )
        });
        let frame = state
            .autoscroll
            .filter(|clock| clock.scrolling)
            .map(|clock| {
                clock
                    .since
                    .map_or(Some(at_once), |since| since.checked_add(AUTOSCROLL_FRAME))
            });
        springs.chain(frame).flatten().min()
    }
}
