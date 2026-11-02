//! What each method the agent serves does: parse its typed parameters, ask
//! the window's owner, wait a bounded time for the answer, and write the
//! typed output.

// Every refusal here becomes the reply line itself, written once per request,
// so the error object is returned by value: boxing it would add an allocation
// to each refusal to save a copy of a few hundred bytes.
#![expect(
    clippy::result_large_err,
    reason = "an error object is the reply, returned once per request"
)]

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use flui_protocol::method::{
    ElementActionability, EventsSubscribe, EventsUnsubscribe, InspectLayers, InspectRender,
    InspectWidgets, Method, MotionSet, SemanticsAct, SemanticsAnnouncements, SemanticsLocate,
    SemanticsRead, WaitUntil, WindowsList,
};
use flui_protocol::{
    ActParams, ActionRequest, Effect, EffectKind, ErrorCode, ErrorObject, HandleKind, Locator,
    MethodEffect, MethodInfo, Pick, Retry, Subscription, Target, WindowId, WindowList,
};
use flui_view::dev_agent::{AgentAnswer, AgentEvents, AgentFault, AgentWindow, InspectTarget};
use parking_lot::Mutex;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::endpoint::Shared;
use super::registry::Lookup;

/// The longest single wait on an answer before the stop flag is looked at.
const WAIT_SLICE: Duration = Duration::from_millis(50);

/// The methods this agent answers, by wire name, in table order.
pub(super) const SERVED: &[&str] = &[
    ElementActionability::NAME,
    EventsSubscribe::NAME,
    EventsUnsubscribe::NAME,
    InspectLayers::NAME,
    InspectRender::NAME,
    InspectWidgets::NAME,
    MotionSet::NAME,
    SemanticsAct::NAME,
    SemanticsAnnouncements::NAME,
    SemanticsLocate::NAME,
    SemanticsRead::NAME,
    WaitUntil::NAME,
    WindowsList::NAME,
];

/// One connection's event subscriptions: each a stream per window it was
/// made on.
#[derive(Default)]
pub(super) struct Subscriptions {
    next: u64,
    live: BTreeMap<u64, Vec<AgentEvents>>,
}

impl Subscriptions {
    /// Every event the streams hold now, oldest first per stream; a stream
    /// whose window closed is let go.
    pub(super) fn drain(&mut self) -> Vec<flui_protocol::Event> {
        let mut events = Vec::new();
        for streams in self.live.values_mut() {
            for stream in streams.iter_mut() {
                while let Some(event) = stream.try_next() {
                    events.push(event);
                }
            }
            streams.retain(|stream| !stream.is_finished());
        }
        events
    }
}

/// What one connection's requests run with.
pub(super) struct Context<'a> {
    pub(super) shared: &'a Shared,
    pub(super) subscriptions: &'a Mutex<Subscriptions>,
}

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
        SemanticsRead::NAME => serve::<SemanticsRead>(params, |params| {
            let window = window(cx, params.window)?;
            let root = params.query.root;
            settle(cx, method, window.read(params.query), |fault| {
                named(fault, params.window, root)
            })
        }),
        SemanticsAct::NAME => serve::<SemanticsAct>(params, |params| act(cx, method, params)),
        SemanticsLocate::NAME => serve::<SemanticsLocate>(params, |params| {
            let window = locator_window(cx, &params.locator)?;
            let id = window.id();
            settle(cx, method, window.locate(params), |fault| {
                named(fault, id, None)
            })
        }),
        ElementActionability::NAME => serve::<ElementActionability>(params, |params| {
            let element = params.element;
            let window = window(cx, element.window)?;
            settle(cx, method, window.actionability(params), |fault| {
                named(fault, element.window, Some(element.element))
            })
        }),
        WaitUntil::NAME => serve::<WaitUntil>(params, |params| {
            let id = params.window;
            let window = window(cx, id)?;
            // The wait's own timeout runs on the owner; the reply waits that
            // long and the usual bound on top.
            let extra = Duration::from_millis(params.timeout_ms.unwrap_or(0));
            settle_within(cx, method, extra, window.wait(params), |fault| {
                named(fault, id, None)
            })
        }),
        InspectWidgets::NAME => serve::<InspectWidgets>(params, |params| {
            inspect(cx, method, InspectTarget::Widgets, params)
        }),
        InspectRender::NAME => serve::<InspectRender>(params, |params| {
            inspect(cx, method, InspectTarget::Render, params)
        }),
        InspectLayers::NAME => serve::<InspectLayers>(params, |params| {
            inspect(cx, method, InspectTarget::Layers, params)
        }),
        MotionSet::NAME => serve::<MotionSet>(params, |params| {
            let id = params.window;
            let window = window(cx, id)?;
            settle(cx, method, window.motion(params.request), |fault| {
                named(fault, id, None)
            })
        }),
        SemanticsAnnouncements::NAME => serve::<SemanticsAnnouncements>(params, |params| {
            let id = params.window;
            let window = window(cx, id)?;
            settle(cx, method, window.announcements(params), |fault| {
                named(fault, id, None)
            })
        }),
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
                ))
            }
        }),
        _ => Err(not_supported(method)),
    }
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

fn act(
    cx: &Context<'_>,
    method: &MethodInfo,
    params: ActParams,
) -> Result<flui_protocol::ActOutcome, ErrorObject> {
    let element = match params.target {
        Target::Element(element) => element,
        Target::Locator(locator) => resolve(cx, &locator)?,
        Target::Point { .. } => {
            let mut error = ErrorObject::new(
                ErrorCode::NotSupported,
                "a semantics action takes an element or a locator; a point takes input",
            );
            error.capability = Some("input.inject".to_owned());
            return Err(error);
        }
        _ => {
            return Err(ErrorObject::new(
                ErrorCode::InvalidArgument,
                "this app's agent does not know that kind of target",
            ));
        }
    };
    let window = window(cx, element.window)?;
    let mut request = ActionRequest::new(element.element, params.action);
    request.value = params.value;
    settle(
        cx,
        method,
        window.act_and_read(request, params.snapshot),
        |fault| named(fault, element.window, Some(element.element)),
    )
}

/// The one element `locator` resolves to now, as its pick says.
fn resolve(cx: &Context<'_>, locator: &Locator) -> Result<flui_protocol::ElementRef, ErrorObject> {
    let window = locator_window(cx, locator)?;
    let id = window.id();
    let strict = locator.pick == Pick::Strict;
    let locate = flui_protocol::method::SemanticsLocate::NAME;
    let method =
        flui_protocol::method::find(locate).expect("BUG: semantics.locate is in the table");
    let mut params = flui_protocol::LocateParams::new(locator.clone());
    if strict {
        // Two are enough to know the locator is ambiguous; ten name the
        // candidates.
        params.max_matches = Some(10);
    }
    let matches = settle(cx, method, window.locate(params), |fault| {
        named(fault, id, None)
    })?;
    match matches.matches.as_slice() {
        [] => Err(
            ErrorObject::new(ErrorCode::NotFound, "the locator matches no element now")
                .with_retry(Retry::WhenAppears),
        ),
        [only] => Ok(only.element),
        several if strict => Err(ErrorObject::ambiguous(
            format!("the locator matches {} elements", several.len()),
            several
                .iter()
                .map(|candidate| {
                    flui_protocol::tree::outline(std::slice::from_ref(&candidate.node))
                })
                .collect(),
        )),
        [first, ..] => Ok(first.element),
    }
}

fn inspect(
    cx: &Context<'_>,
    method: &MethodInfo,
    target: InspectTarget,
    params: flui_protocol::InspectParams,
) -> Result<flui_protocol::InspectReading, ErrorObject> {
    let id = params.window;
    let window = window(cx, id)?;
    settle(cx, method, window.inspect(target, params), |fault| {
        named(fault, id, None)
    })
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
    for id in cx.shared.registry.list() {
        let Lookup::Open(window) = cx.shared.registry.lookup(id) else {
            continue;
        };
        match settle(cx, method, window.subscribe(filters.clone()), |fault| {
            named(fault, id, None)
        }) {
            Ok(stream) => streams.push(stream),
            // A window that closed in between has nothing to stream.
            Err(error) if error.code == ErrorCode::Gone => {}
            Err(error) => return Err(error),
        }
    }
    let mut subscriptions = cx.subscriptions.lock();
    subscriptions.next += 1;
    let subscription = subscriptions.next;
    subscriptions.live.insert(subscription, streams);
    Ok(Subscription::new(subscription))
}

/// The open window `id` names.
fn window(cx: &Context<'_>, id: WindowId) -> Result<AgentWindow, ErrorObject> {
    match cx.shared.registry.lookup(id) {
        Lookup::Open(window) => Ok(window),
        Lookup::Closed => Err(fault(&AgentFault::window_gone(), Some(id.to_string()))),
        Lookup::Unknown => Err(ErrorObject::new(
            ErrorCode::UnknownHandle,
            format!("window {id} was never issued"),
        )
        .with_handle(HandleKind::Window, id.to_string())),
    }
}

/// The window a locator searches: the one it names, or the app's only
/// window.
fn locator_window(cx: &Context<'_>, locator: &Locator) -> Result<AgentWindow, ErrorObject> {
    if let Some(id) = locator.window {
        return window(cx, id);
    }
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

/// Wait for the owner's answer within the reply timeout.
fn settle<T>(
    cx: &Context<'_>,
    method: &MethodInfo,
    asked: Result<AgentAnswer<T>, AgentFault>,
    name: impl Fn(AgentFault) -> (AgentFault, Option<String>),
) -> Result<T, ErrorObject> {
    settle_within(cx, method, Duration::ZERO, asked, name)
}

/// Wait for the owner's answer within the reply timeout and `extra`. A
/// timeout answers `timeout`: retry `soon` for a method that only reads, and
/// the `may_have_run` effect for one that acts, which is still queued.
fn settle_within<T>(
    cx: &Context<'_>,
    method: &MethodInfo,
    extra: Duration,
    asked: Result<AgentAnswer<T>, AgentFault>,
    name: impl Fn(AgentFault) -> (AgentFault, Option<String>),
) -> Result<T, ErrorObject> {
    let fail = |agent_fault: AgentFault| {
        let (agent_fault, handle) = name(agent_fault);
        fault(&agent_fault, handle)
    };
    let answer = asked.map_err(fail)?;
    let timeout = cx.shared.limits.reply_timeout.saturating_add(extra);
    match wait(answer, timeout, cx.shared) {
        Some(Ok(value)) => Ok(value),
        Some(Err(agent_fault)) => Err(fail(agent_fault)),
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
/// between slices. `None` on a timeout or a stop.
fn wait<T>(
    mut answer: AgentAnswer<T>,
    timeout: Duration,
    shared: &Shared,
) -> Option<Result<T, AgentFault>> {
    let deadline = Instant::now().checked_add(timeout);
    loop {
        let left = deadline.map_or(Duration::MAX, |deadline| {
            deadline.saturating_duration_since(Instant::now())
        });
        if let Some(result) = answer.recv_timeout(left.min(WAIT_SLICE)) {
            return Some(result);
        }
        if left <= WAIT_SLICE || shared.stop.load(std::sync::atomic::Ordering::SeqCst) {
            return None;
        }
    }
}

/// Pairs `fault` with the handle its kind names.
fn named(
    fault: AgentFault,
    window: WindowId,
    element: Option<flui_protocol::ElementId>,
) -> (AgentFault, Option<String>) {
    let handle = match fault.kind() {
        Some(flui_view::dev_agent::HandleKind::Window) => Some(window.to_string()),
        Some(flui_view::dev_agent::HandleKind::Element) => {
            element.map(|element| element.to_string())
        }
        _ => None,
    };
    (fault, handle)
}

/// ADR-0080's error object for an owner's fault.
pub(super) fn fault(fault: &AgentFault, handle: Option<String>) -> ErrorObject {
    let mut error = ErrorObject::new(fault.code(), fault.message()).with_retry(fault.retry());
    if let (Some(kind), Some(handle)) = (fault.kind(), handle) {
        let kind = match kind {
            flui_view::dev_agent::HandleKind::Element => HandleKind::Element,
            flui_view::dev_agent::HandleKind::Window | _ => HandleKind::Window,
        };
        error = error.with_handle(kind, handle);
    }
    if fault.may_have_run() {
        error = error.with_effect(Effect::new(EffectKind::MayHaveRun));
    }
    error
}
