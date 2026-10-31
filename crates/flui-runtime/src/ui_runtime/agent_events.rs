//! The events an agent streams from a window (ADR-0238 §2, R19).
//!
//! A subscription is registered on its presentation by the owner and filled by it: a frame the
//! window presented or skipped, a tree it published at a new revision, its close. Each
//! subscriber has a bounded queue, so a reader that falls behind costs the window nothing but
//! its own events: the ones that do not fit are counted, and the next event that fits is
//! preceded by a `gap` saying how many were missed. The same events feed the waits parked on the
//! window, so a wait on an event and a subscription to it see the same stream.
//!
//! Input and announcement events are accepted as filters and not emitted yet: the input journal
//! and the announcement fan-out are what they come from.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use flui_foundation::PresentationId;
use flui_protocol::{
    Event, EventFilter, EventKind, FrameEvent, GapEvent, SemanticsEvent, WindowEvent, WindowId,
};
use flui_view::__runtime::EventSource;
use parking_lot::{Condvar, Mutex};

use super::UiRuntime;
use super::agent::{AgentError, ReplySender, send_reply};
use crate::presentation::PresentationState;
use crate::semantics_host::SemanticsHandle;

/// The most live subscriptions one presentation admits. A subscription past it is refused with
/// [`AgentError::TooManySubscriptions`]; one whose stream was dropped no longer counts.
pub(crate) const MAX_SUBSCRIPTIONS: usize = 64;

/// The most events one subscriber's queue holds before it counts the rest as missed.
const QUEUE_EVENTS: usize = 256;

#[derive(Default)]
struct QueueState {
    events: VecDeque<Event>,
    /// Events that did not fit since the last `gap` was queued.
    missed: u64,
    /// The window closed: nothing is queued after what is there.
    ended: bool,
}

/// One subscriber's queue, shared by its stream and the presentation's hub. The hub's clone is
/// the only one left once the stream is dropped, which is how a dropped subscription is seen.
#[derive(Default)]
struct Queue {
    state: Mutex<QueueState>,
    arrived: Condvar,
}

impl Queue {
    fn push(&self, event: Event) {
        let mut state = self.state.lock();
        if state.ended {
            return;
        }
        if state.events.len() >= QUEUE_EVENTS {
            state.missed = state.missed.saturating_add(1);
            return;
        }
        if state.missed > 0 {
            let missed = std::mem::take(&mut state.missed);
            state.events.push_back(Event::Gap(GapEvent::new(missed)));
            if state.events.len() >= QUEUE_EVENTS {
                // The gap took the last place: this event is missed too, and the next gap says so.
                state.missed = 1;
                drop(state);
                self.arrived.notify_one();
                return;
            }
        }
        state.events.push_back(event);
        drop(state);
        self.arrived.notify_one();
    }

    /// The window closed. A gap still owed, and the event that says so, are queued past the
    /// bound: the stream never ends shorter than it says, and a reader that fell behind still
    /// sees the window close.
    fn end(&self, last: Option<Event>) {
        let mut state = self.state.lock();
        if state.missed > 0 {
            let missed = std::mem::take(&mut state.missed);
            state.events.push_back(Event::Gap(GapEvent::new(missed)));
        }
        state.events.extend(last);
        state.ended = true;
        drop(state);
        self.arrived.notify_all();
    }
}

/// The reading half of a subscription: what [`SemanticsAgent::subscribe`] answers with. Dropping
/// it ends the subscription.
///
/// [`SemanticsAgent::subscribe`]: super::SemanticsAgent::subscribe
pub struct EventStream {
    queue: Arc<Queue>,
}

impl EventStream {
    /// The next event, if one is queued.
    pub fn try_next(&mut self) -> Option<Event> {
        self.queue.state.lock().events.pop_front()
    }

    /// The next event, waiting up to `timeout` for one; `None` if none came by then, and once
    /// the stream has ended. Never call this on the UI runtime's owner thread: the owner fills
    /// the queue only at its frame boundaries.
    pub fn recv_timeout(&mut self, timeout: Duration) -> Option<Event> {
        let deadline = Instant::now().checked_add(timeout);
        let mut state = self.queue.state.lock();
        loop {
            if let Some(event) = state.events.pop_front() {
                return Some(event);
            }
            if state.ended {
                return None;
            }
            match deadline {
                Some(deadline) => {
                    if self
                        .queue
                        .arrived
                        .wait_until(&mut state, deadline)
                        .timed_out()
                    {
                        return state.events.pop_front();
                    }
                }
                None => self.queue.arrived.wait(&mut state),
            }
        }
    }

    /// Whether the stream has ended and every event it queued was taken.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        let state = self.queue.state.lock();
        state.ended && state.events.is_empty()
    }
}

impl std::fmt::Debug for EventStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventStream").finish_non_exhaustive()
    }
}

impl EventSource for EventStream {
    fn try_next(&mut self) -> Option<Event> {
        Self::try_next(self)
    }

    fn recv_timeout(&mut self, timeout: Duration) -> Option<Event> {
        Self::recv_timeout(self, timeout)
    }

    fn is_finished(&self) -> bool {
        Self::is_finished(self)
    }
}

struct Subscriber {
    filters: Vec<EventFilter>,
    queue: Arc<Queue>,
    /// Keeps the window collecting semantics while a subscription to `semantics` events lives,
    /// whoever else holds an agent.
    _semantics: Option<SemanticsHandle>,
}

impl Subscriber {
    fn selects(&self, window: WindowId, event: &Event) -> bool {
        let kind = event.kind();
        self.filters
            .iter()
            .any(|filter| filter.kind == kind && filter.window.is_none_or(|w| w == window))
    }
}

/// What a presentation keeps to emit its events: its subscribers, and how far it has reported.
/// Owner thread only.
#[derive(Default)]
pub(crate) struct EventHub {
    subscribers: Vec<Subscriber>,
    /// The last tree revision a `semantics` event named.
    revision: Option<flui_protocol::TreeRevision>,
    /// Frames presented and frames skipped that `frame` events already named.
    frames: (u64, u64),
}

impl EventHub {
    /// Forgets the subscribers whose stream was dropped.
    fn prune(&mut self) {
        self.subscribers
            .retain(|subscriber| Arc::strong_count(&subscriber.queue) > 1);
    }

    fn subscribe(
        &mut self,
        filters: Vec<EventFilter>,
        semantics: Option<SemanticsHandle>,
    ) -> Result<EventStream, AgentError> {
        if filters.is_empty() {
            return Err(AgentError::InvalidArgument {
                reason: "a subscription names at least one event",
            });
        }
        self.prune();
        if self.subscribers.len() >= MAX_SUBSCRIPTIONS {
            return Err(AgentError::TooManySubscriptions);
        }
        let queue = Arc::new(Queue::default());
        self.subscribers.push(Subscriber {
            filters,
            queue: Arc::clone(&queue),
            _semantics: semantics,
        });
        Ok(EventStream { queue })
    }

    fn publish(&mut self, window: WindowId, event: &Event) {
        self.prune();
        for subscriber in &self.subscribers {
            if subscriber.selects(window, event) {
                subscriber.queue.push(event.clone());
            }
        }
    }

    /// Ends every stream; `last` goes to the ones that select it, past their bound.
    fn end(&mut self, window: WindowId, last: &Event) {
        for subscriber in self.subscribers.drain(..) {
            let last = subscriber.selects(window, last).then(|| last.clone());
            subscriber.queue.end(last);
        }
    }
}

impl PresentationState {
    fn agent_window(&self) -> WindowId {
        WindowId::from_u64(self.id().as_u64())
            .expect("BUG: a presentation id packs a non-zero generation")
    }

    /// Emits `event` to the window's subscribers and notes it for the waits parked on it.
    pub(crate) fn emit_agent_event(&self, event: Event) {
        for wait in self.agent_waits.borrow_mut().iter_mut() {
            wait.note_event(event.kind());
        }
        self.agent_events
            .borrow_mut()
            .publish(self.agent_window(), &event);
    }

    /// Emits what a frame left to report: the frames presented or skipped since the last report,
    /// and the tree revision when it changed.
    pub(crate) fn publish_frame_events(&self) {
        // A dropped stream stops holding the window collecting semantics from here.
        self.agent_events.borrow_mut().prune();
        let window = self.agent_window();
        let (rendered, dropped) = (self.frames_rendered(), self.frames_dropped());
        let (seen_rendered, seen_dropped) = {
            let mut hub = self.agent_events.borrow_mut();
            std::mem::replace(&mut hub.frames, (rendered, dropped))
        };
        let mut number = seen_rendered + seen_dropped;
        for (count, presented) in [
            (rendered.saturating_sub(seen_rendered), true),
            (dropped.saturating_sub(seen_dropped), false),
        ] {
            for _ in 0..count.min(QUEUE_EVENTS as u64) {
                number += 1;
                self.emit_agent_event(Event::Frame(FrameEvent::new(window, number, presented)));
            }
        }
        // Revision 0 is an owner that has published nothing yet: not a tree to report.
        let revision = self.pipeline().with(|pipeline| {
            pipeline
                .semantics_owner()
                .map(flui_semantics::SemanticsOwner::revision)
                .filter(|revision| revision.0 != 0)
        });
        if let Some(revision) = revision {
            let changed = {
                let mut hub = self.agent_events.borrow_mut();
                hub.revision.replace(revision) != Some(revision)
            };
            if changed {
                self.emit_agent_event(Event::Semantics(SemanticsEvent::new(window, revision)));
            }
        }
    }

    /// The window closes: its last event says so, and every stream ends after it.
    pub(crate) fn end_agent_events(&self) {
        let window = self.agent_window();
        let closed = Event::Window(WindowEvent::new(window, false));
        for wait in self.agent_waits.borrow_mut().iter_mut() {
            wait.note_event(closed.kind());
        }
        self.agent_events.borrow_mut().end(window, &closed);
    }
}

impl UiRuntime {
    /// The owner half of [`SemanticsAgent::subscribe`](super::SemanticsAgent::subscribe):
    /// registers the subscription and answers with its stream. Returns whether the presentation
    /// was still open.
    pub(super) fn subscribe_agent_events(
        &self,
        presentation: PresentationId,
        filters: Vec<EventFilter>,
        reply: &ReplySender<EventStream>,
    ) -> bool {
        let Some(state) = self.presentations.get(presentation) else {
            send_reply(reply, None, Err(AgentError::PresentationGone));
            return false;
        };
        let semantics = filters
            .iter()
            .any(|filter| filter.kind == EventKind::Semantics)
            .then(|| state.semantics_host().ensure_semantics());
        let wants_tree = semantics.is_some();
        let result = state
            .agent_events
            .borrow_mut()
            .subscribe(filters, semantics);
        let subscribed = wants_tree && result.is_ok();
        // Answered first: the redraw below reaches the platform's wake, which is user code.
        send_reply(reply, None, result);
        if subscribed {
            // Collection starts at the next frame: ask for one, as vending an agent does.
            self.request_redraw_for(state);
        }
        true
    }

    /// Emits what the frame the owner just ran left to report, in every presentation.
    pub(crate) fn publish_agent_events(&self) {
        for presentation in self.presentations.iter() {
            presentation.publish_frame_events();
        }
    }
}
