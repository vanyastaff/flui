//! What a realm resolves once when it is constructed: its own scheduler and
//! the handles derived from it, and a fresh identity.

use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use flui_foundation::{PresentationId, RealmId};
use flui_painting::{FontCollection, TextContext};
use flui_platform_api::Clipboard;
use flui_rendering::TextContextHandle;
use flui_scheduler::{ClockSource, OwnerFrame, UpdateScheduler};

/// What [`UiRealm`](crate::ui_realm::UiRealm)'s constructors need to wire it
/// up: a fresh, realm-owned [`UpdateScheduler`] — the strong root — plus the
/// [`OwnerFrame`] made for that SAME scheduler, the only strong owner of the
/// realm's owner-local post-frame callbacks and async tasks (ADR-0136 §2).
/// Resolved once, here, so the realm's own source reaches no process-global
/// scheduler.
pub(crate) struct RealmServices {
    pub(crate) owner_frame: OwnerFrame,
    pub(crate) scheduler: UpdateScheduler,
    /// The platform clipboard every presentation of this realm hands its
    /// widgets (`LifecycleContext::clipboard_handle`).
    pub(crate) clipboard: Arc<dyn Clipboard>,
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
    /// constructed. Every `UiRealm` constructor calls this instead of
    /// reaching for a process-global scheduler — each realm gets its OWN
    /// strong root, torn down when the realm drops.
    ///
    /// `clipboard` is the platform clipboard the realm's presentations hand
    /// their widgets; a realm always has one.
    ///
    /// `fonts` is the app's font collection; the realm gets its own
    /// [`TextContext`] over it, so a face registered on the collection
    /// reaches this realm as it reaches every other. `clock` is where it
    /// reads time.
    pub(crate) fn construct(
        clipboard: Arc<dyn Clipboard>,
        fonts: &FontCollection,
        clock: ClockSource,
    ) -> Self {
        let scheduler = UpdateScheduler::new();
        Self {
            owner_frame: OwnerFrame::new(&scheduler),
            scheduler,
            clipboard,
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
