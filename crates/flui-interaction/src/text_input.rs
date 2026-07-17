//! Platform text input (IME) client registry.
//!
//! [`TextInputRegistry`] tracks the single widget currently receiving IME
//! composition events, using the same owner-thread-singleton shape as
//! [`FocusManager`](crate::routing::FocusManager) and
//! [`MouseTracker`](crate::routing::MouseTracker): `global()` for production
//! call sites, and `new_for_test()` for tests that must not pollute the
//! process-wide singleton. `flui-widgets`' `EditableText` never calls
//! `global()` itself — attaching a client must also toggle the platform's
//! `set_ime_allowed`, which only `flui-app` can reach, so `EditableText`
//! goes through [`TextInputHandle`], the binding-installed capability that
//! wraps `flui-app`'s own attach/detach and forwards to this registry from
//! there. See [`TextInputHandle`]'s doc for the full seam.
//!
//! # Why this crate, not `flui-platform`
//!
//! The IME *platform capability* (`set_ime_allowed`/`set_ime_cursor_area`)
//! lives on `flui_platform::traits::PlatformTextInput` — `flui-interaction`
//! cannot depend on `flui-platform` (both are L2 substrate crates in
//! `docs/FOUNDATIONS.md`'s target layer graph; `interaction --> platform`
//! is not a drawn edge, and adding one needs its own ADR, not a
//! side-effect of this feature). `TextInputRegistry` therefore does not
//! name `PlatformWindow`/`PlatformTextInput` at all — see
//! [`OpaqueWindowHandle`] for how it still carries a window identity
//! across that boundary. `flui-app` (which depends on both crates) is the
//! only production caller that constructs and downcasts the handle; see
//! the "Platform text input (IME) capability" ADR for the full rationale.
//!
//! # Single active client, attach-replaces
//!
//! Only one client can receive IME events at a time (Flutter parity: one
//! `TextInputConnection` is "current" at a time). [`TextInputRegistry::attach`]
//! always replaces whatever was previously active — a newly focused text
//! field does not need to coordinate with the field it is replacing.
//!
//! # Identity tokens kill the stale-detach race
//!
//! [`TextInputRegistry::detach`] is a no-op unless the token passed is still
//! the *currently attached* client's token. Without this, the following
//! interleaving would disable IME for the wrong field:
//!
//! 1. Field A attaches (`token_a`).
//! 2. Field B gains focus and attaches (`token_b`), replacing A.
//! 3. Field A's (now-stale) blur/dispose handler fires and calls
//!    `detach(token_a)`.
//!
//! Without the token guard, step 3 would clear B's active registration and
//! (via `flui-app`'s bridge) disable platform IME while B still has focus.
//! With the guard, `detach(token_a)` at step 3 is a harmless no-op because
//! the active client's token is `token_b`.
//!
//! # Suppression contract (PR2 deferral)
//!
//! See [`flui_types::ImeEvent`]'s type-level doc for the full contract a
//! client must implement once it exists: suppress `Key::Character`
//! insertion only while a composition is non-empty, strip the composing
//! slice on a mid-composition [`flui_types::ImeEvent::Disabled`], and
//! detach on dispose. This registry only tracks *which* client is active
//! and *delivers* events to it — it does not implement or enforce that
//! contract, which is `EditableText`'s job (PR2).
//!
//! # Per-window routing (deferred)
//!
//! [`TextInputRegistry`] carries the attaching window's opaque handle (the
//! private `AttachedClient::window` field) but V1 routes every dispatched
//! event to the single global active client regardless of which window it
//! came from.
//! Real multi-window preedit routing is deferred to when `UiRealm`'s
//! realm-scoped ownership (ADR-0027) reaches this registry — the handle is
//! recorded now so that migration is not a breaking signature change.

use std::any::Any;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use flui_types::ImeEvent;
use parking_lot::RwLock;

/// Attach half of [`TextInputHandle`]'s installed bridge — the signature
/// matches `flui-app`'s `AppBinding::attach_text_input` exactly, so that
/// method can be installed directly, closure-wrapped.
type TextInputAttachFn = dyn Fn(ImeEventCallback) -> Option<ClientToken> + Send + Sync;
/// Detach half of [`TextInputHandle`]'s installed bridge — matches
/// `AppBinding::detach_text_input`'s signature.
type TextInputDetachFn = dyn Fn(ClientToken) + Send + Sync;

/// Type-erased handle to the platform window a client attached from.
///
/// See the module doc's "Why this crate, not `flui-platform`" section: this
/// crate cannot name `Arc<dyn flui_platform::traits::PlatformWindow>`
/// directly, so callers erase it behind [`Any`] and recover it with
/// [`OpaqueWindowHandle::downcast_ref`]. `flui-app` is the only production
/// caller and always wraps `Arc<dyn PlatformWindow>`.
#[derive(Clone)]
pub struct OpaqueWindowHandle(Arc<dyn Any + Send + Sync>);

impl OpaqueWindowHandle {
    /// Wrap a concrete window handle.
    pub fn new<T: Send + Sync + 'static>(window: T) -> Self {
        Self(Arc::new(window))
    }

    /// Attempt to recover the concrete handle type `T` the caller wrapped.
    #[must_use]
    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.0.downcast_ref()
    }
}

impl std::fmt::Debug for OpaqueWindowHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("OpaqueWindowHandle").finish_non_exhaustive()
    }
}

/// Identity token returned by [`TextInputRegistry::attach`].
///
/// [`TextInputRegistry::detach`] is a no-op unless the token passed is still
/// the active client's token — see the module doc's "Identity tokens" section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientToken(u64);

/// Owner-thread callback invoked with each [`ImeEvent`] delivered to the
/// active client. Mirrors
/// [`KeyEventCallback`](crate::routing::KeyEventCallback)'s shape:
/// `Rc<dyn Fn>`, not `Send`-bound, because IME dispatch happens on the same
/// owner thread that owns the focus tree.
pub type ImeEventCallback = Rc<dyn Fn(&ImeEvent)>;

/// Binding-installed capability that lets a mounted widget attach/detach an
/// IME client without `flui-interaction` (or `flui-widgets`, which sits below
/// `flui-app` in the crate dependency graph and cannot name `AppBinding`
/// directly) depending on `flui-app`.
///
/// This closes the same crate-boundary gap [`OpaqueWindowHandle`] closes for
/// window identity, but for the *reverse* direction: attaching a client here
/// must also toggle the platform's `PlatformTextInput::set_ime_allowed`,
/// which only `flui-app`'s `ImeBackend` can reach (it is the crate that
/// depends on both `flui-interaction` and `flui-platform`). `TextInputHandle`
/// is the capability-injection seam that lets a widget reach that
/// binding-owned behavior anyway — the same shape `flui_scheduler`'s
/// `PostFrameHandle`/`AsyncDriver` already use (in `flui-view`'s
/// `BuildContext`, which depends on `flui-scheduler` — this crate does not,
/// so those types are named here in prose, not as an intra-doc link) to let
/// a `BuildContext` reach a binding capability without an upward dependency
/// edge: a binding constructs a `TextInputHandle` wrapping its own
/// `attach_text_input`/`detach_text_input` methods and installs it once
/// (`BuildOwner::set_text_input_handle`, wired from `flui-app`'s
/// `UiRealm::bind_to_app`); `BuildContext::text_input_handle` hands out
/// clones to any mounted widget that needs one.
///
/// Acquire it from a lifecycle hook (`ViewState::init_state` /
/// `did_change_dependencies`), the same rule `PostFrameHandle` follows —
/// this is not itself a frame capability (attaching does not schedule a
/// rebuild), but the *token* stored from it must live in owned state, not be
/// re-derived per build.
#[derive(Clone)]
pub struct TextInputHandle {
    attach: Arc<TextInputAttachFn>,
    detach: Arc<TextInputDetachFn>,
}

impl TextInputHandle {
    /// Wrap a binding's own attach/detach methods. `attach` returns `None`
    /// exactly when the binding cannot honor an attach yet (e.g. no active
    /// window), matching `AppBinding::attach_text_input`'s own `None`.
    #[must_use]
    pub fn new(
        attach: impl Fn(ImeEventCallback) -> Option<ClientToken> + Send + Sync + 'static,
        detach: impl Fn(ClientToken) + Send + Sync + 'static,
    ) -> Self {
        Self {
            attach: Arc::new(attach),
            detach: Arc::new(detach),
        }
    }

    /// Attach `callback` as the active IME client through the installed
    /// binding. `None` when the binding could not honor the attach (no
    /// active window yet).
    #[must_use]
    pub fn attach(&self, callback: ImeEventCallback) -> Option<ClientToken> {
        (self.attach)(callback)
    }

    /// Detach the client identified by `token` through the installed
    /// binding. A no-op if `token` is already stale — see
    /// [`TextInputRegistry::detach`]'s stale-token guard, which the
    /// installed binding forwards to.
    pub fn detach(&self, token: ClientToken) {
        (self.detach)(token);
    }
}

impl std::fmt::Debug for TextInputHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextInputHandle").finish_non_exhaustive()
    }
}

struct AttachedClient {
    token: ClientToken,
    callback: ImeEventCallback,
    /// See the module doc's "Per-window routing (deferred)" section —
    /// recorded now, not yet consulted by [`TextInputRegistry::dispatch`].
    #[allow(
        dead_code,
        reason = "recorded for the deferred per-window routing migration"
    )]
    window: OpaqueWindowHandle,
}

/// Single-active-client registry for platform IME (text input) events.
///
/// See the module doc for the ambient-singleton pattern, the attach-replaces
/// rule, and the identity-token detach guard.
pub struct TextInputRegistry {
    next_token: AtomicU64,
    active: RwLock<Option<AttachedClient>>,
}

impl Default for TextInputRegistry {
    fn default() -> Self {
        Self {
            next_token: AtomicU64::new(1),
            active: RwLock::new(None),
        }
    }
}

impl std::fmt::Debug for TextInputRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextInputRegistry")
            .field(
                "active_token",
                &self.active.read().as_ref().map(|client| client.token),
            )
            .finish_non_exhaustive()
    }
}

impl TextInputRegistry {
    /// Get the global text-input registry instance.
    ///
    /// Owner-thread singleton, matching
    /// [`FocusManager::global`](crate::routing::FocusManager::global):
    /// the same instance is returned every time on the current thread.
    pub fn global() -> &'static TextInputRegistry {
        thread_local! {
            static INSTANCE: &'static TextInputRegistry =
                Box::leak(Box::new(TextInputRegistry::default()));
        }
        INSTANCE.with(|registry| *registry)
    }

    /// Create a fresh, non-global registry (for testing).
    ///
    /// Normally you should use [`Self::global`] instead — a test using this
    /// constructor cannot pollute (or be polluted by) other tests sharing
    /// the process-wide singleton.
    #[cfg(test)]
    pub(crate) fn new_for_test() -> Self {
        Self::default()
    }

    /// Attach a new active client, replacing whatever was previously
    /// active. Returns the [`ClientToken`] identifying this attachment for
    /// a later [`Self::detach`] call.
    pub fn attach(&self, window: OpaqueWindowHandle, callback: ImeEventCallback) -> ClientToken {
        let token = ClientToken(self.next_token.fetch_add(1, Ordering::Relaxed));
        *self.active.write() = Some(AttachedClient {
            token,
            callback,
            window,
        });
        tracing::trace!(token = token.0, "IME client attached");
        token
    }

    /// Detach the client identified by `token`.
    ///
    /// Returns `true` if `token` was the active client (and detach actually
    /// happened), `false` if `token` was already stale — see the module
    /// doc's "Identity tokens kill the stale-detach race" section. A stale
    /// detach is a no-op, not an error: the race it guards against is
    /// expected, not exceptional.
    pub fn detach(&self, token: ClientToken) -> bool {
        let mut active = self.active.write();
        if active.as_ref().is_some_and(|client| client.token == token) {
            *active = None;
            tracing::trace!(token = token.0, "IME client detached");
            true
        } else {
            tracing::trace!(token = token.0, "stale IME detach ignored");
            false
        }
    }

    /// Dispatch an event to the active client, if any.
    pub fn dispatch(&self, event: &ImeEvent) {
        // Clone the callback out from under the read lock so a reentrant
        // attach/detach inside the callback (a field reacting to its own
        // commit by blurring) cannot deadlock against `self.active`.
        let callback = self
            .active
            .read()
            .as_ref()
            .map(|client| client.callback.clone());
        if let Some(callback) = callback {
            callback(event);
        }
    }

    /// Whether `token` currently names the active client.
    #[must_use]
    pub fn is_attached(&self, token: ClientToken) -> bool {
        self.active
            .read()
            .as_ref()
            .is_some_and(|client| client.token == token)
    }

    /// The number of currently active clients (0 or 1) — test-only
    /// introspection, matching
    /// [`FocusManager::listener_count`](crate::routing::FocusManager::listener_count)'s
    /// rationale: proves a detach path actually cleared the process-wide
    /// singleton instead of leaking a stale registration.
    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    pub fn active_count(&self) -> usize {
        usize::from(self.active.read().is_some())
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    #[test]
    fn attach_replaces_the_previously_active_client_without_an_explicit_detach() {
        let registry = TextInputRegistry::new_for_test();
        let a_events = Rc::new(RefCell::new(Vec::new()));
        let b_events = Rc::new(RefCell::new(Vec::new()));

        let a_sink = Rc::clone(&a_events);
        let token_a = registry.attach(
            OpaqueWindowHandle::new(1u32),
            Rc::new(move |event: &ImeEvent| a_sink.borrow_mut().push(event.clone())),
        );
        assert!(registry.is_attached(token_a));

        let b_sink = Rc::clone(&b_events);
        let token_b = registry.attach(
            OpaqueWindowHandle::new(2u32),
            Rc::new(move |event: &ImeEvent| b_sink.borrow_mut().push(event.clone())),
        );

        assert!(!registry.is_attached(token_a), "A's token is now stale");
        assert!(registry.is_attached(token_b));
        assert_eq!(
            registry.active_count(),
            1,
            "attach replaces, it doesn't stack"
        );

        registry.dispatch(&ImeEvent::Commit("hi".to_string()));
        assert!(
            a_events.borrow().is_empty(),
            "the replaced client hears nothing"
        );
        assert_eq!(
            b_events.borrow().as_slice(),
            [ImeEvent::Commit("hi".to_string())]
        );
    }

    /// The stale-blur interleaving named in the module doc: field A attaches,
    /// field B replaces it, and A's now-stale detach must not disable IME
    /// for B.
    ///
    /// Red-check: drop the token guard in `detach` (unconditionally clear
    /// `active`) — this test's final `is_attached(token_b)` assertion fails.
    #[test]
    fn a_stale_detach_from_a_replaced_token_does_not_disturb_the_new_active_client() {
        let registry = TextInputRegistry::new_for_test();

        let token_a = registry.attach(OpaqueWindowHandle::new(1u32), Rc::new(|_: &ImeEvent| {}));
        let token_b = registry.attach(OpaqueWindowHandle::new(2u32), Rc::new(|_: &ImeEvent| {}));

        let detached = registry.detach(token_a);
        assert!(!detached, "a stale token reports no-op, not success");
        assert!(
            registry.is_attached(token_b),
            "B must still be active after A's stale detach"
        );
        assert_eq!(registry.active_count(), 1);
    }

    #[test]
    fn detach_from_the_active_token_actually_clears_it() {
        let registry = TextInputRegistry::new_for_test();
        let token = registry.attach(OpaqueWindowHandle::new(1u32), Rc::new(|_: &ImeEvent| {}));

        let detached = registry.detach(token);
        assert!(detached);
        assert!(!registry.is_attached(token));
        assert_eq!(registry.active_count(), 0);

        // Detaching again (the same, now-stale token) is a harmless no-op.
        assert!(!registry.detach(token));
    }

    #[test]
    fn dispatch_with_no_active_client_does_nothing() {
        let registry = TextInputRegistry::new_for_test();
        // Must not panic with nothing attached.
        registry.dispatch(&ImeEvent::Enabled);
    }

    #[test]
    fn dispatch_delivers_the_exact_event_payload_to_the_active_client() {
        let registry = TextInputRegistry::new_for_test();
        let received = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&received);
        registry.attach(
            OpaqueWindowHandle::new(1u32),
            Rc::new(move |event: &ImeEvent| sink.borrow_mut().push(event.clone())),
        );

        registry.dispatch(&ImeEvent::Enabled);
        registry.dispatch(&ImeEvent::Preedit {
            text: "ni".to_string(),
            cursor: Some((0, 2)),
        });
        registry.dispatch(&ImeEvent::Commit("你好".to_string()));
        registry.dispatch(&ImeEvent::Disabled);

        assert_eq!(
            received.borrow().as_slice(),
            [
                ImeEvent::Enabled,
                ImeEvent::Preedit {
                    text: "ni".to_string(),
                    cursor: Some((0, 2)),
                },
                ImeEvent::Commit("你好".to_string()),
                ImeEvent::Disabled,
            ]
        );
    }

    #[test]
    fn opaque_window_handle_recovers_its_concrete_type_and_rejects_a_mismatched_one() {
        let handle = OpaqueWindowHandle::new(42u32);
        assert_eq!(handle.downcast_ref::<u32>(), Some(&42));
        assert_eq!(handle.downcast_ref::<String>(), None);
    }

    // ------------------------------------------------------------------
    // TextInputHandle — the binding-installed capability seam
    // ------------------------------------------------------------------

    /// `TextInputHandle::attach`/`detach` forward to exactly the closures the
    /// binding installed, with the exact arguments — proving the wrapper adds
    /// no translation bugs of its own.
    ///
    /// The recorded state is `Arc`/atomic-based, not `Rc`/`RefCell`: a real
    /// binding's closures capture nothing (they call a zero-capture
    /// `AppBinding::instance()` accessor, `Send + Sync` trivially — see
    /// `TextInputHandle`'s doc), so this test's captured state must satisfy
    /// the same bound `TextInputHandle::new` requires. A fresh
    /// `TextInputRegistry` mints the real `ClientToken` used below, kept
    /// outside the closures (a live registry itself is `Rc`-based and not
    /// `Send`).
    ///
    /// Red-check: swap `(self.attach)(callback)` for a call that drops the
    /// callback or ignores the return value — this test's assertions on both
    /// the recorded call count and the forwarded token fail.
    #[test]
    fn text_input_handle_forwards_attach_and_detach_to_the_installed_closures() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let registry = TextInputRegistry::new_for_test();
        let minted_token = registry.attach(OpaqueWindowHandle::new(()), Rc::new(|_: &ImeEvent| {}));

        let attach_calls = Arc::new(AtomicUsize::new(0));
        let detach_calls: Arc<parking_lot::Mutex<Vec<ClientToken>>> =
            Arc::new(parking_lot::Mutex::new(Vec::new()));

        let attach_calls_for_closure = Arc::clone(&attach_calls);
        let detach_calls_for_closure = Arc::clone(&detach_calls);
        let handle = TextInputHandle::new(
            move |_callback: ImeEventCallback| {
                attach_calls_for_closure.fetch_add(1, Ordering::Relaxed);
                Some(minted_token)
            },
            move |token: ClientToken| detach_calls_for_closure.lock().push(token),
        );

        let token = handle
            .attach(Rc::new(|_event: &ImeEvent| {}))
            .expect("the installed attach closure returns Some");
        assert_eq!(attach_calls.load(Ordering::Relaxed), 1);
        assert_eq!(token, minted_token);

        handle.detach(token);
        assert_eq!(detach_calls.lock().as_slice(), [minted_token]);
    }

    #[test]
    fn text_input_handle_debug_does_not_panic() {
        let handle = TextInputHandle::new(|_callback| None, |_token| {});
        assert_eq!(format!("{handle:?}"), "TextInputHandle { .. }");
    }
}
