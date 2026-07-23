//! Owner-thread state for one presentation of a UI realm.
//!
//! This is deliberately crate-private. It is the UI-owner domain, not a
//! cross-thread god object: native event-loop ownership remains in the
//! runner/window host and raster/surface ownership remains in
//! `flui_engine::RasterOwner`.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Weak};

use flui_foundation::PresentationId;
use flui_interaction::{FocusManager, GestureBinding, TextInputHandle, TextInputOwner};
#[cfg(test)]
use flui_platform::traits::PlatformTextInput;
use flui_platform::{
    CursorIcon,
    traits::{CursorError, PlatformWindow},
};
use flui_rendering::pipeline::PipelineOwner;
use flui_semantics::{SemanticsActionError, SemanticsActionRequest};
use flui_view::WidgetsBinding;
use parking_lot::RwLock;

#[cfg(test)]
struct TestPresentationWindow {
    text_input: Option<Arc<dyn PlatformTextInput>>,
    cursor: parking_lot::Mutex<CursorIcon>,
}

#[cfg(test)]
impl TestPresentationWindow {
    fn new(text_input: Option<Arc<dyn PlatformTextInput>>) -> Self {
        Self {
            text_input,
            cursor: parking_lot::Mutex::new(CursorIcon::Default),
        }
    }
}

#[cfg(test)]
impl PlatformWindow for TestPresentationWindow {
    fn physical_size(&self) -> flui_types::geometry::Size<flui_types::geometry::DevicePixels> {
        flui_types::geometry::Size::default()
    }

    fn logical_size(&self) -> flui_types::geometry::Size<flui_types::geometry::Pixels> {
        flui_types::geometry::Size::default()
    }

    fn scale_factor(&self) -> f64 {
        1.0
    }

    fn request_redraw(&self) {}

    fn is_focused(&self) -> bool {
        true
    }

    fn is_visible(&self) -> bool {
        true
    }

    fn text_input(&self) -> Option<Arc<dyn PlatformTextInput>> {
        self.text_input.clone()
    }

    fn set_cursor(&self, cursor: CursorIcon) -> Result<(), CursorError> {
        *self.cursor.lock() = cursor;
        Ok(())
    }
}

/// Lifecycle of the owner-thread half of a presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PresentationLifecycle {
    /// Identity exists, but no render surface is attached yet.
    Created,
    /// The presentation accepts input and produces frames.
    SurfaceAttached,
    /// The surface is retained but frame production is paused.
    Suspended,
    /// Teardown has started; new work is rejected.
    Closing,
    /// Owner-local resources have been released.
    Closed,
}

/// Direct owner of mutable UI state scoped to one presentation.
///
/// It owns behavior-bearing subsystems as concrete values. It does not expose
/// a provider trait, service locator, erased resource bag, or arbitrary
/// executor. Cross-thread ingress is handled by closed commands stamped with
/// this presentation's generational identity.
pub(crate) struct PresentationState {
    id: PresentationId,
    lifecycle: Cell<PresentationLifecycle>,
    pipeline: Arc<RwLock<PipelineOwner>>,
    window: Weak<dyn PlatformWindow>,
    gestures: GestureBinding,
    focus: Rc<FocusManager>,
    text_input: Rc<TextInputOwner>,
}

impl PresentationState {
    pub(crate) fn new(
        id: PresentationId,
        pipeline: Arc<RwLock<PipelineOwner>>,
        window: Arc<dyn PlatformWindow>,
    ) -> Self {
        let gestures = GestureBinding::new();
        let cursor_window = Arc::downgrade(&window);
        gestures
            .mouse_tracker()
            .set_cursor_change_callback(Rc::new(move |device_id, cursor| {
                let Some(window) = cursor_window.upgrade() else {
                    tracing::trace!(
                        ?id,
                        ?device_id,
                        ?cursor,
                        "dropping cursor update after the platform window closed"
                    );
                    return;
                };
                if let Err(error) = window.set_cursor(cursor) {
                    match error {
                        CursorError::Unsupported => {
                            tracing::trace!(
                                ?id,
                                ?device_id,
                                ?cursor,
                                "window backend has no pointer-cursor facility"
                            );
                        }
                        CursorError::Backend(_) => {
                            tracing::warn!(
                                ?id,
                                ?device_id,
                                ?cursor,
                                ?error,
                                "failed to apply the presentation cursor"
                            );
                        }
                    }
                }
            }));
        let platform_text_input = window.text_input();
        let state = Self {
            id,
            lifecycle: Cell::new(PresentationLifecycle::Created),
            pipeline,
            window: Arc::downgrade(&window),
            gestures,
            focus: FocusManager::new(),
            text_input: TextInputOwner::new(platform_text_input),
        };
        state.attach_surface();
        state
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        id: PresentationId,
        pipeline: Arc<RwLock<PipelineOwner>>,
        platform_text_input: Option<Arc<dyn PlatformTextInput>>,
    ) -> Self {
        let window: Arc<dyn PlatformWindow> =
            Arc::new(TestPresentationWindow::new(platform_text_input));
        Self::new(id, pipeline, window)
    }

    #[must_use]
    pub(crate) fn id(&self) -> PresentationId {
        self.id
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn lifecycle(&self) -> PresentationLifecycle {
        self.lifecycle.get()
    }

    #[must_use]
    pub(crate) fn pipeline(&self) -> &Arc<RwLock<PipelineOwner>> {
        &self.pipeline
    }

    #[must_use]
    pub(crate) fn gestures(&self) -> &GestureBinding {
        &self.gestures
    }

    /// The exact focus tree owned by this presentation.
    #[must_use]
    pub(crate) fn focus_manager(&self) -> Rc<FocusManager> {
        Rc::clone(&self.focus)
    }

    #[must_use]
    pub(crate) fn text_input(&self) -> &TextInputOwner {
        &self.text_input
    }

    #[must_use]
    pub(crate) fn text_input_handle(&self) -> TextInputHandle {
        self.text_input.handle()
    }

    fn attach_surface(&self) {
        if self.lifecycle.get() == PresentationLifecycle::Created {
            self.lifecycle.set(PresentationLifecycle::SurfaceAttached);
        }
    }

    pub(crate) fn suspend(&self) {
        if self.lifecycle.get() == PresentationLifecycle::SurfaceAttached {
            self.lifecycle.set(PresentationLifecycle::Suspended);
        }
    }

    pub(crate) fn resume(&self) {
        if self.lifecycle.get() == PresentationLifecycle::Suspended {
            self.lifecycle.set(PresentationLifecycle::SurfaceAttached);
        }
    }

    /// Resolve an accessibility action through this presentation's exact
    /// semantics owner, then invoke it after releasing the pipeline lock.
    pub(crate) fn dispatch_semantics_action(
        &self,
        request: SemanticsActionRequest,
    ) -> Result<(), SemanticsActionError> {
        let invocation = {
            let pipeline = self.pipeline.read();
            pipeline.resolve_semantics_action(request)?
        };
        invocation.invoke();
        Ok(())
    }

    /// Apply a hot-reload tier to this presentation and its realm-owned
    /// element tree. Returns whether a redraw is required.
    pub(crate) fn apply_hot_reload(
        &self,
        widgets: &WidgetsBinding,
        tier: flui_hot_reload::HotReloadTier,
    ) -> bool {
        use flui_hot_reload::HotReloadTier;

        match tier {
            HotReloadTier::HotReload => {
                widgets.perform_reassemble();
                self.pipeline.write().reassemble();
                tracing::info!(
                    presentation_id = ?self.id,
                    "hot reload reassembled element and render trees"
                );
                true
            }
            HotReloadTier::HotRestart => {
                tracing::warn!(
                    presentation_id = ?self.id,
                    "HotRestart root remount is not implemented; applying reassemble"
                );
                widgets.perform_reassemble();
                self.pipeline.write().reassemble();
                true
            }
            HotReloadTier::FullRestart => {
                tracing::debug!(
                    presentation_id = ?self.id,
                    "FullRestart is owned by the CLI process supervisor"
                );
                false
            }
        }
    }

    /// Begin deterministic owner-local teardown.
    pub(crate) fn close(&self) {
        match self.lifecycle.get() {
            PresentationLifecycle::Closing | PresentationLifecycle::Closed => return,
            PresentationLifecycle::Created
            | PresentationLifecycle::SurfaceAttached
            | PresentationLifecycle::Suspended => {}
        }
        self.lifecycle.set(PresentationLifecycle::Closing);

        self.gestures.cancel_all_pointer_sequences();
        self.gestures.mouse_tracker().clear_cursor_change_callback();
        if let Some(window) = self.window.upgrade()
            && let Err(error) = window.set_cursor(CursorIcon::Default)
            && !matches!(error, CursorError::Unsupported)
        {
            tracing::warn!(
                presentation_id = ?self.id,
                ?error,
                "failed to restore the default cursor while closing the presentation"
            );
        }
        self.focus.close();
        self.text_input.close();
        self.lifecycle.set(PresentationLifecycle::Closed);
    }
}

impl std::fmt::Debug for PresentationState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PresentationState")
            .field("id", &self.id)
            .field("lifecycle", &self.lifecycle.get())
            .finish_non_exhaustive()
    }
}

impl Drop for PresentationState {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;

    static_assertions::assert_not_impl_any!(PresentationState: Send, Sync);

    fn presentation() -> PresentationState {
        PresentationState::new_for_test(
            PresentationId::new_gen(0, NonZeroU32::MIN),
            Arc::new(RwLock::new(PipelineOwner::new())),
            None,
        )
    }

    #[test]
    fn lifecycle_transitions_are_typed_and_close_is_idempotent() {
        let presentation = presentation();
        assert_eq!(
            presentation.lifecycle(),
            PresentationLifecycle::SurfaceAttached
        );

        presentation.suspend();
        assert_eq!(presentation.lifecycle(), PresentationLifecycle::Suspended);
        presentation.resume();
        assert_eq!(
            presentation.lifecycle(),
            PresentationLifecycle::SurfaceAttached
        );

        presentation.close();
        presentation.close();
        assert_eq!(presentation.lifecycle(), PresentationLifecycle::Closed);
    }

    #[test]
    fn text_input_handle_is_bound_to_the_owned_text_input_state() {
        let presentation = presentation();
        let handle = presentation.text_input_handle();

        presentation.close();

        assert_eq!(
            handle.attach(Rc::new(|_| {})),
            Err(flui_interaction::TextInputError::Closed)
        );
    }

    #[test]
    fn mouse_tracker_applies_cursor_to_the_exact_owned_window() {
        use flui_foundation::RenderId;
        use flui_interaction::{
            events::{PointerType, make_move_event},
            routing::{HitTestEntry, HitTestResult, PointerMotionKind},
        };
        use flui_types::geometry::{Offset, Pixels};

        let window = Arc::new(TestPresentationWindow::new(None));
        let platform_window: Arc<dyn PlatformWindow> = window.clone();
        let presentation = PresentationState::new(
            PresentationId::new_gen(0, NonZeroU32::MIN),
            Arc::new(RwLock::new(PipelineOwner::new())),
            platform_window,
        );
        let position = Offset::new(Pixels(12.0), Pixels(8.0));
        let event = make_move_event(position, PointerType::Mouse);
        let mut hit_test = HitTestResult::new();
        hit_test.add(HitTestEntry::new(RenderId::new(1)).cursor(CursorIcon::Pointer));

        presentation.gestures().mouse_tracker().update_with_motion(
            &event,
            PointerMotionKind::Hover,
            &hit_test,
        );

        assert_eq!(*window.cursor.lock(), CursorIcon::Pointer);
        presentation.close();
        assert_eq!(*window.cursor.lock(), CursorIcon::Default);
    }
}
