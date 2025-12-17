//! Desktop embedder - Windows, macOS, Linux
//!
//! Platform embedder for desktop environments using winit + wgpu.
//! Uses `ui-events-winit` for W3C-compliant event translation.

use crate::app::AppBinding;
use flui_engine::wgpu::SceneRenderer;
use flui_foundation::HasInstance;
use flui_interaction::events::{PointerEvent, ScrollEventData};
use flui_scheduler::Scheduler;
use flui_types::Offset;
use std::sync::Arc;
use std::time::Instant;

use ui_events_winit::{WindowEventReducer, WindowEventTranslation};
use winit::{event::WindowEvent, event_loop::ActiveEventLoop, window::Window};

/// Desktop embedder for Windows, macOS, Linux
///
/// Thin wrapper that connects platform (winit/wgpu) to AppBinding.
/// All framework logic is in AppBinding, this only handles:
/// - Window management
/// - GPU rendering
/// - Event translation (winit → ui-events → AppBinding)
///
/// # Architecture
///
/// ```text
/// DesktopEmbedder (thin platform wrapper)
///   ├─ window: Arc<Window> (winit window)
///   ├─ renderer: SceneRenderer (GPU rendering)
///   └─ event_reducer: WindowEventReducer (winit → ui-events)
///
/// AppBinding (all framework logic)
///   ├─ WidgetsBinding (build phase)
///   ├─ RenderPipelineOwner (layout/paint)
///   ├─ GestureBinding (hit testing)
///   ├─ SceneCache (hit testing cache)
///   └─ FrameCoordinator (frame stats)
/// ```
#[allow(missing_debug_implementations)]
pub struct DesktopEmbedder {
    /// Platform window
    window: Arc<Window>,

    /// GPU renderer
    renderer: SceneRenderer,

    /// Event reducer for translating winit events to ui-events
    event_reducer: WindowEventReducer,
}

impl DesktopEmbedder {
    /// Create a new desktop embedder
    ///
    /// # Arguments
    ///
    /// * `event_loop` - Active event loop for window creation
    pub async fn new(event_loop: &ActiveEventLoop) -> Result<Self, EmbedderError> {
        let _init_span = tracing::info_span!("init_embedder").entered();

        // 1. Create window using ActiveEventLoop (winit 0.30+ API)
        let window = {
            let _span = tracing::info_span!("create_window").entered();
            let window_attributes = Window::default_attributes()
                .with_title("FLUI App")
                .with_inner_size(winit::dpi::PhysicalSize::new(800, 600));

            Arc::new(
                event_loop
                    .create_window(window_attributes)
                    .map_err(|e| EmbedderError::WindowCreation(e.to_string()))?,
            )
        };

        let size = window.inner_size();

        // 2. Initialize GPU renderer
        let renderer = {
            let _span = tracing::info_span!("init_gpu").entered();
            SceneRenderer::with_window(Arc::clone(&window), size.width, size.height)
                .await
                .map_err(|e| EmbedderError::GpuInitialization(format!("{:?}", e)))?
        };

        tracing::info!(width = size.width, height = size.height, "Window created");

        Ok(Self {
            window,
            renderer,
            event_reducer: WindowEventReducer::default(),
        })
    }

    /// Handle window event
    ///
    /// Uses `ui-events-winit::WindowEventReducer` for W3C-compliant event translation.
    pub fn handle_window_event(&mut self, event: WindowEvent, elwt: &ActiveEventLoop) {
        let binding = AppBinding::instance();

        // 1. Handle events that don't go through the reducer
        match &event {
            WindowEvent::CloseRequested => {
                elwt.exit();
                return;
            }
            WindowEvent::Resized(size) => {
                self.renderer.resize(size.width, size.height);
                binding.request_redraw();
                return;
            }
            _ => {}
        }

        // 2. Use WindowEventReducer for pointer/keyboard event translation
        let scale_factor = self.window.scale_factor();
        if let Some(translation) = self.event_reducer.reduce(scale_factor, &event) {
            match translation {
                WindowEventTranslation::Pointer(pointer_event) => {
                    self.handle_pointer_event(pointer_event);
                }
                WindowEventTranslation::Keyboard(keyboard_event) => {
                    binding.handle_key_event(keyboard_event);
                }
            }
        }
    }

    /// Handle translated pointer event from ui-events-winit
    fn handle_pointer_event(&mut self, event: PointerEvent) {
        use flui_interaction::events::PointerEventData;
        use flui_interaction::events::ScrollDelta;

        let binding = AppBinding::instance();

        // Extract position from the pointer event
        if let Some(data) = PointerEventData::from_pointer_event(&event) {
            let position = data.position;
            let device = data.device_kind;

            // Determine event type and route appropriately
            match &event {
                PointerEvent::Down(_) => {
                    binding.handle_pointer_button(
                        position,
                        device,
                        flui_interaction::events::PointerButton::Primary,
                        true,
                    );
                }
                PointerEvent::Up(_) => {
                    binding.handle_pointer_button(
                        position,
                        device,
                        flui_interaction::events::PointerButton::Primary,
                        false,
                    );
                }
                PointerEvent::Move(_) => {
                    binding.handle_pointer_move(position, device);
                }
                PointerEvent::Scroll(scroll) => {
                    // Convert ScrollDelta enum to Offset
                    let delta = match scroll.delta {
                        ScrollDelta::LineDelta(x, y) | ScrollDelta::PageDelta(x, y) => {
                            Offset::new(x, y)
                        }
                        ScrollDelta::PixelDelta(pos) => Offset::new(pos.x as f32, pos.y as f32),
                    };
                    let scroll_event = ScrollEventData {
                        position,
                        delta,
                        modifiers: scroll.state.modifiers,
                    };
                    binding.handle_scroll_event(scroll_event);
                }
                // Gesture events (pinch, rotate) - pass through for future handling
                PointerEvent::Gesture(_) => {
                    // TODO: Implement gesture event handling
                }
                // Other events (Enter, Leave, Cancel, etc.)
                _ => {}
            }
        }
    }

    /// Render a frame
    ///
    /// Delegates to AppBinding for all framework logic, only handles GPU rendering.
    /// Also invokes scheduler callbacks for animations.
    pub fn render_frame(&mut self) {
        // 1. Handle scheduler frame callbacks (animations, etc.)
        let scheduler = Scheduler::instance();
        let frame_id = scheduler.handle_begin_frame(Instant::now());
        tracing::trace!(
            "render_frame: handle_begin_frame completed, frame_id={:?}",
            frame_id
        );
        scheduler.handle_draw_frame();
        tracing::trace!("render_frame: handle_draw_frame completed");

        // 2. Render the frame via AppBinding
        let binding = AppBinding::instance();
        let _scene = binding.render_frame(&mut self.renderer);
    }

    /// Check if redraw is needed
    pub fn needs_redraw(&self) -> bool {
        AppBinding::instance().needs_redraw()
    }

    /// Get the underlying winit window
    pub fn window(&self) -> &Arc<Window> {
        &self.window
    }

    /// Get window size
    pub fn size(&self) -> (u32, u32) {
        let size = self.window.inner_size();
        (size.width, size.height)
    }

    /// Request a redraw
    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }
}

/// Embedder error types
#[derive(Debug, thiserror::Error)]
pub enum EmbedderError {
    /// Failed to create window
    #[error("Failed to create window: {0}")]
    WindowCreation(String),

    /// Failed to initialize GPU
    #[error("Failed to initialize GPU: {0}")]
    GpuInitialization(String),
}
