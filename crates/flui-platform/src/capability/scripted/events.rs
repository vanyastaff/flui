//! [`Events`]: a stream of platform events on a scenario's time.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;
use std::rc::{Rc, Weak};
use std::time::Duration;

use flui_foundation::panic::FirstPanic;

use super::{Scenario, ScenarioFailure, describe};
use crate::Unsupported;
use crate::capability::{Completion, CompletionSource, Resolver};

/// One item a stream hands a subscriber.
#[derive(Clone, Debug, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a stream hands an event or its end; how it ended is StreamEnd's to grow"
)]
pub enum StreamItem<E> {
    /// An event.
    Event(E),
    /// The stream ended: every later item a subscriber asks for is this one.
    End(StreamEnd),
}

/// How a stream ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum StreamEnd {
    /// The platform has nothing more to send.
    Finished,
    /// The service went away, with this refusal.
    Interrupted(Unsupported),
}

/// A stream of events the platform sends at instants of a scenario: a
/// sensor's readings, a connectivity change, an incoming message.
///
/// Each subscriber ([`subscribe`](Self::subscribe)) receives every event
/// sent after it subscribed, in order, and then the end. A stream ends once;
/// an event or a second end sent after it is
/// [`AfterEnd`](ScenarioFailure::AfterEnd). The test sends what the script
/// wrote, so a subscriber's queue holds at most what the test sent.
/// Owner-thread only, cheap to clone; every clone is the same stream.
pub struct Events<E> {
    cell: Rc<EventsCell<E>>,
}

struct EventsCell<E> {
    name: &'static str,
    scenario: Scenario,
    state: RefCell<EventsState<E>>,
}

struct EventsState<E> {
    subscribers: Vec<Weak<Subscriber<E>>>,
    end: Option<StreamEnd>,
}

/// One subscriber's place in an [`Events`] stream: the items sent since it
/// subscribed that it has not asked for yet. Dropping it unsubscribes, and
/// a completion still waiting on it ends
/// [`ProviderDropped`](crate::capability::Abandoned::ProviderDropped).
pub struct EventSubscription<E> {
    subscriber: Rc<Subscriber<E>>,
}

struct Subscriber<E> {
    state: RefCell<SubscriberState<E>>,
}

struct SubscriberState<E> {
    queued: VecDeque<StreamItem<E>>,
    /// The completions asking for the next item, oldest first.
    waiting: VecDeque<Resolver<StreamItem<E>>>,
    /// The stream's end, once it is queued: what every item after the queue
    /// drains is.
    end: Option<StreamEnd>,
}

impl<E> Clone for Events<E> {
    fn clone(&self) -> Self {
        Self {
            cell: Rc::clone(&self.cell),
        }
    }
}

impl<E> Events<E>
where
    E: Clone + fmt::Debug + 'static,
{
    pub(super) fn on(scenario: &Scenario, name: &'static str) -> Self {
        Self {
            cell: Rc::new(EventsCell {
                name,
                scenario: scenario.clone(),
                state: RefCell::new(EventsState {
                    subscribers: Vec::new(),
                    end: None,
                }),
            }),
        }
    }

    /// The stream's name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.cell.name
    }

    /// Subscribe: the new subscriber receives what is sent from now on, or
    /// the end at once when the stream has ended.
    #[must_use]
    pub fn subscribe(&self) -> EventSubscription<E> {
        let mut state = self.cell.state.borrow_mut();
        let subscriber = Rc::new(Subscriber {
            state: RefCell::new(SubscriberState {
                queued: VecDeque::new(),
                waiting: VecDeque::new(),
                end: state.end,
            }),
        });
        state.subscribers.retain(|live| live.strong_count() > 0);
        state.subscribers.push(Rc::downgrade(&subscriber));
        EventSubscription { subscriber }
    }

    /// Send `event` to every subscriber now.
    ///
    /// # Panics
    ///
    /// With the first panic of the event type's `Clone` or `Debug`, once
    /// every subscriber whose copy could be made has it.
    pub fn emit(&self, event: E) {
        let mut first = FirstPanic::new();
        if let Some(subscribers) = self.open(|first| describe(&event, first), &mut first) {
            for subscriber in subscribers {
                // Cloned with no borrow held: `Clone` is the event type's code.
                if let Some(copy) = first.contain(|| event.clone()) {
                    subscriber.deliver(StreamItem::Event(copy));
                }
            }
        }
        first.resume();
    }

    /// Send `event` at `at` on the scenario's clock: at the first
    /// [`Scenario::run_due`] on or after it.
    pub fn emit_at(&self, at: Duration, event: E) {
        let what = Cow::Owned(format!("{}: {event:?}", self.cell.name));
        self.schedule(at, what, move |events| events.emit(event));
    }

    /// End the stream now with `end`.
    pub fn end(&self, end: StreamEnd) {
        let Some(subscribers) = self.open(|_| format!("the end {end:?}"), &mut FirstPanic::new())
        else {
            return;
        };
        self.cell.state.borrow_mut().end = Some(end);
        for subscriber in subscribers {
            subscriber.finish(end);
        }
    }

    /// End the stream with `end` at `at` on the scenario's clock.
    pub fn end_at(&self, at: Duration, end: StreamEnd) {
        let what = Cow::Owned(format!("{} ends {end:?}", self.cell.name));
        self.schedule(at, what, move |events| events.end(end));
    }

    /// The live subscribers while the stream is open; `None`, with the
    /// failure recorded, once it has ended.
    fn open(
        &self,
        sent: impl FnOnce(&mut FirstPanic) -> String,
        first: &mut FirstPanic,
    ) -> Option<Vec<Rc<Subscriber<E>>>> {
        let open = {
            let mut state = self.cell.state.borrow_mut();
            state.end.is_none().then(|| {
                state.subscribers.retain(|live| live.strong_count() > 0);
                state
                    .subscribers
                    .iter()
                    .filter_map(Weak::upgrade)
                    .collect::<Vec<_>>()
            })
        };
        if open.is_none() {
            self.cell.scenario.fail(ScenarioFailure::AfterEnd {
                stream: self.cell.name,
                sent: sent(first),
            });
        }
        open
    }

    fn schedule(&self, at: Duration, what: Cow<'static, str>, send: impl FnOnce(Self) + 'static) {
        let events = Rc::downgrade(&self.cell);
        self.cell.scenario.schedule(
            at,
            what,
            Box::new(move || {
                if let Some(cell) = events.upgrade() {
                    send(Events { cell });
                }
            }),
        );
    }
}

impl<E: 'static> Subscriber<E> {
    /// Hand `item` to the oldest waiting completion, or queue it.
    fn deliver(&self, item: StreamItem<E>) {
        let waiting = {
            let mut state = self.state.borrow_mut();
            let waiting = state.waiting.pop_front();
            if waiting.is_none() {
                state.queued.push_back(item);
                return;
            }
            waiting
        };
        if let Some(resolver) = waiting {
            // A retired owner has nobody left to tell.
            let _ = resolver.resolve(item);
        }
    }

    /// The stream ended: queue the end behind what is queued, and answer
    /// every completion still waiting with it.
    fn finish(&self, end: StreamEnd) {
        let waiting = {
            let mut state = self.state.borrow_mut();
            state.end = Some(end);
            std::mem::take(&mut state.waiting)
        };
        for resolver in waiting {
            let _ = resolver.resolve(StreamItem::End(end));
        }
    }
}

impl<E: 'static> EventSubscription<E> {
    /// The next item, settled at an owner turn: the oldest one queued, the
    /// end once the queue is drained and the stream has ended, else the next
    /// one sent.
    #[must_use = "a completion delivers the item only to on_complete, poll or its future"]
    pub fn next(&self, completions: &CompletionSource) -> Completion<StreamItem<E>> {
        let (completion, resolver) = completions.pending::<StreamItem<E>>();
        let ready = {
            let mut state = self.subscriber.state.borrow_mut();
            let ready = state
                .queued
                .pop_front()
                .or_else(|| state.end.map(StreamItem::End));
            if ready.is_none() {
                state.waiting.push_back(resolver);
                return completion;
            }
            ready
        };
        if let Some(item) = ready {
            // A retired owner has nobody left to tell.
            let _ = resolver.resolve(item);
        }
        completion
    }

    /// How many items are queued for this subscriber.
    #[must_use]
    pub fn queued(&self) -> usize {
        self.subscriber.state.borrow().queued.len()
    }
}

impl<E> fmt::Debug for Events<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (subscribers, end) = {
            let state = self.cell.state.borrow();
            (state.subscribers.len(), state.end)
        };
        f.debug_struct("Events")
            .field("name", &self.cell.name)
            .field("subscribers", &subscribers)
            .field("end", &end)
            .finish_non_exhaustive()
    }
}

impl<E> fmt::Debug for EventSubscription<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (queued, waiting, end) = {
            let state = self.subscriber.state.borrow();
            (state.queued.len(), state.waiting.len(), state.end)
        };
        f.debug_struct("EventSubscription")
            .field("queued", &queued)
            .field("waiting", &waiting)
            .field("end", &end)
            .finish()
    }
}
