//! The frame driver's side of a root registry.

use std::fmt;
use std::rc::Rc;

use flui_foundation::BuildPhase;
use flui_foundation::panic::RecoveryScope;

use super::{Vsync, VsyncRetirement};
use crate::FrameTick;

/// The only owner that ticks, binds and closes a root [`Vsync`] registry.
///
/// A presentation's frame driver creates one per presentation and hands
/// [`vsync`](Self::vsync) to its tree, where controllers and nested registries
/// register. A widget holds that handle and can register, attach a child
/// registry or mute it, but cannot advance, rebind or close a registry: those
/// operations live here, and nothing turns a handle back into its driver. A
/// registry made with [`Vsync::new`] advances only under a parent it is attached to.
///
/// A driver's registry may itself be attached under another registry with
/// [`Vsync::attach_child`]; ticking it directly then keeps the clock authority
/// of its live parent chain (see [`tick_all`](Self::tick_all)).
///
/// A handle has no way to tick:
///
/// ```compile_fail,E0624
/// use std::time::Duration;
/// use flui_animation::{MotionClock, Vsync};
///
/// let handle = Vsync::new();
/// handle.tick_all(&MotionClock::new().frame(Duration::ZERO));
/// ```
#[derive(Default)]
pub struct VsyncDriver {
    root: Vsync,
}

impl VsyncDriver {
    /// A driver of a new, empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The registry this driver ticks, as the handle a tree receives.
    #[must_use]
    pub fn vsync(&self) -> &Vsync {
        &self.root
    }

    /// Bind the registry's frame demand to the presentation's frame driver.
    ///
    /// Running controllers request a sample from the new requester. The
    /// requester runs, and the outgoing one is destroyed, outside registry
    /// borrows. A closed registry keeps no requester.
    ///
    /// # Panics
    /// Propagates the first callback or capture-retirement failure after recovery.
    pub fn set_frame_requester(&self, request_frame: Option<Rc<dyn Fn()>>) {
        self.root.set_frame_requester(request_frame);
    }

    /// Bind the registry to the build phase of its presentation
    /// ([`BuildPhase`]). A controller registered here, or in a registry nested under it,
    /// refuses a mutation made while the phase builds with
    /// [`AnimationError::MutatedDuringBuild`](crate::AnimationError::MutatedDuringBuild).
    pub fn set_build_phase(&self, phase: Option<BuildPhase>) {
        self.root.set_build_phase(phase);
    }

    /// Advance the registry's live controllers, and its attached registries',
    /// to `tick`.
    ///
    /// A fresh run anchors on its first observed frame. Registration migration
    /// preserves its last sampled elapsed time. Moving a child subtree to another
    /// root also re-anchors on its first delivered frame, without replacing runs.
    /// Reattaching under the same root retains the existing absolute anchors.
    /// Older ticks from the same root hold at the registry's last accepted time;
    /// stopped and disposed kernels are skipped. A registry attached under
    /// another and ticked here keeps the clock authority of its live parent
    /// chain.
    ///
    /// State borrows end before sampling user curves or delivering callbacks.
    /// A listener may release or dispose its [owning controller](crate::DrivenController)
    /// during the walk. A later registration withdrawn by that listener is
    /// skipped, while other live controllers remain deliverable.
    ///
    /// Admissions made during this call first tick on the next call. Restarting
    /// an already visited controller also waits for the next call; starting a
    /// resident controller that has not yet been visited can sample this frame.
    /// Registration identities are never reused, so withdrawing and re-admitting
    /// a kernel cannot give the replacement an earlier turn in the current walk.
    ///
    /// Child registries admitted at entry tick before the registry's controllers.
    /// Muting is rechecked between controller samples, so muting during delivery
    /// stops the remaining samples. Ancestor mute gates apply to nested registries.
    ///
    /// # Panics
    /// A controller or child-registry failure leaves healthy frame peers
    /// deliverable. The walk resumes its first failure after those peers tick,
    /// preserving that failure through callback and capture retirement.
    pub fn tick_all(&self, tick: &FrameTick) {
        self.root.tick_all(tick);
    }

    /// [`tick_all`](Self::tick_all) under the presentation's own failure
    /// custody: a failure joins `recovery` instead of resuming here, so the
    /// runtime chooses its publication and report from the captured
    /// provenance. No live peer or admitted delivery is lost on failure.
    pub fn tick_all_with_recovery(&self, tick: &FrameTick, recovery: &mut RecoveryScope<'_>) {
        self.root.tick_all_with_recovery(tick, recovery);
    }

    /// Permanently close the registry and every attached descendant.
    ///
    /// Seats, parent links and frame hooks are withdrawn, and controller kernels
    /// refuse new runs before this returns. Saved handles cannot revive admission.
    /// Closing an attached registry also withdraws its seat from its parent.
    /// Cancellation callbacks and outgoing capture retirement are deferred to the
    /// returned receipt; a repeated close is an empty retirement.
    pub fn prepare_close(&self) -> VsyncRetirement {
        self.root.prepare_close()
    }
}

impl fmt::Debug for VsyncDriver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VsyncDriver")
            .field("root", &self.root)
            .finish()
    }
}
