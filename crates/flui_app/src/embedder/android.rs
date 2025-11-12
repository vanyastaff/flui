//! Android embedder - Android mobile platform
//!
//! This embedder handles Android-specific lifecycle events (Resumed/Suspended)
//! and manages GPU resources appropriately for mobile battery optimization.

use crate::binding::AppBinding;
use crate::{event_callbacks::WindowEventCallbacks, window_state::WindowStateTracker};
use flui_engine::{GpuRenderer, Scene};
use flui_types::{
    constraints::BoxConstraints,
    events::{PointerButton, PointerDeviceKind, PointerEventData},
    Offset, Size,
};
use std::sync::Arc;
use winit::{
    event::*,
    event_loop::ActiveEventLoop,
    window::Window,
};

/// Android embedder for FLUI apps
///
/// This embedder is designed for Android platform with explicit lifecycle management.
/// It handles Android lifecycle events (Resumed/Suspended) to properly manage
/// GPU resources and battery consumption.
///
/// # Architecture
///
/// ```text
/// AndroidEmbedder
///   ├─ Window (winit + android-activity) - Android window
///   ├─ GpuRenderer (flui_engine) - GPU rendering (Vulkan on Android)
///   ├─ AppBinding (framework) - UI framework coordination
///   ├─ is_suspended flag - Lifecycle state tracking
///   └─ Scene cache - For hit testing
/// ```
///
/// # Lifecycle
///
/// ```text
/// Created (no window) → Resumed (window + GPU) ⇄ Suspended (no GPU)
/// ```
pub struct AndroidEmbedder {
    /// Framework binding (gesture, scheduler, renderer, widgets)
    binding: Arc<AppBinding>,

    /// winit window (Android)
    window: Arc<Window>,

    /// GPU renderer (encapsulates ALL wgpu/Vulkan resources)
    renderer: GpuRenderer,

    /// Last cursor position (for touch events)
    last_cursor_position: Offset,

    /// Last rendered scene (cached for hit testing)
    last_scene: Option<Scene>,

    /// Window state tracker (focus, visibility)
    window_state: WindowStateTracker,

    /// User-defined window event callbacks
    event_callbacks: WindowEventCallbacks,

    /// Pending pointer move event (for coalescing)
    pending_pointer_move: Option<PointerEventData>,

    /// Android lifecycle state - true when app is in background
    is_suspended: bool,
}

impl AndroidEmbedder {
    /// Create a new Android embedder
    ///
    /// This constructor is called when the Android app receives the Resumed event.
    ///
    /// # Parameters
    ///
    /// - `binding`: The framework binding
    /// - `event_loop`: The active event loop (from Resumed event)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// Event::Resumed => {
    ///     let embedder = AndroidEmbedder::new(binding.clone(), elwt).await;
    /// }
    /// ```
    pub async fn new(binding: Arc<AppBinding>, event_loop: &ActiveEventLoop) -> Self {
        log::info!("Initializing Android embedder");

        // 1. Create window from active event loop (Android-specific)
        let window_attributes = Window::default_attributes()
            .with_title("FLUI App")
            .with_inner_size(winit::dpi::PhysicalSize::new(800, 600));

        let window = Arc::new(
            event_loop
                .create_window(window_attributes)
                .expect("Failed to create Android window"),
        );

        log::debug!("Android window created");

        // 2. Initialize GPU renderer (Vulkan on Android)
        let renderer = GpuRenderer::new_async(Arc::clone(&window)).await;

        log::info!(
            "Android embedder initialized: {}x{} {:?}",
            renderer.size().0,
            renderer.size().1,
            renderer.format()
        );

        Self {
            binding,
            window,
            renderer,
            last_cursor_position: Offset::ZERO,
            last_scene: None,
            window_state: WindowStateTracker::new(),
            event_callbacks: WindowEventCallbacks::new(),
            pending_pointer_move: None,
            is_suspended: false,
        }
    }

    /// Mark embedder as suspended (app backgrounded)
    ///
    /// This stops rendering to save battery. The embedder should be dropped
    /// shortly after calling this to release GPU resources.
    pub fn suspend(&mut self) {
        if self.is_suspended {
            log::warn!("suspend() called but already suspended");
            return;
        }

        self.is_suspended = true;
        log::info!("Android embedder suspended (rendering stopped)");
    }

    /// Mark embedder as resumed (app foregrounded)
    ///
    /// This resumes rendering. Note: A new embedder is usually created on resume
    /// rather than reusing the old one.
    pub fn resume(&mut self) {
        if !self.is_suspended {
            log::warn!("resume() called but not suspended");
            return;
        }

        self.is_suspended = false;
        log::info!("Android embedder resumed (rendering active)");
    }

    /// Check if embedder is suspended
    pub fn is_suspended(&self) -> bool {
        self.is_suspended
    }

    /// Get mutable reference to window event callbacks
    pub fn event_callbacks_mut(&mut self) -> &mut WindowEventCallbacks {
        &mut self.event_callbacks
    }

    /// Get reference to window state tracker
    pub fn window_state(&self) -> &WindowStateTracker {
        &self.window_state
    }

    /// Get reference to window
    pub fn window(&self) -> &Arc<Window> {
        &self.window
    }

    /// Handle window event (called from external event loop)
    ///
    /// This is public because Android uses an external event loop in android_main().
    pub fn handle_event(
        &mut self,
        event: WindowEvent,
        elwt: &ActiveEventLoop,
    ) {
        self.handle_window_event(event, elwt);
    }

    /// Handle window events
    fn handle_window_event(
        &mut self,
        event: WindowEvent,
        elwt: &ActiveEventLoop,
    ) {
        // STEP 1: Update WindowStateTracker FIRST (before user callbacks)
        match &event {
            WindowEvent::Focused(focused) => {
                self.window_state.on_focus_changed(*focused);
            }
            WindowEvent::Occluded(occluded) => {
                // Occluded = true means window is NOT visible (minimized/covered)
                self.window_state.on_visibility_changed(!occluded);
            }
            _ => {}
        }

        // STEP 2: Call user-defined callbacks for all events
        self.event_callbacks.handle_event(&event);

        // STEP 3: Handle framework-level events
        match event {
            WindowEvent::CloseRequested => {
                log::info!("Window close requested");
                elwt.exit();
            }

            WindowEvent::Resized(size) => {
                log::debug!("Android window resized: {}x{}", size.width, size.height);

                // Delegate resize to GpuRenderer (handles surface reconfiguration)
                self.renderer.resize(size.width, size.height);

                // Request layout for the entire tree with new window size
                let pipeline = self.binding.pipeline.pipeline_owner();
                let mut pipeline_write = pipeline.write();
                if let Some(root_id) = pipeline_write.root_element_id() {
                    pipeline_write.request_layout(root_id);
                    log::debug!("Requested layout for root after resize");
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                // Update cursor position (touch events on Android)
                self.last_cursor_position = Offset::new(position.x as f32, position.y as f32);

                // EVENT COALESCING: Store the move event
                let data =
                    PointerEventData::new(self.last_cursor_position, PointerDeviceKind::Touch);

                self.pending_pointer_move = Some(data);

                // Schedule task with UserInput priority
                self.binding.scheduler.scheduler().add_task(
                    flui_scheduler::Priority::UserInput,
                    || {
                        log::trace!("Touch move task scheduled");
                    },
                );
            }

            WindowEvent::MouseInput { state, button, .. } => {
                // On Android, this handles touch down/up events
                let data = PointerEventData::new(self.last_cursor_position, PointerDeviceKind::Touch)
                    .with_button(convert_mouse_button(button));

                let event = match state {
                    ElementState::Pressed => {
                        flui_types::Event::Pointer(flui_types::PointerEvent::Down(data))
                    }
                    ElementState::Released => {
                        flui_types::Event::Pointer(flui_types::PointerEvent::Up(data))
                    }
                };

                // Route event using cached scene for hit testing
                if let Some(ref scene) = self.last_scene {
                    if let Some(layer) = scene.root_layer() {
                        let layer_ptr = Arc::as_ptr(layer) as *mut flui_engine::CanvasLayer;
                        unsafe {
                            self.binding.gesture.handle_event(event, &mut *layer_ptr);
                        }
                    }
                } else {
                    log::trace!("Touch event (no scene cached): {:?} {:?}", state, button);
                }
            }

            _ => {
                // TODO: Handle other events (keyboard, etc.)
            }
        }
    }

    /// Render a frame (PUBLIC for external event loop)
    ///
    /// Skips rendering if suspended to save battery.
    pub fn render_frame(&mut self) {
        // Skip rendering if suspended (battery optimization)
        if self.is_suspended {
            log::trace!("Skipping render (suspended)");
            return;
        }

        // 1. Begin frame (scheduler callbacks)
        let _frame_id = self.binding.scheduler.scheduler().begin_frame();

        // 1.5. Process coalesced pointer move events (if any)
        if let Some(data) = self.pending_pointer_move.take() {
            let event = flui_types::Event::Pointer(flui_types::PointerEvent::Move(data));

            // Route event using cached scene for hit testing
            if let Some(ref scene) = self.last_scene {
                if let Some(layer) = scene.root_layer() {
                    let layer_ptr = Arc::as_ptr(layer) as *mut flui_engine::CanvasLayer;
                    unsafe {
                        self.binding.gesture.handle_event(event, &mut *layer_ptr);
                    }
                }
            }
        }

        // 2. Draw frame (build + layout + paint → Scene)
        let (width, height) = self.renderer.size();
        let constraints = BoxConstraints::tight(Size::new(width as f32, height as f32));

        let scene = self.binding.renderer.draw_frame(constraints);

        // 3. Cache scene for hit testing (Arc clone is cheap!)
        if scene.has_content() {
            self.last_scene = Some(scene.clone());
            log::trace!("Scene cached for hit testing (frame {})", scene.frame_number());
        }

        // 4. Render scene to GPU (Vulkan on Android)
        if let Some(layer) = scene.root_layer() {
            match self.renderer.render(layer.as_ref()) {
                Ok(()) => {
                    log::trace!("Frame {} rendered successfully", scene.frame_number());
                }
                Err(flui_engine::RenderError::SurfaceLost)
                | Err(flui_engine::RenderError::SurfaceOutdated) => {
                    log::debug!("Surface lost/outdated, will retry next frame");
                }
                Err(e) => {
                    log::error!("Render error: {:?}", e);
                }
            }
        } else {
            log::trace!("Empty scene, skipping render");
        }

        // 5. Post-frame callbacks
        self.binding.scheduler.scheduler().end_frame();
    }
}

/// Convert winit mouse button to FLUI pointer button
fn convert_mouse_button(button: MouseButton) -> PointerButton {
    match button {
        MouseButton::Left => PointerButton::Primary,
        MouseButton::Right => PointerButton::Secondary,
        MouseButton::Middle => PointerButton::Middle,
        MouseButton::Other(n) => PointerButton::Other(n as u8),
        _ => PointerButton::Primary,
    }
}
