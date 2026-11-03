//! What each method the agent serves does: parse its typed parameters, ask the window's owner,
//! and write the typed output once the owner answers.
//!
//! A window method ([`WindowMethod`]) takes one route, generated from the protocol's table: its
//! parameters name the window it asks, through the seam's one call, and the owner answers with
//! the method's output or the protocol's error object. The app's own methods (`windows.list`,
//! `events.*`) are answered here.
//!
//! Nothing here waits. A request [starts](start) at once and is then [polled](Running::poll)
//! until its answer comes or its deadline passes; the waker a poll registers is woken when the
//! owner answers.

// Every refusal here becomes the reply line itself, written once per request, so the error
// object is returned by value: boxing it would add an allocation to each refusal to save a copy
// of a few hundred bytes. The lint fires only where `ErrorObject` is over its size limit.
#![cfg_attr(
    target_pointer_width = "64",
    expect(
        clippy::result_large_err,
        reason = "an error object is the reply, returned once per request"
    )
)]

use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;
use std::task::Waker;
use std::time::Duration;

use flui_protocol::method::{
    Addressed, EventsSubscribe, EventsUnsubscribe, Method, WindowMethod, WindowsList,
};
use flui_protocol::{
    ClientRole, Effect, EffectKind, ErrorCode, ErrorObject, Event, EventFilter, HandleKind,
    METHODS, MethodEffect, MethodInfo, RequestId, Retry, ServedBy, Subscription, WindowId,
    WindowList,
};
use flui_view::dev_agent::{AgentAnswer, AgentEvents, AgentWindow};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use web_time::Instant;

use super::registry::{Lookup, Registry};
use super::service::Shared;

/// The methods the app answers itself, not one of its windows.
const APPS_OWN: &[&str] = &[
    EventsSubscribe::NAME,
    EventsUnsubscribe::NAME,
    WindowsList::NAME,
];

/// Whether the agent answers `method`: one of the app's own, or a window method this build's
/// windows answer.
pub(super) fn serves(method: &MethodInfo) -> bool {
    method.served_by == ServedBy::Agent
        && (APPS_OWN.contains(&method.name) || crate::ui_runtime::window_answers(method.name))
}

/// Whether a connection admitted as `role` may call `method`: the driver anything, an observer
/// only what reads (ADR-0288 §8).
pub(super) fn may_call(role: ClientRole, method: &MethodInfo) -> bool {
    role.is_driver() || method.effect == MethodEffect::ReadOnly
}

/// The methods the agent answers a connection admitted as `role`, by wire name, in table order.
pub(super) fn served(role: ClientRole) -> Vec<String> {
    METHODS
        .iter()
        .filter(|method| serves(method) && may_call(role, method))
        .map(|method| method.name.to_owned())
        .collect()
}

/// `method` acts, and the connection only observes.
pub(super) fn not_driver(method: &MethodInfo) -> ErrorObject {
    ErrorObject::new(
        ErrorCode::NotDriver,
        format!(
            "{} acts, and this connection observes: only the app's driver acts",
            method.name
        ),
    )
}

/// `method` is in the table but not answered here: by another side, or not yet by this agent.
pub(super) fn not_supported(method: &MethodInfo) -> ErrorObject {
    let mut error = ErrorObject::new(
        ErrorCode::NotSupported,
        format!("this app's agent does not answer {}", method.name),
    );
    error.capability = Some(method.name.to_owned());
    error
}

/// A request admitted to a conversation, waiting for its turn.
pub(super) struct Admitted {
    pub(super) id: RequestId,
    pub(super) method: &'static MethodInfo,
    pub(super) params: Value,
}

/// The request a conversation runs: started, and answered once [`Self::poll`] says so.
pub(super) struct Running {
    pub(super) id: RequestId,
    pub(super) method: &'static MethodInfo,
    pub(super) started: Instant,
    /// When it answers `timeout`; `None` waits without a deadline.
    pub(super) deadline: Option<Instant>,
    work: Work,
}

enum Work {
    /// Answered when it started.
    Done(Option<Result<Value, ErrorObject>>),
    /// A window method, waiting for the window's answer.
    Window(Box<dyn WindowAnswer>),
    /// A subscription, waiting for a stream of each window.
    Subscribe(Subscribing),
}

impl Running {
    /// Its answer, once the owner gave it or the deadline passed; `None` while it waits, with
    /// `waker` registered to be woken when the owner answers.
    pub(super) fn poll(
        &mut self,
        subscriptions: &mut Subscriptions,
        now: Instant,
        waker: &Waker,
    ) -> Option<Result<Value, ErrorObject>> {
        let answered = match &mut self.work {
            Work::Done(outcome) => outcome.take(),
            Work::Window(answer) => answer.poll(waker),
            Work::Subscribe(subscribing) => subscribing.poll(subscriptions, waker),
        };
        if answered.is_some() {
            return answered;
        }
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            // Dropping the work drops its answers: the owner forgets a request it holds.
            self.work = Work::Done(None);
            return Some(Err(timed_out(self.method)));
        }
        None
    }
}

/// What a timeout answers: retry `soon` for a method that only reads, and the `may_have_run`
/// effect for one that acts, which is still queued.
fn timed_out(method: &MethodInfo) -> ErrorObject {
    if method.effect == MethodEffect::ReadOnly {
        ErrorObject::new(ErrorCode::Timeout, "the window did not answer in time")
            .with_retry(Retry::Soon)
    } else {
        ErrorObject::new(
            ErrorCode::Timeout,
            "the window did not answer in time; the request is still queued",
        )
        .with_retry(Retry::Never)
        .with_effect(Effect::new(EffectKind::MayHaveRun))
    }
}

/// Start `admitted` at `now`.
pub(super) fn start(
    shared: &Shared,
    subscriptions: &mut Subscriptions,
    admitted: Admitted,
    now: Instant,
) -> Running {
    let Admitted { id, method, params } = admitted;
    let (work, holds_for) = match method.name {
        WindowsList::NAME => done(serve::<WindowsList>(params, |_| {
            Ok(WindowList::new(shared.registry.list()))
        })),
        EventsUnsubscribe::NAME => done(serve::<EventsUnsubscribe>(params, |params| {
            subscriptions.remove(params.subscription)
        })),
        EventsSubscribe::NAME => match parse::<EventsSubscribe>(params) {
            Ok(params) => (
                subscribe(shared, subscriptions, params.events),
                Duration::ZERO,
            ),
            Err(error) => done(Err(error)),
        },
        _ => {
            window_route(shared, method, params).unwrap_or_else(|| done(Err(not_supported(method))))
        }
    };
    // A timeout too long to reach is no deadline at all.
    let deadline = now.checked_add(shared.limits.reply_timeout.saturating_add(holds_for));
    Running {
        id,
        method,
        started: now,
        deadline,
        work,
    }
}

fn done(outcome: Result<Value, ErrorObject>) -> (Work, Duration) {
    (Work::Done(Some(outcome)), Duration::ZERO)
}

/// Start a window method: the route generated for each one the table declares.
fn window_route(shared: &Shared, method: &MethodInfo, params: Value) -> Option<(Work, Duration)> {
    macro_rules! routes {
        ($($ty:ident = $name:literal, $effect:ident, $tier:ident, $params:ty => $output:ty;)+) => {
            match method.name {
                $($name => Some(ask::<flui_protocol::method::$ty>(shared, params)),)+
                _ => None,
            }
        };
    }
    flui_protocol::for_each_window_method!(routes)
}

/// Ask the window `params` address `M`: the work that waits for its answer, and how long the
/// window may hold the request past the reply timeout.
fn ask<M: WindowMethod>(shared: &Shared, params: Value) -> (Work, Duration)
where
    M::Params: DeserializeOwned,
    M::Output: Serialize,
{
    let params = match parse::<M>(params) {
        Ok(params) => params,
        Err(error) => return done(Err(error)),
    };
    let window = match params.window() {
        Some(id) => window(&shared.registry, id),
        None => only_window(&shared.registry),
    };
    let window = match window {
        Ok(window) => window,
        Err(error) => return done(Err(error)),
    };
    let holds_for = params.holds_for();
    match window.call::<M>(params) {
        Ok(answer) => (
            Work::Window(Box::new(Typed::<M> {
                answer,
                _method: PhantomData,
            })),
            holds_for,
        ),
        Err(error) => done(Err(error)),
    }
}

/// A window method's answer, as the JSON value the reply carries.
trait WindowAnswer: Send {
    /// The answer once the owner gave it, registering `waker` while it has not.
    fn poll(&mut self, waker: &Waker) -> Option<Result<Value, ErrorObject>>;
}

struct Typed<M: WindowMethod> {
    answer: AgentAnswer<M::Output>,
    _method: PhantomData<fn() -> M>,
}

impl<M: WindowMethod> WindowAnswer for Typed<M>
where
    M::Output: Serialize,
{
    fn poll(&mut self, waker: &Waker) -> Option<Result<Value, ErrorObject>> {
        // Registered before the answer is looked at, so one sent in between still wakes.
        self.answer.register(waker);
        let outcome = self.answer.try_take()?;
        Some(outcome.and_then(encode::<M>))
    }
}

/// `M`'s parameters, from the request's.
fn parse<M: Method>(params: Value) -> Result<M::Params, ErrorObject>
where
    M::Params: DeserializeOwned,
{
    serde_json::from_value::<M::Params>(params).map_err(|error| {
        ErrorObject::new(
            ErrorCode::InvalidArgument,
            format!("{}'s parameters are malformed: {error}", M::NAME),
        )
    })
}

/// `M`'s output, as the reply carries it.
fn encode<M: Method>(output: M::Output) -> Result<Value, ErrorObject>
where
    M::Output: Serialize,
{
    serde_json::to_value(output).map_err(|_| {
        ErrorObject::new(
            ErrorCode::Platform,
            format!("{}'s answer did not serialize", M::NAME),
        )
    })
}

/// Parse `M`'s parameters, run `body`, and write its output.
fn serve<M: Method>(
    params: Value,
    body: impl FnOnce(M::Params) -> Result<M::Output, ErrorObject>,
) -> Result<Value, ErrorObject>
where
    M::Params: DeserializeOwned,
    M::Output: Serialize,
{
    body(parse::<M>(params)?).and_then(encode::<M>)
}

/// The open window `id` names.
fn window(registry: &Registry, id: WindowId) -> Result<AgentWindow, ErrorObject> {
    match registry.lookup(id) {
        Lookup::Open(window) => Ok(window),
        Lookup::Closed => Err(ErrorObject::new(ErrorCode::Gone, "the window has closed")
            .with_retry(Retry::Never)
            .with_handle(HandleKind::Window, id.to_string())),
        Lookup::Unknown => Err(ErrorObject::new(
            ErrorCode::UnknownHandle,
            format!("window {id} was never issued"),
        )
        .with_handle(HandleKind::Window, id.to_string())),
    }
}

/// The app's only window, for a locator that names none.
fn only_window(registry: &Registry) -> Result<AgentWindow, ErrorObject> {
    match registry.list().as_slice() {
        [only] => window(registry, *only),
        [] => Err(
            ErrorObject::new(ErrorCode::NotFound, "the app has no window open")
                .with_retry(Retry::WhenAppears),
        ),
        _ => Err(ErrorObject::new(
            ErrorCode::InvalidArgument,
            "the app has several windows; the locator names none",
        )),
    }
}

/// Start `events.subscribe`: ask a stream of every open window.
fn subscribe(shared: &Shared, subscriptions: &Subscriptions, filters: Vec<EventFilter>) -> Work {
    if subscriptions.live.len() >= shared.limits.subscriptions {
        return Work::Done(Some(Err(ErrorObject::new(
            ErrorCode::Busy,
            format!(
                "a connection holds at most {} subscriptions",
                shared.limits.subscriptions
            ),
        ))));
    }
    let mut asked = Vec::new();
    let mut reached = BTreeSet::new();
    for id in shared.registry.list() {
        reached.insert(id);
        let Lookup::Open(window) = shared.registry.lookup(id) else {
            continue;
        };
        match window.subscribe(filters.clone()) {
            Ok(answer) => asked.push(answer),
            // A window that closed in between has nothing to stream.
            Err(error) if error.code == ErrorCode::Gone => {}
            Err(error) => return Work::Done(Some(Err(error))),
        }
    }
    Work::Subscribe(Subscribing {
        filters,
        asked,
        streams: Vec::new(),
        reached,
    })
}

/// A subscription waiting for a stream of each window it asked.
struct Subscribing {
    filters: Vec<EventFilter>,
    asked: Vec<AgentAnswer<AgentEvents>>,
    streams: Vec<AgentEvents>,
    reached: BTreeSet<WindowId>,
}

impl Subscribing {
    /// The subscription's number, registered on `subscriptions`, once every window answered;
    /// the first refusal other than `gone`.
    fn poll(
        &mut self,
        subscriptions: &mut Subscriptions,
        waker: &Waker,
    ) -> Option<Result<Value, ErrorObject>> {
        let mut waiting = Vec::with_capacity(self.asked.len());
        for mut answer in self.asked.drain(..) {
            answer.register(waker);
            match answer.try_take() {
                Some(Ok(stream)) => self.streams.push(stream),
                // A window that closed in between has nothing to stream.
                Some(Err(error)) if error.code == ErrorCode::Gone => {}
                Some(Err(error)) => return Some(Err(error)),
                None => waiting.push(answer),
            }
        }
        self.asked = waiting;
        if !self.asked.is_empty() {
            return None;
        }
        let number = subscriptions.insert(Subscribed {
            filters: std::mem::take(&mut self.filters),
            streams: std::mem::take(&mut self.streams),
            asked: Vec::new(),
            reached: std::mem::take(&mut self.reached),
        });
        Some(encode::<EventsSubscribe>(Subscription::new(number)))
    }
}

/// What one drain of a conversation's subscriptions found.
pub(super) struct Drained {
    /// The events the streams held, oldest first per stream.
    pub(super) events: Vec<Event>,
    /// The windows a subscription could not reach for good, and why: traced by the caller.
    pub(super) refused: Vec<(WindowId, ErrorCode)>,
    /// The drain stopped at its room with events still queued.
    pub(super) held: bool,
    /// A window was too busy to give a stream: it is asked again at a later drain.
    pub(super) retry: bool,
}

/// One conversation's event subscriptions.
#[derive(Default)]
pub(super) struct Subscriptions {
    next: u64,
    live: BTreeMap<u64, Subscribed>,
}

/// One subscription: its filters, and a stream per window it reaches, windows opened after it
/// was made included.
struct Subscribed {
    filters: Vec<EventFilter>,
    streams: Vec<AgentEvents>,
    /// Streams asked of windows opened since, not answered yet.
    asked: Vec<(WindowId, AgentAnswer<AgentEvents>)>,
    /// Every window a stream was asked of, or that has none to give.
    reached: BTreeSet<WindowId>,
}

impl Subscriptions {
    fn insert(&mut self, subscribed: Subscribed) -> u64 {
        self.next += 1;
        self.live.insert(self.next, subscribed);
        self.next
    }

    fn remove(&mut self, subscription: u64) -> Result<flui_protocol::session::Empty, ErrorObject> {
        if self.live.remove(&subscription).is_some() {
            Ok(flui_protocol::session::Empty::default())
        } else {
            Err(ErrorObject::new(
                ErrorCode::UnknownHandle,
                format!("subscription {subscription} is not this connection's"),
            )
            .with_handle(HandleKind::Subscription, subscription.to_string()))
        }
    }

    /// Up to `room` events the streams hold now, taken in turn from each stream and oldest
    /// first per stream; a stream whose window closed is let go once it is read out. Each
    /// subscription first asks a stream of every window opened since it was made, and takes the
    /// streams asked earlier that the owners answered. `waker` is registered with every stream
    /// and answer still waited on.
    pub(super) fn drain(&mut self, registry: &Registry, room: usize, waker: &Waker) -> Drained {
        let mut drained = Drained {
            events: Vec::new(),
            refused: Vec::new(),
            held: false,
            retry: false,
        };
        if self.live.is_empty() {
            return drained;
        }
        let windows = registry.list();
        for subscribed in self.live.values_mut() {
            subscribed.reach(&windows, registry, &mut drained, waker);
            for stream in &mut subscribed.streams {
                // Registered before the stream is read, so an event queued in between wakes.
                stream.register(waker);
            }
        }
        let mut taken = true;
        while taken {
            taken = false;
            for stream in self
                .live
                .values_mut()
                .flat_map(|subscribed| subscribed.streams.iter_mut())
            {
                if drained.events.len() >= room {
                    drained.held = true;
                    break;
                }
                if let Some(event) = stream.try_next() {
                    drained.events.push(event);
                    taken = true;
                }
            }
            if drained.held {
                break;
            }
        }
        for subscribed in self.live.values_mut() {
            subscribed.streams.retain(|stream| !stream.is_finished());
        }
        drained
    }
}

impl Subscribed {
    /// Asks a stream of every window not reached yet, and takes the streams the owners
    /// answered. A window whose owner was busy (a full inbox, its subscriptions at their cap)
    /// is asked again at the next drain; one that closed, or refused for good, is not.
    fn reach(
        &mut self,
        windows: &[WindowId],
        registry: &Registry,
        drained: &mut Drained,
        waker: &Waker,
    ) {
        for &id in windows {
            if self.reached.contains(&id) {
                continue;
            }
            let Lookup::Open(window) = registry.lookup(id) else {
                self.reached.insert(id);
                continue;
            };
            match window.subscribe(self.filters.clone()) {
                Ok(answer) => {
                    self.reached.insert(id);
                    self.asked.push((id, answer));
                }
                Err(error) if error.retry == Retry::Soon => drained.retry = true,
                Err(error) => {
                    self.reached.insert(id);
                    drained.refused.push((id, error.code));
                }
            }
        }
        let mut waiting = Vec::with_capacity(self.asked.len());
        for (id, mut answer) in self.asked.drain(..) {
            answer.register(waker);
            match answer.try_take() {
                Some(Ok(stream)) => self.streams.push(stream),
                Some(Err(error)) if error.retry == Retry::Soon => {
                    self.reached.remove(&id);
                    drained.retry = true;
                }
                // The window closed in between, or refused for good.
                Some(Err(error)) => {
                    if error.code != ErrorCode::Gone {
                        drained.refused.push((id, error.code));
                    }
                }
                None => waiting.push((id, answer)),
            }
        }
        self.asked = waiting;
    }
}
