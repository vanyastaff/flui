//! Per-presentation semantics enablement and platform accessibility delivery.
//!
//! Replaces the retired process-wide `SemanticsBinding` singleton
//! (`flui-semantics`). Enablement (via [`SemanticsHandle`] ref-counting or a
//! direct platform toggle) and announcement/event delivery are a per-window
//! platform seam, not process-global state: each `PresentationState`
//! (`crate::app::presentation`) owns exactly one [`SemanticsHost`], so two
//! presentations never share an enablement counter or step on each other's
//! platform callback.
//!
//! `accessibility_features` (the OS-level, read-mostly reduced-motion/
//! high-contrast/etc. flags) is process-scoped, not per-presentation, and
//! lives on `SharedEngineServices` (`crate::app::runtime`) instead — see that
//! module's own field for the other half of the retired binding's state.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use flui_semantics::{Assertiveness, SemanticsEvent};
use parking_lot::RwLock;

/// RAII handle that keeps semantics enabled on its owning [`SemanticsHost`]
/// while held.
///
/// Mirrors the retired `SemanticsBinding::SemanticsHandle`'s ref-counting
/// shape, now scoped to one presentation instead of a process-wide
/// singleton.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "constructed via SemanticsHost::ensure_semantics(), which \
                  is itself only reached from tests until a production \
                  caller wires accessibility-handle acquisition through a \
                  presentation"
    )
)]
pub(crate) struct SemanticsHandle {
    counter: Arc<AtomicUsize>,
}

impl SemanticsHandle {
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "see the SemanticsHandle struct's own doc comment")
    )]
    fn new(counter: Arc<AtomicUsize>) -> Self {
        // Relaxed: bare presence counter -- see `SemanticsHost::semantics_enabled`.
        counter.fetch_add(1, Ordering::Relaxed);
        Self { counter }
    }
}

impl Drop for SemanticsHandle {
    fn drop(&mut self) {
        // Relaxed: nothing is freed or torn down when the count reaches
        // zero; collection stopping one frame late is acceptable.
        self.counter.fetch_sub(1, Ordering::Relaxed);
    }
}

impl std::fmt::Debug for SemanticsHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticsHandle")
            .field("active_handles", &self.counter.load(Ordering::Relaxed))
            .finish()
    }
}

/// Per-presentation semantics enablement gate and platform accessibility
/// delivery (announcements + events).
///
/// One instance lives on each [`PresentationState`](super::presentation::PresentationState);
/// there is no process-wide instance and no `instance()`/`is_initialized()`
/// accessor — a presentation always has one from construction.
pub(crate) struct SemanticsHost {
    /// Number of active [`SemanticsHandle`]s.
    handle_count: Arc<AtomicUsize>,

    /// Whether the platform has requested semantics for this presentation.
    ///
    /// `Arc`-wrapped (not a bare `AtomicBool`) so
    /// [`Self::platform_semantics_enabled_handle`] can hand a cheap clone to
    /// the realm's `RenderingFlutterBinding::add_semantics_enabled_listener`
    /// fan-out closure without that closure borrowing this host (which lives
    /// on the same `UiRealm` the renderer does — a self-reference the
    /// closure's `'static` bound forbids). Mirrors `AppRuntime`'s
    /// `needs_redraw` handle-sharing shape for the identical reason.
    platform_semantics_enabled: Arc<AtomicBool>,

    /// Callback for accessibility announcements. `PresentationState::close()`
    /// clears this unconditionally in production (see
    /// [`Self::clear_announce_callback`]); `announce()`'s read side still
    /// has no production caller until a platform embedder wires delivery.
    #[allow(clippy::type_complexity)]
    announce_callback: RwLock<Option<Arc<dyn Fn(&str, Assertiveness) + Send + Sync>>>,

    /// Callback for semantics events dispatched via [`Self::dispatch_event`]/
    /// [`Self::tooltip`]. Set by the platform embedder when the
    /// accessibility surface is brought up; cleared when the platform goes
    /// silent (or the presentation closes — see
    /// [`Self::clear_event_callback`]). Mirrors [`Self::announce_callback`]'s
    /// shape.
    #[allow(clippy::type_complexity)]
    event_callback: RwLock<Option<Arc<dyn Fn(&SemanticsEvent) + Send + Sync>>>,
}

impl SemanticsHost {
    /// Creates a new, disabled semantics host for one presentation.
    pub(crate) fn new() -> Self {
        Self {
            handle_count: Arc::new(AtomicUsize::new(0)),
            platform_semantics_enabled: Arc::new(AtomicBool::new(false)),
            announce_callback: RwLock::new(None),
            event_callback: RwLock::new(None),
        }
    }

    // ========== Semantics Enabled State ==========

    /// Returns whether semantics are currently enabled for this
    /// presentation.
    ///
    /// Semantics are enabled if either:
    /// - The platform has requested semantics
    /// - There are outstanding `SemanticsHandle`s
    pub(crate) fn semantics_enabled(&self) -> bool {
        // Relaxed: this flag/counter pair only gates whether semantics
        // collection runs. No semantics data is published through these
        // atomics (the tree lives behind its own locks), and the frame loop
        // re-reads them every frame, so eventual visibility is enough.
        self.platform_semantics_enabled.load(Ordering::Relaxed)
            || self.handle_count.load(Ordering::Relaxed) > 0
    }

    /// Returns the number of outstanding semantics handles.
    pub(crate) fn outstanding_handles(&self) -> usize {
        self.handle_count.load(Ordering::Relaxed)
    }

    /// Creates a new `SemanticsHandle` and enables semantics collection.
    ///
    /// The returned handle keeps semantics enabled until it is dropped.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no production caller yet -- accessibility-handle \
                      acquisition through a presentation is future wiring"
        )
    )]
    pub(crate) fn ensure_semantics(&self) -> SemanticsHandle {
        SemanticsHandle::new(Arc::clone(&self.handle_count))
    }

    /// Sets whether the platform has requested semantics.
    ///
    /// Called when accessibility services are activated or deactivated for
    /// this presentation's window: `PresentationState::
    /// wire_platform_accessibility` uses it to seed the already-attached
    /// case at construction (the activation listener writes the underlying
    /// flag handle directly for every later transition).
    pub(crate) fn set_platform_semantics_enabled(&self, enabled: bool) {
        // Relaxed: the flag carries no payload -- see `semantics_enabled`.
        self.platform_semantics_enabled
            .store(enabled, Ordering::Relaxed);
    }

    /// Returns whether the platform has requested semantics.
    pub(crate) fn platform_semantics_enabled(&self) -> bool {
        self.platform_semantics_enabled.load(Ordering::Relaxed)
    }

    /// A cheap clone of the platform-enablement flag, for wiring into a
    /// realm's `RenderingFlutterBinding::add_semantics_enabled_listener` fan-out
    /// closure — see this field's own doc for why a handle rather than
    /// borrowing `&self`. Wired at `UiRealm::construct`.
    pub(crate) fn platform_semantics_enabled_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.platform_semantics_enabled)
    }

    // ========== Announcements ==========

    /// Sets the callback for accessibility announcements.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no production caller yet -- the platform-embedder \
                      registration this exists for is future wiring"
        )
    )]
    pub(crate) fn set_announce_callback<F>(&self, callback: F)
    where
        F: Fn(&str, Assertiveness) + Send + Sync + 'static,
    {
        *self.announce_callback.write() = Some(Arc::new(callback));
    }

    /// Remove the registered announce callback — the platform-embedder
    /// teardown half of [`Self::set_announce_callback`]. Called from
    /// `PresentationState::close()` alongside the cursor-change-callback
    /// clear, for the identical reason: a torn-down presentation must not
    /// keep a live platform accessibility-bridge `Arc` pinned past its own
    /// teardown. **Decides announce-after-close semantics**: once cleared,
    /// a stray `announce()`/`dispatch_event()` call that races teardown
    /// falls through to the existing no-callback trace fallback (never a
    /// panic, never a call into a torn-down bridge) — the same degrade path
    /// a host that never registered a callback in the first place already
    /// exercises.
    pub(crate) fn clear_announce_callback(&self) {
        self.announce_callback.write().take();
    }

    /// Announces a message to assistive technology.
    ///
    /// Uses the clone-and-release lock pattern (see [`Self::dispatch_event`]).
    /// Falls back to a privacy-conscious trace when no platform callback has
    /// been registered yet — exactly the state a host embedding FLUI starts
    /// in, before its platform embedder calls
    /// [`set_announce_callback`](Self::set_announce_callback).
    ///
    /// # Arguments
    ///
    /// * `message` - The message to announce.
    /// * `assertiveness` - How urgently to announce the message.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no production caller yet -- accessibility announcements \
                      through a presentation are future wiring"
        )
    )]
    pub(crate) fn announce(&self, message: &str, assertiveness: Assertiveness) {
        let cb = self.announce_callback.read().as_ref().map(Arc::clone);
        if let Some(cb) = cb {
            cb(message, assertiveness);
        } else {
            // `message_len`, never `message`. A live-region announcement is
            // written to be read aloud to the user and routinely carries
            // their data ("Message from Alice: ...", a balance, a
            // validation echo); a tracing field is world-readable in the
            // device log archive.
            tracing::debug!(
                message_len = message.len(),
                assertiveness = ?assertiveness,
                "SemanticsHost::announce (no platform announce callback registered)"
            );
        }
    }

    // ========== Semantics Events ==========

    /// Sets the callback for semantics events dispatched via
    /// [`Self::dispatch_event`]/[`Self::tooltip`].
    ///
    /// Set by the platform embedder when the accessibility surface is
    /// brought up; pass `None` (via re-setting to a no-op closure) when the
    /// platform goes silent. Mirrors [`Self::set_announce_callback`].
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no production caller yet -- the platform-embedder \
                      registration this exists for is future wiring"
        )
    )]
    pub(crate) fn set_event_callback<F>(&self, callback: F)
    where
        F: Fn(&SemanticsEvent) + Send + Sync + 'static,
    {
        *self.event_callback.write() = Some(Arc::new(callback));
    }

    /// Remove the registered event callback — mirrors
    /// [`Self::clear_announce_callback`]'s teardown rationale and
    /// announce-after-close decision, for `dispatch_event`/`tooltip` instead
    /// of `announce`.
    pub(crate) fn clear_event_callback(&self) {
        self.event_callback.write().take();
    }

    /// Dispatches a semantics event to the registered platform callback,
    /// falling back to a privacy-conscious trace when none is registered
    /// yet (mirrors [`Self::announce`]'s fallback).
    ///
    /// Uses the **clone-and-release** lock-handling pattern: the
    /// `Arc<dyn Fn>` is cloned out of the read-lock before the callback
    /// runs, so user code reaching back into this host (e.g. registering
    /// another callback) cannot deadlock on the host's own lock.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no production caller yet -- platform accessibility \
                      event delivery through a presentation is future wiring"
        )
    )]
    pub(crate) fn dispatch_event(&self, event: &SemanticsEvent) {
        let cb = self.event_callback.read().as_ref().map(Arc::clone);
        if let Some(cb) = cb {
            cb(event);
        } else {
            // The type, not the payload: `SemanticsEventData::String` carries
            // tooltip and announcement text, which `?event` would print.
            tracing::debug!(
                event_type = ?event.event_type(),
                "SemanticsHost::dispatch_event (no platform event callback registered)"
            );
        }
    }

    /// Announces a tooltip.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "no production caller yet -- see dispatch_event, which \
                      this forwards to"
        )
    )]
    pub(crate) fn tooltip(&self, message: &str) {
        self.dispatch_event(&SemanticsEvent::tooltip(message));
    }
}

impl Default for SemanticsHost {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for SemanticsHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SemanticsHost")
            .field("semantics_enabled", &self.semantics_enabled())
            .field("outstanding_handles", &self.outstanding_handles())
            .field(
                "platform_semantics_enabled",
                &self.platform_semantics_enabled(),
            )
            .finish_non_exhaustive()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantics_handle_reference_counting() {
        let host = SemanticsHost::new();

        assert!(!host.semantics_enabled());
        assert_eq!(host.outstanding_handles(), 0);

        let handle1 = host.ensure_semantics();
        assert!(host.semantics_enabled());
        assert_eq!(host.outstanding_handles(), 1);

        let handle2 = host.ensure_semantics();
        assert_eq!(host.outstanding_handles(), 2);

        drop(handle1);
        assert!(host.semantics_enabled());
        assert_eq!(host.outstanding_handles(), 1);

        drop(handle2);
        assert!(!host.semantics_enabled());
        assert_eq!(host.outstanding_handles(), 0);
    }

    #[test]
    fn platform_semantics_toggle() {
        let host = SemanticsHost::new();

        assert!(!host.semantics_enabled());

        host.set_platform_semantics_enabled(true);
        assert!(host.semantics_enabled());
        assert!(host.platform_semantics_enabled());

        host.set_platform_semantics_enabled(false);
        assert!(!host.semantics_enabled());
    }

    #[test]
    fn combined_semantics_enabled() {
        let host = SemanticsHost::new();

        // Neither platform nor handles
        assert!(!host.semantics_enabled());

        // Only platform
        host.set_platform_semantics_enabled(true);
        assert!(host.semantics_enabled());

        // Both platform and handle
        let handle = host.ensure_semantics();
        assert!(host.semantics_enabled());

        // Only handle (platform disabled)
        host.set_platform_semantics_enabled(false);
        assert!(host.semantics_enabled());

        // Neither (handle dropped)
        drop(handle);
        assert!(!host.semantics_enabled());
    }

    #[test]
    fn announce_callback_is_invoked_once_registered() {
        use std::sync::atomic::AtomicUsize;

        let host = SemanticsHost::new();
        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_clone = Arc::clone(&call_count);

        host.set_announce_callback(move |_msg, _assertiveness| {
            call_count_clone.fetch_add(1, Ordering::SeqCst);
        });

        host.announce("Test message", Assertiveness::Polite);
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        host.announce("Another message", Assertiveness::Assertive);
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn event_callback_is_invoked_once_registered() {
        use std::sync::atomic::AtomicUsize;

        let host = SemanticsHost::new();
        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_clone = Arc::clone(&call_count);

        host.set_event_callback(move |_event| {
            call_count_clone.fetch_add(1, Ordering::SeqCst);
        });

        host.dispatch_event(&SemanticsEvent::tooltip("hi"));
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        host.dispatch_event(&SemanticsEvent::tooltip("again"));
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn event_callback_clone_and_release_no_deadlock() {
        // Verify the clone-and-release lock pattern: a callback that
        // mutates host state (registers another callback) must not
        // deadlock on the host's own RwLock.
        let host = Arc::new(SemanticsHost::new());
        let host_clone = Arc::clone(&host);

        host.set_event_callback(move |_event| {
            // Reach back into the host from inside the callback. Holding
            // the read lock across this call (instead of clone-and-release)
            // would deadlock on the write attempt below.
            host_clone.set_event_callback(|_| {});
        });

        host.dispatch_event(&SemanticsEvent::tooltip("first"));
        // If we got here, the clone-and-release pattern released the read
        // lock before invoking the callback.
    }

    #[test]
    fn dispatch_event_without_callback_is_a_no_op() {
        // No callback registered -- must not panic.
        let host = SemanticsHost::new();
        host.dispatch_event(&SemanticsEvent::tooltip("nobody home"));
    }

    // ========================================================================
    // Announcement/event privacy: moved from flui-semantics's
    // `announcements_are_not_logged_verbatim.rs` (deleted alongside
    // `SemanticsService`, whose statics these tests used to target). A live
    // region is written to be read aloud to the user, so it routinely
    // carries their data -- a sender's name, a balance, a validation echo. A
    // `tracing` field is world-readable in Apple's unified log and in
    // Android's logcat, which makes "log the announcement" the same thing
    // as "publish it". Both fallbacks under test fire when no platform
    // callback has been registered yet, which is exactly the state a fresh
    // `SemanticsHost` starts in.
    // ========================================================================

    mod privacy {
        use std::sync::Mutex;

        use tracing::field::{Field, Visit};
        use tracing::{Event, Subscriber};
        use tracing_subscriber::layer::{Context, SubscriberExt as _};
        use tracing_subscriber::registry::LookupSpan;
        use tracing_subscriber::{Layer, Registry};

        use super::*;

        /// A string that could not plausibly be anything but the payload.
        const SECRET: &str = "Message from Alice: account balance is 12345";

        #[derive(Clone, Default)]
        struct FieldCapture(Arc<Mutex<Vec<(String, String)>>>);

        struct Collector<'a>(&'a mut Vec<(String, String)>);

        impl Visit for Collector<'_> {
            fn record_str(&mut self, field: &Field, value: &str) {
                self.0.push((field.name().to_owned(), value.to_owned()));
            }

            fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
                self.0.push((field.name().to_owned(), format!("{value:?}")));
            }

            fn record_u64(&mut self, field: &Field, value: u64) {
                self.0.push((field.name().to_owned(), value.to_string()));
            }

            fn record_i64(&mut self, field: &Field, value: i64) {
                self.0.push((field.name().to_owned(), value.to_string()));
            }
        }

        impl<S> Layer<S> for FieldCapture
        where
            S: Subscriber + for<'lookup> LookupSpan<'lookup>,
        {
            fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
                let mut fields = Vec::new();
                event.record(&mut Collector(&mut fields));
                self.0
                    .lock()
                    .expect("BUG: the capture mutex is only locked by this test's own thread")
                    .extend(fields);
            }
        }

        /// Every `name=value` pair emitted while `emit` ran.
        ///
        /// Thread-local, so this touches no process-global subscriber and
        /// stays order-independent alongside every other test in this
        /// module.
        fn captured_fields(emit: impl FnOnce()) -> Vec<(String, String)> {
            let capture = FieldCapture::default();
            tracing::subscriber::with_default(Registry::default().with(capture.clone()), emit);

            capture
                .0
                .lock()
                .expect("BUG: the capture mutex is only locked by this test's own thread")
                .clone()
        }

        fn assert_secret_absent(fields: &[(String, String)], what: &str) {
            for (name, value) in fields {
                assert!(
                    !value.contains(SECRET),
                    "{what} published the announcement verbatim in field `{name}` = {value:?}"
                );
            }
        }

        #[test]
        fn announce_logs_the_length_and_never_the_message() {
            let host = SemanticsHost::new();
            let fields = captured_fields(|| {
                host.announce(SECRET, Assertiveness::Polite);
            });

            assert!(
                !fields.is_empty(),
                "the no-callback-registered fallback is expected to emit an event"
            );
            assert_secret_absent(&fields, "SemanticsHost::announce");

            // The diagnostic still has to be worth emitting: the length is
            // what tells a developer the announcement happened and roughly
            // how big it was.
            let length = fields
                .iter()
                .find(|(name, _)| name == "message_len")
                .map(|(_, value)| value.as_str());
            assert_eq!(
                length,
                Some(SECRET.len().to_string().as_str()),
                "expected a `message_len` field; captured {fields:?}"
            );
        }

        #[test]
        fn send_event_logs_the_type_and_never_the_payload() {
            // `tooltip` builds a `SemanticsEvent` whose data is the string,
            // so a `Debug` of the whole event would print it.
            let host = SemanticsHost::new();
            let fields = captured_fields(|| {
                host.dispatch_event(&SemanticsEvent::tooltip(SECRET));
            });

            assert!(
                !fields.is_empty(),
                "the no-callback-registered fallback is expected to emit an event"
            );
            assert_secret_absent(&fields, "SemanticsHost::dispatch_event");

            assert!(
                fields.iter().any(|(name, _)| name == "event_type"),
                "expected an `event_type` field; captured {fields:?}"
            );
        }

        #[test]
        fn tooltip_routes_through_dispatch_event_without_publishing_the_text() {
            // The convenience wrapper must not have its own logging path.
            let host = SemanticsHost::new();
            let fields = captured_fields(|| {
                host.tooltip(SECRET);
            });

            assert_secret_absent(&fields, "SemanticsHost::tooltip");
        }
    }
}
