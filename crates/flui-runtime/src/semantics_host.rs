//! Per-presentation semantics enablement and platform accessibility delivery.
//!
//! Replaces the retired process-wide `SemanticsBinding` singleton
//! (`flui-semantics`). Enablement (via `SemanticsHandle` ref-counting or a
//! direct platform toggle) and announcement/event delivery are a per-window
//! platform seam, not process-global state: each `PresentationState`
//! (`flui-app`'s `app::presentation`) owns exactly one [`SemanticsHost`], so
//! two presentations never share an enablement counter or step on each
//! other's platform callback.
//!
//! `accessibility_features` (the OS-level, read-mostly reduced-motion/
//! high-contrast/etc. flags) is process-scoped, not per-presentation, and
//! lives on `SharedEngineServices` (`flui-app`'s `app::runtime`) instead — see
//! that module's own field for the other half of the retired binding's state.
//!
//! Handle acquisition keeps semantics collected while an agent reads the
//! tree: `UiRealm::semantics_agent` holds one handle for all clones of the
//! agent it vends. Announcements and event delivery have no production
//! caller yet: they are compiled only for tests and the `test-support`
//! feature until a platform embedder wires them through a presentation.

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
///
/// Constructed only by [`SemanticsHost::ensure_semantics`]; the realm's
/// semantics agent holds one while any clone of it is alive.
pub(crate) struct SemanticsHandle {
    counter: Arc<AtomicUsize>,
}

impl SemanticsHandle {
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
/// One instance lives on each `PresentationState` (in `flui-app`); there is
/// no process-wide instance and no `instance()`/`is_initialized()` accessor —
/// a presentation always has one from construction.
pub struct SemanticsHost {
    /// Number of active `SemanticsHandle`s.
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

    /// Whether the platform adapter needs a self-contained full republish.
    ///
    /// Set (from the adapter's own thread, via
    /// [`Self::full_republish_handle`]) whenever assistive technology
    /// activates: the adapter's state is unknown at that moment — a freshly
    /// started screen reader has nothing, and `SemanticsOwner::flush`
    /// publishes incrementally, so waiting for the next diff would leave it
    /// with a fragment. Consumed on the owner thread by
    /// [`Self::take_full_republish_request`] during
    /// `PresentationState::reconcile_semantics_enablement`, which routes it
    /// to `PipelineOwner::request_semantics_full_publish`. Same
    /// `Arc`-wrapped shape as `platform_semantics_enabled`, for the same
    /// self-reference reason.
    full_republish_requested: Arc<AtomicBool>,

    /// Callback for accessibility announcements. `PresentationState::close()`
    /// clears this unconditionally in production (see
    /// [`Self::clear_announce_callback`]); `announce()`'s read side still
    /// has no production caller until a platform embedder wires delivery.
    #[expect(clippy::type_complexity)]
    announce_callback: RwLock<Option<Arc<dyn Fn(&str, Assertiveness) + Send + Sync>>>,

    /// Callback for semantics events dispatched via `Self::dispatch_event`/
    /// `Self::tooltip`. Set by the platform embedder when the
    /// accessibility surface is brought up; cleared when the platform goes
    /// silent (or the presentation closes — see
    /// [`Self::clear_event_callback`]). Mirrors [`Self::announce_callback`]'s
    /// shape.
    #[expect(clippy::type_complexity)]
    event_callback: RwLock<Option<Arc<dyn Fn(&SemanticsEvent) + Send + Sync>>>,
}

impl SemanticsHost {
    /// Creates a new, disabled semantics host for one presentation.
    #[must_use]
    pub fn new() -> Self {
        Self {
            handle_count: Arc::new(AtomicUsize::new(0)),
            platform_semantics_enabled: Arc::new(AtomicBool::new(false)),
            full_republish_requested: Arc::new(AtomicBool::new(false)),
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
    #[must_use]
    pub fn semantics_enabled(&self) -> bool {
        // Relaxed: this flag/counter pair only gates whether semantics
        // collection runs. No semantics data is published through these
        // atomics (the tree lives behind its own locks), and the frame loop
        // re-reads them every frame, so eventual visibility is enough.
        self.platform_semantics_enabled.load(Ordering::Relaxed)
            || self.handle_count.load(Ordering::Relaxed) > 0
    }

    /// Returns the number of outstanding semantics handles.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    #[must_use]
    pub fn outstanding_handles(&self) -> usize {
        self.handle_count.load(Ordering::Relaxed)
    }

    /// Creates a new `SemanticsHandle` and enables semantics collection.
    ///
    /// The returned handle keeps semantics enabled until it is dropped.
    #[must_use]
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
    pub fn set_platform_semantics_enabled(&self, enabled: bool) {
        // Relaxed: the flag carries no payload -- see `semantics_enabled`.
        self.platform_semantics_enabled
            .store(enabled, Ordering::Relaxed);
    }

    /// Returns whether the platform has requested semantics.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    #[must_use]
    pub fn platform_semantics_enabled(&self) -> bool {
        self.platform_semantics_enabled.load(Ordering::Relaxed)
    }

    /// A cheap clone of the platform-enablement flag, for wiring into a
    /// realm's `RenderingFlutterBinding::add_semantics_enabled_listener` fan-out
    /// closure — see this field's own doc for why a handle rather than
    /// borrowing `&self`. Wired at `UiRealm::construct`.
    #[must_use]
    pub fn platform_semantics_enabled_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.platform_semantics_enabled)
    }

    /// A cheap clone of the full-republish request flag, for the activation
    /// listener (which runs on the adapter's thread and can only touch
    /// `Send + Sync` state). See the field's own doc for the contract.
    #[must_use]
    pub fn full_republish_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.full_republish_requested)
    }

    /// Consumes a pending full-republish request, if any — the owner-thread
    /// half of the activation seam. Returns `true` at most once per
    /// request.
    pub fn take_full_republish_request(&self) -> bool {
        self.full_republish_requested.swap(false, Ordering::Relaxed)
    }

    // ========== Announcements ==========

    /// Sets the callback for accessibility announcements.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn set_announce_callback<F>(&self, callback: F)
    where
        F: Fn(&str, Assertiveness) + Send + Sync + 'static,
    {
        let _prev = self.announce_callback.write().replace(Arc::new(callback));
    }

    /// Remove the registered announce callback — the platform-embedder
    /// teardown half of `Self::set_announce_callback`. Called from
    /// `PresentationState::close()` alongside the cursor-change-callback
    /// clear, for the identical reason: a torn-down presentation must not
    /// keep a live platform accessibility-bridge `Arc` pinned past its own
    /// teardown. **Decides announce-after-close semantics**: once cleared,
    /// a stray `announce()`/`dispatch_event()` call that races teardown
    /// falls through to the existing no-callback trace fallback (never a
    /// panic, never a call into a torn-down bridge) — the same degrade path
    /// a host that never registered a callback in the first place already
    /// exercises.
    pub fn clear_announce_callback(&self) {
        let _prev = self.announce_callback.write().take();
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
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn announce(&self, message: &str, assertiveness: Assertiveness) {
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
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn set_event_callback<F>(&self, callback: F)
    where
        F: Fn(&SemanticsEvent) + Send + Sync + 'static,
    {
        let _prev = self.event_callback.write().replace(Arc::new(callback));
    }

    /// Remove the registered event callback — mirrors
    /// [`Self::clear_announce_callback`]'s teardown rationale and
    /// announce-after-close decision, for `dispatch_event`/`tooltip` instead
    /// of `announce`.
    pub fn clear_event_callback(&self) {
        let _prev = self.event_callback.write().take();
    }

    /// Dispatches a semantics event to the registered platform callback,
    /// falling back to a privacy-conscious trace when none is registered
    /// yet (mirrors [`Self::announce`]'s fallback).
    ///
    /// Uses the **clone-and-release** lock-handling pattern: the
    /// `Arc<dyn Fn>` is cloned out of the read-lock before the callback
    /// runs, so user code reaching back into this host (e.g. registering
    /// another callback) cannot deadlock on the host's own lock.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn dispatch_event(&self, event: &SemanticsEvent) {
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
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn tooltip(&self, message: &str) {
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
            .field(
                "outstanding_handles",
                &self.handle_count.load(Ordering::Relaxed),
            )
            .field(
                "platform_semantics_enabled",
                &self.platform_semantics_enabled.load(Ordering::Relaxed),
            )
            .finish_non_exhaustive()
    }
}

// ============================================================================
// TESTS
// ============================================================================
