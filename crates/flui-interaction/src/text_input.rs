//! Platform text input (IME) client registry.
//!
//! [`TextInputRegistry`] tracks the single widget currently receiving IME
//! composition events, using the same owner-thread-singleton shape as
//! [`FocusManager`](crate::routing::FocusManager) and
//! [`MouseTracker`](crate::routing::MouseTracker): `global()` for production
//! call sites (PR2's `EditableText` attaches/detaches through it directly,
//! the same way a focusable widget reaches `FocusManager::global()`), and
//! `new_for_test()` for tests that must not pollute the process-wide
//! singleton.
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
}
