//! What a realm resolves once when it is constructed: its own scheduler and
//! the handles derived from it, and a fresh identity.

use std::num::NonZeroU32;
use std::sync::atomic::{AtomicU32, Ordering};

use flui_foundation::{PresentationId, RealmId};
use flui_scheduler::{AsyncDriver, LocalPostFrameLane, UpdateScheduler};

/// What [`UiRealm`](crate::ui_realm::UiRealm)'s constructors need to wire it
/// up: a fresh, realm-owned [`UpdateScheduler`] — the strong root — plus the
/// `local_post_frame_lane()` and `async_driver()` handles derived from that
/// SAME scheduler. Resolved once, here, so the realm's own source reaches no
/// process-global scheduler.
pub(crate) struct RealmServices {
    pub(crate) local_post_frame: LocalPostFrameLane,
    pub(crate) async_driver: AsyncDriver,
    pub(crate) scheduler: UpdateScheduler,
}

impl RealmServices {
    /// Builds a brand-new `UpdateScheduler` for a realm about to be
    /// constructed. Every `UiRealm` constructor calls this instead of
    /// reaching for a process-global scheduler — each realm gets its OWN
    /// strong root, torn down when the realm drops.
    pub(crate) fn construct() -> Self {
        let scheduler = UpdateScheduler::new();
        Self {
            local_post_frame: scheduler.new_local_post_frame_lane(),
            async_driver: scheduler.async_driver().clone(),
            scheduler,
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
    let incarnation = NEXT_INCARNATION.fetch_add(1, Ordering::Relaxed);
    let generation = NonZeroU32::new(incarnation)
        .expect("BUG: incarnation counter starts at 1 and only increments");
    (
        RealmId::new_gen(0, generation),
        PresentationId::new_gen(0, generation),
    )
}

#[cfg(test)]
mod identity_tests {
    use super::*;

    #[test]
    fn next_identity_mints_distinct_generations() {
        let (realm_a, _) = next_identity();
        let (realm_b, _) = next_identity();
        assert_ne!(
            realm_a, realm_b,
            "every mint must produce a fresh generation, never repeating"
        );
    }
}
