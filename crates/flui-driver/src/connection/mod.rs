//! The conversation with the app's agent (ADR-0238 §2, ADR-0288): a hello, then one request at a
//! time, each answered by the line that carries its id, with the events the app sends between
//! answers kept aside.
//!
//! A [`Transport`] carries that conversation: a local socket to a launched or attached
//! app ([`socket`]), a web app's page export reached through the Chrome lane's BiDi session
//! (`bidi`, feature `chrome`), or the headless target's agent served in process (`in_process`,
//! feature `headless`). It is private: front ends see the `Driver`.

#[cfg(feature = "chrome")]
pub(crate) mod bidi;
#[cfg(feature = "headless")]
pub(crate) mod in_process;
#[cfg(any(unix, windows))]
pub(crate) mod socket;

use std::collections::VecDeque;
use std::time::Duration;

use flui_protocol::act::{Effect, EffectKind};
use flui_protocol::envelope::{Greeting, Request, RequestId, ServerLine};
use flui_protocol::error::Retry;
use flui_protocol::events::Event;
use flui_protocol::method::MethodEffect;
use flui_protocol::session::{Hello, HelloReply};
use flui_protocol::version::PROTOCOL_VERSION;
use serde_json::Value;

use crate::error::{Error, Result};

/// How the driver names itself in its hello.
pub(crate) const CLIENT: &str = concat!("flui-driver/", env!("CARGO_PKG_VERSION"));

/// The longest line read from an app. The agent bounds its reads with budgets; a line past this
/// is not one of its answers.
pub(crate) const MAX_REPLY: usize = 16 << 20;

/// The longest a call waits for its answer: a wait's longest timeout (a day) and a margin, so a
/// budget that saturated (an input sequence of endless pauses) still names an instant.
pub(crate) const MAX_BUDGET: Duration = Duration::from_mins(24 * 60 + 1);

/// How many events wait for the caller before the oldest are dropped and counted.
const EVENT_ROOM: usize = 1024;

/// One request as a transport sends it.
#[derive(Debug)]
pub(crate) struct Call {
    /// The wire name.
    pub(crate) method: &'static str,
    /// What the method can change: an acting call that gets no answer is reported
    /// `may_have_run`, never resent.
    pub(crate) effect: MethodEffect,
    /// Its parameters.
    pub(crate) params: Value,
    /// How long the answer may take: the agent's reply bound, and as long again as the app may
    /// hold the request (a wait's timeout). The in-process target counts it on its virtual clock.
    pub(crate) budget: Duration,
}

/// A transport of the conversation, beneath the `Driver`'s typed calls.
pub(crate) trait Transport: Send {
    /// What the app answered the hello with.
    fn hello(&self) -> &HelloReply;

    /// Send `call` and wait for its answer within its budget.
    ///
    /// # Errors
    ///
    /// The app's error, read into its variant; [`Error::Timeout`] when no answer came within the
    /// budget, `may_have_run` for an acting call; [`Error::Gone`] of the process once the app went,
    /// for this call and every later one.
    fn call(&mut self, call: Call) -> Result<Value>;

    /// The events the app sent since the last take, oldest first, and how many were dropped
    /// because nobody took them.
    fn take_events(&mut self) -> (Vec<Event>, u64);

    /// How long the connection has been open on the clock its budgets are counted in: the wall
    /// clock over a socket, the app's virtual clock on the headless target.
    fn clock(&self) -> Duration;
}

/// The hello line presenting `token`, as the app's driver.
pub(crate) fn hello_line(token: &str) -> Result<Vec<u8>> {
    let hello = Hello::new(token, PROTOCOL_VERSION).client(CLIENT);
    line(&Greeting::new(hello))
}

/// `value` as one line, its newline included.
pub(crate) fn line(value: &impl serde::Serialize) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(value).map_err(|error| Error::Protocol {
        message: format!("a request did not serialize: {error}"),
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Request `id`'s line.
pub(crate) fn request_line(id: u64, call: &Call) -> Result<Vec<u8>> {
    line(&Request::new(
        RequestId::Number(id),
        call.method,
        &call.params,
    ))
}

/// What one line from the app means to a caller waiting for request `waiting`.
pub(crate) enum Heard {
    /// The answer to the hello.
    Hello(Box<HelloReply>),
    /// The answer to the request waited for.
    Answer(Result<Value>),
    /// An event.
    Event(Event),
    /// A late answer to an earlier request, or a line of a newer app's this driver does not know.
    Other,
}

/// Reads `bytes`, one line without its newline, for a caller waiting for request `waiting`.
pub(crate) fn hear(bytes: &[u8], waiting: Option<u64>) -> Result<Heard> {
    let line =
        serde_json::from_slice::<ServerLine<Value>>(bytes).map_err(|error| Error::Protocol {
            message: format!("the app sent a line that is not one of the protocol's: {error}"),
        })?;
    let waited = |id: &RequestId| waiting.is_some_and(|waiting| *id == RequestId::Number(waiting));
    Ok(match line {
        ServerLine::Hello { hello } => Heard::Hello(hello),
        ServerLine::Event(event) => Heard::Event(event),
        ServerLine::Answered { id, result } if waited(&id) => Heard::Answer(Ok(result)),
        ServerLine::Failed {
            id: Some(id),
            error,
        } if waited(&id) => Heard::Answer(Err(Error::from_wire(*error))),
        // A refusal that answers no request: of the hello, or of a line that was not a request,
        // which while a request is out is that request.
        ServerLine::Failed { id: None, error } => Heard::Answer(Err(Error::from_wire(*error))),
        _ => Heard::Other,
    })
}

/// What a call that got no answer within its budget fails with: a call that changes the app may
/// have run and is never resent; a read or an annotation is asked again (ADR-0395).
pub(crate) fn unanswered(call: &Call) -> Error {
    let effect = call.effect.changes_app().then(|| {
        Effect::new(EffectKind::MayHaveRun)
            .detail("the request was accepted and is still queued; read the tree for its effect")
    });
    // A read or an annotation can be asked again; an act is never resent as it stands.
    let retry = if effect.is_some() {
        Retry::Never
    } else {
        Retry::Soon
    };
    Error::Timeout {
        effect,
        message: format!(
            "`{}` had no answer within {} ms",
            call.method,
            call.budget.as_millis()
        ),
        retry,
    }
}

/// Events waiting for the caller, the oldest dropped past [`EVENT_ROOM`].
#[derive(Debug, Default)]
pub(crate) struct Events {
    queue: VecDeque<Event>,
    dropped: u64,
}

impl Events {
    pub(crate) fn push(&mut self, event: Event) {
        if self.queue.len() >= EVENT_ROOM {
            self.queue.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
        self.queue.push_back(event);
    }

    /// Count `dropped` events lost before they reached this queue.
    pub(crate) fn count_dropped(&mut self, dropped: u64) {
        self.dropped = self.dropped.saturating_add(dropped);
    }

    pub(crate) fn take(&mut self) -> (Vec<Event>, u64) {
        (
            self.queue.drain(..).collect(),
            std::mem::take(&mut self.dropped),
        )
    }
}

#[cfg(test)]
mod tests {
    use flui_protocol::method::{BuildReport, Method as _, MethodEffect};

    use super::*;

    /// A call that timed out may be asked again exactly when it leaves the app as it was: a read
    /// or an annotation (ADR-0395), never an act.
    #[test]
    fn a_timed_out_call_is_resent_only_when_it_leaves_the_app_as_it_was() {
        for effect in MethodEffect::ALL.iter().copied() {
            let call = Call {
                method: BuildReport::NAME,
                effect,
                params: Value::Null,
                budget: Duration::from_millis(5),
            };
            let Error::Timeout { effect: ran, .. } = unanswered(&call) else {
                panic!("a timeout answers Error::Timeout");
            };
            assert_eq!(ran.is_some(), effect.changes_app(), "{effect:?}");
        }
    }
}
