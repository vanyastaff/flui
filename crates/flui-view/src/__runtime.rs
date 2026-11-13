//! The composition-root seam of ADR-0081 §4.
//!
//! These items let the crates that own a UI runtime drive a [`WidgetsBinding`]
//! from outside `flui-view`: activating its `GlobalKey` registry for one
//! UI runtime entry, stamping the frame phase at the build-to-finalize boundary,
//! and running the presentation's terminal lifecycle ladder; the host's sides
//! of a presentation's close guard ([`CloseGuardSource`]) and of the flush
//! registry ([`FlushHost`]), and the [`OwnerNotify`] channel both signal
//! through; and the
//! development agent's port ([`AgentPort`]), which only the runtime
//! implements and through which it builds the [`AgentWindow`]s a
//! [`DevAgentHook`](crate::dev_agent::DevAgentHook) is handed. Their intended
//! users are `flui-runtime`, `flui-app`, `flui-testing` and `flui-hot-reload`.
//!
//! **No semver promise.** Anything here may change or disappear in any
//! release. The module is always compiled and `#[doc(hidden)]`, and the
//! whole-crate re-exports in `flui-sdk` and the `flui` facade shadow it, so
//! `flui_sdk::view::__runtime` and `flui::view::__runtime` do not resolve.
//! The seam's methods on [`WidgetsBinding`] live on the sealed
//! [`BindingRuntime`] trait rather than as inherent methods, so they are not
//! part of the binding's public surface either: they resolve only where the
//! trait is imported.

use std::any::{Any, TypeId};
use std::cell::Cell;
use std::fmt;
use std::num::NonZeroU64;
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Weak};
use std::task::Waker;
use std::time::Duration;

use flui_foundation::panic::FirstPanic;
use flui_protocol::RecordRow;
use flui_protocol::input_recording::Header;
use flui_protocol::method::WindowMethod;
use flui_protocol::{ErrorObject, Event, EventFilter, RecordParams, WindowId};
use flui_scheduler::AppLifecycleState;

use crate::WidgetsBinding;
pub use crate::close_guard::{CloseGuardSource, SettledClose};
use crate::dev_agent::{AgentAnswer, AgentEvents, AgentRecording, AgentWindow};
pub use crate::flush_registry::{
    FlushChanged, FlushHost, FlushPublisher, FlushRegistry, FlushReport, FlushState, FlushWrite,
    FlushWriter, SESSION_END_BUDGET,
};
pub use crate::lifecycle::{CloseDelivery, LifecycleCloseWindow, LifecycleSource};
pub use crate::navigation::url_owner::{UrlOwnerRegistration, UrlOwnerScope, UrlOwnerSlot};
pub use crate::owner_notify::{LifecycleEvent, OwnerNotify, Undeliverable};
pub use crate::window_handle::WindowSource;

/// What an [`AgentWindow`] calls: one window's requests, enqueued on its
/// owner. The runtime's development-agent port is the implementation.
// A refusal is the protocol's error object itself, which the window answers with.
// The error object is under the lint's size only where a pointer is 32 bits.
#[cfg_attr(
    target_pointer_width = "64",
    expect(
        clippy::result_large_err,
        reason = "the error object is the answer, returned once per call"
    )
)]
pub trait AgentPort: Send + Sync {
    /// Whether the window is still open. A port a call still holds outlives
    /// its window's close, so this, not the port's lifetime, says whether
    /// the next call may be enqueued.
    fn is_open(&self) -> bool;

    /// Enqueue `call` on the window's owner, which answers it through the
    /// call's reply.
    ///
    /// # Errors
    ///
    /// When the call cannot be enqueued: the call, and with it its reply, is
    /// dropped.
    fn call(&self, call: WindowCall) -> Result<(), ErrorObject>;

    /// Enqueue a subscription to the window's events: the owner registers it at its next drain
    /// and answers with the stream.
    ///
    /// # Errors
    ///
    /// When the request cannot be enqueued.
    fn subscribe(&self, filters: Vec<EventFilter>)
    -> Result<AgentAnswer<AgentEvents>, ErrorObject>;

    /// A lease on the window's synthetic input: a fresh id, never reused within this port
    /// (see [`InputLease`](crate::dev_agent::InputLease)).
    ///
    /// # Errors
    ///
    /// `gone` once the window has closed; `busy` once the port has minted every id.
    fn lease(&self) -> Result<LeaseId, ErrorObject>;

    /// The lease's holder let it go: what `lease` holds is released at the owner's next
    /// drain. Never refused: the release is recorded before the owner is woken, and a
    /// window that closed released everything already.
    fn release(&self, lease: LeaseId);

    /// Enqueue a recording of the UI runtime that hosts the window: the owner starts it at its
    /// next drain and answers with the recording.
    ///
    /// # Errors
    ///
    /// When the request cannot be enqueued.
    fn record(&self, params: RecordParams) -> Result<AgentAnswer<AgentRecording>, ErrorObject>;
}

/// One input lease of a window, minted by its [`AgentPort`]: the synthetic input injected
/// through the lease is held under it, and released with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LeaseId(NonZeroU64);

impl LeaseId {
    /// The lease `value`.
    #[must_use]
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    /// The lease's value.
    #[must_use]
    pub const fn get(self) -> NonZeroU64 {
        self.0
    }
}

/// One window method's call on its way to the window's owner: the method,
/// its parameters and the reply its answer goes to, with the method's types
/// erased so one port method and one command carry every method.
///
/// Only [`AgentWindow::call`] makes one for `M`, and the owner opens it as
/// the method it is ([`Self::open`]), so the types inside always are `M`'s.
pub struct WindowCall {
    method: TypeId,
    name: &'static str,
    params: Box<dyn Any + Send>,
    reply: Box<dyn Any + Send>,
    /// The input lease the call was asked through.
    lease: Option<LeaseId>,
}

impl WindowCall {
    /// A call of `M` with `params`, and the answer its reply fills.
    pub(crate) fn new<M: WindowMethod>(params: M::Params) -> (Self, AgentAnswer<M::Output>) {
        let (reply, answer) = reply_pair::<M::Output>();
        let call = Self {
            method: TypeId::of::<M>(),
            name: M::NAME,
            params: Box::new(params),
            reply: Box::new(reply),
            lease: None,
        };
        (call, answer)
    }

    /// The input lease the call was asked through, if any.
    #[must_use]
    pub fn lease(&self) -> Option<LeaseId> {
        self.lease
    }

    /// The call, asked through `lease`.
    pub(crate) fn through(mut self, lease: LeaseId) -> Self {
        self.lease = Some(lease);
        self
    }

    /// The method's wire name, for traces.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The parameters and reply of a call of `M`, or the call back when it
    /// is another method's.
    ///
    /// # Errors
    ///
    /// The call itself, untouched, when it is not `M`'s.
    pub fn open<M: WindowMethod>(self) -> Result<(M::Params, Reply<M::Output>), Self> {
        if self.method != TypeId::of::<M>() {
            return Err(self);
        }
        let params = self
            .params
            .downcast::<M::Params>()
            .expect("BUG: a window call carries its own method's parameters");
        let reply = self
            .reply
            .downcast::<Reply<M::Output>>()
            .expect("BUG: a window call carries its own method's reply");
        Ok((*params, *reply))
    }
}

impl fmt::Debug for WindowCall {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowCall")
            .field("method", &self.name)
            .finish_non_exhaustive()
    }
}

/// Where the owner sends one call's answer. `Clone`, so an owner that keeps
/// a request can still answer it if serving it panics; the first answer
/// sent is the one taken.
///
/// Sending wakes the waker the asker registered, and so does the last clone
/// going unsent, which the answer reads as the owner gone. A waker is
/// `Send + Sync` and cannot reach the owner's state, so it may run while the
/// owner holds that state borrowed; its panic is contained (see [`WakeSlot`]).
pub struct Reply<T> {
    tx: SyncSender<Result<T, ErrorObject>>,
    /// The asker's answer holds the other end: once it is dropped nobody
    /// takes the answer.
    asker: Weak<()>,
    /// Shared by every clone: woken on a send, and when the last clone drops.
    wake: Arc<WakeOnDrop>,
}

impl<T> Clone for Reply<T> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            asker: Weak::clone(&self.asker),
            wake: Arc::clone(&self.wake),
        }
    }
}

impl<T> Reply<T> {
    /// Send the answer. `false` when nobody takes it: the asker dropped its
    /// answer, or an answer was sent already. The value is then dropped here.
    pub fn send(&self, answer: Result<T, ErrorObject>) -> bool {
        let sent = self.tx.try_send(answer).is_ok();
        if sent {
            self.wake.0.wake();
        }
        sent
    }

    /// Whether anyone can still take the answer.
    #[must_use]
    pub fn is_awaited(&self) -> bool {
        self.asker.strong_count() > 0
    }
}

impl<T> fmt::Debug for Reply<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Reply")
            .field("awaited", &self.is_awaited())
            .finish_non_exhaustive()
    }
}

/// Wakes its slot when the last reply that shares it drops: the answer can
/// be taken then, as the owner gone if nothing was sent.
struct WakeOnDrop(Arc<WakeSlot>);

impl Drop for WakeOnDrop {
    fn drop(&mut self) {
        self.0.wake();
    }
}

/// The one waker an answer or a stream wakes when it can be read: the last
/// registered, woken once, and registered again by whoever wants the next
/// wake. No lock is held while a waker is cloned, woken or dropped.
#[derive(Default)]
pub struct WakeSlot(parking_lot::Mutex<Option<Waker>>);

impl WakeSlot {
    /// Keep `waker` for the next wake, in place of the one kept before.
    pub fn register(&self, waker: &Waker) {
        let kept = self
            .0
            .lock()
            .as_ref()
            .is_some_and(|held| held.will_wake(waker));
        if kept {
            return;
        }
        let candidate = waker.clone();
        let displaced = self.0.lock().replace(candidate);
        drop(displaced);
    }

    /// Wake the kept waker, if any. A waker that panics is contained and
    /// reported, never resumed into the owner that woke it: what it announces
    /// is already in place, and an agent's failure does not fail the app it
    /// serves (ADR-0288 §3).
    pub fn wake(&self) {
        let woken = self.0.lock().take();
        let mut caught = FirstPanic::new();
        if let Some(waker) = woken
            && caught.contain(|| waker.wake()).is_none()
        {
            // The report is contained too; the collectors destroy both
            // payloads under containment when they drop.
            FirstPanic::new().contain(|| {
                tracing::error!(
                    panic = caught.held().and_then(flui_foundation::panic::payload_text),
                    "a development agent's waker panicked; its answer or event stays readable"
                );
            });
        }
    }
}

impl fmt::Debug for WakeSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WakeSlot").finish_non_exhaustive()
    }
}

/// A reply and the answer it fills, for a request the runtime answers that
/// is not a window method (a subscription).
pub fn reply_pair<T>() -> (Reply<T>, AgentAnswer<T>) {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let asker = Arc::new(());
    let wake = Arc::new(WakeSlot::default());
    let reply = Reply {
        tx,
        asker: Arc::downgrade(&asker),
        wake: Arc::new(WakeOnDrop(Arc::clone(&wake))),
    };
    (reply, AgentAnswer::new(rx, asker, wake))
}

/// The queue behind an [`AgentEvents`], which the runtime fills.
pub trait EventSource: Send {
    /// The next event, if one is queued.
    fn try_next(&mut self) -> Option<Event>;

    /// The next event, waiting up to `timeout`; `None` if none came, and once the stream ended.
    fn recv_timeout(&mut self, timeout: Duration) -> Option<Event>;

    /// Whether the stream ended and every event it queued was taken.
    fn is_finished(&self) -> bool;

    /// Wake `waker` once an event is queued or the stream ends (see [`WakeSlot`]).
    fn register(&mut self, waker: &Waker);
}

/// Wraps a runtime's queue as the [`AgentEvents`] a hook receives.
pub fn agent_events(source: impl EventSource + 'static) -> AgentEvents {
    AgentEvents::new(Box::new(source))
}

/// The queue behind an [`AgentRecording`], which the runtime fills.
pub trait RecordSource: Send {
    /// The next row, if one is queued.
    fn try_next(&mut self) -> Option<RecordRow>;

    /// The next row, waiting up to `timeout`; `None` if none came, and once the recording ended.
    fn recv_timeout(&mut self, timeout: Duration) -> Option<RecordRow>;

    /// Whether the recording ended and every row it queued was taken.
    fn is_finished(&self) -> bool;

    /// Wake `waker` once a row is queued or the recording ends (see [`WakeSlot`]).
    fn register(&mut self, waker: &Waker);

    /// Ask the owner to end the recording with its `end` row. Never refused: the request is
    /// recorded before the owner is woken, and a recording that ended already ignores it.
    fn stop(&mut self);
}

/// Wraps a runtime's recording, which starts from `header`, as the [`AgentRecording`] a hook
/// receives.
pub fn agent_recording(header: Header, source: impl RecordSource + 'static) -> AgentRecording {
    AgentRecording::new(header, Box::new(source))
}

/// The [`AgentWindow`] for window `id`, answering through `port` while the
/// port is alive and `gone` after, and holding `collecting` (the window's
/// semantics handle) while any clone of it is alive.
#[must_use]
pub fn agent_window(
    id: WindowId,
    port: Weak<dyn AgentPort>,
    collecting: Arc<dyn Any + Send + Sync>,
) -> AgentWindow {
    AgentWindow::new(id, port, collecting)
}

/// Data-only phase cell shared with an internal frame composition driver.
///
/// It lets the widget binding stamp an externally-owned `Copy` phase value
/// at its exact build-to-finalize boundary without invoking foreign code
/// while the binding's inner write guard is held.
#[derive(Debug)]
pub struct FramePhaseMarker<T: Copy> {
    phase: Cell<T>,
    /// Data-only fault injection for downstream `testing` consumers.
    /// Consumed at the next build-to-finalize boundary.
    #[cfg(any(test, feature = "testing"))]
    panic_once_at_boundary: Cell<bool>,
}

impl<T: Copy> FramePhaseMarker<T> {
    /// Create a marker with its initial phase.
    pub fn new(initial_phase: T) -> Self {
        Self {
            phase: Cell::new(initial_phase),
            #[cfg(any(test, feature = "testing"))]
            panic_once_at_boundary: Cell::new(false),
        }
    }

    /// Store the phase that is about to run.
    pub fn set(&self, phase: T) {
        self.phase.set(phase);
    }

    pub(crate) fn set_at_frame_boundary(&self, phase: T) {
        self.phase.set(phase);
        #[cfg(any(test, feature = "testing"))]
        let should_panic = self.panic_once_at_boundary.replace(false);
        #[cfg(any(test, feature = "testing"))]
        assert!(
            !should_panic,
            "frame phase marker — intentional one-shot test panic"
        );
    }

    /// Read the last phase stored.
    #[must_use]
    pub fn get(&self) -> T {
        self.phase.get()
    }

    /// Arm a fixed one-shot panic at the next build-to-finalize boundary.
    ///
    /// This is a data-only runtime test seam: it stores only a phase value
    /// and never accepts executable code across the widget binding's lock.
    #[cfg(any(test, feature = "testing"))]
    pub fn arm_test_panic_once(&self) {
        self.panic_once_at_boundary.set(true);
    }
}

/// A UI runtime-level `GlobalKey` registry spanning several [`WidgetsBinding`]s —
/// one per presentation sharing a UI runtime's `GlobalKeyScope` (ADR-0043 §1).
///
/// Assembled once over the presentations installed at the time
/// [`Self::assemble`] runs, tried in the given order (a UI runtime's mount
/// order). `GlobalKeyScope`'s uniqueness invariant guarantees at most one
/// binding ever answers a given hash, so trying each in turn and returning
/// the first hit is exact, not a heuristic — see
/// `key::registry::build_composite`'s doc for why resolving the follow-up
/// `with_element` call correctly (rather than by the same blind scan) needs
/// a small correlation cache.
#[derive(Debug)]
pub struct GlobalKeyRegistryComposite(crate::key::registry::GlobalKeyRegistryHandle);

impl GlobalKeyRegistryComposite {
    /// Assemble a composite spanning `bindings`' own registries, in order.
    ///
    /// A composite of one binding is that binding's own registry: with a
    /// single member, trying each in turn and remembering which one answered
    /// both reduce to asking it, so a UI runtime with one presentation
    /// enters its scope without building anything.
    #[must_use]
    pub fn assemble<'a>(bindings: impl IntoIterator<Item = &'a WidgetsBinding>) -> Self {
        let mut bindings = bindings.into_iter();
        let first = bindings.next();
        let second = bindings.next();
        if let (Some(only), None) = (first, second) {
            return Self(only.global_key_registry_handle());
        }
        let handles: Vec<crate::key::registry::GlobalKeyRegistryHandle> = first
            .into_iter()
            .chain(second)
            .chain(bindings)
            .map(WidgetsBinding::global_key_registry_handle)
            .collect();
        Self(crate::key::registry::build_composite(handles))
    }

    /// Activate this composite for the dynamic extent of `f` — the
    /// multi-presentation counterpart to
    /// [`BindingRuntime::with_global_key_registry`].
    pub fn enter<R>(&self, f: impl FnOnce() -> R) -> R {
        crate::key::registry::with_active_registry(&self.0, f)
    }
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for crate::WidgetsBinding {}
}

/// One GlobalKey claim withdrawn from a closing owner. Dropping it retires
/// the key, during an unwind too (ADR-0281).
pub struct WithdrawnKey(
    #[expect(dead_code, reason = "held only so dropping it retires the key")]
    crate::owner::ScopedKeyOwner,
);

impl std::fmt::Debug for WithdrawnKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WithdrawnKey").finish_non_exhaustive()
    }
}

/// The composition root's access to one [`WidgetsBinding`].
///
/// Sealed: [`WidgetsBinding`] is its only implementation. Callers import it
/// as `use flui_view::__runtime::BindingRuntime as _;` and keep calling the
/// methods on the binding.
pub trait BindingRuntime: sealed::Sealed {
    /// Withdraw graph, rebuild and key authority without invoking widget code.
    fn withdraw_root_owner(&self, preserving: bool);
    /// Withdraw graph, rebuild and key authority without dropping anything:
    /// the withdrawn key owners are returned for the caller to retire.
    fn withdraw_owner_authority(&self) -> Vec<WithdrawnKey>;
    /// Run one owner-runtime entry with this binding's `GlobalKey` registry
    /// active on the current thread.
    ///
    /// Activation is nested and unwind-safe; after `f` returns or panics the
    /// previous UI runtime is restored. Raw TLS/registry handles remain private to
    /// `flui-view`.
    fn with_global_key_registry<R>(&self, f: impl FnOnce() -> R) -> R;

    /// Pump a widget frame and stamp `finalize_phase` at the exact boundary
    /// between the build drain and inactive-element finalization.
    ///
    /// Only a [`FramePhaseMarker`] write occurs while the binding's inner
    /// write guard and debug building-flag guard remain active; no external
    /// executable callback crosses that lock boundary. Ordinary users call
    /// [`WidgetsBinding::draw_frame`].
    fn draw_frame_with_phase_marker<T: Copy>(
        &self,
        marker: &FramePhaseMarker<T>,
        finalize_phase: T,
    );

    /// This binding's exact local lifecycle source.
    fn lifecycle_source(&self) -> &LifecycleSource;

    /// Notify legacy observers and scoped subscriptions of a state already
    /// committed through [`Self::lifecycle_source`] (the terminal ladder
    /// commits with `commit_terminal`, which
    /// [`WidgetsBinding::handle_app_lifecycle_state_changed`] refuses).
    fn notify_committed_lifecycle(&self, state: AppLifecycleState);
}

impl BindingRuntime for WidgetsBinding {
    fn withdraw_root_owner(&self, preserving: bool) {
        WidgetsBinding::withdraw_root_owner(self, preserving);
    }
    fn withdraw_owner_authority(&self) -> Vec<WithdrawnKey> {
        WidgetsBinding::withdraw_owner_authority(self)
            .into_iter()
            .map(WithdrawnKey)
            .collect()
    }
    fn with_global_key_registry<R>(&self, f: impl FnOnce() -> R) -> R {
        crate::key::registry::with_active_registry(&self.global_key_registry, f)
    }

    fn draw_frame_with_phase_marker<T: Copy>(
        &self,
        marker: &FramePhaseMarker<T>,
        finalize_phase: T,
    ) {
        self.draw_frame_impl(crate::binding::BuildPass::FrameEntry, || {
            marker.set_at_frame_boundary(finalize_phase);
        });
    }

    fn lifecycle_source(&self) -> &LifecycleSource {
        &self.lifecycle
    }

    fn notify_committed_lifecycle(&self, state: AppLifecycleState) {
        self.notify_lifecycle(state);
    }
}
