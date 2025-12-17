//! Shared embedder implementation
//!
//! `EmbedderCore` contains all the common logic shared across platforms.
//! Platform-specific embedders compose this rather than duplicating code.

use super::{EmbedderScheduler, FrameCoordinator, PointerState, SceneCache};
use crate::app::AppLifecycle;
use flui_engine::wgpu::SceneRenderer;
use flui_interaction::{
    binding::GestureBinding,
    events::{
        make_pointer_event, Event, PointerButton, PointerEventData, PointerEventKind, PointerType,
    },
};
use flui_layer::Scene;
use flui_rendering::{constraints::BoxConstraints, pipeline::PipelineOwner};
use flui_types::{Offset, Size};
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
///   ├─ pipeline_owner: Arc<RwLock<PipelineOwner>> - Rendering pipeline
///   ├─ needs_redraw: Arc<AtomicBool> - On-demand rendering flag
///   ├─ scene_cache: SceneCache - Type-safe scene caching
///   ├─ pointer_state: PointerState - Pointer tracking/coalescing
///   ├─ frame_coordinator: FrameCoordinator - Frame rendering
///   ├─ scheduler: EmbedderScheduler - Frame scheduling
///   ├─ gesture: GestureBinding - Safe hit testing
///   └─ lifecycle: AppLifecycle - Lifecycle tracking
/// ```
#[derive(Debug)]
pub struct EmbedderCore {
    /// Render pipeline owner
    pipeline_owner: Arc<RwLock<PipelineOwner>>,

    /// On-demand rendering flag
    needs_redraw: Arc<AtomicBool>,

    /// Scene cache for hit testing (type-safe!)
    scene_cache: SceneCache,

    /// Pointer state tracker (coalescing, position tracking)
    pointer_state: PointerState,

    /// Frame coordinator (orchestrates rendering)
    frame_coordinator: FrameCoordinator,

    /// Embedder scheduler (frame scheduling)
    scheduler: EmbedderScheduler,

    /// Gesture binding (safe hit testing)
    gesture: GestureBinding,

    /// Lifecycle tracking
    lifecycle: AppLifecycle,
}

impl EmbedderCore {
    /// Create a new embedder core
    ///
    /// # Arguments
    ///
    /// * `pipeline_owner` - Shared pipeline owner from AppBinding
    /// * `needs_redraw` - Shared redraw flag from AppBinding
    /// * `scheduler` - Scheduler instance
    pub fn new(
        pipeline_owner: Arc<RwLock<PipelineOwner>>,
        needs_redraw: Arc<AtomicBool>,
        scheduler: Arc<flui_scheduler::Scheduler>,
    ) -> Self {
        let scheduler_binding = EmbedderScheduler::new(scheduler);
        let gesture_binding = GestureBinding::new();

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
            lifecycle: AppLifecycle::default(),
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
    pub fn handle_resize(&mut self, renderer: &mut SceneRenderer, width: u32, height: u32) {
        renderer.resize(width, height);

        // Request visual update which will trigger layout flush
        let pipeline = self.pipeline_owner.read();
        pipeline.request_visual_update();
        drop(pipeline);

        self.request_redraw();
    }

    /// Handle cursor/touch move
    ///
    /// Updates position and stores coalesced event for frame processing.
    pub fn handle_pointer_move(&mut self, position: Offset, device: PointerType) {
        self.pointer_state.update_position(position, device);

        // Schedule high-priority input task
        self.scheduler.schedule_user_input(|| {});
    }

    /// Handle pointer button (mouse click / touch)
    ///
    /// Routes event through interaction system
    pub fn handle_pointer_button(
        &mut self,
        position: Offset,
        device: PointerType,
        _button: PointerButton,
        is_down: bool,
    ) {
        let data = PointerEventData::new(position, device);

        let kind = if is_down {
            self.pointer_state.set_down(true);
            PointerEventKind::Down
        } else {
            self.pointer_state.set_down(false);
            PointerEventKind::Up
        };

        let pointer_event = make_pointer_event(kind, data);
        let event = Event::Pointer(pointer_event);

        // Route through interaction system
        self.route_event(event);
    }

    /// Handle keyboard event
    pub fn handle_key_event(&mut self, key_event: flui_interaction::events::KeyboardEvent) {
        let event = Event::Keyboard(key_event);
        self.route_event(event);
    }

    /// Handle scroll event
    pub fn handle_scroll_event(&mut self, scroll_event: flui_interaction::events::ScrollEventData) {
        let event = Event::Scroll(scroll_event);
        self.route_event(event);
    }

    /// Route event through hit testing
    fn route_event(&mut self, event: Event) {
        // For pointer events, use GestureBinding's hit test system
        if let Event::Pointer(ref pointer_event) = event {
            self.gesture
                .handle_pointer_event(pointer_event, |_position| {
                    // TODO: Implement proper hit testing through scene/render tree
                    // For now return empty result
                    flui_interaction::routing::HitTestResult::new()
                });
        }
        // Keyboard and scroll events don't go through gesture binding
        // They are routed directly to focused elements (future implementation)
    }

    // ========================================================================
    // Lifecycle
    // ========================================================================

    /// Handle focus change
    pub fn handle_focus_changed(&mut self, focused: bool) {
        self.lifecycle = if focused {
            AppLifecycle::Resumed
        } else {
            AppLifecycle::Inactive
        };
    }

    /// Handle visibility change
    pub fn handle_visibility_changed(&mut self, visible: bool) {
        self.lifecycle = if visible {
            AppLifecycle::Resumed
        } else {
            AppLifecycle::Paused
        };
    }

    /// Get lifecycle state
    pub fn lifecycle(&self) -> AppLifecycle {
        self.lifecycle
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

        // Execute pipeline phases: layout → compositing bits → paint → semantics
        pipeline.flush_all();

        // Extract size from constraints
        let size = constraints.constrain(Size::ZERO);

        // Create scene
        // TODO: Get canvas from paint phase output when fully integrated
        let _frame_number = self.frame_coordinator.frames_rendered() + 1;
        Scene::empty(size.into())
    }

    /// Render a complete frame
    ///
    /// Orchestrates: begin_frame → process_events → draw → render → end_frame
    #[tracing::instrument(level = "debug", skip_all)]
    pub fn render_frame(&mut self, renderer: &mut SceneRenderer) -> Arc<Scene> {
        // 1. Begin frame (scheduler callbacks)
        self.scheduler.begin_frame();

        // 2. Process coalesced pointer moves
        self.process_pending_events();

        // 3. Draw frame (build + layout + paint → Scene)
        let (width, height) = renderer.size();
        let constraints = BoxConstraints::tight(Size::new(width as f32, height as f32));
        let scene = Arc::new(self.draw_frame(constraints));

        // 4. Cache scene for hit testing (Arc clone is cheap!)
        self.scene_cache.update(Arc::clone(&scene));

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
    pub fn scheduler_stats(&self) -> SchedulerStats {
        self.scheduler.stats()
    }
}

use super::SchedulerStats;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pointer_state_integration() {
        let mut state = PointerState::new();

        state.update_position(Offset::new(100.0, 200.0), PointerType::Mouse);
        assert_eq!(state.last_position(), Offset::new(100.0, 200.0));
        assert!(state.has_pending_move());
    }
}
