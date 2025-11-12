//! wgpu embedder - winit + wgpu integration
//!
//! This embedder creates a window using winit and renders using wgpu.
//! It bridges platform events to the binding layer and renders scenes to the GPU.
//!
//! # Clean Architecture
//!
//! This implementation follows clean architecture principles:
//! - **Platform Layer** (this file): Window management, event loop
//! - **Rendering Layer** (flui_engine::GpuRenderer): GPU abstraction
//! - **Framework Layer** (AppBinding): UI framework coordination

use crate::binding::AppBinding;
use crate::{event_callbacks::WindowEventCallbacks, window_state::WindowStateTracker};
use flui_engine::{GpuRenderer, Scene};
use flui_types::{
    constraints::BoxConstraints,
    events::{PointerButton, PointerDeviceKind, PointerEventData},
    Offset, Size,
};
use std::sync::Arc;
use std::time::Instant;
use winit::{
    event::*,
    event_loop::{ControlFlow, EventLoop},
    window::Window,
};

/// wgpu embedder for FLUI apps
///
/// # Clean Architecture
///
/// ```text
/// WgpuEmbedder (Platform Layer)
///   ├─ Window (winit) - Platform window management
///   ├─ GpuRenderer (flui_engine) - Encapsulates ALL GPU resources
///   ├─ AppBinding (framework) - UI framework coordination
///   └─ Scene cache - For hit testing (Arc-based sharing)
/// ```
///
/// # Event Flow
///
/// ```text
/// winit events → handle_window_event() → GestureBinding → EventRouter
/// vsync → render_frame() → SchedulerBinding → PipelineOwner → Scene → GpuRenderer
/// ```
///
/// # Design Principles
///
/// - **Separation of Concerns**: Platform vs Rendering vs Framework
/// - **Dependency Inversion**: Embedder depends on GpuRenderer abstraction, not wgpu directly
/// - **Zero Duplication**: All wgpu code is in flui_engine::GpuRenderer
/// - **Arc-based Sharing**: Scene layers shared between rendering and hit testing
pub struct WgpuEmbedder {
    /// Framework binding (gesture, scheduler, renderer, widgets)
    binding: Arc<AppBinding>,

    /// winit window (platform)
    window: Arc<Window>,

    /// GPU renderer (encapsulates ALL wgpu resources: device, queue, surface, painter)
    /// This is the ONLY GPU-related field - clean separation of concerns!
    renderer: GpuRenderer,

    /// App start time (for frame timestamps)
    start_time: Instant,

    /// Last cursor position (for mouse events)
    last_cursor_position: Offset,

    /// Last rendered scene (cached for hit testing)
    /// Arc-based sharing allows zero-copy access to layer tree
    last_scene: Option<Scene>,

    /// Window state tracker (focus, visibility)
    window_state: WindowStateTracker,

    /// User-defined window event callbacks
    event_callbacks: WindowEventCallbacks,

    /// Pending pointer move event (for coalescing high-frequency events)
    ///
    /// Mouse move events can fire at very high rates (1000+ Hz on gaming mice).
    /// We coalesce consecutive moves by only processing the last one per frame.
    /// This reduces CPU overhead while maintaining smooth cursor tracking.
    pending_pointer_move: Option<PointerEventData>,
}

impl WgpuEmbedder {
    /// Create a new wgpu embedder (async version for WASM compatibility)
    ///
    /// # Parameters
    ///
    /// - `binding`: The framework binding (from `AppBinding::ensure_initialized()`)
    /// - `event_loop`: The winit event loop
    ///
    /// # Clean Architecture
    ///
    /// This constructor delegates ALL GPU initialization to `GpuRenderer::new_async()`.
    /// The embedder only handles platform-specific concerns (window creation).
    ///
    /// **Before**: 90+ lines of wgpu initialization code (duplicated from GpuRenderer)
    /// **After**: 1 line - `GpuRenderer::new_async(window).await`
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let binding = AppBinding::ensure_initialized();
    /// let event_loop = EventLoop::new();
    /// let embedder = WgpuEmbedder::new(binding, &event_loop).await;
    /// embedder.run(event_loop);
    /// ```
    pub async fn new(binding: Arc<AppBinding>, event_loop: &EventLoop<()>) -> Self {
        tracing::info!("Initializing wgpu embedder");

        // 1. Create window (platform layer responsibility)
        let window_attributes = Window::default_attributes()
            .with_title("FLUI App")
            .with_inner_size(winit::dpi::PhysicalSize::new(800, 600));

        let window = Arc::new(
            event_loop
                .create_window(window_attributes)
                .expect("Failed to create window"),
        );

        tracing::debug!("Window created");

        // 2. Initialize GPU renderer (delegates to flui_engine - ZERO duplication!)
        let renderer = GpuRenderer::new_async(Arc::clone(&window)).await;

        tracing::info!(
            size = ?renderer.size(),
            format = ?renderer.format(),
            "WgpuEmbedder initialized with clean architecture"
        );

        Self {
            binding,
            window,
            renderer,
            start_time: Instant::now(),
            last_cursor_position: Offset::ZERO,
            last_scene: None,
            window_state: WindowStateTracker::new(),
            event_callbacks: WindowEventCallbacks::new(),
            pending_pointer_move: None,
        }
    }

    /// Get mutable reference to window event callbacks
    ///
    /// Use this to register callbacks for system events like focus changes,
    /// minimization, DPI changes, theme changes, etc.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let mut embedder = WgpuEmbedder::new(binding, &event_loop).await;
    ///
    /// embedder.event_callbacks_mut().on_focus(|focused| {
    ///     if focused {
    ///         println!("Window gained focus");
    ///     } else {
    ///         println!("Window lost focus");
    ///     }
    /// });
    /// ```
    pub fn event_callbacks_mut(&mut self) -> &mut WindowEventCallbacks {
        &mut self.event_callbacks
    }

    /// Get reference to window state tracker
    ///
    /// Access window focus and visibility state.
    pub fn window_state(&self) -> &WindowStateTracker {
        &self.window_state
    }

    /// Run the event loop
    ///
    /// This method blocks until the window is closed.
    ///
    /// # Event Loop Strategy
    ///
    /// Uses `ControlFlow::Poll` for continuous rendering (suitable for animations/games).
    /// For lower power consumption, could switch to `ControlFlow::Wait` and only
    /// request_redraw() when state changes.
    pub fn run(mut self, event_loop: EventLoop<()>) -> ! {
        tracing::info!("Starting event loop");

        event_loop
            .run(move |event, elwt| {
                elwt.set_control_flow(ControlFlow::Poll);

                match event {
                    Event::AboutToWait => {
                        // Request redraw every frame (for animations)
                        self.window.request_redraw();
                    }

                    Event::WindowEvent { event, .. } => match event {
                        WindowEvent::RedrawRequested => {
                            self.render_frame();
                        }
                        other => {
                            self.handle_window_event(other, elwt);
                        }
                    },

                    _ => {}
                }
            })
            .expect("Event loop error");

        // Unreachable, but needed to satisfy return type !
        std::process::exit(0)
    }

    /// Handle window events
    ///
    /// Routes platform events to the appropriate binding layer.
    /// Processes system events (focus, theme, DPI) before user events.
    fn handle_window_event(
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
            }

            WindowEvent::CursorMoved { position, .. } => {
                // Update cursor position
                self.last_cursor_position = Offset::new(position.x as f32, position.y as f32);

                // EVENT COALESCING: Store the move event, will be processed in render_frame()
                // This reduces CPU overhead for high-frequency mouse moves (1000+ Hz gaming mice)
                let data =
                    PointerEventData::new(self.last_cursor_position, PointerDeviceKind::Mouse);

                self.pending_pointer_move = Some(data);

                // Schedule task with UserInput priority (highest) to process on next frame
                // This ensures smooth cursor tracking while avoiding redundant processing
                self.binding.scheduler.scheduler().add_task(
                    flui_scheduler::Priority::UserInput,
                    || {
                        // Task will trigger frame processing
                        tracing::trace!("Pointer move task scheduled");
                    },
                );
            }

            WindowEvent::MouseInput { state, button, .. } => {
                // Use last tracked cursor position
                let data = PointerEventData::new(self.last_cursor_position, PointerDeviceKind::Mouse)
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
                        // SAFETY: Safe for same reasons as pointer move event above
                        let layer_ptr = Arc::as_ptr(layer) as *mut flui_engine::CanvasLayer;
                        unsafe {
                            self.binding.gesture.handle_event(event, &mut *layer_ptr);
                        }
                    }
                } else {
                    tracing::trace!(
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

    /// Render a frame
    ///
    /// # Clean Architecture Flow
    ///
    /// ```text
    /// 1. Scheduler::handle_begin_frame() - Frame callbacks
    /// 2. RendererBinding::draw_frame() - Build → Layout → Paint → Scene
    /// 3. GpuRenderer::render() - Scene → GPU commands → Present
    /// 4. Scheduler::handle_draw_frame() - Post-frame callbacks
    /// ```
    ///
    /// # Layer Sharing
    ///
    /// The Scene is cloned (Arc clone is cheap!) and cached for hit testing.
    /// This allows both rendering and event routing to access the same layer tree.
    fn render_frame(&mut self) {
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

        let scene = self.binding.renderer.draw_frame(constraints);

        // 3. Cache scene for hit testing (Arc clone is cheap!)
        //    This enables zero-copy sharing between rendering and event routing
        if scene.has_content() {
            self.last_scene = Some(scene.clone());
            tracing::trace!(
                frame = scene.frame_number(),
                "Scene cached for hit testing"
            );
        }

        // 4. Render scene to GPU (delegates ALL GPU work to GpuRenderer)
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
