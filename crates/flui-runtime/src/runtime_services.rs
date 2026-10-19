//! What a UI runtime resolves once when it is constructed: the services its host
//! hands it, its own scheduler and the handles derived from it, and a fresh
//! identity.

use std::fmt;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use flui_foundation::{PresentationId, UiRuntimeId};
use flui_painting::{FontCollection, TextContext};
use flui_platform_api::{Clipboard, Storage};
use flui_rendering::TextContextHandle;
use flui_scheduler::{ClockSource, OwnerFrame, UpdateScheduler};

/// What a host hands a [`UiRuntime`](crate::ui_runtime::UiRuntime) it builds: the
/// services the host owns and every UI runtime shares, as opposed to the
/// scheduler and identity each UI runtime makes for itself.
///
/// [`new`](Self::new) takes what every UI runtime needs; the `with_*` methods add
/// what a host may lack.
pub struct RuntimeHostServices<'a> {
    pub(crate) wake: Arc<dyn Fn() + Send + Sync>,
    pub(crate) needs_redraw: Arc<AtomicBool>,
    pub(crate) clipboard: Arc<dyn Clipboard>,
    pub(crate) storage: Option<Arc<dyn Storage>>,
    pub(crate) fonts: &'a FontCollection,
    pub(crate) clock: ClockSource,
    pub(crate) preferences: Option<crate::owner::SystemPreferencesSnapshot>,
}

impl fmt::Debug for RuntimeHostServices<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeHostServices")
            .field("storage", &self.storage.is_some())
            .finish_non_exhaustive()
    }
}

impl<'a> RuntimeHostServices<'a> {
    /// The services every UI runtime needs.
    ///
    /// `wake` is the platform wake: it must deliver a wake to the owner's
    /// event loop without spawning a thread — in production
    /// `AppRuntime::frame_wake_callback()`. `needs_redraw` is a clone of
    /// that same runtime's flag (see `UiRuntime::needs_redraw`).
    ///
    /// `clipboard` is the platform clipboard every presentation of the UI runtime
    /// hands its widgets through `LifecycleContext::clipboard_handle`.
    ///
    /// `fonts` is the app's shared font collection; the UI runtime owns a
    /// `TextContext` built from it (ADR-0092 §3), which lives exactly as long
    /// as the UI runtime.
    ///
    /// `clock` is where the UI runtime reads time: its frame-time origin, every
    /// presentation's gesture-arena deadlines and its frame clock's produce
    /// gate. A host passes [`ClockSource::Platform`]; a headless test driver
    /// passes the [`ClockSource::Manual`] clock it advances by hand.
    #[must_use]
    pub fn new(
        wake: Arc<dyn Fn() + Send + Sync>,
        needs_redraw: Arc<AtomicBool>,
        clipboard: Arc<dyn Clipboard>,
        fonts: &'a FontCollection,
        clock: ClockSource,
    ) -> Self {
        Self {
            wake,
            needs_redraw,
            clipboard,
            storage: None,
            fonts,
            clock,
            preferences: None,
        }
    }

    /// The byte storage every presentation of the UI runtime hands its widgets
    /// through `LifecycleContext::storage`. Without it they get none.
    #[must_use]
    pub fn with_storage(mut self, storage: Arc<dyn Storage>) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Seed this runtime from the same host's latest accepted observation.
    /// First root construction observes this revision; queued older revisions
    /// cannot replace it after the runtime is installed.
    #[must_use]
    pub fn with_preferences(
        mut self,
        preferences: crate::owner::SystemPreferencesSnapshot,
    ) -> Self {
        self.preferences = Some(preferences);
        self
    }
}

/// What [`UiRuntime`](crate::ui_runtime::UiRuntime)'s constructors need to wire it
/// up: the host's services, plus a fresh, UI runtime-owned [`UpdateScheduler`] —
/// the strong root — and the [`OwnerFrame`] made for that SAME scheduler, the
/// only strong owner of the UI runtime's owner-local post-frame callbacks and async
/// tasks (ADR-0136 §2). Resolved once, here, so the UI runtime's own source reaches
/// no process-global scheduler.
pub(crate) struct RuntimeServices {
    pub(crate) preferences: Option<crate::owner::SystemPreferencesSnapshot>,
    pub(crate) owner_frame: OwnerFrame,
    pub(crate) scheduler: UpdateScheduler,
    /// The platform wake the UI runtime's scheduler, presentations and command
    /// sender fire.
    pub(crate) wake: Arc<dyn Fn() + Send + Sync>,
    /// The host's redraw flag, which `wake` also sets.
    pub(crate) needs_redraw: Arc<AtomicBool>,
    /// The platform clipboard every presentation of this UI runtime hands its
    /// widgets (`LifecycleContext::clipboard_handle`).
    pub(crate) clipboard: Arc<dyn Clipboard>,
    /// The byte storage every presentation of this UI runtime hands its widgets
    /// (`LifecycleContext::storage`); `None` when the host gave it none.
    pub(crate) storage: Option<Arc<dyn Storage>>,
    /// Where the UI runtime reads time: its frame-time origin, and every
    /// presentation's gesture arena and frame clock.
    pub(crate) clock: ClockSource,
    /// The UI runtime's text service (ADR-0092 §3): one per UI runtime, built from the
    /// app's [`FontCollection`] and dropped with the UI runtime. Every
    /// presentation's pipeline holds a clone of the handle and lends the
    /// context to its layout (ADR-0092 §10 step 3).
    pub(crate) text: TextContextHandle,
}

impl RuntimeServices {
    /// Builds a brand-new `UpdateScheduler` for a UI runtime about to be
    /// constructed, beside the `host`'s services. Every `UiRuntime`
    /// constructor calls this instead of reaching for a process-global
    /// scheduler — each UI runtime gets its OWN strong root, torn down when the
    /// UI runtime drops — and its own [`TextContext`] over the host's fonts, so a
    /// face registered on the collection reaches this UI runtime as it reaches
    /// every other.
    pub(crate) fn construct(host: RuntimeHostServices<'_>) -> Self {
        let RuntimeHostServices {
            wake,
            needs_redraw,
            clipboard,
            storage,
            fonts,
            clock,
            preferences,
        } = host;
        let scheduler = UpdateScheduler::new();
        Self {
            owner_frame: OwnerFrame::new(&scheduler)
                .expect("BUG: a fresh scheduler has no owner frame"),
            scheduler,
            wake,
            needs_redraw,
            clipboard,
            storage,
            clock,
            preferences,
            text: TextContextHandle::new(TextContext::new(fonts)),
        }
    }
}

/// Monotonic incarnation counter: every successfully constructed UI runtime gets
/// a fresh `UiRuntimeId` generation, so a recreated UI runtime never compares equal
/// to its predecessor.
static NEXT_INCARNATION: AtomicU32 = AtomicU32::new(1);

/// Mints a fresh, process-unique `(UiRuntimeId, PresentationId)` pair. Slot 0 is
/// the single-window slot; a multi-window registry would mint other slots.
pub(crate) fn next_identity() -> (UiRuntimeId, PresentationId) {
    next_identity_with_counter(&NEXT_INCARNATION)
}

fn next_identity_with_counter(counter: &AtomicU32) -> (UiRuntimeId, PresentationId) {
    let incarnation = counter
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            // Zero is permanent exhaustion; MAX remains the last valid generation.
            (current != 0).then(|| current.wrapping_add(1))
        })
        .expect("BUG: ui_runtime incarnation space exhausted");
    let generation = NonZeroU32::new(incarnation)
        .expect("BUG: incarnation counter starts at 1 and only increments");
    (
        UiRuntimeId::new_gen(0, generation),
        PresentationId::new_gen(0, generation),
    )
}

#[cfg(test)]
pub(crate) fn exhausted_incarnations_never_alias_previous_ui_runtimes() {
    // A local counter reaches an otherwise impractical failure boundary without
    // changing the process-global identity source used by unrelated ui_runtime tests.
    let counter = AtomicU32::new(u32::MAX - 1);
    let (before_last, before_last_presentation) = next_identity_with_counter(&counter);
    let (last, last_presentation) = next_identity_with_counter(&counter);
    assert_ne!(before_last, last);
    assert_ne!(before_last_presentation, last_presentation);
    for _ in 0..3 {
        assert!(std::panic::catch_unwind(|| next_identity_with_counter(&counter)).is_err());
    }
    let fresh = AtomicU32::new(1);
    let (ui_runtime, presentation) = next_identity_with_counter(&fresh);
    assert_ne!(ui_runtime, last);
    assert_ne!(presentation, last_presentation);
}
