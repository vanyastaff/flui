//! What a window's detach needs of a raster thread without the host: the
//! thread's link to each window's slot, and the wait for a slot to come down.

use std::rc::Rc;
use std::sync::Arc;

use super::Redraws;

use flui_engine::{RasterSender, SlotId};
use flui_foundation::PresentationAddress;
use flui_foundation::panic::FirstPanic;
use flui_platform::SurfaceTarget;

/// What a window's detach needs of a raster thread without the host: the
/// thread, its sender and each window's slot. The host writes it as it
/// places the executor and attaches and detaches slots, with no code of
/// another's running.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub(super) struct ThreadLink {
    /// Weak: the executor the host retires stops when the host lets go of
    /// it, not when the last window lets go of the link.
    pub(super) thread: Option<(
        std::rc::Weak<flui_engine::ExecutorHandle>,
        RasterSender,
        Redraws,
    )>,
    /// Each window's slot, with the target its surfaces were made over, which a
    /// detach that finds the thread wedged keeps past the window's close.
    pub(super) slots: rustc_hash::FxHashMap<PresentationAddress, (SlotId, Arc<dyn SurfaceTarget>)>,
}

#[cfg(not(target_arch = "wasm32"))]
impl ThreadLink {
    /// The wait for `address`'s slot to come down, when a raster thread
    /// holds it.
    pub(super) fn detach_wait(&self, address: PresentationAddress) -> Option<DetachWait> {
        let (handle, sender, redraws) = self.thread.as_ref()?;
        let (slot, target) = self.slots.get(&address)?;
        Some(DetachWait {
            handle: handle.upgrade()?,
            sender: sender.clone(),
            slot: *slot,
            redraws: redraws.clone(),
            target: Arc::clone(target),
        })
    }
}

/// A window's detach a raster thread has to answer before the window's
/// surface is given back.
#[cfg(not(target_arch = "wasm32"))]
pub(super) struct DetachWait {
    pub(super) handle: Rc<flui_engine::ExecutorHandle>,
    pub(super) sender: RasterSender,
    pub(super) slot: SlotId,
    /// The windows a death this wait finds wakes, each to settle what the
    /// executor will not and to report it dead.
    pub(super) redraws: Redraws,
    /// What the window's surfaces were made over: a thread that does not
    /// answer may still present over it.
    pub(super) target: Arc<dyn SurfaceTarget>,
}

#[cfg(not(target_arch = "wasm32"))]
impl DetachWait {
    /// Wait until the thread took the slot down, at most
    /// [`flui_engine::DETACH_DEADLINE`]; answers whether it did. A thread
    /// that does not answer dies: the window's native window is kept past its
    /// close and the target is leaked, and every window still attached is
    /// asked for a frame, whose wake quarantines it (ADR-0305 §9). A panic of
    /// the host's keep, the log's subscriber or a window's redraw requester
    /// is held in `caught`, and the target is leaked after any of them.
    pub(super) fn wait(self, caught: &mut FirstPanic) -> bool {
        match self
            .handle
            .detach(&self.sender, self.slot, flui_engine::DETACH_DEADLINE)
        {
            Ok(()) => true,
            Err(failure) => {
                // First, before any code that is not the host's: the native
                // window the wedged thread may still present to outlives
                // the close.
                let _kept = caught.contain(|| self.target.keep_past_close());
                let _logged = caught.contain(|| {
                    tracing::error!(%failure, "raster host: the raster thread did not take a window's slot down");
                });
                let _woken = caught.contain(|| self.redraws.wake());
                // The target lives until the process exits, so the handles
                // the wedged thread holds stay the host's (ADR-0305 §9).
                #[expect(
                    clippy::mem_forget,
                    reason = "ADR-0305 §9: a raster thread that did not answer may still present over this target"
                )]
                std::mem::forget(self.target);
                false
            }
        }
    }
}
