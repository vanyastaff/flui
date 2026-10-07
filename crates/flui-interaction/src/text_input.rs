//! Presentation-owned platform text input (IME).
//!
//! [`TextInputOwner`] is the single authority for one presentation's current
//! text-input connection. It owns the platform capability directly, so there
//! is no process registry, active-window lookup, type-erased window handle, or
//! closure bundle between a widget and its presentation.
//!
//! A mounted text widget receives a [`TextInputHandle`]. The handle is a
//! concrete, owner-local `Weak` reference: it cannot keep a presentation alive,
//! cannot cross threads, and reports teardown through [`TextInputError`].
//!
//! # Connection semantics
//!
//! - One active client per presentation.
//! - Attaching replaces the previous client.
//! - Detach is token guarded: a stale token cannot close the replacement.
//! - A client replaced or detached inside a frame transaction keeps its
//!   store until that frame's commit anchor, where the grants it queued run.
//! - A push platform's IME is enabled on attach until an enable has
//!   completed, and disabled on the active detach or explicit owner close; a
//!   pull platform's host is told the attached store on each attach, and
//!   `None` on the active detach or close.
//! - [`TextInputOwner::complete_composition`] commits the active client's
//!   composition, keeping its text: through the host on a pull platform, in
//!   the store itself otherwise, or when the host abandoned it.
//! - Platform events are demultiplexed to the presentation before
//!   [`TextInputOwner::dispatch`] is called.
//!
//! # The client is a text store
//!
//! A client attaches a [`TextInputClient`], which carries the field's
//! [`TextStore`] (ADR-0090). A push-model event (winit's [`ImeEvent`]) is
//! projected onto that store as edits under a read-write lock
//! ([`project_ime_event`]), so a field has one editing path whichever model
//! its platform speaks.
//!
//! The owner also holds its presentation's frame transaction (ADR-0027 §3)
//! as a [`CommitGate`], and installs that gate into every store it attaches
//! ([`TextStore::set_commit_gate`]), so no store can miss it. While the gate
//! is shut, a store refuses synchronous locks and queues asynchronous ones;
//! the composition root opens it once the frame has returned and calls
//! [`TextInputOwner::run_deferred_grants`].
//!
//! # Backends
//!
//! The owner speaks to its platform one of two ways ([`TextInputBackend`]).
//! A push-model platform (winit, AppKit today) gets `set_ime_allowed` and
//! the cursor area, and pushes [`ImeEvent`]s back. A pull-model platform
//! (Win32 text services) hands the presentation a [`TextStoreHost`], and the
//! owner tells it which store the focused field is: the host then reads and
//! edits that store itself (ADR-0135).
//!
//! Host operations are ordered and never nested. While the frame
//! transaction is open, or while the owner is already inside a call on the
//! host (the host reached application code that attached, detached or
//! completed again), an operation is queued; the queue drains once the
//! outer call returns with the transaction closed, and at the anchor before
//! any deferred grant. A queued completion captures the store it was asked
//! for, so a later detach does not cancel it.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::num::NonZeroU64;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use flui_foundation::geometry::Bounds;
use flui_platform_api::ImeEvent;
use flui_platform_api::PlatformTextInput;
use flui_platform_api::text_store::{
    CommitGate, CompositionEnd, LockOutcome, OwnerCalls, RetainOnFailure, TextStore, TextStoreHost,
    commit_composition_in_place, project_ime_event,
};

use crate::__runtime::{CloseMode, ClosePanic, CloseTombstone};
use crate::retain::Retain;

/// Identity returned by [`TextInputHandle::attach`].
///
/// Only the currently active token can detach a connection. This prevents a
/// delayed blur/dispose from field A from closing field B after B replaced A.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientToken(NonZeroU64);

/// What a text field attaches: its store, and what to do when the platform
/// (re)starts an input session.
#[derive(Clone)]
pub struct TextInputClient {
    store: Rc<dyn TextStore>,
    on_session_start: Option<Rc<dyn Fn()>>,
}

impl Retain for TextInputClient {
    fn retain(self) {
        Retain::retain(self.on_session_start);
        Retain::retain(self.store);
    }
}

impl TextInputClient {
    /// A client editing `store`.
    #[must_use]
    pub fn new(store: Rc<dyn TextStore>) -> Self {
        Self {
            store,
            on_session_start: None,
        }
    }

    /// Call `f` on every [`ImeEvent::Enabled`]: the platform started or
    /// restarted an input session.
    #[must_use]
    pub fn on_session_start(self, f: impl Fn() + 'static) -> Self {
        Self {
            on_session_start: Some(Rc::new(f)),
            ..self
        }
    }
}

impl std::fmt::Debug for TextInputClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextInputClient")
            .field("status", &self.store.status())
            .field("on_session_start", &self.on_session_start.is_some())
            .finish()
    }
}

/// Result of a token-guarded detach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum DetachOutcome {
    /// The token named the active client and the connection was closed.
    Detached,
    /// The token had already been replaced or detached.
    Stale,
}

/// Text-input capability failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TextInputError {
    /// The presentation does not expose platform IME support.
    #[error("this presentation does not support platform text input")]
    Unsupported,
    /// The presentation explicitly entered teardown.
    #[error("the presentation text-input owner is closed")]
    Closed,
    /// The presentation was dropped; this weak handle is permanently inert.
    #[error("the presentation text-input owner no longer exists")]
    OwnerGone,
}

/// How a presentation's platform takes text input.
pub enum TextInputBackend {
    /// A push-model platform: the owner enables its IME and reports the
    /// cursor area; the platform pushes [`ImeEvent`]s, which the owner
    /// projects onto the active store.
    Push(Arc<dyn PlatformTextInput>),
    /// A pull-model platform: the owner tells the host which store the
    /// focused field is, and the platform reads and edits it (ADR-0135).
    Pull(Rc<dyn TextStoreHost>),
    /// No input-method support: attaching returns
    /// [`TextInputError::Unsupported`].
    Unsupported,
}

impl std::fmt::Debug for TextInputBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Push(_) => "Push",
            Self::Pull(_) => "Pull",
            Self::Unsupported => "Unsupported",
        })
    }
}

/// A call on the pull host, in the order the owner's state changed.
enum HostOp {
    /// Tell the host which store is focused.
    Focus(Option<Rc<dyn TextStore>>),
    /// End the composition in this store, captured when it was asked for.
    Complete(Rc<dyn TextStore>),
}

impl Retain for HostOp {
    fn retain(self) {
        match self {
            Self::Focus(store) => Retain::retain(store),
            Self::Complete(store) => Retain::retain(store),
        }
    }
}

/// Counts one owner call on the host for as long as it runs, unwinding
/// included; the queue is drained explicitly, never from here.
struct HostCall<'a>(&'a Cell<u32>);

impl<'a> HostCall<'a> {
    fn enter(depth: &'a Cell<u32>) -> Self {
        depth.set(depth.get() + 1);
        Self(depth)
    }
}

impl Drop for HostCall<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

/// Whose turn drains the host queue, which decides what happens to a
/// failure parked in the presentation's gate before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostTurn {
    /// The owner's turn to report (a completion, the anchor): a failure
    /// parked before a host call came first, so it is taken ahead of it.
    Own,
    /// A client change (attach, detach): a failure parked before it stays
    /// for the owner's next turn, and what the host calls park goes behind it.
    Behind,
}

/// Ask `host` to end its composition in `store`, committing it in place
/// when the host abandoned it, does not serve `store` or is gone. A
/// `Deferred` answer leaves it to the host, which finishes it when the
/// platform call in progress returns.
///
/// Returns whether an in-place commit was queued in the store rather than
/// run: accepted work the caller owes a run ([`TextInputOwner::owe_commit`]).
fn complete_through(host: &dyn TextStoreHost, store: &Rc<dyn TextStore>) -> bool {
    match host.complete_composition(store) {
        Ok(CompositionEnd::Abandoned) => {}
        Err(error) => {
            tracing::debug!(%error, "the host did not end the composition; committing it in place");
        }
        Ok(_) => return false,
    }
    commit_queued(&**store)
}

/// Commit `store`'s composition in place; whether the commit was queued in
/// the store (behind its shut gate, or behind a grant running on it) rather
/// than run or refused.
fn commit_queued(store: &dyn TextStore) -> bool {
    commit_composition_in_place(store) == Ok(LockOutcome::Deferred)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OwnerLifecycle {
    Open,
    /// Refuses callers, but [`TextInputOwner::close_with_mode`] has not yet
    /// disabled the platform or retired the clients.
    Withdrawn,
    Closed,
}

struct AttachedClient {
    token: ClientToken,
    client: TextInputClient,
}

impl RetainOnFailure for TextInputClient {
    fn retain(self) {
        RetainOnFailure::retain(self.on_session_start);
        RetainOnFailure::retain(self.store);
    }
}

/// Retire the independent client owners separately, in the client's field
/// order (store, then session callback), inside `calls`: after a failure, and
/// while the thread is already unwinding, they are retained rather than
/// destroyed (ADR-0127). With `gate`, a failure their destruction parks there
/// (a store dropped while it settles a grant) is taken too.
fn retire_client_owners(
    client: TextInputClient,
    calls: &mut OwnerCalls,
    gate: Option<&CommitGate>,
) {
    let TextInputClient {
        store,
        on_session_start,
    } = client;
    if let Some(gate) = gate {
        calls.retire_parking(gate, store);
        calls.retire_parking(gate, on_session_start);
    } else {
        calls.retire(store);
        calls.retire(on_session_start);
    }
}

/// Retire a client a closed owner rejected, one owner at a time (store, then
/// session callback) inside the close's containment: dropping the client
/// whole would drop the callback during the store's unwind.
fn retire_rejected(failure: &mut ClosePanic, client: TextInputClient) {
    let TextInputClient {
        store,
        on_session_start,
    } = client;
    failure.retire(store);
    failure.retire(on_session_start);
}

/// Release a local clone of the framework-owned platform capability before
/// any user-owned value retires, so the clone is never destroyed by a later
/// unwind; only an unwind already in progress retains it (ADR-0127).
/// The pull host's clone is released the same way.
fn release_platform<T>(platform: T, calls: &mut OwnerCalls) {
    if std::thread::panicking() {
        std::mem::forget(platform);
    } else {
        calls.run(|| drop(platform));
    }
}

/// Run a call of a close on the platform or the pull host (or a client
/// owner's destruction, [`close_retire`]), inside the close's `failure`. The
/// call reaches application code whose grants park failures in `gate`, so it
/// is ordered against them as every owner call is ([`OwnerCalls::run_parking`]):
/// when it panics, a failure parked before its panic is raised ahead of it
/// and one its unwind's cleanup parked is kept behind it. A call that
/// returns leaves what it parked in `gate`, for the close to take at its
/// end, ahead of its own failures.
fn close_host_call<T>(
    gate: &CommitGate,
    failure: &mut ClosePanic,
    call: impl FnOnce() -> T,
) -> Option<T> {
    let mut calls = OwnerCalls::new();
    let value = calls.run_parking(gate, call);
    if let Some(payload) = calls.into_failure() {
        if value.is_some() {
            // What the call parked, which `run_parking` just took: the gate
            // is empty, so it is back where the close's end looks for it.
            gate.defer_failure(payload);
        } else {
            failure.keep_caught(payload);
        }
    }
    value
}

/// Retire `value`, an owner a close withdrew, inside the close's `failure`:
/// retained once a failure is owed, otherwise destroyed through
/// [`close_host_call`], so a failure its destruction's unwind parks in
/// `gate` is kept behind that destruction's own.
fn close_retire<T: Retain>(gate: &CommitGate, failure: &mut ClosePanic, value: T) {
    if failure.preserving() {
        value.retain();
    } else {
        close_host_call(gate, failure, || drop(value));
    }
}

fn retire_stores(stores: Vec<Rc<dyn TextStore>>, calls: &mut OwnerCalls, gate: &CommitGate) {
    for store in stores {
        calls.retire_parking(gate, store);
    }
}

struct OwnerState {
    lifecycle: OwnerLifecycle,
    active: Option<AttachedClient>,
    /// Stores replaced or detached while the frame transaction was open.
    /// A grant they queued then was the platform's answer to them, so it
    /// runs at the anchor that closes this frame, not whenever the field is
    /// next focused.
    retired: Vec<Rc<dyn TextStore>>,
    /// Stores whose in-place composition commit waits as a queued grant,
    /// oldest first: behind the shut gate (a completion asked for inside a
    /// frame on a push or storeless backend), or behind a grant that was
    /// running on the store when it was asked for, which may fail and leave
    /// it queued. The anchor runs them; a close runs them too, since no
    /// anchor follows it, while it cancels every other store's queued grants.
    completing: Vec<Rc<dyn TextStore>>,
    /// Pull host operations not yet applied, oldest first.
    host_ops: VecDeque<HostOp>,
    /// Whether the last focus the host applied named a store.
    host_focused: bool,
    /// Whether a push platform's `set_ime_allowed(true)` completed since the
    /// platform was last disabled. An attach enables the platform while it
    /// has not, so an enable that panicked is retried by the next attach,
    /// replacing or not; the call is idempotent. A pull host has no enable:
    /// every attach queues its focus, so a failed one is already retried.
    platform_enabled: bool,
}

impl OwnerState {
    /// `client` stops being the active one; keep its store for the anchor
    /// if it may still hold grants queued behind the frame.
    fn retire(&mut self, client: &AttachedClient, transaction_open: bool) {
        if transaction_open {
            push_unique(&mut self.retired, Rc::clone(&client.client.store));
        }
    }
}

/// Queue `store` for the anchor unless it is already queued: a store runs its
/// grants once per anchor, however many times it left the active slot.
fn push_unique(stores: &mut Vec<Rc<dyn TextStore>>, store: Rc<dyn TextStore>) {
    if !stores.iter().any(|queued| Rc::ptr_eq(queued, &store)) {
        stores.push(store);
    }
}

/// Direct owner of one presentation's platform text-input connection.
///
/// Construct it with the [`TextInputBackend`] that presentation's window
/// offers. A presentation without IME support passes
/// [`TextInputBackend::Unsupported`]; attempts to attach then return
/// [`TextInputError::Unsupported`].
///
/// The returned `Rc` is intentional: widgets receive weak handles derived from
/// this exact owner, while the presentation retains the only strong ownership.
pub struct TextInputOwner {
    close_mode: CloseTombstone,
    /// The platform capability or host owned by one presentation; no
    /// intermediary. Framework-owned: close releases it, leaving
    /// `Unsupported`, even when the rest of the owner is retained after a
    /// failure, since on some backends it keeps the native window alive.
    backend: RefCell<TextInputBackend>,
    /// How many owner calls on the pull host are running; non-zero queues
    /// the next.
    host_depth: Cell<u32>,
    next_token: Cell<NonZeroU64>,
    /// Shut while the presentation is inside a frame transaction, where text
    /// stores may not commit (ADR-0027 §3); installed into every attached
    /// store.
    gate: CommitGate,
    state: RefCell<OwnerState>,
}

impl TextInputOwner {
    /// Create the text-input owner for one presentation.
    #[must_use]
    pub fn new(backend: TextInputBackend) -> Rc<Self> {
        Rc::new(Self {
            close_mode: CloseTombstone::default(),
            backend: RefCell::new(backend),
            host_depth: Cell::new(0),
            next_token: Cell::new(NonZeroU64::MIN),
            gate: CommitGate::new(),
            state: RefCell::new(OwnerState {
                lifecycle: OwnerLifecycle::Open,
                active: None,
                retired: Vec::new(),
                completing: Vec::new(),
                host_ops: VecDeque::new(),
                host_focused: false,
                platform_enabled: false,
            }),
        })
    }

    /// Create a weak widget capability tied to this exact owner.
    #[must_use]
    pub fn handle(self: &Rc<Self>) -> TextInputHandle {
        TextInputHandle {
            owner: Rc::downgrade(self),
            close_mode: self.close_mode.clone(),
        }
    }

    /// The push capability, cloned so no borrow spans a call into it.
    fn push_platform(&self) -> Option<Arc<dyn PlatformTextInput>> {
        match &*self.backend.borrow() {
            TextInputBackend::Push(platform) => Some(Arc::clone(platform)),
            _ => None,
        }
    }

    /// The pull host, cloned so no borrow spans a call into it.
    fn pull_host(&self) -> Option<Rc<dyn TextStoreHost>> {
        match &*self.backend.borrow() {
            TextInputBackend::Pull(host) => Some(Rc::clone(host)),
            _ => None,
        }
    }

    fn is_pull(&self) -> bool {
        matches!(*self.backend.borrow(), TextInputBackend::Pull(_))
    }

    fn ensure_supported(&self) -> Result<(), TextInputError> {
        if matches!(*self.backend.borrow(), TextInputBackend::Unsupported) {
            Err(TextInputError::Unsupported)
        } else {
            Ok(())
        }
    }

    fn ensure_open(&self) -> Result<(), TextInputError> {
        if self.state.borrow().lifecycle == OwnerLifecycle::Open {
            Ok(())
        } else {
            Err(TextInputError::Closed)
        }
    }

    fn attach(&self, client: TextInputClient) -> Result<ClientToken, TextInputError> {
        if let Err(error) = self.ensure_open() {
            let mut failure = ClosePanic::for_rejection(self.close_mode.mode());
            retire_rejected(&mut failure, client);
            failure.finish();
            return Err(error);
        }
        // No strong platform clone is held across the user store below: a
        // store that closes this owner must leave the close as the
        // capability's last owner, so a failing rejection of the client
        // cannot destroy the backend during its unwind.
        self.ensure_supported()?;

        let current = self.next_token.get();
        let next = current
            .get()
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .expect("BUG: text-input client token space exhausted");
        self.next_token.set(next);
        let token = ClientToken(current);

        // Before the client is reachable through `dispatch`, so no lock is
        // ever requested on a store that does not yet follow the frame. The
        // store is user code: a failure there rejects the client, which is
        // retained rather than destroyed during the unwind (ADR-0127). The
        // store may request grants of stores behind this owner's gate, whose
        // settles park their failures there: one parked during a call that
        // then panics came first, and is the one raised. When the store took
        // the gate, what it parked stays for this owner's next turn, as any
        // grant's parked failure does.
        let mut installing = OwnerCalls::new();
        let installed = installing
            .run_parking(&self.gate, || {
                client.store.set_commit_gate(self.gate.clone());
            })
            .is_some();
        if let Some(payload) = installing.into_failure() {
            if installed {
                self.gate.defer_failure(payload);
            } else {
                RetainOnFailure::retain(client);
                std::panic::resume_unwind(payload);
            }
        }
        // A user-defined store may close the owner while installing its gate.
        // The rejected client was never admitted; its owners still retire
        // one at a time, store first, behind the close-mode failure fence.
        if let Err(error) = self.ensure_open() {
            let mut failure = ClosePanic::for_rejection(self.close_mode.mode());
            let TextInputClient {
                store,
                on_session_start,
            } = client;
            failure.retire(store);
            failure.retire(on_session_start);
            failure.finish();
            return Err(error);
        }
        let mut calls = OwnerCalls::new();
        // A replaced client's composition is committed before the incoming
        // client is installed, whatever order the fields hear of the focus
        // change in: the outgoing field's own completion arrives with a stale
        // token and does nothing. On a push or storeless backend the commit
        // runs here, through the path `complete_composition` takes; a failure
        // it raises is kept and the attach goes on. A pull host hears of it
        // as a completion queued ahead of the incoming focus, below.
        let outgoing = if self.is_pull() {
            None
        } else {
            self.state
                .borrow()
                .active
                .as_ref()
                .map(|active| Rc::clone(&active.client.store))
        };
        if let Some(store) = outgoing {
            let queued = calls.run_parking(&self.gate, || commit_queued(&*store));
            if queued == Some(true) {
                self.owe_commit(&store, &mut calls);
            }
            calls.retire_parking(&self.gate, store);
            // The outgoing store's commit may have closed the owner.
            if let Err(error) = self.ensure_open() {
                if let Some(payload) = calls.into_failure() {
                    RetainOnFailure::retain(client);
                    std::panic::resume_unwind(payload);
                }
                let mut failure = ClosePanic::for_rejection(self.close_mode.mode());
                retire_rejected(&mut failure, client);
                failure.finish();
                return Err(error);
            }
        }
        // Open, so close has not taken the capability.
        let platform = self.push_platform();
        let focus = self.is_pull().then(|| Rc::clone(&client.store));
        let transaction_open = self.is_transaction_open();
        let (enable_platform, replaced) = {
            let mut state = self.state.borrow_mut();
            let replaced = state.active.replace(AttachedClient { token, client });
            let enable_platform = platform.is_some() && !state.platform_enabled;
            if let Some(replaced) = &replaced {
                state.retire(replaced, transaction_open);
                if focus.is_some() {
                    state
                        .host_ops
                        .push_back(HostOp::Complete(Rc::clone(&replaced.client.store)));
                }
            }
            if let Some(store) = focus {
                state.host_ops.push_back(HostOp::Focus(Some(store)));
            }
            (enable_platform, replaced)
        };

        if enable_platform
            && let Some(platform) = &platform
            && calls.run(|| platform.set_ime_allowed(true)).is_some()
        {
            // Enabled, unless the platform's call detached the last client
            // (which disabled it) or closed the owner.
            let mut state = self.state.borrow_mut();
            state.platform_enabled =
                state.lifecycle == OwnerLifecycle::Open && state.active.is_some();
        }
        // Callback captures and custom stores may reenter through this owner.
        // Both owner state and platform enablement are committed first, and
        // the local capability clone is released before them: a store that
        // closes the owner and then panics must not leave this clone as the
        // backend's last owner, destroyed during that unwind. A failure the
        // replaced client's destruction parks is taken in time order.
        if let Some(platform) = platform {
            release_platform(platform, &mut calls);
        }
        if let Some(replaced) = replaced {
            retire_client_owners(replaced.client, &mut calls, Some(&self.gate));
        }
        // The host hears of the focus after the retirement, in the order the
        // owner's state changed. A failure already parked for this owner's
        // next turn stays there, ahead of what this attach parks behind it.
        self.apply_host_ops(&mut calls, HostTurn::Behind);
        // A diagnostic runs a user-installed subscriber.
        calls.run(|| tracing::trace!(token = token.0.get(), "IME client attached"));
        // The client is active and the token is the caller's: a failure here is
        // this owner's to report at its next turn, so it cannot take the token
        // with it. An owner closed meanwhile has no next turn, and the token
        // no use: the failure is raised.
        if let Some(payload) = calls.into_failure() {
            if self.ensure_open().is_ok() {
                self.gate.defer_failure(payload);
            } else {
                std::panic::resume_unwind(payload);
            }
        }
        Ok(token)
    }

    fn detach(&self, token: ClientToken) -> Result<DetachOutcome, TextInputError> {
        self.ensure_open()?;
        self.ensure_supported()?;
        let platform = self.push_platform();
        let pull = self.is_pull();

        let transaction_open = self.is_transaction_open();
        let detached = {
            let mut state = self.state.borrow_mut();
            let active = state.active.take_if(|client| client.token == token);
            if let Some(active) = &active {
                state.retire(active, transaction_open);
                state.platform_enabled = false;
                if pull {
                    state.host_ops.push_back(HostOp::Focus(None));
                }
            }
            active
        };

        if let Some(detached) = detached {
            let mut calls = OwnerCalls::new();
            calls.run(|| {
                if let Some(platform) = &platform {
                    platform.set_ime_allowed(false);
                }
            });
            if let Some(platform) = platform {
                release_platform(platform, &mut calls);
            }
            retire_client_owners(detached.client, &mut calls, Some(&self.gate));
            // A failure already parked stays for this owner's next turn, as
            // one the retirement did not park does.
            self.apply_host_ops(&mut calls, HostTurn::Behind);
            calls.run(|| tracing::trace!(token = token.0.get(), "IME client detached"));
            calls.resume();
            Ok(DetachOutcome::Detached)
        } else {
            // Released before the diagnostic, as on the active path: a
            // subscriber that closes this owner and then panics must not
            // leave this clone the backend's last owner during the unwind.
            let mut calls = OwnerCalls::new();
            release_platform(platform, &mut calls);
            calls.run(|| tracing::trace!(token = token.0.get(), "stale IME detach ignored"));
            calls.resume();
            Ok(DetachOutcome::Stale)
        }
    }

    fn set_cursor_area(&self, area: Bounds<f64>) -> Result<(), TextInputError> {
        self.ensure_open()?;
        self.ensure_supported()?;
        // A pull platform asks the store for geometry itself.
        if let Some(platform) = self.push_platform() {
            // The platform's code may close this owner and then panic: the
            // local capability clone is released inside the scope, not in
            // the unwind.
            let mut calls = OwnerCalls::new();
            calls.run(|| platform.set_ime_cursor_area(area));
            release_platform(platform, &mut calls);
            calls.resume();
        }
        Ok(())
    }

    /// Commit the active client's composition, keeping its text: what a
    /// pointer-down in the presentation or an accepted close request does
    /// before its handlers run (ADR-0142 item 4).
    ///
    /// On a pull platform the host ends its composition; if it abandons it
    /// (or is gone), the owner clears the store's composing range itself.
    /// Elsewhere the owner does that directly. Inside a frame, or under
    /// another call on the host, the request is queued with its store and
    /// runs at the anchor or when that call returns.
    ///
    /// # Panics
    ///
    /// Resumes the first failure once every queued host operation has run:
    /// one parked in this presentation's gate before the call, then one a
    /// grant settled inside a host call parked, then the host call's own
    /// panic. Later ones are retained.
    pub fn complete_composition(&self) {
        let store = {
            let state = self.state.borrow();
            if state.lifecycle != OwnerLifecycle::Open {
                return;
            }
            state
                .active
                .as_ref()
                .map(|active| Rc::clone(&active.client.store))
        };
        if let Some(store) = store {
            self.complete_store_composition(store);
        }
    }

    fn complete_store_composition(&self, store: Rc<dyn TextStore>) {
        let mut calls = OwnerCalls::new();
        if self.is_pull() {
            self.state
                .borrow_mut()
                .host_ops
                .push_back(HostOp::Complete(store));
            self.apply_host_ops(&mut calls, HostTurn::Own);
        } else {
            // The commit settles a grant, which may park an owner failure.
            // It is queued behind the shut gate inside a frame, and behind
            // the running grant when asked for from inside one on this store,
            // gate open or not: the store is then owed its anchor, or the
            // close, whichever comes first.
            let queued = calls.run_parking(&self.gate, || commit_queued(&*store));
            if queued == Some(true) {
                self.owe_commit(&store, &mut calls);
            }
            calls.retire_parking(&self.gate, store);
        }
        calls.resume();
    }

    /// Record that `store` holds a queued in-place commit this owner
    /// accepted, for the anchor or the close to run (whichever comes first).
    /// The record follows what the request did, not the gate: a grant ahead
    /// of the commit that fails leaves it queued with the gate open.
    fn owe_commit(&self, store: &Rc<dyn TextStore>, calls: &mut OwnerCalls) {
        let mut state = self.state.borrow_mut();
        if state.lifecycle == OwnerLifecycle::Open {
            push_unique(&mut state.completing, Rc::clone(store));
        } else {
            // A custom store's request closed this owner meanwhile: the
            // close opened the gate before the commit was recorded, so it
            // runs now.
            drop(state);
            calls.run_parking(&self.gate, || store.run_deferred_grants());
        }
    }

    /// Apply the queued host operations, oldest first, inside `calls`. They
    /// wait while a host call is running (its return drains them) or the
    /// frame transaction is open (the anchor drains them).
    ///
    /// The host is platform code that reaches application code (a text
    /// service editing a store settles its grant), so each call goes
    /// through `calls` like any owner code: the operation, its store and
    /// the host clone are taken from the queue before the call; on the
    /// owner's own turn ([`HostTurn::Own`]) a failure parked in this
    /// presentation's gate is taken before the call (it came earlier), and
    /// one a grant the call ran parked is taken after it, ahead of the
    /// call's own panic, unless the gate already held one. A failing operation releases its values and the
    /// queue goes on, so a panicking focus change does not strand the
    /// completion behind it; the first failure stays authoritative.
    fn apply_host_ops(&self, calls: &mut OwnerCalls, turn: HostTurn) {
        loop {
            if self.host_depth.get() > 0 || self.is_transaction_open() {
                return;
            }
            let (op, host) = {
                let mut state = self.state.borrow_mut();
                if state.lifecycle != OwnerLifecycle::Open {
                    return;
                }
                let Some(host) = self.pull_host() else {
                    return;
                };
                let Some(op) = state.host_ops.pop_front() else {
                    return;
                };
                if let HostOp::Focus(store) = &op {
                    state.host_focused = store.is_some();
                }
                (op, host)
            };
            {
                let _call = HostCall::enter(&self.host_depth);
                if turn == HostTurn::Own {
                    calls.take_parked(&self.gate);
                }
                match op {
                    HostOp::Focus(store) => {
                        calls.run_parking(&self.gate, || host.focus_store(store));
                    }
                    HostOp::Complete(store) => {
                        let queued =
                            calls.run_parking(&self.gate, || complete_through(&*host, &store));
                        if queued == Some(true) {
                            self.owe_commit(&store, calls);
                        }
                        // A host call that detached the client left this
                        // clone the store's last owner.
                        calls.retire_parking(&self.gate, store);
                    }
                }
            }
            // A host call that closed the owner left this clone the host's
            // last owner: framework-owned, it is released even after a
            // failure, contained.
            release_platform(host, calls);
        }
    }

    /// Deliver a push-model platform event to the active client, if any.
    ///
    /// [`ImeEvent::Enabled`] runs the client's session-start callback and
    /// edits nothing; every other event is projected onto the client's store
    /// ([`project_ime_event`]). A projection the store refuses is logged,
    /// not propagated: the platform has no one to hand the error back to.
    ///
    /// The client is cloned out before use so the store's grant may
    /// reentrantly attach, detach, or close without colliding with a
    /// `RefCell` borrow.
    ///
    /// # Panics
    ///
    /// Resumes a panic the owner's code raised after a grant (a field's
    /// `on_changed`), which the store parked in this presentation's gate —
    /// during this projection or before the dispatch, whatever path it then
    /// takes — once the event is handled: the grant stands, and the failure
    /// reaches the caller's report. The earliest failure wins: one parked
    /// before the dispatch, then one parked by a grant the session-start
    /// callback or the projection ran, then that callback's or projection's
    /// own panic; later ones are retained.
    pub fn dispatch(&self, event: &ImeEvent) {
        let mut calls = OwnerCalls::new();
        // A failure parked by a grant before this dispatch (one the platform
        // requested directly) is this turn's to report, on every path, and it
        // came before anything this dispatch raises.
        calls.take_parked(&self.gate);
        let client = {
            let state = self.state.borrow();
            (state.lifecycle == OwnerLifecycle::Open)
                .then(|| state.active.as_ref().map(|active| active.client.clone()))
                .flatten()
        };
        if let Some(client) = client {
            if matches!(event, ImeEvent::Enabled) {
                if let Some(on_session_start) = &client.on_session_start {
                    calls.run_parking(&self.gate, || on_session_start());
                }
            } else if let Some(Err(error)) =
                calls.run_parking(&self.gate, || project_ime_event(&*client.store, event))
            {
                // A diagnostic runs a user-installed subscriber.
                calls.run(|| {
                    tracing::warn!(
                        ?error,
                        "an IME event could not be applied to the text store"
                    );
                });
            }
            // A grant or callback that detached the client left this clone its
            // last owner, and its destruction may settle a grant of its own.
            retire_client_owners(client, &mut calls, Some(&self.gate));
        }
        calls.resume();
    }

    /// Open or close this presentation's frame transaction: while it is
    /// open, the gate every attached store follows is shut.
    pub fn set_transaction_open(&self, open: bool) {
        self.gate.set_open(!open);
    }

    /// Whether this presentation's frame transaction is open.
    #[must_use]
    pub fn is_transaction_open(&self) -> bool {
        !self.gate.is_open()
    }

    /// Run what waited for the frame to close: first the pull host's queued
    /// operations, then the grants queued while commits were closed — those
    /// of the stores replaced or detached during the frame, in that order,
    /// then the active client's. The composition root calls this once the
    /// frame returns; returns how many grants ran.
    ///
    /// # Panics
    ///
    /// Resumes the first failure, after everything else has run: an owner
    /// panic a store parked in this presentation's gate while settling a
    /// grant since the last anchor, then the host operations' failures (one
    /// a grant settled inside a host call parked before that call's own),
    /// then a grant that panicked, or one parked while settling here. Later
    /// ones are retained. A panicking host operation does not hold the
    /// grants back.
    pub fn run_deferred_grants(&self) -> usize {
        if self.is_transaction_open() {
            // Nothing could run, and the retired stores wait for the anchor
            // that closes this transaction.
            return 0;
        }
        // An owner failure parked since the last turn came before anything
        // this anchor runs.
        let mut calls = OwnerCalls::new();
        calls.take_parked(&self.gate);
        self.apply_host_ops(&mut calls, HostTurn::Own);
        let ran = self.run_store_grants(&mut calls);
        calls.resume();
        ran
    }

    /// The store half of [`Self::run_deferred_grants`], inside its `calls`.
    fn run_store_grants(&self, calls: &mut OwnerCalls) -> usize {
        let (retired, completing, active) = {
            let mut state = self.state.borrow_mut();
            let active = state
                .active
                .as_ref()
                .map(|active| Rc::clone(&active.client.store));
            (
                std::mem::take(&mut state.retired),
                state.completing.clone(),
                active,
            )
        };
        let mut stores = retired;
        for store in completing.into_iter().chain(active) {
            push_unique(&mut stores, store);
        }
        let mut ran = 0;
        for index in 0..stores.len() {
            if self.is_transaction_open() || self.ensure_open().is_err() {
                self.retain_pending_stores(&mut stores, index);
                break;
            }
            let count = calls.run_parking(&self.gate, || stores[index].run_deferred_grants());
            ran += count.unwrap_or(0);
            if count.is_none() || self.is_transaction_open() || self.ensure_open().is_err() {
                // A failed store may still owe grants, as do later stores, and
                // a store may have a tail behind a newly shut gate: ownership
                // is restored before the first failure propagates.
                self.retain_pending_stores(&mut stores, index);
                break;
            }
        }
        // What is left ran to completion: a commit they owed is done.
        Vec::retain(&mut self.state.borrow_mut().completing, |owed| {
            !stores.iter().any(|ran| Rc::ptr_eq(ran, owed))
        });
        retire_stores(stores, calls, &self.gate);
        ran
    }

    fn retain_pending_stores(&self, stores: &mut Vec<Rc<dyn TextStore>>, from: usize) {
        let mut state = self.state.borrow_mut();
        if state.lifecycle == OwnerLifecycle::Open {
            // The active store is not requeued: the next anchor runs it as the
            // active client, and listing it twice would run its grants twice.
            // Each dropped handle is a clone the active slot or the queue
            // still holds, so dropping it runs no user code.
            let mut pending = Vec::with_capacity(stores.len() - from + state.retired.len());
            let active = state
                .active
                .as_ref()
                .map(|active| Rc::clone(&active.client.store));
            for store in stores
                .drain(from..)
                .chain(std::mem::take(&mut state.retired))
            {
                if !active
                    .as_ref()
                    .is_some_and(|active| Rc::ptr_eq(active, &store))
                {
                    push_unique(&mut pending, store);
                }
            }
            state.retired = pending;
        }
        // Closing explicitly cancels the tail; its owners retire outside this borrow.
    }

    /// Close this presentation's text-input owner.
    ///
    /// Closing is idempotent. If a client is active, the exact capability
    /// owned by this presentation is disabled once. Existing weak handles
    /// subsequently return [`TextInputError::Closed`].
    ///
    /// The close is the presentation's last turn: it ends a frame
    /// transaction still open, so the completions queued in it commit, through
    /// the host or in place, before their stores are retired. A store that
    /// owes such a commit runs its queued grants first, in request order; the
    /// grants every other store queued behind the frame are cancelled.
    pub fn close(&self) {
        self.close_with_mode(CloseMode::Ordinary);
    }

    pub(crate) fn close_tombstone(&self) -> CloseTombstone {
        self.close_mode.clone()
    }

    pub(crate) fn close_with_mode(&self, mode: CloseMode) {
        let mut failure = ClosePanic::for_close(mode, self.close_mode.clone());
        let (retired, completing, active, host_ops, host_focused) = {
            let mut state = self.state.borrow_mut();
            if state.lifecycle == OwnerLifecycle::Closed {
                return;
            }
            state.lifecycle = OwnerLifecycle::Closed;
            (
                std::mem::take(&mut state.retired),
                std::mem::take(&mut state.completing),
                state.active.take(),
                std::mem::take(&mut state.host_ops),
                std::mem::replace(&mut state.host_focused, false),
            )
        };
        // The close is this presentation's last turn, so it ends a frame
        // transaction still open: no anchor follows it. A queued completion
        // then commits through the host or, abandoned, in place now, before
        // its store is retired, instead of queueing a grant no one runs
        // (ADR-0142 items 4 and 6).
        self.gate.set_open(true);
        // A commit a push or storeless backend queued behind the frame is
        // accepted work: the stores that owe one run their queued grants,
        // oldest request first, so the commit lands behind the grants
        // accepted before it. Every other store's queued grants are
        // cancelled with it below. After a failure the rest are retired, as
        // the rest of a failed close is.
        for store in completing {
            if !failure.preserving() {
                close_host_call(&self.gate, &mut failure, || store.run_deferred_grants());
            }
            close_retire(&self.gate, &mut failure, store);
        }
        let backend = self.backend.replace(TextInputBackend::Unsupported);
        match &backend {
            TextInputBackend::Push(platform) if active.is_some() => {
                close_host_call(&self.gate, &mut failure, || platform.set_ime_allowed(false));
            }
            TextInputBackend::Pull(host) => {
                self.close_host(&**host, host_ops, host_focused, &mut failure);
            }
            _ => {}
        }
        // Framework-owned: released even when the clients below are retained.
        failure.release(backend);
        if let Some(active) = active {
            let TextInputClient {
                store,
                on_session_start,
            } = active.client;
            close_retire(&self.gate, &mut failure, store);
            close_retire(&self.gate, &mut failure, on_session_start);
        }
        for store in retired {
            close_retire(&self.gate, &mut failure, store);
        }
        // A failure a store parked for this presentation's next turn came
        // before the close; this is that turn. One a call of the close parked
        // and then returned came before the close's later failures; one a
        // call's unwind parked was ordered behind that call's panic already
        // ([`close_host_call`]).
        if let Some(parked) = self.gate.take_failure() {
            failure.keep_earlier(parked);
        }
        failure.finish();
    }

    /// The pull host's part of a close: the queued operations run in order
    /// (a close commits what the user typed, in the store it was asked for,
    /// after the focus changes queued before it), and a host left focused on
    /// a store is told `None`. After a failure the rest are retired, not
    /// run, and the final `None` still is. Every host call counts as one, so
    /// anything it reaches that asks for another is refused by the closed
    /// lifecycle instead of nesting.
    fn close_host(
        &self,
        host: &dyn TextStoreHost,
        ops: VecDeque<HostOp>,
        mut focused: bool,
        failure: &mut ClosePanic,
    ) {
        let _call = HostCall::enter(&self.host_depth);
        for op in ops {
            if failure.preserving() {
                failure.retire(op);
                continue;
            }
            match op {
                HostOp::Focus(store) => {
                    let names_store = store.is_some();
                    // An unwind leaves the host's focus unknown: the final
                    // `None` then clears it.
                    focused = close_host_call(&self.gate, failure, || host.focus_store(store))
                        .is_none_or(|()| names_store);
                }
                HostOp::Complete(store) => {
                    close_host_call(&self.gate, failure, || complete_through(host, &store));
                    close_retire(&self.gate, failure, store);
                }
            }
        }
        if focused {
            close_host_call(&self.gate, failure, || host.focus_store(None));
        }
    }

    /// Refuse every later caller without running user code; a later
    /// [`Self::close_with_mode`] still disables the platform and retires the
    /// clients (ADR-0123).
    pub(crate) fn withdraw(&self) {
        let mut state = self.state.borrow_mut();
        if state.lifecycle == OwnerLifecycle::Open {
            state.lifecycle = OwnerLifecycle::Withdrawn;
        }
    }

    /// Whether `token` currently names the active client.
    #[must_use]
    pub fn is_attached(&self, token: ClientToken) -> bool {
        self.state
            .borrow()
            .active
            .as_ref()
            .is_some_and(|client| client.token == token)
    }

    /// Number of active clients (always zero or one).
    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    pub fn active_count(&self) -> usize {
        usize::from(self.state.borrow().active.is_some())
    }
}

impl std::fmt::Debug for TextInputOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.borrow();
        f.debug_struct("TextInputOwner")
            .field("lifecycle", &state.lifecycle)
            .field("next_token", &self.next_token.get())
            .field("transaction_open", &!self.gate.is_open())
            .field(
                "active_token",
                &state.active.as_ref().map(|client| client.token),
            )
            .field("backend", &*self.backend.borrow())
            .field("queued_host_ops", &state.host_ops.len())
            .finish_non_exhaustive()
    }
}

impl Drop for TextInputOwner {
    fn drop(&mut self) {
        let mut failure = ClosePanic::for_close(CloseMode::Ordinary, self.close_mode.clone());
        let state = self.state.get_mut();
        let disable = state.lifecycle != OwnerLifecycle::Closed && state.active.is_some();
        state.lifecycle = OwnerLifecycle::Closed;
        let active = state.active.take();
        let retired = std::mem::take(&mut state.retired);
        let completing = std::mem::take(&mut state.completing);
        let host_ops = std::mem::take(&mut state.host_ops);
        let host_focused = std::mem::replace(&mut state.host_focused, false);
        // Keep backend custody outside the invocation, including a callback
        // that releases its other last owner before it unwinds.
        let backend = std::mem::replace(self.backend.get_mut(), TextInputBackend::Unsupported);
        // An owner dropped without a close runs no queued completion; it
        // only takes its store away from the host. Each operation and store
        // retires on its own, as a close retires them: one collection
        // dropped whole would destroy the rest during the first's unwind,
        // and a second panicking destructor would abort. After a failure
        // the tail is retained (ADR-0127).
        for op in host_ops {
            close_retire(&self.gate, &mut failure, op);
        }
        for store in completing {
            close_retire(&self.gate, &mut failure, store);
        }
        match &backend {
            TextInputBackend::Push(platform) if disable => {
                close_host_call(&self.gate, &mut failure, || platform.set_ime_allowed(false));
            }
            TextInputBackend::Pull(host) if host_focused => {
                close_host_call(&self.gate, &mut failure, || host.focus_store(None));
            }
            _ => {}
        }
        failure.release(backend);
        if let Some(active) = active {
            let TextInputClient {
                store,
                on_session_start,
            } = active.client;
            close_retire(&self.gate, &mut failure, store);
            close_retire(&self.gate, &mut failure, on_session_start);
        }
        for store in retired {
            close_retire(&self.gate, &mut failure, store);
        }
        if let Some(parked) = self.gate.take_failure() {
            failure.keep_earlier(parked);
        }
        failure.finish_contained();
    }
}
/// Weak, owner-local text-input capability stored by mounted widgets.
#[derive(Clone)]
pub struct TextInputHandle {
    owner: Weak<TextInputOwner>,
    close_mode: CloseTombstone,
}

impl TextInputHandle {
    fn owner(&self) -> Result<Rc<TextInputOwner>, TextInputError> {
        self.owner.upgrade().ok_or(TextInputError::OwnerGone)
    }

    /// Attach `client` as this presentation's active IME client, installing
    /// the presentation's commit gate into its store.
    ///
    /// Once the client is active the token is returned: a failure after that
    /// point (enabling the platform, retiring the client it replaced, telling
    /// a pull host the store) waits
    /// in the presentation's gate for its next dispatch, anchor or close.
    ///
    /// # Panics
    ///
    /// Resumes a failure that rejects the client before it is active (its
    /// store failing to take the gate, or a closed owner's retirement of
    /// it), and one after it when the owner closed meanwhile.
    pub fn attach(&self, client: TextInputClient) -> Result<ClientToken, TextInputError> {
        match self.owner() {
            Ok(owner) => owner.attach(client),
            Err(error) => {
                let mut failure = ClosePanic::for_rejection(self.close_mode.mode());
                retire_rejected(&mut failure, client);
                failure.finish();
                Err(error)
            }
        }
    }

    /// Whether the presentation still takes text input.
    ///
    /// # Errors
    ///
    /// [`TextInputError::Closed`] or [`TextInputError::OwnerGone`] once the
    /// presentation is closing or gone: a store then refuses every lock.
    pub fn ensure_open(&self) -> Result<(), TextInputError> {
        self.owner()?.ensure_open()
    }

    /// Detach `token` if it still names the active client.
    pub fn detach(&self, token: ClientToken) -> Result<DetachOutcome, TextInputError> {
        self.owner()?.detach(token)
    }

    /// Update the platform IME candidate/composition area. A pull platform
    /// reads geometry from the store instead, so there it does nothing.
    pub fn set_cursor_area(&self, area: Bounds<f64>) -> Result<(), TextInputError> {
        self.owner()?.set_cursor_area(area)
    }

    /// Commit `token`'s composition, keeping its text, as
    /// [`TextInputOwner::complete_composition`] does for the active client;
    /// a stale token is a no-op. What a field does before blur, paste or
    /// undo (ADR-0142 item 4).
    ///
    /// # Errors
    ///
    /// [`TextInputError::Closed`] or [`TextInputError::OwnerGone`] once the
    /// presentation is closing or gone.
    pub fn complete_composition(&self, token: ClientToken) -> Result<(), TextInputError> {
        let owner = self.owner()?;
        owner.ensure_open()?;
        if owner.is_attached(token) {
            owner.complete_composition();
        }
        Ok(())
    }
}

impl std::fmt::Debug for TextInputHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextInputHandle")
            .field("owner_alive", &self.owner.strong_count().gt(&0))
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {

    use flui_platform_api::text_store::InMemoryTextStore;
    use parking_lot::Mutex;

    use super::*;

    static_assertions::assert_not_impl_any!(TextInputHandle: Send, Sync);
    static_assertions::assert_not_impl_any!(TextInputOwner: Send, Sync);

    #[derive(Debug, Clone, PartialEq)]
    enum PlatformCall {
        Allowed(bool),
        CursorArea(Bounds<f64>),
    }

    #[derive(Default)]
    struct RecordingTextInput {
        calls: Mutex<Vec<PlatformCall>>,
    }

    impl RecordingTextInput {
        fn calls(&self) -> Vec<PlatformCall> {
            self.calls.lock().clone()
        }
    }

    impl PlatformTextInput for RecordingTextInput {
        fn set_ime_allowed(&self, allowed: bool) {
            self.calls.lock().push(PlatformCall::Allowed(allowed));
        }

        fn set_ime_cursor_area(&self, area: Bounds<f64>) {
            self.calls.lock().push(PlatformCall::CursorArea(area));
        }
    }

    fn owner_with_recorder() -> (Rc<TextInputOwner>, Arc<RecordingTextInput>) {
        let recorder = Arc::new(RecordingTextInput::default());
        let capability: Arc<dyn PlatformTextInput> = recorder.clone(); // test exercises the real erased OS-capability boundary.
        (
            TextInputOwner::new(TextInputBackend::Push(capability)),
            recorder,
        )
    }

    fn client(store: &Rc<InMemoryTextStore>) -> TextInputClient {
        let store: Rc<dyn TextStore> = store.clone(); // the owner holds the field's store through the erased contract.
        TextInputClient::new(store)
    }

    fn empty_client() -> TextInputClient {
        client(&InMemoryTextStore::new(""))
    }

    #[test]
    fn stale_detach_cannot_disable_the_replacement_connection() {
        let (owner, platform) = owner_with_recorder();
        let handle = owner.handle();
        let first = handle.attach(empty_client()).expect("first connection");
        let second = handle
            .attach(empty_client())
            .expect("replacement connection");

        assert_eq!(
            handle.detach(first).expect("owner open"),
            DetachOutcome::Stale
        );
        assert!(owner.is_attached(second));
        assert_eq!(platform.calls(), [PlatformCall::Allowed(true)]);

        assert_eq!(
            handle.detach(second).expect("owner open"),
            DetachOutcome::Detached
        );
        assert_eq!(
            platform.calls(),
            [PlatformCall::Allowed(true), PlatformCall::Allowed(false)]
        );
    }
}
