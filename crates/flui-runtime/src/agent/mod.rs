//! The development agent's service (ADR-0288): the conversation a driver holds with the app,
//! written once for every transport.
//!
//! A [`Service`] owns what every connection shares: the windows the host handed it, the token a
//! driver presents, the [`Limits`], the app's introduction and the driver's seat. A [`Session`]
//! is one conversation, and it does no I/O: a transport hands it the bytes it read and the
//! current instant, writes the lines it [transmits](Session::transmit), and
//! [polls](Session::poll) it again when the waker it passed is woken or the
//! [deadline](Session::next_deadline) passes. A session never reads, writes, sleeps, spawns a
//! thread or reads a clock, so the same conversation runs over a socket's threads, a page's event
//! loop or a test's pump, and every limit below holds on each of them.
//!
//! The service is always compiled. An app that never starts an agent creates none, so it pays
//! for the code only: no thread, no socket, no allocation.
//!
//! # The conversation
//!
//! Protocol [`PROTOCOL_VERSION`](flui_protocol::version::PROTOCOL_VERSION) (ADR-0238):
//! newline-delimited JSON lines of at most [`MAX_LINE`](flui_protocol::framing::MAX_LINE) bytes.
//!
//! ```text
//! → {"hello":{"token":"…","protocol":"0.9","client":"flui-driver/0.1.0"}}
//! ← {"hello":{"protocol":"0.9","app":{…},"capabilities":{…}}}
//! → {"id":7,"method":"semantics.locate","params":{…}}
//! ← {"id":7,"result":{…}}  |  {"id":7,"error":{"code":…,"message":…,"retry":…}}
//! ← {"event":"frame","params":{…}}   (after events.subscribe)
//! ```
//!
//! A wrong token, a first line that is not a hello, or no hello within the handshake timeout
//! closes the conversation without a word; a hello in a protocol the app cannot serve is answered
//! `version` and closed. Requests run in order, one at a time, so an act followed by a read reads
//! the act's effect. A conversation holds at most [`Limits::queue`] requests, those answered and
//! not yet transmitted included, and one beyond that is answered `busy` at once. An answer the
//! window has not given within the reply timeout, and as long again as the window may hold the
//! request (a wait's timeout), answers `timeout`: with retry `soon` for a read-only method, and
//! with the `may_have_run` effect for one that acts, which is still queued and runs at the
//! owner's next drain.
//!
//! Events of the conversation's subscriptions are written between answers, a bounded number at a
//! time; a stream that is not read keeps its own events, and counts the ones it has no room for.
//!
//! What the service traces carries the method, window and element ids, error codes and timings,
//! never a label, a value or a request line. Every trace is contained: a subscriber is user code.
//!
//! # The driver
//!
//! A hello names its role (ADR-0288 §8). One conversation drives the app at a time: a hello that
//! asks to drive, as one naming no role does, takes the driver's seat, and while another holds
//! it the hello is answered `not_driver`, retry `soon`, and closed. An observer
//! (`"role":"observer"`) reads and subscribes beside the driver; a call of its that acts is
//! answered `not_driver`, and its hello lists only the methods it may call.
//!
//! When the driver's conversation ends (its client closed it, broke it, or it was closed for a
//! fault of the client's), the seat is free for the next driver. A service given an
//! `on_driver_gone` obligation runs it instead, once and contained, unless the service stopped:
//! the app is ending, so a later driver's hello is answered `shutting_down`.

mod dispatch;
mod registry;
mod service;
mod session;

pub use self::service::{Limits, Refused, Service, ServiceConfig};
pub use self::session::{Session, Status};

/// Emits what `event` traces, contained: a subscriber is user code, and one that panics must not
/// take down the transport the conversation runs on, nor the work it accepted.
fn trace(event: impl FnOnce()) {
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: a tracing subscriber's panic must not stop the transport"
    )]
    let traced = std::panic::catch_unwind(std::panic::AssertUnwindSafe(event));
    if let Err(payload) = traced {
        flui_foundation::panic::discard_payload(payload);
    }
}
