//! Desktop embedder - Windows, macOS, Linux
//!
//! This embedder handles desktop platforms using a synchronous event loop.
//! It creates a window, initializes GPU rendering, and runs the application
//! event loop until the window is closed.

use crate::binding::AppBinding;
use crate::{event_callbacks::WindowEventCallbacks, window_state::WindowStateTracker};
use flui_engine::{GpuRenderer, Scene};
use flui_types::{
    constraints::BoxConstraints,
    events::{PointerButton, PointerDeviceKind, PointerEventData},
    Offset, Size,
};
use std::sync::Arc;
use winit::{event::*, window::Window};

/// Desktop embedder for FLUI apps
///
/// This embedder is designed for desktop platforms (Windows, macOS, Linux).
/// It uses a synchronous event loop and manages the complete application lifecycle.
///
/// # Architecture
///
/// ```text
/// DesktopEmbedder
///   ├─ Window (winit) - Desktop window management
///   ├─ GpuRenderer (flui_engine) - GPU rendering abstraction
///   ├─ AppBinding (framework) - UI framework coordination
///   └─ Scene cache - For hit testing
/// ```
pub struct DesktopEmbedder {
    /// Framework binding (gesture, scheduler, renderer, widgets)
    binding: Arc<AppBinding>,

    /// winit window (desktop)
    window: Arc<Window>,

    /// GPU renderer (encapsulates ALL wgpu resources)
    renderer: GpuRenderer,

    /// Last cursor position (for mouse events)
    last_cursor_position: Offset,

    /// Last rendered scene (cached for hit testing)
    last_scene: Option<Scene>,

    /// Window state tracker (focus, visibility)
    window_state: WindowStateTracker,

    /// User-defined window event callbacks
    event_callbacks: WindowEventCallbacks,

    /// Pending pointer move event (for coalescing)
    pending_pointer_move: Option<PointerEventData>,
}

impl DesktopEmbedder {
    /// Create a new desktop embedder
    ///
    /// # Parameters
    ///
    /// - `binding`: The framework binding
    /// - `event_loop`: The active event loop (winit 0.30+ requires ActiveEventLoop)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let binding = AppBinding::ensure_initialized();
    /// // Create embedder inside event loop after Resumed event
    /// event_loop.run(|event, elwt| {
    ///     if let Event::Resumed = event {
    ///         let embedder = DesktopEmbedder::new(binding, elwt).await;
    ///     }
    /// });
    /// ```
    pub async fn new(
        binding: Arc<AppBinding>,
        event_loop: &winit::event_loop::ActiveEventLoop,
    ) -> Self {
        // 1. Create window using ActiveEventLoop (NEW winit 0.30+ API)
        // This avoids the deprecated EventLoop::create_window()
        let window_attributes = Window::default_attributes()
            .with_title("FLUI App")
            .with_inner_size(winit::dpi::PhysicalSize::new(800, 600));

        let window = Arc::new(
            event_loop
                .create_window(window_attributes)
                .expect("Failed to create window"),
        );

        let size = window.inner_size();

        // 2. Initialize GPU renderer with window (using raw_window_handle)
        // This ensures surface is created from the same wgpu::Instance that GpuRenderer uses
        // Fixes "Surface does not exist" error when instance/surface are from different instances
        let renderer =
            GpuRenderer::new_async_with_window(Arc::clone(&window), size.width, size.height).await;

        tracing::info!(
            width = size.width,
            height = size.height,
            format = ?renderer.format(),
            "GPU initialized"
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
        }
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

    /// Handle window events (PUBLIC - called from run_app event loop)
    pub fn handle_window_event(
        &mut self,
        event: WindowEvent,
        elwt: &winit::event_loop::ActiveEventLoop,
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
                tracing::info!("Window close requested");
                elwt.exit();
            }

            WindowEvent::Resized(size) => {
                tracing::debug!(width = size.width, height = size.height, "Window resized");

                // Delegate resize to GpuRenderer (handles surface reconfiguration)
                self.renderer.resize(size.width, size.height);

                // Request layout for the entire tree with new window size
                let pipeline = self.binding.pipeline();
                let mut pipeline_write = pipeline.write();
                if let Some(root_id) = pipeline_write.root_element_id() {
                    pipeline_write.request_layout(root_id);
                    tracing::debug!("Requested layout for root after resize");
                }

                // Request redraw after resize
                self.binding.request_redraw();
            }

            WindowEvent::CursorMoved { position, .. } => {
                // Update cursor position
                self.last_cursor_position = Offset::new(position.x as f32, position.y as f32);

                // EVENT COALESCING: Store the move event, will be processed in render_frame()
                let data =
                    PointerEventData::new(self.last_cursor_position, PointerDeviceKind::Mouse);

                self.pending_pointer_move = Some(data);

                // Schedule task with UserInput priority (highest)
                self.binding.scheduler.scheduler().add_task(
                    flui_scheduler::Priority::UserInput,
                    || {
                        tracing::trace!("Pointer move task scheduled");
                    },
                );
            }

            WindowEvent::MouseInput { state, button, .. } => {
                // Use last tracked cursor position
                let data =
                    PointerEventData::new(self.last_cursor_position, PointerDeviceKind::Mouse)
                        .with_button(convert_mouse_button(button));

                let event = match state {
                    ElementState::Pressed => {
                        flui_types::Event::Pointer(flui_types::PointerEvent::Down(data))
                    }
                    ElementState::Released => {
                        flui_types::Event::Pointer(flui_types::PointerEvent::Up(data))
                    }
                };

                tracing::trace!(
                    position = ?self.last_cursor_position,
                    state = ?state,
                    button = ?button,
                    "MouseInput event"
                );

                // Route event using cached scene for hit testing
                if let Some(ref scene) = self.last_scene {
                    if let Some(layer) = scene.root_layer() {
                        // SAFETY: Safe for same reasons as pointer move event above
                        let layer_ptr = Arc::as_ptr(layer) as *mut flui_engine::CanvasLayer;
                        unsafe {
                            self.binding.gesture.handle_event(event, &mut *layer_ptr);
                        }
                    } else {
                        tracing::warn!("Scene has no root layer for hit testing");
                    }
                } else {
                    tracing::warn!(
                        "Mouse button event (no scene cached): {:?} {:?}",
                        state,
                        button
                    );
                }
            }

            _ => {
                // TODO: Handle other events (keyboard, scroll, touch, etc.)
            }
        }
    }

    /// Render a frame (PUBLIC - called from run_app event loop)
    pub fn render_frame(&mut self) {
        // 1. Begin frame (scheduler callbacks)
        let _frame_id = self.binding.scheduler.scheduler().begin_frame();

        // 1.5. Process coalesced pointer move events (if any)
        if let Some(data) = self.pending_pointer_move.take() {
            let event = flui_types::Event::Pointer(flui_types::PointerEvent::Move(data));

            // Route event using cached scene for hit testing
            if let Some(ref scene) = self.last_scene {
                if let Some(layer) = scene.root_layer() {
                    // SAFETY: This is safe because:
                    // 1. We have exclusive access to self (via &mut self)
                    // 2. Hit testing only reads, doesn't mutate the layer structure
                    // 3. The Arc ensures the layer stays alive during this call
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

        let scene = self.binding.draw_frame(constraints);

        // 3. Cache scene for hit testing (Arc clone is cheap!)
        if scene.has_content() {
            self.last_scene = Some(scene.clone());
            tracing::trace!(frame = scene.frame_number(), "Scene cached for hit testing");
        }

        // 4. Render scene to GPU
        if let Some(layer) = scene.root_layer() {
            match self.renderer.render(layer.as_ref()) {
                Ok(()) => {
                    tracing::trace!(frame = scene.frame_number(), "Frame rendered successfully");
                }
                Err(flui_engine::RenderError::SurfaceLost)
                | Err(flui_engine::RenderError::SurfaceOutdated) => {
                    // GpuRenderer already reconfigured surface, will retry next frame
                    tracing::debug!("Surface lost/outdated, will retry next frame");
                }
                Err(e) => {
                    tracing::error!("Render error: {:?}", e);
                }
            }
        } else {
            // No content to render (empty scene)
            tracing::trace!("Empty scene, skipping render");
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
        _ => PointerButton::Primary, // Default for unknown buttons
    }
}
