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
//! - A push platform's IME is enabled on the first attach and disabled on the
//!   active detach or explicit owner close; a pull platform's host is told the
//!   attached store on each attach, and `None` on the active detach or close.
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
    CommitGate, CompositionEnd, OwnerCalls, RetainOnFailure, TextStore, TextStoreHost,
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

/// Ask `host` to end its composition in `store`, committing it in place
/// when the host abandoned it, does not serve `store` or is gone. A
/// `Deferred` answer leaves it to the host, which finishes it when the
/// platform call in progress returns.
fn complete_through(host: &dyn TextStoreHost, store: &Rc<dyn TextStore>) {
    match host.complete_composition(store) {
        Ok(CompositionEnd::Abandoned) => commit_composition_in_place(&**store),
        Err(error) => {
            tracing::debug!(%error, "the host did not end the composition; committing it in place");
            commit_composition_in_place(&**store);
        }
        Ok(_) => {}
    }
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
    /// Pull host operations not yet applied, oldest first.
    host_ops: VecDeque<HostOp>,
    /// Whether the last focus the host applied named a store.
    host_focused: bool,
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
                host_ops: VecDeque::new(),
                host_focused: false,
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
            failure.retire(client);
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
        // ever requested on a store that does not yet follow the frame.
        client.store.set_commit_gate(self.gate.clone());
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
        // Open, so close has not taken the capability.
        let platform = self.push_platform();
        let focus = self.is_pull().then(|| Rc::clone(&client.store));
        let transaction_open = self.is_transaction_open();
        let (enable_platform, replaced) = {
            let mut state = self.state.borrow_mut();
            let replaced = state.active.replace(AttachedClient { token, client });
            let enable_platform = replaced.is_none();
            if let Some(replaced) = &replaced {
                state.retire(replaced, transaction_open);
            }
            if let Some(store) = focus {
                state.host_ops.push_back(HostOp::Focus(Some(store)));
            }
            (enable_platform, replaced)
        };

        let mut calls = OwnerCalls::new();
        calls.run(|| {
            if enable_platform && let Some(platform) = &platform {
                platform.set_ime_allowed(true);
            }
        });
        // Callback captures and custom stores may reenter through this owner.
        // Both owner state and platform enablement are committed first, and
        // the local capability clone is released before them: a store that
        // closes the owner and then panics must not leave this clone as the
        // backend's last owner, destroyed during that unwind. A failure the
        // replaced client parks waits for this owner's next turn.
        if let Some(platform) = platform {
            release_platform(platform, &mut calls);
        }
        if let Some(replaced) = replaced {
            retire_client_owners(replaced.client, &mut calls, None);
        }
        self.apply_host_ops(&mut calls);
        calls.resume();
        tracing::trace!(token = token.0.get(), "IME client attached");
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
            retire_client_owners(detached.client, &mut calls, None);
            self.apply_host_ops(&mut calls);
            calls.resume();
            tracing::trace!(token = token.0.get(), "IME client detached");
            Ok(DetachOutcome::Detached)
        } else {
            tracing::trace!(token = token.0.get(), "stale IME detach ignored");
            Ok(DetachOutcome::Stale)
        }
    }

    fn set_cursor_area(&self, area: Bounds<f64>) -> Result<(), TextInputError> {
        self.ensure_open()?;
        self.ensure_supported()?;
        // A pull platform asks the store for geometry itself.
        if let Some(platform) = self.push_platform() {
            platform.set_ime_cursor_area(area);
        }
        Ok(())
    }

    /// Commit the active client's composition, keeping its text: what a
    /// pointer-down in the presentation or an accepted close request does
    /// before its handlers run (ADR-0090 amendment item 4).
    ///
    /// On a pull platform the host ends its composition; if it abandons it
    /// (or is gone), the owner clears the store's composing range itself.
    /// Elsewhere the owner does that directly. Inside a frame, or under
    /// another call on the host, the request is queued with its store and
    /// runs at the anchor or when that call returns.
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
            self.apply_host_ops(&mut calls);
        } else {
            calls.run(|| commit_composition_in_place(&*store));
            calls.retire(store);
        }
        calls.resume();
    }

    /// Apply the queued host operations, oldest first, inside `calls`. They
    /// wait while a host call is running (its return drains them) or the
    /// frame transaction is open (the anchor drains them).
    ///
    /// A failing operation releases its values and the queue goes on, so a
    /// panicking focus change does not strand the completion behind it.
    fn apply_host_ops(&self, calls: &mut OwnerCalls) {
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
                match op {
                    HostOp::Focus(store) => {
                        calls.run(|| host.focus_store(store));
                    }
                    HostOp::Complete(store) => {
                        calls.run(|| complete_through(&*host, &store));
                        calls.retire(store);
                    }
                }
            }
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
                tracing::warn!(
                    ?error,
                    "an IME event could not be applied to the text store"
                );
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
    /// grant since the last anchor, a host operation that panicked, a grant
    /// that panicked, or one parked while settling here. Later ones are
    /// retained. A panicking host operation does not hold the grants back.
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
        self.apply_host_ops(&mut calls);
        let ran = self.run_store_grants(&mut calls);
        calls.resume();
        ran
    }

    /// The store half of [`Self::run_deferred_grants`], inside its `calls`.
    fn run_store_grants(&self, calls: &mut OwnerCalls) -> usize {
        let (retired, active) = {
            let mut state = self.state.borrow_mut();
            let active = state
                .active
                .as_ref()
                .map(|active| Rc::clone(&active.client.store));
            (std::mem::take(&mut state.retired), active)
        };
        let mut stores = retired;
        if let Some(active) = active {
            push_unique(&mut stores, active);
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
    pub fn close(&self) {
        self.close_with_mode(CloseMode::Ordinary);
    }

    pub(crate) fn close_tombstone(&self) -> CloseTombstone {
        self.close_mode.clone()
    }

    pub(crate) fn close_with_mode(&self, mode: CloseMode) {
        let mut failure = ClosePanic::for_close(mode, self.close_mode.clone());
        let (retired, active, host_ops, host_focused) = {
            let mut state = self.state.borrow_mut();
            if state.lifecycle == OwnerLifecycle::Closed {
                return;
            }
            state.lifecycle = OwnerLifecycle::Closed;
            (
                std::mem::take(&mut state.retired),
                state.active.take(),
                std::mem::take(&mut state.host_ops),
                std::mem::replace(&mut state.host_focused, false),
            )
        };
        let backend = self.backend.replace(TextInputBackend::Unsupported);
        match &backend {
            TextInputBackend::Push(platform) if active.is_some() => {
                failure.invoke(|| platform.set_ime_allowed(false));
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
            failure.retire(store);
            failure.retire(on_session_start);
        }
        for store in retired {
            failure.retire(store);
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
                    focused = failure
                        .invoke(|| host.focus_store(store))
                        .is_none_or(|()| names_store);
                }
                HostOp::Complete(store) => {
                    failure.run(|| complete_through(host, &store));
                    failure.retire(store);
                }
            }
        }
        if focused {
            failure.invoke(|| host.focus_store(None));
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
        let host_ops = std::mem::take(&mut state.host_ops);
        let host_focused = std::mem::replace(&mut state.host_focused, false);
        // Keep backend custody outside the invocation, including a callback
        // that releases its other last owner before it unwinds.
        let backend = std::mem::replace(self.backend.get_mut(), TextInputBackend::Unsupported);
        // An owner dropped without a close runs no queued completion; it
        // only takes its store away from the host.
        failure.retire(Vec::from(host_ops));
        match &backend {
            TextInputBackend::Push(platform) if disable => {
                failure.invoke(|| platform.set_ime_allowed(false));
            }
            TextInputBackend::Pull(host) if host_focused => {
                failure.invoke(|| host.focus_store(None));
            }
            _ => {}
        }
        failure.release(backend);
        if let Some(active) = active {
            let TextInputClient {
                store,
                on_session_start,
            } = active.client;
            failure.retire(store);
            failure.retire(on_session_start);
        }
        for store in retired {
            failure.retire(store);
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
    pub fn attach(&self, client: TextInputClient) -> Result<ClientToken, TextInputError> {
        match self.owner() {
            Ok(owner) => owner.attach(client),
            Err(error) => {
                let mut failure = ClosePanic::for_rejection(self.close_mode.mode());
                failure.retire(client);
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
    /// undo (ADR-0090 amendment item 4).
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
