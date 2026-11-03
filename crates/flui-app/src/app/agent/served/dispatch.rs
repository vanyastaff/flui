//! What each method the agent serves does: parse its typed parameters, ask
//! the window's owner, wait a bounded time for the answer, and write the
//! typed output.
//!
//! A window method ([`WindowMethod`]) takes one route, generated from the
//! protocol's table: its parameters name the window it asks, through the
//! seam's one call, and the owner answers with the method's output or the
//! protocol's error object. The app's own methods (`windows.list`,
//! `events.*`) are answered here.

// Every refusal here becomes the reply line itself, written once per request,
// so the error object is returned by value: boxing it would add an allocation
// to each refusal to save a copy of a few hundred bytes.
#![expect(
    clippy::result_large_err,
    reason = "an error object is the reply, returned once per request"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use flui_protocol::method::{
    Addressed, EventsSubscribe, EventsUnsubscribe, Method, WindowMethod, WindowsList,
};
use flui_protocol::{
    Effect, EffectKind, ErrorCode, ErrorObject, HandleKind, METHODS, MethodEffect, MethodInfo,
    Retry, ServedBy, Subscription, WindowId, WindowList,
};
use flui_view::dev_agent::{AgentAnswer, AgentEvents, AgentWindow};
use parking_lot::Mutex;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::endpoint::Shared;
use super::registry::{Lookup, Registry};

/// The longest single wait on an answer before the stop flag is looked at.
const WAIT_SLICE: Duration = Duration::from_millis(50);

/// The methods the app answers itself, not one of its windows.
const APPS_OWN: &[&str] = &[
    EventsSubscribe::NAME,
    EventsUnsubscribe::NAME,
    WindowsList::NAME,
];

/// Whether this agent answers `method`: one of the app's own, or a window
/// method this build's windows answer.
pub(super) fn serves(method: &MethodInfo) -> bool {
    method.served_by == ServedBy::Agent
        && (APPS_OWN.contains(&method.name)
            || flui_runtime::ui_runtime::window_answers(method.name))
}

/// The methods this agent answers, by wire name, in table order.
pub(super) fn served() -> Vec<String> {
    METHODS
        .iter()
        .filter(|method| serves(method))
        .map(|method| method.name.to_owned())
        .collect()
}

/// What one drain of a connection's subscriptions found.
pub(super) struct Drained {
    /// The events the streams held, oldest first per stream.
    pub(super) events: Vec<flui_protocol::Event>,
    /// The windows a subscription could not reach for good, and why: traced
    /// once the subscriptions are let go.
    pub(super) refused: Vec<(WindowId, ErrorCode)>,
}

/// One connection's event subscriptions.
#[derive(Default)]
pub(super) struct Subscriptions {
    next: u64,
    live: BTreeMap<u64, Subscribed>,
}

/// One subscription: its filters, and a stream per window it reaches,
/// windows opened after it was made included.
struct Subscribed {
    filters: Vec<flui_protocol::EventFilter>,
    streams: Vec<AgentEvents>,
    /// Streams asked of windows opened since, not answered yet.
    asked: Vec<(WindowId, AgentAnswer<AgentEvents>)>,
    /// Every window a stream was asked of, or that has none to give.
    reached: BTreeSet<WindowId>,
}

impl Subscriptions {
    /// Every event the streams hold now, oldest first per stream; a stream
    /// whose window closed is let go. Each subscription first asks a stream
    /// of every window opened since it was made, and takes the streams
    /// asked earlier that the owners answered.
    pub(super) fn drain(&mut self, registry: &Registry) -> Drained {
        let windows = if self.live.is_empty() {
            Vec::new()
        } else {
            registry.list()
        };
        let mut events = Vec::new();
        let mut refused = Vec::new();
        for subscribed in self.live.values_mut() {
            subscribed.reach(&windows, registry, &mut refused);
            for stream in &mut subscribed.streams {
                while let Some(event) = stream.try_next() {
                    events.push(event);
                }
            }
            subscribed.streams.retain(|stream| !stream.is_finished());
        }
        Drained { events, refused }
    }
}

impl Subscribed {
    /// Asks a stream of every window not reached yet, and takes the streams
    /// the owners answered. A window whose owner was busy (a full inbox, its
    /// subscriptions at their cap) is asked again at the next drain; one that
    /// closed, or refused for good, is not.
    fn reach(
        &mut self,
        windows: &[WindowId],
        registry: &Registry,
        refused: &mut Vec<(WindowId, ErrorCode)>,
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
                Err(error) if error.retry == Retry::Soon => {}
                Err(error) => {
                    self.reached.insert(id);
                    refused.push((id, error.code));
                }
            }
        }
        let mut waiting = Vec::with_capacity(self.asked.len());
        for (id, mut answer) in self.asked.drain(..) {
            match answer.try_take() {
                Some(Ok(stream)) => self.streams.push(stream),
                Some(Err(error)) if error.retry == Retry::Soon => {
                    self.reached.remove(&id);
                }
                // The window closed in between, or refused for good.
                Some(Err(error)) => {
                    if error.code != ErrorCode::Gone {
                        refused.push((id, error.code));
                    }
                }
                None => waiting.push((id, answer)),
            }
        }
        self.asked = waiting;
    }
}

/// What one connection's requests run with.
pub(super) struct Context<'a> {
    pub(super) shared: &'a Shared,
    pub(super) subscriptions: &'a Mutex<Subscriptions>,
    /// Set once the connection ended: its requests stop waiting.
    pub(super) closed: &'a AtomicBool,
}

/// Run `method` with `params`: its output, or the error object to answer.
/// Run `method` with `params`: its output, or the error object to answer.
pub(super) fn run(
    method: &MethodInfo,
    params: Value,
    cx: &Context<'_>,
) -> Result<Value, ErrorObject> {
    match method.name {
        WindowsList::NAME => {
            serve::<WindowsList>(params, |_| Ok(WindowList::new(cx.shared.registry.list())))
        }
        EventsSubscribe::NAME => {
            serve::<EventsSubscribe>(params, |params| subscribe(cx, method, params.events))
        }
        EventsUnsubscribe::NAME => serve::<EventsUnsubscribe>(params, |subscription| {
            if cx
                .subscriptions
                .lock()
                .live
                .remove(&subscription.subscription)
                .is_some()
            {
                Ok(flui_protocol::session::Empty::default())
            } else {
                Err(ErrorObject::new(
                    ErrorCode::UnknownHandle,
                    format!(
                        "subscription {} is not this connection's",
                        subscription.subscription
                    ),
                )
                .with_handle(
                    HandleKind::Subscription,
                    subscription.subscription.to_string(),
                ))
            }
        }),
        _ => window_route(method, params, cx).unwrap_or_else(|| Err(not_supported(method))),
    }
}

/// Run a window method: the route generated for each one the table declares.
fn window_route(
    method: &MethodInfo,
    params: Value,
    cx: &Context<'_>,
) -> Option<Result<Value, ErrorObject>> {
    macro_rules! routes {
        ($($ty:ident = $name:literal, $effect:ident, $tier:ident, $params:ty => $output:ty;)+) => {
            match method.name {
                $($name => Some(serve::<flui_protocol::method::$ty>(params, |params| {
                    ask::<flui_protocol::method::$ty>(cx, params)
                })),)+
                _ => None,
            }
        };
    }
    flui_protocol::for_each_window_method!(routes)
}

/// `method` is in the table but not answered here: by another side, or not
/// yet by this agent.
pub(super) fn not_supported(method: &MethodInfo) -> ErrorObject {
    let mut error = ErrorObject::new(
        ErrorCode::NotSupported,
        format!("this app's agent does not answer {}", method.name),
    );
    error.capability = Some(method.name.to_owned());
    error
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
    let params = serde_json::from_value::<M::Params>(params).map_err(|error| {
        ErrorObject::new(
            ErrorCode::InvalidArgument,
            format!("{}'s parameters are malformed: {error}", M::NAME),
        )
    })?;
    let output = body(params)?;
    serde_json::to_value(output).map_err(|_| {
        ErrorObject::new(
            ErrorCode::Platform,
            format!("{}'s answer did not serialize", M::NAME),
        )
    })
}

/// Ask the window `params` address `M`, and wait for its answer: within the
/// reply timeout, and as long again as the window may hold the request (a
/// wait's timeout runs on the owner).
fn ask<M: WindowMethod>(cx: &Context<'_>, params: M::Params) -> Result<M::Output, ErrorObject> {
    let window = match params.window() {
        Some(id) => window(cx, id)?,
        None => only_window(cx)?,
    };
    let holds = params.holds_for();
    let method = flui_protocol::MethodInfo::of::<M>();
    settle(cx, &method, holds, window.call::<M>(params))
}

fn subscribe(
    cx: &Context<'_>,
    method: &MethodInfo,
    filters: Vec<flui_protocol::EventFilter>,
) -> Result<Subscription, ErrorObject> {
    if cx.subscriptions.lock().live.len() >= cx.shared.limits.subscriptions {
        return Err(ErrorObject::new(
            ErrorCode::Busy,
            format!(
                "a connection holds at most {} subscriptions",
                cx.shared.limits.subscriptions
            ),
        ));
    }
    let mut streams = Vec::new();
    let mut reached = BTreeSet::new();
    for id in cx.shared.registry.list() {
        reached.insert(id);
        let Lookup::Open(window) = cx.shared.registry.lookup(id) else {
            continue;
        };
        match settle(
            cx,
            method,
            Duration::ZERO,
            window.subscribe(filters.clone()),
        ) {
            Ok(stream) => streams.push(stream),
            // A window that closed in between has nothing to stream.
            Err(error) if error.code == ErrorCode::Gone => {}
            Err(error) => return Err(error),
        }
    }
    let mut subscriptions = cx.subscriptions.lock();
    subscriptions.next += 1;
    let subscription = subscriptions.next;
    // Windows opened from now on are reached as the connection drains.
    subscriptions.live.insert(
        subscription,
        Subscribed {
            filters,
            streams,
            asked: Vec::new(),
            reached,
        },
    );
    Ok(Subscription::new(subscription))
}

/// The open window `id` names.
fn window(cx: &Context<'_>, id: WindowId) -> Result<AgentWindow, ErrorObject> {
    match cx.shared.registry.lookup(id) {
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
fn only_window(cx: &Context<'_>) -> Result<AgentWindow, ErrorObject> {
    match cx.shared.registry.list().as_slice() {
        [only] => window(cx, *only),
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

/// Wait for the owner's answer within the reply timeout and `extra`. A
/// timeout answers `timeout`: retry `soon` for a method that only reads, and
/// the `may_have_run` effect for one that acts, which is still queued.
fn settle<T>(
    cx: &Context<'_>,
    method: &MethodInfo,
    extra: Duration,
    asked: Result<AgentAnswer<T>, ErrorObject>,
) -> Result<T, ErrorObject> {
    let answer = asked?;
    let timeout = cx.shared.limits.reply_timeout.saturating_add(extra);
    match wait(answer, timeout, cx) {
        Some(answer) => answer,
        None if method.effect == MethodEffect::ReadOnly => Err(ErrorObject::new(
            ErrorCode::Timeout,
            "the window did not answer in time",
        )
        .with_retry(Retry::Soon)),
        None => Err(ErrorObject::new(
            ErrorCode::Timeout,
            "the window did not answer in time; the request is still queued",
        )
        .with_retry(Retry::Never)
        .with_effect(Effect::new(EffectKind::MayHaveRun))),
    }
}

/// Waits for the owner's answer, up to `timeout`, looking at the stop flag
/// and the connection between slices. `None` on a timeout, a stop or a
/// connection that ended: dropping the answer lets the owner forget a wait.
fn wait<T>(
    mut answer: AgentAnswer<T>,
    timeout: Duration,
    cx: &Context<'_>,
) -> Option<Result<T, ErrorObject>> {
    let deadline = Instant::now().checked_add(timeout);
    loop {
        let left = deadline.map_or(Duration::MAX, |deadline| {
            deadline.saturating_duration_since(Instant::now())
        });
        if let Some(result) = answer.recv_timeout(left.min(WAIT_SLICE)) {
            return Some(result);
        }
        if left <= WAIT_SLICE
            || cx.shared.stop.load(Ordering::SeqCst)
            || cx.closed.load(Ordering::SeqCst)
        {
            return None;
        }
    }
}
