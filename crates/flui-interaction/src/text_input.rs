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
//! - The platform IME is enabled on the first attach and disabled on the active
//!   detach or explicit owner close.
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
//! A platform backend cannot hold the store yet: `PlatformTextInput` is
//! `Send + Sync` and the store is an owner-thread `Rc`, so the pull
//! connection waits for ADR-0082's owner-thread capability split. Until
//! then the only production caller is the push projection here.

use std::cell::{Cell, RefCell};
use std::num::NonZeroU64;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use flui_foundation::geometry::Bounds;
use flui_platform_api::ImeEvent;
use flui_platform_api::PlatformTextInput;
use flui_platform_api::text_store::{CommitGate, TextStore, project_ime_event};

use crate::__runtime::{CloseMode, ClosePanic, CloseTombstone};
use crate::{retain::Retain, routing::RoutePanic};

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
        self.on_session_start.retain();
        self.store.retain();
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OwnerLifecycle {
    Open,
    Closed,
}

struct AttachedClient {
    token: ClientToken,
    client: TextInputClient,
}

/// Retire the independent client owners separately, preserving the first
/// failure. After it, and while the thread is already unwinding, the remaining
/// owners are retained rather than destroyed (ADR-0127).
fn retire_client(client: Option<AttachedClient>, first: &mut Option<RoutePanic>) {
    if let Some(client) = client {
        let TextInputClient {
            store,
            on_session_start,
        } = client.client;
        retire_owner(
            on_session_start,
            first,
            "text-input session callback retirement",
        );
        retire_owner(store, first, "text-input store retirement");
    }
}

fn retire_stores(stores: Vec<Rc<dyn TextStore>>, first: &mut Option<RoutePanic>) {
    for store in stores {
        retire_owner(store, first, "text-input store retirement");
    }
}

fn retire_owner<T: Retain>(owner: T, first: &mut Option<RoutePanic>, phase: &'static str) {
    if first.is_some() || std::thread::panicking() {
        owner.retain();
    } else {
        RoutePanic::preserve_first(first, RoutePanic::capture(|| drop(owner)), phase);
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
/// Construct it with the exact [`PlatformTextInput`] capability obtained from
/// that presentation's surface. A presentation without IME support passes
/// `None`; attempts to attach then return [`TextInputError::Unsupported`].
///
/// The returned `Rc` is intentional: widgets receive weak handles derived from
/// this exact owner, while the presentation retains the only strong ownership.
pub struct TextInputOwner {
    close_mode: CloseTombstone,
    /// Direct OS text-input capability owned by one presentation; no
    /// intermediary. Framework-owned: close releases it even when the rest of
    /// the owner is retained after a failure, since on some backends it keeps
    /// the native window alive.
    platform: RefCell<Option<Arc<dyn PlatformTextInput>>>,
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
    pub fn new(
        platform: Option<Arc<dyn PlatformTextInput>>, // direct presentation OS capability.
    ) -> Rc<Self> {
        Rc::new(Self {
            close_mode: CloseTombstone::default(),
            platform: RefCell::new(platform),
            next_token: Cell::new(NonZeroU64::MIN),
            gate: CommitGate::new(),
            state: RefCell::new(OwnerState {
                lifecycle: OwnerLifecycle::Open,
                active: None,
                retired: Vec::new(),
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

    /// The platform capability, cloned so no borrow spans a call into it.
    fn platform(&self) -> Result<Arc<dyn PlatformTextInput>, TextInputError> {
        self.platform
            .borrow()
            .clone()
            .ok_or(TextInputError::Unsupported)
    }

    fn ensure_open(&self) -> Result<(), TextInputError> {
        if self.state.borrow().lifecycle == OwnerLifecycle::Closed {
            Err(TextInputError::Closed)
        } else {
            Ok(())
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
        if self.platform.borrow().is_none() {
            return Err(TextInputError::Unsupported);
        }

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
        if let Err(error) = self.ensure_open() {
            let mut failure = ClosePanic::for_rejection(self.close_mode.mode());
            failure.retire(client);
            failure.finish();
            return Err(error);
        }
        // Open, so close has not taken the capability.
        let platform = self.platform()?;
        let transaction_open = self.is_transaction_open();
        let (enable_platform, replaced) = {
            let mut state = self.state.borrow_mut();
            let replaced = state.active.replace(AttachedClient { token, client });
            let enable_platform = replaced.is_none();
            if let Some(replaced) = &replaced {
                state.retire(replaced, transaction_open);
            }
            (enable_platform, replaced)
        };

        let mut failure = RoutePanic::capture(|| {
            if enable_platform {
                platform.set_ime_allowed(true);
            }
        });
        // Callback captures and custom stores may reenter through this owner.
        // Both owner state and platform enablement are committed first.
        retire_client(replaced, &mut failure);
        if let Some(failure) = failure {
            failure.resume();
        }
        tracing::trace!(token = token.0.get(), "IME client attached");
        Ok(token)
    }

    fn detach(&self, token: ClientToken) -> Result<DetachOutcome, TextInputError> {
        self.ensure_open()?;
        let platform = self.platform()?;

        let transaction_open = self.is_transaction_open();
        let detached = {
            let mut state = self.state.borrow_mut();
            let active = state.active.take_if(|client| client.token == token);
            if let Some(active) = &active {
                state.retire(active, transaction_open);
            }
            active
        };

        if let Some(detached) = detached {
            let mut failure = RoutePanic::capture(|| platform.set_ime_allowed(false));
            retire_client(Some(detached), &mut failure);
            if let Some(failure) = failure {
                failure.resume();
            }
            tracing::trace!(token = token.0.get(), "IME client detached");
            Ok(DetachOutcome::Detached)
        } else {
            tracing::trace!(token = token.0.get(), "stale IME detach ignored");
            Ok(DetachOutcome::Stale)
        }
    }

    fn set_cursor_area(&self, area: Bounds<f64>) -> Result<(), TextInputError> {
        self.ensure_open()?;
        let platform = self.platform()?;
        platform.set_ime_cursor_area(area);
        Ok(())
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
    pub fn dispatch(&self, event: &ImeEvent) {
        let client = {
            let state = self.state.borrow();
            if state.lifecycle == OwnerLifecycle::Closed {
                return;
            }
            state.active.as_ref().map(|active| active.client.clone())
        };
        let Some(client) = client else {
            return;
        };
        if matches!(event, ImeEvent::Enabled) {
            if let Some(on_session_start) = &client.on_session_start {
                on_session_start();
            }
            return;
        }
        if let Err(error) = project_ime_event(&*client.store, event) {
            tracing::warn!(
                ?error,
                "an IME event could not be applied to the text store"
            );
        }
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

    /// Run the grants queued while commits were closed: first those of the
    /// stores replaced or detached during the frame, in that order, then
    /// the active client's. The composition root calls this once the frame
    /// returns; returns how many ran.
    pub fn run_deferred_grants(&self) -> usize {
        if self.is_transaction_open() {
            // Nothing could run, and the retired stores wait for the anchor
            // that closes this transaction.
            return 0;
        }
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
                let mut failure = None;
                retire_stores(stores, &mut failure);
                if let Some(failure) = failure {
                    failure.resume();
                }
                return ran;
            }
            match RoutePanic::try_run(|| stores[index].run_deferred_grants()) {
                Ok(count) => {
                    ran += count;
                    if self.is_transaction_open() || self.ensure_open().is_err() {
                        // This store may also have a tail behind the newly shut gate.
                        self.retain_pending_stores(&mut stores, index);
                        break;
                    }
                }
                Err(failure) => {
                    // The failed store may still owe grants, as do later stores.
                    // Restore ownership before propagating the first failure.
                    self.retain_pending_stores(&mut stores, index);
                    let mut failure = Some(failure);
                    retire_stores(stores, &mut failure);
                    failure
                        .expect("BUG: the failed grant retains its panic")
                        .resume();
                }
            }
        }
        let mut failure = None;
        retire_stores(stores, &mut failure);
        if let Some(failure) = failure {
            failure.resume();
        }
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

    /// The active client's store: what a pull-model platform backend reads
    /// and edits.
    ///
    /// Hidden rather than feature-gated (a feature would leak it into every
    /// build that unifies it; `design/architecture.md` §5, rule 2): only tests and
    /// the widget test harness call it until the Windows TSF backend, the
    /// first platform consumer of the pull connection, does, and it is
    /// documented then.
    #[doc(hidden)]
    #[must_use]
    pub fn active_store(&self) -> Option<Rc<dyn TextStore>> {
        self.state
            .borrow()
            .active
            .as_ref()
            .map(|active| Rc::clone(&active.client.store))
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
        let (retired, active) = {
            let mut state = self.state.borrow_mut();
            if state.lifecycle == OwnerLifecycle::Closed {
                return;
            }
            state.lifecycle = OwnerLifecycle::Closed;
            (std::mem::take(&mut state.retired), state.active.take())
        };
        let platform = self.platform.borrow_mut().take();
        failure.invoke(|| {
            if active.is_some()
                && let Some(platform) = &platform
            {
                platform.set_ime_allowed(false);
            }
        });
        // Framework-owned: released even when the clients below are retained.
        failure.release(platform);
        if let Some(active) = active {
            let TextInputClient {
                store,
                on_session_start,
            } = active.client;
            failure.retire(on_session_start);
            failure.retire(store);
        }
        for store in retired {
            failure.retire(store);
        }
        failure.finish();
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
            .field("platform_supported", &self.platform.borrow().is_some())
            .finish_non_exhaustive()
    }
}

impl Drop for TextInputOwner {
    fn drop(&mut self) {
        let mut failure = ClosePanic::for_close(CloseMode::Ordinary, self.close_mode.clone());
        let state = self.state.get_mut();
        let disable = state.lifecycle == OwnerLifecycle::Open && state.active.is_some();
        state.lifecycle = OwnerLifecycle::Closed;
        let active = state.active.take();
        let retired = std::mem::take(&mut state.retired);
        // Keep platform custody outside the invocation, including a callback
        // that releases its other last owner before it unwinds.
        let platform = self.platform.get_mut().take();
        failure.invoke(|| {
            if disable && let Some(platform) = &platform {
                platform.set_ime_allowed(false);
            }
        });
        if let Some(active) = active {
            let TextInputClient {
                store,
                on_session_start,
            } = active.client;
            failure.retire(on_session_start);
            failure.retire(store);
        }
        for store in retired {
            failure.retire(store);
        }
        failure.release(platform);
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

    /// Update the platform IME candidate/composition area.
    pub fn set_cursor_area(&self, area: Bounds<f64>) -> Result<(), TextInputError> {
        self.owner()?.set_cursor_area(area)
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
        (TextInputOwner::new(Some(capability)), recorder)
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
