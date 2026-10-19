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
}

impl GpuLossLedger {
    /// The WebGL context was lost. Frames stop and no rebuild starts until
    /// [`context_restored`](Self::context_restored).
    pub(super) fn context_lost(&self) {
        let mut state = self.state.lock();
        state.context_lost = true;
        state.rebuild_owed = true;
    }

    /// The WebGL context came back: what was built on the lost one is dead,
    /// so a rebuild is owed now that it can succeed.
    pub(super) fn context_restored(&self) {
        let mut state = self.state.lock();
        state.context_lost = false;
        state.rebuild_owed = true;
    }

    /// The graphics device is lost (`GPUDevice.lost`, or the renderer says so).
    /// A new one can be asked for at once.
    pub(super) fn device_lost(&self) {
        self.state.lock().rebuild_owed = true;
    }

    /// Whether a frame may draw: not while the WebGL context is lost.
    pub(super) fn frames_allowed(&self) -> bool {
        !self.state.lock().context_lost
    }

    /// Claim the rebuild, when one is owed, can succeed and none is running.
    ///
    /// A loss that arrives while the claim is held owes another rebuild: the
    /// claim clears the debt, the loss sets it again.
    #[must_use]
    pub(super) fn claim_rebuild(self: &Arc<Self>) -> Option<RebuildClaim> {
        let mut state = self.state.lock();
        if !state.rebuild_owed || state.context_lost || state.rebuilding {
            return None;
        }
        state.rebuild_owed = false;
        state.rebuilding = true;
        Some(RebuildClaim {
            ledger: Arc::clone(self),
            rebuilt: false,
        })
    }

    /// Whether the presentation owes a full repaint after a rebuild; reading
    /// it clears it.
    #[must_use]
    pub(super) fn take_repaint_owed(&self) -> bool {
        std::mem::take(&mut self.state.lock().repaint_owed)
    }
}

/// The right to rebuild the renderer, held until the rebuild ends.
///
/// Dropping it without [`rebuilt`](Self::rebuilt) — the rebuild failed, was
/// abandoned, or its future was dropped unfinished — owes the rebuild again,
/// to the next wake rather than at once: a driver still resetting would be
/// asked in a tight loop, and the browser's animation frames pace a visible
/// tab.
pub(super) struct RebuildClaim {
    ledger: Arc<GpuLossLedger>,
    rebuilt: bool,
}

impl RebuildClaim {
    /// The rebuild produced a renderer: the presentation repaints.
    pub(super) fn rebuilt(mut self) {
        self.rebuilt = true;
    }
}

impl Drop for RebuildClaim {
    fn drop(&mut self) {
        let mut state = self.ledger.state.lock();
        state.rebuilding = false;
        if self.rebuilt {
            state.repaint_owed = true;
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
        loss.device_lost();
        assert!(
            loss.frames_allowed(),
            "a lost device has no context to wait for"
        );
        let claim = loss.claim_rebuild().expect("owed");
        assert!(loss.claim_rebuild().is_none(), "one rebuild at a time");
        claim.rebuilt();
        assert!(
            loss.claim_rebuild().is_none(),
            "a finished rebuild owes nothing"
        );
    }

    #[test]
    fn a_lost_context_waits_for_its_restoration() {
        let loss = ledger();
        loss.context_lost();
        assert!(!loss.frames_allowed());
        assert!(
            loss.claim_rebuild().is_none(),
            "a rebuild now would be made on nothing"
        );
        loss.context_restored();
        assert!(loss.frames_allowed());
        assert!(
            loss.claim_rebuild().is_some(),
            "the restored context is rebuilt on"
        );
    }

    #[test]
    fn a_restoration_without_a_loss_still_rebuilds() {
        let loss = ledger();
        loss.context_restored();
        assert!(
            loss.claim_rebuild().is_some(),
            "what was built on the context is dead"
        );
    }

    #[test]
    fn a_loss_during_a_rebuild_owes_another() {
        let loss = ledger();
        loss.device_lost();
        let claim = loss.claim_rebuild().expect("owed");
        loss.device_lost();
        assert!(
            loss.claim_rebuild().is_none(),
            "the running rebuild is not doubled"
        );
        claim.rebuilt();
        assert!(
            loss.claim_rebuild().is_some(),
            "the loss during it is still owed"
        );
    }

    #[test]
    fn a_claim_that_is_dropped_owes_the_rebuild_again() {
        let loss = ledger();
        loss.device_lost();
        drop(loss.claim_rebuild().expect("owed"));
        assert!(!loss.take_repaint_owed(), "nothing was drawn to repaint");
        assert!(
            loss.claim_rebuild().is_some(),
            "a failed or abandoned rebuild is owed again"
        );
    }

    #[test]
    fn a_rebuild_owes_one_repaint() {
        let loss = ledger();
        assert!(!loss.take_repaint_owed());
        loss.device_lost();
        let claim = loss.claim_rebuild().expect("owed");
        assert!(!loss.take_repaint_owed(), "not before it finishes");
        claim.rebuilt();
        assert!(loss.take_repaint_owed());
        assert!(!loss.take_repaint_owed(), "read once");
    }

    #[test]
    fn a_context_lost_while_rebuilding_waits_for_the_restoration() {
        let loss = ledger();
        loss.device_lost();
        let claim = loss.claim_rebuild().expect("owed");
        loss.context_lost();
        claim.rebuilt();
        assert!(!loss.frames_allowed());
        assert!(loss.claim_rebuild().is_none());
        loss.context_restored();
        assert!(loss.claim_rebuild().is_some());
    }
}
