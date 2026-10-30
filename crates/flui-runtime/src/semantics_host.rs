//! Per-presentation semantics enablement.
//!
//! Replaces the retired process-wide `SemanticsBinding` singleton
//! (`flui-semantics`). Enablement (via `SemanticsHandle` ref-counting or a
//! direct platform toggle) is a per-window seam, not process-global state:
//! each `PresentationState` (`flui-app`'s `app::presentation`) owns exactly
//! one [`SemanticsHost`], so two presentations never share an enablement
//! counter.
//!
//! System settings come from the installed host's `SystemPreferences` source
//! (ADR-0172). They are independent of semantics collection and platform delivery;
//! this module does not maintain another settings snapshot.
//!
//! Handle acquisition keeps semantics collected while an agent reads the
//! tree: `UiRuntime::semantics_agent` holds one handle for all clones of the
//! agent it vends. Announcements are not here: they are published through
//! the semantics tree's announcer nodes (`flui_semantics::Announcer`).

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

/// RAII handle that keeps semantics enabled on its owning [`SemanticsHost`]
/// while held.
///
/// Mirrors the retired `SemanticsBinding::SemanticsHandle`'s ref-counting
/// shape, now scoped to one presentation instead of a process-wide
/// singleton.
///
/// Constructed only by [`SemanticsHost::ensure_semantics`]; the UI runtime's
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

/// Per-presentation semantics enablement gate.
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
    /// the UI runtime's `RenderingBinding::add_semantics_enabled_listener`
    /// fan-out closure without that closure borrowing this host (which lives
    /// on the same `UiRuntime` the renderer does — a self-reference the
    /// closure's `'static` bound forbids). Mirrors `AppRuntime`'s
    /// `needs_redraw` handle-sharing shape for the identical reason.
    platform_semantics_enabled: Arc<AtomicBool>,

    /// Whether the platform adapter needs a self-contained full republish.
    ///
    /// Set (from the adapter's own thread, via
    /// [`Self::full_republish_handle`]) whenever assistive technology
    /// activate: the adapter's state is unknown at that moment — a freshly
    /// started screen reader has nothing, and `SemanticsOwner::flush`
    /// publishes incrementally, so waiting for the next diff would leave it
    /// with a fragment. Consumed on the owner thread by
    /// [`Self::take_full_republish_request`] during
    /// `PresentationState::reconcile_semantics_enablement`, which routes it
    /// to `PipelineOwner::request_semantics_full_publish`. Same
    /// `Arc`-wrapped shape as `platform_semantics_enabled`, for the same
    /// self-reference reason.
    full_republish_requested: Arc<AtomicBool>,
}

impl SemanticsHost {
    /// Creates a new, disabled semantics host for one presentation.
    #[must_use]
    pub fn new() -> Self {
        Self {
            handle_count: Arc::new(AtomicUsize::new(0)),
            platform_semantics_enabled: Arc::new(AtomicBool::new(false)),
            full_republish_requested: Arc::new(AtomicBool::new(false)),
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
    #[cfg(any(test, feature = "testing"))]
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
    #[cfg(any(test, feature = "testing"))]
    #[doc(hidden)]
    #[must_use]
    pub fn platform_semantics_enabled(&self) -> bool {
        self.platform_semantics_enabled.load(Ordering::Relaxed)
    }

    /// A cheap clone of the platform-enablement flag, for wiring into a
    /// UI runtime's `RenderingBinding::add_semantics_enabled_listener` fan-out
    /// closure — see this field's own doc for why a handle rather than
    /// borrowing `&self`. Wired at `UiRuntime::construct`.
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
