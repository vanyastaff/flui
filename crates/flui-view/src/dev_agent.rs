//! The development-agent seam (ADR-0095 §3).
//!
//! A development tool that serves the application's windows to an agent —
//! `flui-app`'s driver agent (ADR-0239) — implements [`DevAgentHook`]; the
//! host or the application installs the instance on its configuration
//! (`flui_app::AppConfig::with_dev_agent`), and the host hands it an
//! [`AgentWindow`] for each window that has content to read. The trait lives
//! here, below the runtime, so the host never names the tool and the tool
//! never names the runtime: `flui-app` has no edge to any devtools crate, and
//! a package reaches this module through `flui-sdk`
//! (`flui_sdk::view::dev_agent`).
//!
//! An [`AgentWindow`] answers the protocol's window methods
//! ([`WindowMethod`]): [`AgentWindow::call`] takes a method's typed
//! parameters and answers its typed output, or the protocol's
//! [`ErrorObject`], so a tool adds nothing per method.
//!
//! # What the host promises
//!
//! | Host | `attach` / `detach` | `window_opened` |
//! |------|---------------------|-----------------|
//! | desktop, iOS, Android | yes | each window that mounts a root view |
//! | web | attach only: the page's loop never ends | the page's window |
//!
//! - The event loop that starts first with the hook's configuration takes the
//!   hook and owns it until it ends (ADR-0288 §7). [`DevAgentHook::attach`]
//!   runs once, on that loop's owner thread, before its first window opens.
//!   When it answers that it serves, [`DevAgentHook::detach`] runs once when
//!   that loop ends and the hook is then dropped; when it answers that it
//!   does not, it is dropped at once and nothing else is called.
//! - [`DevAgentHook::window_opened`] runs on the owner thread once a window's
//!   UI runtime is installed, and only between `attach` and `detach`. A window
//!   whose installation fails is never handed over.
//! - A hook that panics in any method is dropped and never called again;
//!   the window being opened opens without it.
//!
//! There is no close notification: an [`AgentWindow`] holds its window
//! weakly, so once the window closes every call answers `gone` with kind
//! `window`, and [`AgentWindow::is_open`] turns `false`.
//!
//! While any clone of a window's [`AgentWindow`] is alive, that window
//! collects semantics on every frame, as it does while assistive technology
//! is on; once the hook has dropped every clone (at detach, or with the hook
//! itself when it panics), collection stops at the next frame. A hook that
//! does not serve answers so from `attach` and is handed no window, so it
//! costs the application nothing.
//!
//! # What a hook promises
//!
//! - It never waits on an [`AgentAnswer`] inside a hook method, and never on
//!   the owner thread at all: the owner fills an answer only at its next
//!   drain, a frame boundary, so waiting there waits forever. Serve agents
//!   from threads of the hook's own.
//! - It keeps what it logs free of labels and values: a read returns the
//!   application's text.
//! - It drops every [`AgentWindow`] at `detach`, and keeps none it does not
//!   serve.

use std::any::Any;
use std::fmt;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Weak};
use std::task::Waker;
use std::time::Duration;

use flui_protocol::RecordRow;
use flui_protocol::input_recording::Header;
use flui_protocol::method::{Addressed, WindowMethod};
use flui_protocol::{
    ErrorCode, ErrorObject, Event, EventFilter, HandleKind, RecordParams, Retry, WindowId,
};

use crate::__runtime::{AgentPort, EventSource, LeaseId, RecordSource, WakeSlot, WindowCall};

/// A development tool that serves the application's windows to an agent,
/// installed by the application and called by the host on the owner thread.
/// See the [module docs](self) for the call order.
///
/// `Send` because the instance travels inside the application's
/// configuration, which is `Send`; the host still calls it only on the owner
/// thread, so an implementation needs no `Sync` and no locking of its own
/// for the calls themselves.
///
/// # Example
///
/// ```
/// use flui_view::dev_agent::{AgentWindow, DevAgentHook};
///
/// /// Keeps every window it is handed, for a server thread to read.
/// #[derive(Default)]
/// struct Windows(Vec<AgentWindow>);
///
/// impl DevAgentHook for Windows {
///     fn attach(&mut self) -> bool {
///         true
///     }
///
///     fn detach(&mut self) {
///         self.0.clear();
///     }
///
///     fn window_opened(&mut self, window: AgentWindow) {
///         self.0.push(window);
///     }
/// }
/// ```
pub trait DevAgentHook: Send + 'static {
    /// Once, when the event loop that took the hook starts, before its first
    /// window: start serving (bind an endpoint, start the threads that answer
    /// agents), and answer whether it serves.
    ///
    /// `false` (the endpoint could not be bound, the build or the
    /// credentials rule serving out) leaves the hook unattached: it is
    /// dropped, handed no window and not detached, so the application
    /// collects no semantics for it.
    #[must_use = "the host attaches the hook only when it serves"]
    fn attach(&mut self) -> bool;

    /// The loop is ending: stop serving and let go of every window. The
    /// host pairs an `attach` that answered `true` with one `detach`, then
    /// drops the hook.
    fn detach(&mut self) {}

    /// A window with content to read has opened.
    fn window_opened(&mut self, window: AgentWindow);
}

/// The answer to one call on an [`AgentWindow`], filled by the window's
/// owner at its next drain, or later for a request the window holds (a wait,
/// an action's reading, a read of the next scene's layers).
///
/// Dropping it tells the owner nobody takes the answer: a request the window
/// holds is let go at its next look.
///
/// Never wait on it on the owner thread, or inside a [`DevAgentHook`] method:
/// the owner answers only at its drain.
#[must_use = "an answer carries the result of the call"]
pub struct AgentAnswer<T> {
    rx: Receiver<Result<T, ErrorObject>>,
    /// Held while someone can take the answer; the owner's reply holds it
    /// weakly.
    _asker: Arc<()>,
    taken: bool,
    /// Woken when the answer can be taken.
    wake: Arc<WakeSlot>,
}

impl<T> AgentAnswer<T> {
    pub(crate) fn new(
        rx: Receiver<Result<T, ErrorObject>>,
        asker: Arc<()>,
        wake: Arc<WakeSlot>,
    ) -> Self {
        Self {
            rx,
            _asker: asker,
            taken: false,
            wake,
        }
    }

    /// Wake `waker` once the answer can be taken: when the owner sends it,
    /// or when the owner goes without answering. Only the waker registered
    /// last is woken, once; register again for another wake. Take the
    /// answer after registering, so one that arrived first is not missed.
    pub fn register(&self, waker: &Waker) {
        self.wake.register(waker);
    }

    /// The answer, if the owner has sent it; `None` while it has not, and
    /// after the answer has been taken once.
    ///
    /// An owner that went without answering answers `shutting_down`.
    pub fn try_take(&mut self) -> Option<Result<T, ErrorObject>> {
        if self.taken {
            return None;
        }
        let answer = match self.rx.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err(owner_gone()),
        };
        self.taken = true;
        Some(answer)
    }

    /// The answer, waiting up to `timeout` for it; `None` if it has not come
    /// by then, and after the answer has been taken once.
    ///
    /// An owner that went without answering answers `shutting_down`.
    pub fn recv_timeout(&mut self, timeout: Duration) -> Option<Result<T, ErrorObject>> {
        if self.taken {
            return None;
        }
        let answer = match self.rx.recv_timeout(timeout) {
            Ok(answer) => answer,
            Err(RecvTimeoutError::Timeout) => return None,
            Err(RecvTimeoutError::Disconnected) => Err(owner_gone()),
        };
        self.taken = true;
        Some(answer)
    }
}

impl<T> fmt::Debug for AgentAnswer<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentAnswer")
            .field("taken", &self.taken)
            .finish_non_exhaustive()
    }
}

/// What an answer is when the owner dropped the request unanswered: it shut
/// down, or its UI runtime went.
fn owner_gone() -> ErrorObject {
    ErrorObject::new(
        ErrorCode::ShuttingDown,
        "the window's owner went without answering",
    )
}

/// The events one subscription selects, in the order the window emitted them. Dropping it ends
/// the subscription.
///
/// A subscription holds a bounded queue: when events outrun the reader, the ones that do not fit
/// are counted and the next event that fits is preceded by a [`GapEvent`](flui_protocol::GapEvent)
/// naming how many were missed. The stream ends with a `window` event saying the window closed.
///
/// Never wait on it on the owner thread, or inside a [`DevAgentHook`] method: the owner fills it
/// at frame boundaries.
#[must_use = "dropping the stream ends the subscription"]
pub struct AgentEvents(Box<dyn EventSource>);

impl AgentEvents {
    pub(crate) fn new(source: Box<dyn EventSource>) -> Self {
        Self(source)
    }

    /// The next event, if one is queued.
    pub fn try_next(&mut self) -> Option<Event> {
        self.0.try_next()
    }

    /// The next event, waiting up to `timeout` for one; `None` if none came by then, and once the
    /// stream has ended. Never call this on the owner thread.
    pub fn recv_timeout(&mut self, timeout: Duration) -> Option<Event> {
        self.0.recv_timeout(timeout)
    }

    /// Whether the stream has ended and every event it queued was taken: the window closed.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.0.is_finished()
    }

    /// Wake `waker` once an event is queued or the stream ends. Only the
    /// waker registered last is woken, once; register again for another
    /// wake, and read after registering.
    pub fn register(&mut self, waker: &Waker) {
        self.0.register(waker);
    }
}

impl fmt::Debug for AgentEvents {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentEvents").finish_non_exhaustive()
    }
}

/// A recording of the UI runtime that hosts a window, from [`AgentWindow::record`]: its
/// [`header`](Self::header), then its rows in the order the runtime applied them, in the input
/// recording's format (ADR-0215, ADR-0288 §11).
///
/// It covers every window that runtime hosts, and ends with an `end` row once [`Self::stop`]
/// was asked, the window it was started on closed, or the runtime went. A row the reader has
/// no room for is counted, and the next row that fits is preceded by a `gap` row naming how many
/// were lost; the `end` row counts every row lost. A recording whose `end` row could not be
/// queued ends without it, and [`Self::is_finished`] still says so.
///
/// Dropping it ends the recording at the owner's next drain, with no `end` row.
///
/// Never wait on it on the owner thread, or inside a [`DevAgentHook`] method: the owner fills
/// it as it applies input.
#[must_use = "dropping the recording ends it"]
pub struct AgentRecording {
    header: Header,
    source: Box<dyn RecordSource>,
}

impl AgentRecording {
    pub(crate) fn new(header: Header, source: Box<dyn RecordSource>) -> Self {
        Self { header, source }
    }

    /// What the recording starts from: its first line.
    #[must_use]
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// The next row, if one is queued.
    pub fn try_next(&mut self) -> Option<RecordRow> {
        self.source.try_next()
    }

    /// The next row, waiting up to `timeout` for one; `None` if none came by then, and once the
    /// recording has ended. Never call this on the owner thread.
    pub fn recv_timeout(&mut self, timeout: Duration) -> Option<RecordRow> {
        self.source.recv_timeout(timeout)
    }

    /// Whether the recording has ended and every row it queued was taken.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.source.is_finished()
    }

    /// Wake `waker` once a row is queued or the recording ends. Only the waker registered last
    /// is woken, once; register again for another wake, and read after registering.
    pub fn register(&mut self, waker: &Waker) {
        self.source.register(waker);
    }

    /// End the recording: the owner delivers what it recorded so far and the `end` row at its
    /// next drain. Asked again, or once the recording has ended, it does nothing.
    pub fn stop(&mut self) {
        self.source.stop();
    }
}

impl fmt::Debug for AgentRecording {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentRecording")
            .field("windows", &self.header.windows.len())
            .field("finished", &self.is_finished())
            .finish_non_exhaustive()
    }
}

/// One window an agent can read and act on, from any thread.
///
/// Handed to [`DevAgentHook::window_opened`] by the host. `Clone + Send +
/// Sync`. It holds the window weakly, so it never keeps a closed window
/// alive, and it keeps the window's semantics collected while any clone is
/// alive: a hook that lets go of every clone ends that work. Its element
/// handles are scoped to this window: another window can report the same
/// `e<n>` for a different element.
#[derive(Clone)]
pub struct AgentWindow {
    id: WindowId,
    port: Weak<dyn AgentPort>,
    /// Keeps the window's semantics collected; opaque to this crate.
    _collecting: Arc<dyn Any + Send + Sync>,
}

// A refusal is the protocol's error object itself, which the caller answers with: boxing it
// would add an allocation to each refusal to save moving a few hundred bytes.
// The error object is under the lint's size only where a pointer is 32 bits.
#[cfg_attr(
    target_pointer_width = "64",
    expect(
        clippy::result_large_err,
        reason = "the error object is the answer, returned once per call"
    )
)]
impl AgentWindow {
    pub(crate) fn new(
        id: WindowId,
        port: Weak<dyn AgentPort>,
        collecting: Arc<dyn Any + Send + Sync>,
    ) -> Self {
        Self {
            id,
            port,
            _collecting: collecting,
        }
    }

    /// The window's handle, `w<n>` on the wire. Unique within the process.
    #[must_use]
    pub fn id(&self) -> WindowId {
        self.id
    }

    /// Whether the window is still open. A window reported open can close
    /// before the next call, which then answers `gone`.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open_port().is_some()
    }

    /// The port, while the window is open. A call on another thread may
    /// still hold the port after the window closed, so the port's own flag
    /// is read too.
    fn open_port(&self) -> Option<Arc<dyn AgentPort>> {
        self.port.upgrade().filter(|port| port.is_open())
    }

    /// Ask this window `M` with `params`: the owner answers at its next
    /// drain, or later for a request the window holds (`wait.until`, an
    /// action's reading, `inspect.layers`). What each method answers is the
    /// protocol's, documented on its type in [`flui_protocol::method`].
    ///
    /// Element handles in the answer are this window's. A method the
    /// window does not answer in this build answers `not_supported` naming
    /// it. Synthetic input is asked through an [`InputLease`]
    /// ([`Self::input_lease`]): `input.inject` and `input.release` asked
    /// here answer `invalid_argument`.
    ///
    /// # Errors
    ///
    /// The protocol's error object when the call cannot be asked:
    /// `invalid_argument` when `params` name another window; `gone` (kind
    /// `window`) once the window has closed; `busy` when the owner's inbox is
    /// full; `shutting_down` when the owner has gone. The answer carries the
    /// rest.
    pub fn call<M: WindowMethod>(
        &self,
        params: M::Params,
    ) -> Result<AgentAnswer<M::Output>, ErrorObject> {
        self.ask::<M>(params, None)
    }

    /// A lease on this window's synthetic input: `input.inject` and
    /// `input.release` are asked through it ([`InputLease::call`]), and
    /// what it injected is released when it is dropped.
    ///
    /// # Errors
    ///
    /// `gone` (kind `window`) once the window has closed; `busy` when the
    /// window has handed out every lease it can.
    pub fn input_lease(&self) -> Result<InputLease, ErrorObject> {
        let port = self.open_port().ok_or_else(|| window_gone(self.id))?;
        let lease = port.lease()?;
        Ok(InputLease {
            window: self.clone(),
            lease,
        })
    }

    fn ask<M: WindowMethod>(
        &self,
        params: M::Params,
        lease: Option<LeaseId>,
    ) -> Result<AgentAnswer<M::Output>, ErrorObject> {
        if let Some(named) = params.window()
            && named != self.id
        {
            return Err(ErrorObject::new(
                ErrorCode::InvalidArgument,
                format!(
                    "{}'s parameters name window {named}; this is window {}",
                    M::NAME,
                    self.id
                ),
            ));
        }
        let port = self.open_port().ok_or_else(|| window_gone(self.id))?;
        let (call, answer) = WindowCall::new::<M>(params);
        let call = match lease {
            Some(lease) => call.through(lease),
            None => call,
        };
        port.call(call)?;
        Ok(answer)
    }

    /// Subscribe to this window's events: `filters` select the kinds, and a filter that names
    /// another window selects nothing here. The answer is the stream once the owner has
    /// registered it; events emitted after that reach it.
    ///
    /// The window emits `frame`, `input`, `semantics` and `window` events. `announcement` is
    /// accepted as a filter but not emitted yet, so a stream that selects only it stays silent
    /// until the window closes, and a wait on it runs to its timeout.
    ///
    /// # Errors
    ///
    /// As [`Self::call`]; the answer carries the rest, among them `busy` when the window already
    /// holds as many subscriptions as it admits.
    pub fn subscribe(
        &self,
        filters: Vec<EventFilter>,
    ) -> Result<AgentAnswer<AgentEvents>, ErrorObject> {
        self.open_port()
            .ok_or_else(|| window_gone(self.id))?
            .subscribe(filters)
    }

    /// Record the UI runtime that hosts this window, as [`AgentRecording`] describes: the answer
    /// is the recording once the owner has started it; what the runtime applies after that is
    /// in it.
    ///
    /// # Errors
    ///
    /// As [`Self::call`]; the answer carries the rest: `invalid_argument` when `params` name no
    /// layer, `not_supported` naming a layer this build does not record, and `busy` when the
    /// runtime already holds as many recordings as it admits.
    pub fn record(&self, params: RecordParams) -> Result<AgentAnswer<AgentRecording>, ErrorObject> {
        if let Some(named) = params.window
            && named != self.id
        {
            return Err(ErrorObject::new(
                ErrorCode::InvalidArgument,
                format!(
                    "record.start's parameters name window {named}; this is window {}",
                    self.id
                ),
            ));
        }
        self.open_port()
            .ok_or_else(|| window_gone(self.id))?
            .record(params)
    }
}

/// A lease on one window's synthetic input, from
/// [`AgentWindow::input_lease`].
///
/// `input.inject` asked through the lease presses and moves the window's
/// input as the platform's would, and what it leaves held (a contact down,
/// a key pressed) stays the lease's: `input.release` through the lease lets
/// it go, and so does dropping the lease, at the owner's next drain, the
/// latest first: calls asked through it before it was dropped are taken
/// first, and the ones still queued then are answered `gone`. A connection
/// that holds one lease per window it injects into leaves nothing held when
/// it goes, and two connections never release each other's input. The
/// window asks `input.inject` and `input.release` through a lease only:
/// [`AgentWindow::call`] answers them `invalid_argument`.
///
/// Not `Clone`: one holder, one release.
pub struct InputLease {
    window: AgentWindow,
    lease: LeaseId,
}

// As `AgentWindow::call`: a refusal is the protocol's error object.
#[cfg_attr(
    target_pointer_width = "64",
    expect(
        clippy::result_large_err,
        reason = "the error object is the answer, returned once per call"
    )
)]
impl InputLease {
    /// Whether the lease's window is still open: once it has closed, the lease holds nothing.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.window.is_open()
    }

    /// The window the lease holds input on.
    #[must_use]
    pub fn window(&self) -> WindowId {
        self.window.id
    }

    /// Ask the window `M` with `params` through this lease, as
    /// [`AgentWindow::call`] does: input it injects is held under the lease.
    ///
    /// # Errors
    ///
    /// As [`AgentWindow::call`].
    pub fn call<M: WindowMethod>(
        &self,
        params: M::Params,
    ) -> Result<AgentAnswer<M::Output>, ErrorObject> {
        self.window.ask::<M>(params, Some(self.lease))
    }
}

impl Drop for InputLease {
    /// Releases what the lease holds, at the owner's next drain. A closed
    /// window released it already.
    fn drop(&mut self) {
        if let Some(port) = self.window.port.upgrade() {
            port.release(self.lease);
        }
    }
}

impl fmt::Debug for InputLease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InputLease")
            .field("window", &self.window.id)
            .field("lease", &self.lease.get())
            .finish()
    }
}

/// What every call on a closed window answers.
fn window_gone(window: WindowId) -> ErrorObject {
    ErrorObject::new(ErrorCode::Gone, "the window has closed")
        .with_retry(Retry::Never)
        .with_handle(HandleKind::Window, window.to_string())
}

impl fmt::Debug for AgentWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentWindow")
            .field("id", &self.id)
            .field("open", &self.is_open())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use flui_protocol::method::{SemanticsAct, SemanticsLocate, SemanticsRead};
    use flui_protocol::{LocateParams, Locator, ReadParams, Tree};

    use super::*;

    static_assertions::assert_impl_all!(AgentWindow: Send, Sync, Clone);
    static_assertions::assert_impl_all!(AgentAnswer<Tree>: Send);

    /// Answers every read with an empty tree and leaves every other call
    /// unanswered.
    struct Answering;

    impl AgentPort for Answering {
        fn is_open(&self) -> bool {
            true
        }

        fn call(&self, call: WindowCall) -> Result<(), ErrorObject> {
            if let Ok((_, reply)) = call.open::<SemanticsRead>() {
                reply.send(Ok(Tree::new(Vec::new(), false)));
            }
            Ok(())
        }

        fn subscribe(&self, _: Vec<EventFilter>) -> Result<AgentAnswer<AgentEvents>, ErrorObject> {
            Err(window_gone(WindowId::from_u64(1).expect("non-zero")))
        }

        fn lease(&self) -> Result<LeaseId, ErrorObject> {
            Err(window_gone(WindowId::from_u64(1).expect("non-zero")))
        }

        fn release(&self, _: LeaseId) {}

        fn record(&self, _: RecordParams) -> Result<AgentAnswer<AgentRecording>, ErrorObject> {
            Err(window_gone(WindowId::from_u64(1).expect("non-zero")))
        }
    }

    #[test]
    fn a_window_answers_through_its_port_until_the_port_is_gone() {
        let id = WindowId::from_u64(3).expect("non-zero");
        let other = WindowId::from_u64(4).expect("non-zero");
        let port: Arc<dyn AgentPort> = Arc::new(Answering);
        let collecting: Arc<dyn std::any::Any + Send + Sync> = Arc::new(());
        let window =
            crate::__runtime::agent_window(id, Arc::downgrade(&port), Arc::clone(&collecting));
        assert_eq!(Arc::strong_count(&collecting), 2, "the window holds it");
        assert!(window.is_open());
        let mut answer = window
            .call::<SemanticsRead>(ReadParams::new(id))
            .expect("the port is alive");
        assert!(matches!(answer.try_take(), Some(Ok(_))));
        assert!(answer.try_take().is_none(), "an answer is taken once");

        // A call the port dropped unanswered reads as the owner gone.
        let mut dropped = window
            .call::<SemanticsLocate>(LocateParams::new(Locator::default()))
            .expect("the port is alive");
        let error = dropped
            .try_take()
            .expect("a dropped reply answers")
            .expect_err("nobody answered");
        assert_eq!(error.code, ErrorCode::ShuttingDown);

        let elsewhere = window
            .call::<SemanticsRead>(ReadParams::new(other))
            .expect_err("the parameters name another window");
        assert_eq!(elsewhere.code, ErrorCode::InvalidArgument);
        let elsewhere = window
            .record(RecordParams::input(Some(other)))
            .expect_err("the parameters name another window");
        assert_eq!(elsewhere.code, ErrorCode::InvalidArgument);

        drop(port);
        assert!(!window.is_open());
        assert_eq!(
            Arc::strong_count(&collecting),
            2,
            "a closed window still holds it until dropped"
        );
        let gone = window
            .call::<SemanticsRead>(ReadParams::new(id))
            .expect_err("closed");
        assert_eq!(
            (gone.code, gone.retry, gone.kind, gone.handle.as_deref()),
            (
                ErrorCode::Gone,
                Retry::Never,
                Some(HandleKind::Window),
                Some("w3")
            )
        );
        assert!(gone.effect.is_none());
        drop(window);
        assert_eq!(Arc::strong_count(&collecting), 1, "dropping lets go");
    }

    /// Mints leases from 1 and records the lease each call came through and each release.
    #[derive(Default)]
    struct Leasing {
        next: std::sync::atomic::AtomicU64,
        asked: parking_lot::Mutex<Vec<Option<LeaseId>>>,
        released: parking_lot::Mutex<Vec<LeaseId>>,
    }

    impl AgentPort for Leasing {
        fn is_open(&self) -> bool {
            true
        }

        fn call(&self, call: WindowCall) -> Result<(), ErrorObject> {
            self.asked.lock().push(call.lease());
            Ok(())
        }

        fn subscribe(&self, _: Vec<EventFilter>) -> Result<AgentAnswer<AgentEvents>, ErrorObject> {
            Err(window_gone(WindowId::from_u64(1).expect("non-zero")))
        }

        fn lease(&self) -> Result<LeaseId, ErrorObject> {
            let next = self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            Ok(LeaseId::new(
                std::num::NonZeroU64::new(next).expect("from 1"),
            ))
        }

        fn release(&self, lease: LeaseId) {
            self.released.lock().push(lease);
        }

        fn record(&self, _: RecordParams) -> Result<AgentAnswer<AgentRecording>, ErrorObject> {
            Err(window_gone(WindowId::from_u64(1).expect("non-zero")))
        }
    }

    /// A call through a lease carries it to the port and a plain call carries none; each lease
    /// is released once, when it is dropped, and a window gone mints none.
    #[test]
    fn an_input_lease_carries_its_calls_and_is_released_when_dropped() {
        let id = WindowId::from_u64(3).expect("non-zero");
        let port = Arc::new(Leasing::default());
        let dyn_port: Arc<dyn AgentPort> = Arc::clone(&port) as _;
        let window = crate::__runtime::agent_window(id, Arc::downgrade(&dyn_port), Arc::new(()));
        let first = window.input_lease().expect("the window is open");
        let second = window.input_lease().expect("the window is open");
        assert_eq!(first.window(), id);
        drop(first.call::<SemanticsRead>(ReadParams::new(id)));
        drop(window.call::<SemanticsRead>(ReadParams::new(id)));
        drop(second.call::<SemanticsRead>(ReadParams::new(id)));
        let lease = |value: u64| LeaseId::new(std::num::NonZeroU64::new(value).expect("non-zero"));
        assert_eq!(
            *port.asked.lock(),
            [Some(lease(1)), None, Some(lease(2))],
            "a call names the lease it came through"
        );
        assert!(port.released.lock().is_empty(), "held while the lease is");
        drop(second);
        drop(first);
        assert_eq!(*port.released.lock(), [lease(2), lease(1)]);
        drop(dyn_port);
        drop(port);
        let gone = window.input_lease().expect_err("closed");
        assert_eq!(gone.code, ErrorCode::Gone);
    }

    /// A queued call's `Debug` names its method and leaves its parameters out, so a value an
    /// agent sets never reaches a log, and the call opens only as the method it was made for.
    #[test]
    fn a_window_call_shows_only_its_method_and_opens_only_as_it() {
        const VALUE: &str = "correct horse battery staple";
        let element = flui_protocol::ElementRef::new(
            WindowId::from_u64(3).expect("non-zero"),
            flui_protocol::ElementId::from_u64(7).expect("non-zero"),
        );
        let (call, _answer) =
            WindowCall::new::<SemanticsAct>(flui_protocol::ActParams::set_value(element, VALUE));
        let shown = format!("{call:?}");
        assert!(shown.contains("semantics.act"), "{shown}");
        assert!(!shown.contains(VALUE), "a value reached Debug: {shown}");

        let call = call
            .open::<SemanticsRead>()
            .expect_err("an act is not a read");
        let (params, _reply) = call.open::<SemanticsAct>().expect("an act opens as one");
        assert_eq!(params.value.as_deref(), Some(VALUE));
    }
}
