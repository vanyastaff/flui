//! What a realm resolves once when it is constructed: the services its host
//! hands it, its own scheduler and the handles derived from it, and a fresh
//! identity.

use std::fmt;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use flui_foundation::{PresentationId, RealmId};
use flui_painting::{FontCollection, TextContext};
use flui_platform_api::{Clipboard, Storage};
use flui_rendering::TextContextHandle;
use flui_scheduler::{AsyncDriver, ClockSource, LocalPostFrameLane, UpdateScheduler};

/// What a host hands a [`UiRealm`](crate::ui_realm::UiRealm) it builds: the
/// services the host owns and every realm shares, as opposed to the
/// scheduler and identity each realm makes for itself.
///
/// [`new`](Self::new) takes what every realm needs; the `with_*` methods add
/// what a host may lack.
pub struct RealmHostServices<'a> {
    pub(crate) wake: Arc<dyn Fn() + Send + Sync>,
    pub(crate) needs_redraw: Arc<AtomicBool>,
    pub(crate) clipboard: Arc<dyn Clipboard>,
    pub(crate) storage: Option<Arc<dyn Storage>>,
    pub(crate) fonts: &'a FontCollection,
    pub(crate) clock: ClockSource,
}

impl fmt::Debug for RealmHostServices<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RealmHostServices")
            .field("storage", &self.storage.is_some())
            .finish_non_exhaustive()
    }
}

impl<'a> RealmHostServices<'a> {
    /// The services every realm needs.
    ///
    /// `wake` is the platform wake: it must deliver a wake to the owner's
    /// event loop without spawning a thread — in production
    /// `AppRuntime::frame_wake_callback()`. `needs_redraw` is a clone of
    /// that same runtime's flag (see `UiRealm::needs_redraw`).
    ///
    /// `clipboard` is the platform clipboard every presentation of the realm
    /// hands its widgets through `LifecycleContext::clipboard_handle`.
    ///
    /// `fonts` is the app's shared font collection; the realm owns a
    /// `TextContext` built from it (ADR-0092 §3), which lives exactly as long
    /// as the realm.
    ///
    /// `clock` is where the realm reads time: its frame-time origin, every
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
        }
    }

    /// The byte storage every presentation of the realm hands its widgets
    /// through `LifecycleContext::storage`. Without it they get none.
    #[must_use]
    pub fn with_storage(mut self, storage: Arc<dyn Storage>) -> Self {
        self.storage = Some(storage);
        self
    }
}

/// What [`UiRealm`](crate::ui_realm::UiRealm)'s constructors need to wire it
/// up: the host's services, plus a fresh, realm-owned [`UpdateScheduler`] —
/// the strong root — and the `local_post_frame_lane()` and `async_driver()`
/// handles derived from that SAME scheduler. Resolved once, here, so the
/// realm's own source reaches no process-global scheduler.
pub(crate) struct RealmServices {
    pub(crate) local_post_frame: LocalPostFrameLane,
    pub(crate) async_driver: AsyncDriver,
    pub(crate) scheduler: UpdateScheduler,
    /// The platform wake the realm's scheduler, presentations and command
    /// sender fire.
    pub(crate) wake: Arc<dyn Fn() + Send + Sync>,
    /// The host's redraw flag, which `wake` also sets.
    pub(crate) needs_redraw: Arc<AtomicBool>,
    /// The platform clipboard every presentation of this realm hands its
    /// widgets (`LifecycleContext::clipboard_handle`).
    pub(crate) clipboard: Arc<dyn Clipboard>,
    /// The byte storage every presentation of this realm hands its widgets
    /// (`LifecycleContext::storage`); `None` when the host gave it none.
    pub(crate) storage: Option<Arc<dyn Storage>>,
    /// Where the realm reads time: its frame-time origin, and every
    /// presentation's gesture arena and frame clock.
    pub(crate) clock: ClockSource,
    /// The realm's text service (ADR-0092 §3): one per realm, built from the
    /// app's [`FontCollection`] and dropped with the realm. Every
    /// presentation's pipeline holds a clone of the handle and lends the
    /// context to its layout (ADR-0092 §10 step 3).
    pub(crate) text: TextContextHandle,
}

impl RealmServices {
    /// Builds a brand-new `UpdateScheduler` for a realm about to be
    /// constructed, beside the `host`'s services. Every `UiRealm`
    /// constructor calls this instead of reaching for a process-global
    /// scheduler — each realm gets its OWN strong root, torn down when the
    /// realm drops — and its own [`TextContext`] over the host's fonts, so a
    /// face registered on the collection reaches this realm as it reaches
    /// every other.
    pub(crate) fn construct(host: RealmHostServices<'_>) -> Self {
        let RealmHostServices {
            wake,
            needs_redraw,
            clipboard,
            storage,
            fonts,
            clock,
        } = host;
        let scheduler = UpdateScheduler::new();
        Self {
            local_post_frame: scheduler.new_local_post_frame_lane(),
            async_driver: scheduler.async_driver().clone(),
            scheduler,
            wake,
            needs_redraw,
            clipboard,
            storage,
            clock,
            text: TextContextHandle::new(TextContext::new(fonts)),
        }
    }
}

/// Monotonic incarnation counter: every successfully constructed realm gets
/// a fresh `RealmId` generation, so a recreated realm never compares equal
/// to its predecessor.
static NEXT_INCARNATION: AtomicU32 = AtomicU32::new(1);

/// Mints a fresh, process-unique `(RealmId, PresentationId)` pair. Slot 0 is
/// the single-window slot; a multi-window registry would mint other slots.
pub(crate) fn next_identity() -> (RealmId, PresentationId) {
    next_identity_with_counter(&NEXT_INCARNATION)
}

fn next_identity_with_counter(counter: &AtomicU32) -> (RealmId, PresentationId) {
    let incarnation = counter
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            // Zero is permanent exhaustion; MAX remains the last valid generation.
            (current != 0).then(|| current.wrapping_add(1))
        })
        .expect("BUG: realm incarnation space exhausted");
    let generation = NonZeroU32::new(incarnation)
        .expect("BUG: incarnation counter starts at 1 and only increments");
    (
        RealmId::new_gen(0, generation),
        PresentationId::new_gen(0, generation),
    )
}

#[cfg(test)]
pub(crate) fn exhausted_incarnations_never_alias_previous_realms() {
    // A local counter reaches an otherwise impractical failure boundary without
    // changing the process-global identity source used by unrelated realm tests.
    let counter = AtomicU32::new(u32::MAX - 1);
    let (before_last, before_last_presentation) = next_identity_with_counter(&counter);
    let (last, last_presentation) = next_identity_with_counter(&counter);
    assert_ne!(before_last, last);
    assert_ne!(before_last_presentation, last_presentation);
    for _ in 0..3 {
        assert!(std::panic::catch_unwind(|| next_identity_with_counter(&counter)).is_err());
    }
    let fresh = AtomicU32::new(1);
    let (realm, presentation) = next_identity_with_counter(&fresh);
    assert_ne!(realm, last);
    assert_ne!(presentation, last_presentation);
}
