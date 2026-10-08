//! Assembly capabilities that can be borrowed independently of runtime checkout.

use std::sync::Arc;

use flui_interaction::InteractionDispatchHandle;
use flui_platform_api::{Clipboard, Storage};
use flui_scheduler::{AsyncDriver, ClockSource, LocalPostFrameHandle, WeakUpdateScheduler};
use flui_view::GlobalKeyScope;

use super::{UiCommandSender, UiRuntime};
use crate::presentation::{PresentationState, PresentationWindow, RuntimeCapabilities};

/// Preparing a presentation keeps no strong scheduler or owner-frame root alive.
/// Assembly does not grant permission to publish: the host must revalidate the
/// exact authorizing presentation after platform callbacks have returned.
pub struct PresentationFactory {
    global_key_scope: GlobalKeyScope,
    async_driver: AsyncDriver,
    local_post_frame_handle: LocalPostFrameHandle,
    interaction_dispatch_handle: InteractionDispatchHandle,
    scheduler: WeakUpdateScheduler,
    wake: Arc<dyn Fn() + Send + Sync>,
    sender: UiCommandSender,
    clipboard: Arc<dyn Clipboard>,
    storage: Option<Arc<dyn Storage>>,
    clock: ClockSource,
    text: flui_rendering::TextContextHandle,
}

impl std::fmt::Debug for PresentationFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PresentationFactory")
            .finish_non_exhaustive()
    }
}

impl UiRuntime {
    /// Capture assembly capabilities without borrowing this runtime during assembly.
    #[must_use]
    pub fn presentation_factory(&self) -> PresentationFactory {
        PresentationFactory {
            global_key_scope: self.global_key_scope.clone(),
            async_driver: self.owner_frame.async_driver(),
            local_post_frame_handle: self.owner_frame.local_post_frame_handle(),
            interaction_dispatch_handle: self.interaction_lane.dispatch_handle(),
            scheduler: self.scheduler.downgrade(),
            wake: Arc::clone(&self.wake),
            sender: self.sender_prototype.clone(),
            clipboard: Arc::clone(&self.clipboard),
            storage: self.storage.clone(),
            clock: self.clock.clone(),
            text: self.text.clone(),
        }
    }
}

impl PresentationFactory {
    /// Assembly may call platform code; callers must release host registry borrows.
    ///
    /// # Errors
    /// Returns the untouched window when the scheduler has already been retired.
    /// Successful assembly still requires host authorization before publication.
    pub fn assemble(
        &self,
        window: PresentationWindow,
    ) -> Result<PresentationState, PresentationWindow> {
        let Some(scheduler) = self.scheduler.upgrade() else {
            return Err(window);
        };
        let (_, presentation_id) = crate::runtime_services::next_identity();
        let device_pixel_ratio = window.window().scale_factor();
        let command_sender = UiCommandSender {
            presentation_id,
            ..self.sender.clone()
        };
        Ok(PresentationState::new(
            presentation_id,
            Some(device_pixel_ratio),
            window,
            RuntimeCapabilities {
                global_key_scope: self.global_key_scope.clone(),
                async_driver: self.async_driver.clone(),
                local_post_frame_handle: self.local_post_frame_handle.clone(),
                interaction_dispatch_handle: self.interaction_dispatch_handle.clone(),
                scheduler: &scheduler,
                wake: Arc::clone(&self.wake),
                command_sender,
                clipboard: Arc::clone(&self.clipboard),
                storage: self.storage.clone(),
                clock: &self.clock,
                text: self.text.clone(),
            },
        ))
    }
}
