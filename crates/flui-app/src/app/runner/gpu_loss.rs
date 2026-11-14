//! What the browser runner owes a lost GPU, kept apart from the browser.
//!
//! A canvas loses its graphics in two ways. A WebGL context is taken away
//! (`webglcontextlost`) and may be given back (`webglcontextrestored`); a
//! WebGPU device is lost for good (`GPUDevice.lost`) and a new one can be asked
//! for at once. Either way the renderer has to be rebuilt, and the frame that
//! would have noticed the loss may never come: a hidden tab has no animation
//! frames. The ledger records what happened, so the wake that the loss raises
//! can start the rebuild without waiting for a frame, and so a rebuild is never
//! started against a context that has not come back.
//!
//! It holds no browser type, so its rules run under the native test runner.

use std::sync::Arc;

use parking_lot::Mutex;
use web_time::{Duration, Instant};

/// How long the builds of a lost context may keep failing before the
/// rebuild is given up and the application is told (ADR-0267): longer than a
/// driver reset takes, as the native executor's backoff is.
pub(super) const GIVE_UP_AFTER: Duration = Duration::from_secs(12);

/// The graphics losses seen since the renderer was last rebuilt.
#[derive(Default)]
pub(super) struct GpuLossLedger {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// Something was lost and no rebuild has taken it on.
    rebuild_owed: bool,
    /// The WebGL context is gone and has not been restored: any rebuild now
    /// would be made against nothing, and no frame can draw.
    context_lost: bool,
    /// A rebuild is running; a second one would race it for the renderer.
    rebuilding: bool,
    /// A rebuild finished: the canvas starts empty, so the presentation must
    /// paint everything again.
    repaint_owed: bool,
    /// Builds that failed in a row since the last one that succeeded.
    failed_builds: u32,
    /// When the first of them failed.
    failing_since: Option<Instant>,
    /// The builds failed for [`GIVE_UP_AFTER`]: none is claimed until the
    /// application asks again or the browser restores the context.
    given_up: bool,
    /// Whether the presentation was told of the give-up.
    give_up_reported: bool,
}

impl GpuLossLedger {
    /// The WebGL context was lost. Frames stop and no rebuild starts until
    /// [`record_context_restore`](Self::record_context_restore).
    pub(super) fn record_context_loss(&self) {
        let mut state = self.state.lock();
        state.context_lost = true;
        state.rebuild_owed = true;
    }

    /// The WebGL context came back: what was built on the lost one is dead,
    /// so a rebuild is owed now that it can succeed, a given-up one too.
    pub(super) fn record_context_restore(&self) {
        let mut state = self.state.lock();
        state.context_lost = false;
        state.rebuild_owed = true;
        state.end_streak();
    }

    /// The graphics device is lost (`GPUDevice.lost`, which the GPU context
    /// reports to the raster host).
    /// A new one can be asked for at once.
    pub(super) fn record_device_loss(&self) {
        self.state.lock().rebuild_owed = true;
    }

    /// Whether a frame may draw: not while the WebGL context is lost.
    pub(super) fn can_draw(&self) -> bool {
        !self.state.lock().context_lost
    }

    /// Claim the rebuild, when one is owed, can succeed, none is running and
    /// it was not given up.
    ///
    /// A loss that arrives while the claim is held owes another rebuild: the
    /// claim clears the debt, the loss sets it again.
    #[must_use]
    pub(super) fn claim_rebuild(self: &Arc<Self>) -> Option<RebuildClaim> {
        let mut state = self.state.lock();
        if !state.rebuild_owed || state.context_lost || state.rebuilding || state.given_up {
            return None;
        }
        state.rebuild_owed = false;
        state.rebuilding = true;
        Some(RebuildClaim {
            ledger: Arc::clone(self),
            succeeded: false,
        })
    }

    /// Claim the first build of the renderer, which no loss owes: the
    /// ledger holds it like a rebuild, so a loss that arrives while the
    /// adapter is being asked for does not start a second one beside it.
    /// Dropped unfinished, it owes the build again.
    #[must_use]
    pub(super) fn claim_initialization(self: &Arc<Self>) -> RebuildClaim {
        self.state.lock().rebuilding = true;
        RebuildClaim {
            ledger: Arc::clone(self),
            succeeded: false,
        }
    }

    /// Whether the presentation owes a full repaint after a rebuild; reading
    /// it clears it.
    #[must_use]
    pub(super) fn take_repaint_owed(&self) -> bool {
        std::mem::take(&mut self.state.lock().repaint_owed)
    }

    /// The failed builds of a rebuild just given up, once: the presentation
    /// reports it as abandoned (ADR-0267).
    #[must_use]
    pub(super) fn take_given_up(&self) -> Option<u32> {
        let mut state = self.state.lock();
        if !state.given_up || state.give_up_reported {
            return None;
        }
        state.give_up_reported = true;
        Some(state.failed_builds)
    }

    /// The application asked for the rebuild again: one given up is owed
    /// once more, from a fresh streak. Answers whether one was given up.
    pub(super) fn retry(&self) -> bool {
        let mut state = self.state.lock();
        if !state.given_up {
            return false;
        }
        state.end_streak();
        state.rebuild_owed = true;
        true
    }
}

impl State {
    fn end_streak(&mut self) {
        self.failed_builds = 0;
        self.failing_since = None;
        self.given_up = false;
        self.give_up_reported = false;
    }
}

/// The right to rebuild the renderer, held until the rebuild ends.
///
/// Dropping it without [`complete`](Self::complete) — the rebuild failed, was
/// abandoned, or its future was dropped unfinished — owes the rebuild again,
/// to the next wake rather than at once: a driver still resetting would be
/// asked in a tight loop, and the browser's animation frames pace a visible
/// tab.
pub(super) struct RebuildClaim {
    ledger: Arc<GpuLossLedger>,
    succeeded: bool,
}

impl RebuildClaim {
    /// The rebuild produced a renderer: the presentation repaints.
    pub(super) fn complete(mut self) {
        self.succeeded = true;
    }

    /// The build failed at `now`: it is owed again, and counted toward the
    /// streak that gives the rebuild up once it has failed for
    /// [`GIVE_UP_AFTER`]. A claim dropped without either (a busy host, a page
    /// closed during the build) is owed again uncounted.
    pub(super) fn fail(self, now: Instant) {
        let mut state = self.ledger.state.lock();
        state.failed_builds = state.failed_builds.saturating_add(1);
        let since = *state.failing_since.get_or_insert(now);
        if now.saturating_duration_since(since) >= GIVE_UP_AFTER {
            state.given_up = true;
        }
        // Released before the claim drops and takes the lock again.
        drop(state);
    }
}

impl Drop for RebuildClaim {
    fn drop(&mut self) {
        let mut state = self.ledger.state.lock();
        state.rebuilding = false;
        if self.succeeded {
            state.repaint_owed = true;
            state.end_streak();
        } else {
            state.rebuild_owed = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::GpuLossLedger;

    fn ledger() -> Arc<GpuLossLedger> {
        Arc::default()
    }

    #[test]
    fn a_device_loss_is_rebuilt_at_once() {
        let loss = ledger();
        assert!(
            loss.claim_rebuild().is_none(),
            "nothing is owed before a loss"
        );
        loss.record_device_loss();
        assert!(loss.can_draw(), "a lost device has no context to wait for");
        let claim = loss.claim_rebuild().expect("owed");
        assert!(loss.claim_rebuild().is_none(), "one rebuild at a time");
        claim.complete();
        assert!(
            loss.claim_rebuild().is_none(),
            "a finished rebuild owes nothing"
        );
    }

    #[test]
    fn a_lost_context_waits_for_its_restoration() {
        let loss = ledger();
        loss.record_context_loss();
        assert!(!loss.can_draw());
        assert!(
            loss.claim_rebuild().is_none(),
            "a rebuild now would be made on nothing"
        );
        loss.record_context_restore();
        assert!(loss.can_draw());
        assert!(
            loss.claim_rebuild().is_some(),
            "the restored context is rebuilt on"
        );
    }

    #[test]
    fn a_restoration_without_a_loss_still_rebuilds() {
        let loss = ledger();
        loss.record_context_restore();
        assert!(
            loss.claim_rebuild().is_some(),
            "what was built on the context is dead"
        );
    }

    #[test]
    fn a_loss_during_a_rebuild_owes_another() {
        let loss = ledger();
        loss.record_device_loss();
        let claim = loss.claim_rebuild().expect("owed");
        loss.record_device_loss();
        assert!(
            loss.claim_rebuild().is_none(),
            "the running rebuild is not doubled"
        );
        claim.complete();
        assert!(
            loss.claim_rebuild().is_some(),
            "the loss during it is still owed"
        );
    }

    #[test]
    fn a_claim_that_is_dropped_owes_the_rebuild_again() {
        let loss = ledger();
        loss.record_device_loss();
        drop(loss.claim_rebuild().expect("owed"));
        assert!(!loss.take_repaint_owed(), "nothing was drawn to repaint");
        assert!(
            loss.claim_rebuild().is_some(),
            "a failed or abandoned rebuild is owed again"
        );
    }

    #[test]
    fn a_loss_during_the_first_build_waits_for_it() {
        let loss = ledger();
        let first = loss.claim_initialization();
        loss.record_device_loss();
        assert!(
            loss.claim_rebuild().is_none(),
            "no second build beside the first"
        );
        first.complete();
        assert!(
            loss.claim_rebuild().is_some(),
            "the loss during it is still owed"
        );
    }

    #[test]
    fn builds_that_keep_failing_give_the_rebuild_up_until_it_is_asked_again() {
        let loss = ledger();
        let start = web_time::Instant::now();
        loss.claim_initialization().fail(start);
        drop(loss.claim_rebuild().expect("owed"));
        loss.claim_rebuild()
            .expect("a claim dropped unbuilt is owed again")
            .fail(start + super::GIVE_UP_AFTER / 2);
        assert_eq!(loss.take_given_up(), None, "still within the streak");
        loss.claim_rebuild()
            .expect("owed")
            .fail(start + super::GIVE_UP_AFTER);
        assert_eq!(
            loss.take_given_up(),
            Some(3),
            "told once, with each failure"
        );
        assert_eq!(loss.take_given_up(), None);
        assert!(loss.claim_rebuild().is_none(), "a given-up rebuild waits");

        assert!(loss.retry(), "the application asked again");
        assert!(!loss.retry(), "nothing is given up now");
        loss.claim_rebuild()
            .expect("owed again")
            .fail(start + super::GIVE_UP_AFTER * 2);
        assert_eq!(loss.take_given_up(), None, "a fresh streak");
        loss.claim_rebuild().expect("owed").complete();
        assert!(loss.claim_rebuild().is_none(), "built");

        // The browser restoring the context also asks again.
        loss.record_device_loss();
        let claim = loss.claim_rebuild().expect("owed");
        claim.fail(start);
        loss.claim_rebuild()
            .expect("owed")
            .fail(start + super::GIVE_UP_AFTER);
        assert!(loss.take_given_up().is_some());
        loss.record_context_restore();
        assert!(
            loss.claim_rebuild().is_some(),
            "the restored context is tried"
        );
    }

    #[test]
    fn a_first_build_that_fails_is_owed_again() {
        let loss = ledger();
        drop(loss.claim_initialization());
        assert!(loss.claim_rebuild().is_some(), "the next wake builds again");
    }

    #[test]
    fn a_rebuild_owes_one_repaint() {
        let loss = ledger();
        assert!(!loss.take_repaint_owed());
        loss.record_device_loss();
        let claim = loss.claim_rebuild().expect("owed");
        assert!(!loss.take_repaint_owed(), "not before it finishes");
        claim.complete();
        assert!(loss.take_repaint_owed());
        assert!(!loss.take_repaint_owed(), "read once");
    }

    #[test]
    fn a_context_lost_while_rebuilding_waits_for_the_restoration() {
        let loss = ledger();
        loss.record_device_loss();
        let claim = loss.claim_rebuild().expect("owed");
        loss.record_context_loss();
        claim.complete();
        assert!(!loss.can_draw());
        assert!(loss.claim_rebuild().is_none());
        loss.record_context_restore();
        assert!(loss.claim_rebuild().is_some());
    }
}
