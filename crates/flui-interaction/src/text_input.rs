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
//! - The platform IME is enabled on the first attach and disabled on the active
//!   detach or explicit owner close.
//! - Platform events are demultiplexed to the presentation before
//!   [`TextInputOwner::dispatch`] is called.

use std::cell::{Cell, RefCell};
use std::num::NonZeroU64;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use flui_platform::traits::PlatformTextInput;
use flui_types::ImeEvent;
use flui_types::geometry::{Bounds, Pixels};

/// Identity returned by [`TextInputHandle::attach`].
///
/// Only the currently active token can detach a connection. This prevents a
/// delayed blur/dispose from field A from closing field B after B replaced A.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientToken(NonZeroU64);

/// Owner-thread callback invoked for an IME event.
pub type ImeEventCallback = Rc<dyn Fn(&ImeEvent)>;

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
    callback: ImeEventCallback,
}

struct OwnerState {
    lifecycle: OwnerLifecycle,
    active: Option<AttachedClient>,
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
    platform: Option<Arc<dyn PlatformTextInput>>, // PORT-CHECK-OK-DYN: direct OS text-input capability owned by one presentation; no intermediary.
    next_token: Cell<NonZeroU64>,
    state: RefCell<OwnerState>,
}

impl TextInputOwner {
    /// Create the text-input owner for one presentation.
    #[must_use]
    pub fn new(
        platform: Option<Arc<dyn PlatformTextInput>>, // PORT-CHECK-OK-DYN: direct presentation OS capability.
    ) -> Rc<Self> {
        Rc::new(Self {
            platform,
            next_token: Cell::new(NonZeroU64::MIN),
            state: RefCell::new(OwnerState {
                lifecycle: OwnerLifecycle::Open,
                active: None,
            }),
        })
    }

    /// Create a weak widget capability tied to this exact owner.
    #[must_use]
    pub fn handle(self: &Rc<Self>) -> TextInputHandle {
        TextInputHandle {
            owner: Rc::downgrade(self),
        }
    }

    fn ensure_open(&self) -> Result<(), TextInputError> {
        if self.state.borrow().lifecycle == OwnerLifecycle::Closed {
            Err(TextInputError::Closed)
        } else {
            Ok(())
        }
    }

    fn attach(&self, callback: ImeEventCallback) -> Result<ClientToken, TextInputError> {
        self.ensure_open()?;
        let platform = self.platform.as_ref().ok_or(TextInputError::Unsupported)?;

        let current = self.next_token.get();
        let next = current
            .get()
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .expect("BUG: text-input client token space exhausted");
        self.next_token.set(next);
        let token = ClientToken(current);

        let enable_platform = {
            let mut state = self.state.borrow_mut();
            let enable_platform = state.active.is_none();
            state.active = Some(AttachedClient { token, callback });
            enable_platform
        };

        if enable_platform {
            platform.set_ime_allowed(true);
        }
        tracing::trace!(token = token.0.get(), "IME client attached");
        Ok(token)
    }

    fn detach(&self, token: ClientToken) -> Result<DetachOutcome, TextInputError> {
        self.ensure_open()?;
        let platform = self.platform.as_ref().ok_or(TextInputError::Unsupported)?;

        let detached = {
            let mut state = self.state.borrow_mut();
            if state
                .active
                .as_ref()
                .is_some_and(|client| client.token == token)
            {
                state.active = None;
                true
            } else {
                false
            }
        };

        if detached {
            platform.set_ime_allowed(false);
            tracing::trace!(token = token.0.get(), "IME client detached");
            Ok(DetachOutcome::Detached)
        } else {
            tracing::trace!(token = token.0.get(), "stale IME detach ignored");
            Ok(DetachOutcome::Stale)
        }
    }

    fn set_cursor_area(&self, area: Bounds<Pixels>) -> Result<(), TextInputError> {
        self.ensure_open()?;
        let platform = self.platform.as_ref().ok_or(TextInputError::Unsupported)?;
        platform.set_ime_cursor_area(area);
        Ok(())
    }

    /// Dispatch a platform event to the active client, if any.
    ///
    /// The callback is cloned out before invocation so it may reentrantly
    /// attach, detach, or close without colliding with a `RefCell` borrow.
    pub fn dispatch(&self, event: &ImeEvent) {
        let callback = {
            let state = self.state.borrow();
            if state.lifecycle == OwnerLifecycle::Closed {
                return;
            }
            state
                .active
                .as_ref()
                .map(|client| Rc::clone(&client.callback))
        };
        if let Some(callback) = callback {
            callback(event);
        }
    }

    /// Close this presentation's text-input owner.
    ///
    /// Closing is idempotent. If a client is active, the exact capability
    /// owned by this presentation is disabled once. Existing weak handles
    /// subsequently return [`TextInputError::Closed`].
    pub fn close(&self) {
        let disable_platform = {
            let mut state = self.state.borrow_mut();
            if state.lifecycle == OwnerLifecycle::Closed {
                return;
            }
            state.lifecycle = OwnerLifecycle::Closed;
            state.active.take().is_some()
        };
        if disable_platform && let Some(platform) = &self.platform {
            platform.set_ime_allowed(false);
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
            .field(
                "active_token",
                &state.active.as_ref().map(|client| client.token),
            )
            .field("platform_supported", &self.platform.is_some())
            .finish()
    }
}

impl Drop for TextInputOwner {
    fn drop(&mut self) {
        let state = self.state.get_mut();
        if state.lifecycle == OwnerLifecycle::Open
            && state.active.take().is_some()
            && let Some(platform) = &self.platform
        {
            // Explicit `PresentationState::close` is the correctness path.
            // Drop is only a non-panicking best-effort guard for abnormal
            // teardown.
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                platform.set_ime_allowed(false);
            }));
            if result.is_err() {
                tracing::error!("platform text-input teardown panicked during owner drop");
            }
        }
        state.lifecycle = OwnerLifecycle::Closed;
    }
}

/// Weak, owner-local text-input capability stored by mounted widgets.
#[derive(Clone)]
pub struct TextInputHandle {
    owner: Weak<TextInputOwner>,
}

impl TextInputHandle {
    fn owner(&self) -> Result<Rc<TextInputOwner>, TextInputError> {
        self.owner.upgrade().ok_or(TextInputError::OwnerGone)
    }

    /// Attach `callback` as this presentation's active IME client.
    pub fn attach(&self, callback: ImeEventCallback) -> Result<ClientToken, TextInputError> {
        self.owner()?.attach(callback)
    }

    /// Detach `token` if it still names the active client.
    pub fn detach(&self, token: ClientToken) -> Result<DetachOutcome, TextInputError> {
        self.owner()?.detach(token)
    }

    /// Update the platform IME candidate/composition area.
    pub fn set_cursor_area(&self, area: Bounds<Pixels>) -> Result<(), TextInputError> {
        self.owner()?.set_cursor_area(area)
    }
}

impl std::fmt::Debug for TextInputHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextInputHandle")
            .field("owner_alive", &self.owner.strong_count().gt(&0))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use flui_types::geometry::{Point, Size, px};
    use parking_lot::Mutex;

    use super::*;

    static_assertions::assert_not_impl_any!(TextInputHandle: Send, Sync);
    static_assertions::assert_not_impl_any!(TextInputOwner: Send, Sync);

    #[derive(Debug, Clone, PartialEq)]
    enum PlatformCall {
        Allowed(bool),
        CursorArea(Bounds<Pixels>),
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

        fn set_ime_cursor_area(&self, area: Bounds<Pixels>) {
            self.calls.lock().push(PlatformCall::CursorArea(area));
        }
    }

    fn owner_with_recorder() -> (Rc<TextInputOwner>, Arc<RecordingTextInput>) {
        let recorder = Arc::new(RecordingTextInput::default());
        let capability: Arc<dyn PlatformTextInput> = recorder.clone(); // PORT-CHECK-OK-DYN: test exercises the real erased OS-capability boundary.
        (TextInputOwner::new(Some(capability)), recorder)
    }

    #[test]
    fn attach_replaces_without_toggling_the_same_presentations_platform() {
        let (owner, platform) = owner_with_recorder();
        let first_events = Rc::new(RefCell::new(Vec::new()));
        let second_events = Rc::new(RefCell::new(Vec::new()));

        let first_sink = Rc::clone(&first_events);
        let first = owner
            .handle()
            .attach(Rc::new(move |event| {
                first_sink.borrow_mut().push(event.clone());
            }))
            .expect("supported presentation");

        let second_sink = Rc::clone(&second_events);
        let second = owner
            .handle()
            .attach(Rc::new(move |event| {
                second_sink.borrow_mut().push(event.clone());
            }))
            .expect("supported presentation");

        assert!(!owner.is_attached(first));
        assert!(owner.is_attached(second));
        assert_eq!(platform.calls(), [PlatformCall::Allowed(true)]);

        owner.dispatch(&ImeEvent::Commit("hello".to_owned()));
        assert!(first_events.borrow().is_empty());
        assert_eq!(
            second_events.borrow().as_slice(),
            [ImeEvent::Commit("hello".to_owned())]
        );
    }

    #[test]
    fn stale_detach_cannot_disable_the_replacement_connection() {
        let (owner, platform) = owner_with_recorder();
        let handle = owner.handle();
        let first = handle.attach(Rc::new(|_| {})).expect("first connection");
        let second = handle
            .attach(Rc::new(|_| {}))
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

    #[test]
    fn cursor_area_targets_the_owned_platform_capability() {
        let (owner, platform) = owner_with_recorder();
        let area = Bounds::new(
            Point::new(px(10.0), px(20.0)),
            Size::new(px(30.0), px(40.0)),
        );

        owner
            .handle()
            .set_cursor_area(area)
            .expect("presentation supports IME");

        assert_eq!(platform.calls(), [PlatformCall::CursorArea(area)]);
    }

    #[test]
    fn unsupported_presentation_returns_a_typed_error() {
        let owner = TextInputOwner::new(None);
        let handle = owner.handle();

        assert_eq!(
            handle.attach(Rc::new(|_| {})),
            Err(TextInputError::Unsupported)
        );
        assert_eq!(
            handle.set_cursor_area(Bounds::default()),
            Err(TextInputError::Unsupported)
        );
    }

    #[test]
    fn closed_state_takes_precedence_over_missing_platform_support() {
        let owner = TextInputOwner::new(None);
        let handle = owner.handle();
        owner.close();

        assert_eq!(handle.attach(Rc::new(|_| {})), Err(TextInputError::Closed));
        assert_eq!(
            handle.set_cursor_area(Bounds::default()),
            Err(TextInputError::Closed)
        );
    }

    #[test]
    fn explicit_close_disables_once_and_makes_handles_inert() {
        let (owner, platform) = owner_with_recorder();
        let handle = owner.handle();
        let token = handle.attach(Rc::new(|_| {})).expect("connection");

        owner.close();
        owner.close();

        assert_eq!(
            platform.calls(),
            [PlatformCall::Allowed(true), PlatformCall::Allowed(false)]
        );
        assert_eq!(handle.detach(token), Err(TextInputError::Closed));
        assert_eq!(handle.attach(Rc::new(|_| {})), Err(TextInputError::Closed));
    }

    #[test]
    fn weak_handle_reports_owner_gone() {
        let handle = {
            let (owner, _) = owner_with_recorder();
            owner.handle()
        };

        assert_eq!(
            handle.attach(Rc::new(|_| {})),
            Err(TextInputError::OwnerGone)
        );
    }

    #[test]
    fn dispatch_with_no_active_client_is_a_no_op() {
        let (owner, _) = owner_with_recorder();
        owner.dispatch(&ImeEvent::Enabled);
    }
}
