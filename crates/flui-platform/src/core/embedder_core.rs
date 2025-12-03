//! Shared embedder implementation
//!
//! `EmbedderCore` contains all the common logic shared across platforms.
//! Platform-specific embedders compose this rather than duplicating code.

use crate::{
    bindings::{GestureBinding, SchedulerBinding},
    core::{FrameCoordinator, PointerState, SceneCache},
    traits::{DefaultLifecycle, PlatformLifecycle},
};
use flui_core::pipeline::PipelineOwner;
use flui_engine::{CanvasLayer, GpuRenderer, Layer, Scene};
use flui_types::{
    constraints::BoxConstraints,
    events::{PointerButton, PointerDeviceKind, PointerEventData},
    Event, Offset, PointerEvent, Size,
};
use parking_lot::RwLock;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// Shared embedder implementation
///
/// Contains all the common logic shared across platform embedders.
/// Platform-specific embedders compose this struct rather than duplicating code.
///
/// # Architecture
///
/// ```text
/// EmbedderCore (shared 90%+ logic)
///   ├─ pipeline_owner: Arc<RwLock<PipelineOwner>> - Framework pipeline
///   ├─ needs_redraw: Arc<AtomicBool> - On-demand rendering flag
///   ├─ scene_cache: SceneCache - Type-safe scene caching
///   ├─ pointer_state: PointerState - Pointer tracking/coalescing
///   ├─ frame_coordinator: FrameCoordinator - Frame rendering
///   ├─ scheduler: SchedulerBinding - Frame scheduling
///   ├─ gesture: GestureBinding - Safe hit testing
///   └─ lifecycle: DefaultLifecycle - Lifecycle tracking
/// ```
///
/// # Usage
///
/// ```rust,ignore
/// pub struct DesktopEmbedder {
///     core: EmbedderCore,
///     window: WinitWindow,
///     renderer: GpuRenderer,
/// }
///
/// impl DesktopEmbedder {
///     pub fn render_frame(&mut self) {
///         let scene = self.core.render_frame(&mut self.renderer);
///         // Platform-specific post-processing
///     }
/// }
/// ```
pub struct EmbedderCore {
    /// Core pipeline - single source of truth for element tree
    pipeline_owner: Arc<RwLock<PipelineOwner>>,

    /// On-demand rendering flag
    needs_redraw: Arc<AtomicBool>,

    /// Scene cache for hit testing (type-safe!)
    scene_cache: SceneCache,

    /// Pointer state tracker (coalescing, position tracking)
    pointer_state: PointerState,

    /// Frame coordinator (orchestrates rendering)
    frame_coordinator: FrameCoordinator,

    /// Scheduler binding (frame scheduling)
    scheduler: SchedulerBinding,

    /// Gesture binding (safe hit testing)
    gesture: GestureBinding,

    /// Lifecycle tracking
    lifecycle: DefaultLifecycle,
}

impl EmbedderCore {
    /// Create a new embedder core
    ///
    /// # Arguments
    ///
    /// * `pipeline_owner` - Shared pipeline owner from AppBinding
    /// * `needs_redraw` - Shared redraw flag from AppBinding
    /// * `scheduler` - Scheduler from AppBinding
    /// * `event_router` - Event router from GestureBinding
    pub fn new(
        pipeline_owner: Arc<RwLock<PipelineOwner>>,
        needs_redraw: Arc<AtomicBool>,
        scheduler: Arc<flui_scheduler::Scheduler>,
        event_router: Arc<RwLock<flui_interaction::EventRouter>>,
    ) -> Self {
        let scheduler_binding = SchedulerBinding::new(scheduler);
        let gesture_binding = GestureBinding::new(event_router);

        // Wire up scheduler callbacks
        let pipeline_weak = Arc::downgrade(&pipeline_owner);
        let redraw_flag = needs_redraw.clone();
        scheduler_binding.wire_up_pipeline(pipeline_weak, redraw_flag);

        Self {
            pipeline_owner,
            needs_redraw,
            scene_cache: SceneCache::new(),
            pointer_state: PointerState::new(),
            frame_coordinator: FrameCoordinator::new(),
            scheduler: scheduler_binding,
            gesture: gesture_binding,
            lifecycle: DefaultLifecycle::new(),
        }
    }

    // ========================================================================
    // Pipeline Access
    // ========================================================================

    /// Get the pipeline owner
    pub fn pipeline(&self) -> &Arc<RwLock<PipelineOwner>> {
        &self.pipeline_owner
    }

    /// Check if redraw is needed
    pub fn needs_redraw(&self) -> bool {
        self.needs_redraw.load(Ordering::Relaxed)
    }

    /// Request a redraw
    pub fn request_redraw(&self) {
        self.needs_redraw.store(true, Ordering::Relaxed);
    }

    /// Mark frame as rendered
    pub fn mark_rendered(&self) {
        self.needs_redraw.store(false, Ordering::Relaxed);
    }

    // ========================================================================
    // Event Handling
    // ========================================================================

    /// Handle window resize
    ///
    /// Reconfigures GPU surface and requests layout.
    pub fn handle_resize(&mut self, renderer: &mut GpuRenderer, width: u32, height: u32) {
        tracing::debug!(width, height, "Window resized");

        // Reconfigure GPU surface
        renderer.resize(width, height);

        // Request layout for entire tree
        let mut pipeline = self.pipeline_owner.write();
        if let Some(root_id) = pipeline.root_element_id() {
            pipeline.request_layout(root_id);
            tracing::debug!("Requested layout for root after resize");
        }

        // Request redraw
        self.request_redraw();
    }

    /// Handle cursor/touch move
    ///
    /// Updates position and stores coalesced event for frame processing.
    pub fn handle_pointer_move(&mut self, position: Offset, device: PointerDeviceKind) {
        self.pointer_state.update_position(position, device);

        // Schedule high-priority input task
        self.scheduler.schedule_user_input(|| {
            tracing::trace!("Pointer move task executed");
        });
    }

    /// Handle pointer button (mouse click / touch)
    ///
    /// Routes event through interaction system (SAFE - no unsafe code!)
    pub fn handle_pointer_button(
        &mut self,
        position: Offset,
        device: PointerDeviceKind,
        button: PointerButton,
        is_down: bool,
    ) {
        let data = PointerEventData::new(position, device).with_button(button);

        let event = if is_down {
            self.pointer_state.set_down(true);
            Event::Pointer(PointerEvent::Down(data))
        } else {
            self.pointer_state.set_down(false);
            Event::Pointer(PointerEvent::Up(data))
        };

        tracing::trace!(?position, ?device, ?button, is_down, "Pointer button event");

        // Route through interaction bridge (type-safe!)
        self.route_event(event);
    }

    /// Route event through hit testing (SAFE)
    ///
    /// Uses InteractionBridge which eliminates unsafe code.
    fn route_event(&mut self, event: Event) {
        if let Some(scene) = self.scene_cache.get() {
            self.gesture.route_event(&scene, event);
        } else {
            tracing::trace!("Event dropped (no scene cached)");
        }
    }

    // ========================================================================
    // Lifecycle
    // ========================================================================

    /// Handle focus change
    pub fn handle_focus_changed(&mut self, focused: bool) {
        self.lifecycle.on_focus_changed(focused);
    }

    /// Handle visibility change
    pub fn handle_visibility_changed(&mut self, visible: bool) {
        self.lifecycle.on_visibility_changed(visible);
    }

    /// Check if rendering should occur
    pub fn should_render(&self) -> bool {
        self.lifecycle.state().should_render()
    }

    /// Get lifecycle state
    pub fn lifecycle(&self) -> &DefaultLifecycle {
        &self.lifecycle
    }

    // ========================================================================
    // Frame Rendering
    // ========================================================================

    /// Process pending events (called at start of frame)
    pub fn process_pending_events(&mut self) {
        if let Some(event) = self.pointer_state.take_pending_move() {
            self.route_event(event);
        }
    }

    /// Draw a frame (build + layout + paint)
    ///
    /// Returns the scene for GPU rendering.
    pub fn draw_frame(&self, constraints: BoxConstraints) -> Scene {
        let mut pipeline = self.pipeline_owner.write();

        // Execute pipeline: build → layout → paint
        // build_frame returns Option<Canvas>, we need to convert to Layer
        let canvas_opt = match pipeline.build_frame(constraints) {
            Ok(canvas_opt) => canvas_opt,
            Err(e) => {
                tracing::error!(error = ?e, "Pipeline build_frame failed");
                None
            }
        };

        // Extract size from constraints
        let size = constraints.constrain(Size::ZERO);

        // Create scene
        let frame_number = self.frame_coordinator.frames_rendered() + 1;
        match canvas_opt {
            Some(canvas) => {
                // Convert Canvas → CanvasLayer → Layer → Arc<Layer>
                let canvas_layer = CanvasLayer::from_canvas(canvas);
                let layer = Arc::new(Layer::Canvas(canvas_layer));
                Scene::with_layer(size, layer, frame_number)
            }
            None => Scene::new(size),
        }
    }

    /// Render a complete frame
    ///
    /// Orchestrates: begin_frame → process_events → draw → render → end_frame
    pub fn render_frame(&mut self, renderer: &mut GpuRenderer) -> Scene {
        // 1. Begin frame (scheduler callbacks)
        self.scheduler.begin_frame();

        // 2. Process coalesced pointer moves
        self.process_pending_events();

        // 3. Draw frame (build + layout + paint → Scene)
        let (width, height) = renderer.size();
        let constraints = BoxConstraints::tight(Size::new(width as f32, height as f32));
        let scene = self.draw_frame(constraints);

        // 4. Cache scene for hit testing (Arc clone is cheap!)
        self.scene_cache.update(scene.clone());

        // 5. Render scene to GPU
        let _result = self.frame_coordinator.render_scene(renderer, &scene);

        // 6. End frame (post-frame callbacks)
        self.scheduler.end_frame();

        scene
    }

    // ========================================================================
    // Statistics
    // ========================================================================

    /// Get scene cache
    pub fn scene_cache(&self) -> &SceneCache {
        &self.scene_cache
    }

    /// Get last pointer position
    pub fn last_pointer_position(&self) -> Offset {
        self.pointer_state.last_position()
    }

    /// Get frame coordinator
    pub fn frame_coordinator(&self) -> &FrameCoordinator {
        &self.frame_coordinator
    }

    /// Get scheduler statistics
    pub fn scheduler_stats(&self) -> crate::bindings::SchedulerStats {
        self.scheduler.stats()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Full integration tests require mocking PipelineOwner and Scheduler
    // These are basic unit tests for the core logic.

    #[test]
    fn test_pointer_state_integration() {
        let mut state = PointerState::new();

        state.update_position(Offset::new(100.0, 200.0), PointerDeviceKind::Mouse);
        assert_eq!(state.last_position(), Offset::new(100.0, 200.0));
        assert!(state.has_pending_move());
    }
}
