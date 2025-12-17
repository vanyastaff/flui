//! Desktop embedder - Windows, macOS, Linux
//!
//! Platform embedder for desktop environments using winit + wgpu.
//! Uses `ui-events-winit` for W3C-compliant event translation.

use super::EmbedderCore;
use flui_engine::wgpu::SceneRenderer;
use flui_interaction::events::{PointerEvent, ScrollEventData};
use flui_rendering::pipeline::PipelineOwner;
use flui_types::Offset;
use parking_lot::RwLock;
use std::sync::{atomic::AtomicBool, Arc};

use ui_events_winit::{WindowEventReducer, WindowEventTranslation};
use winit::{event::WindowEvent, event_loop::ActiveEventLoop, window::Window};

/// Desktop embedder for Windows, macOS, Linux
///
/// Thin wrapper around `EmbedderCore` with desktop-specific event handling.
/// Uses `ui-events-winit::WindowEventReducer` for W3C-compliant event translation.
///
/// # Architecture
///
/// ```text
/// DesktopEmbedder (thin wrapper)
///   ├─ core: EmbedderCore (90% of logic)
///   ├─ window: Arc<Window> (winit window)
///   ├─ renderer: SceneRenderer (GPU rendering)
///   └─ event_reducer: WindowEventReducer (winit → ui-events)
/// ```
#[allow(missing_debug_implementations)]
pub struct DesktopEmbedder {
    /// Shared embedder core (90% of logic)
    core: EmbedderCore,

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
    /// * `pipeline_owner` - Shared pipeline from AppBinding
    /// * `needs_redraw` - Shared redraw flag from AppBinding
    /// * `scheduler` - Scheduler instance
    /// * `event_loop` - Active event loop for window creation
    pub async fn new(
        pipeline_owner: Arc<RwLock<PipelineOwner>>,
        needs_redraw: Arc<AtomicBool>,
        scheduler: Arc<flui_scheduler::Scheduler>,
        event_loop: &ActiveEventLoop,
    ) -> Result<Self, EmbedderError> {
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

        // 3. Create embedder core (shared logic)
        let core = EmbedderCore::new(pipeline_owner, needs_redraw, scheduler);

        Ok(Self {
            core,
            window,
            renderer,
            event_reducer: WindowEventReducer::default(),
        })
    }

    /// Handle window event
    ///
    /// Uses `ui-events-winit::WindowEventReducer` for W3C-compliant event translation.
    pub fn handle_window_event(&mut self, event: WindowEvent, elwt: &ActiveEventLoop) {
        // 1. Update lifecycle state
        match &event {
            WindowEvent::Focused(focused) => {
                self.core.handle_focus_changed(*focused);
            }
            WindowEvent::Occluded(occluded) => {
                self.core.handle_visibility_changed(!occluded);
            }
            _ => {}
        }

        // 2. Handle events that don't go through the reducer
        match &event {
            WindowEvent::CloseRequested => {
                elwt.exit();
                return;
            }
            WindowEvent::Resized(size) => {
                self.core
                    .handle_resize(&mut self.renderer, size.width, size.height);
                return;
            }
            _ => {}
        }

        // 3. Use WindowEventReducer for pointer/keyboard event translation
        let scale_factor = self.window.scale_factor();
        if let Some(translation) = self.event_reducer.reduce(scale_factor, &event) {
            match translation {
                WindowEventTranslation::Pointer(pointer_event) => {
                    self.handle_pointer_event(pointer_event);
                }
                WindowEventTranslation::Keyboard(keyboard_event) => {
                    self.core.handle_key_event(keyboard_event);
                }
            }
        }
    }

    /// Handle translated pointer event from ui-events-winit
    fn handle_pointer_event(&mut self, event: PointerEvent) {
        use flui_interaction::events::PointerEventData;
        use flui_interaction::events::ScrollDelta;

        // Extract position from the pointer event
        if let Some(data) = PointerEventData::from_pointer_event(&event) {
            let position = data.position;
            let device = data.device_kind;

            // Determine event type and route appropriately
            match &event {
                PointerEvent::Down(_) => {
                    self.core.handle_pointer_button(
                        position,
                        device,
                        flui_interaction::events::PointerButton::Primary,
                        true,
                    );
                }
                PointerEvent::Up(_) => {
                    self.core.handle_pointer_button(
                        position,
                        device,
                        flui_interaction::events::PointerButton::Primary,
                        false,
                    );
                }
                PointerEvent::Move(_) => {
                    self.core.handle_pointer_move(position, device);
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
                    self.core.handle_scroll_event(scroll_event);
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
    pub fn render_frame(&mut self) {
        let _scene = self.core.render_frame(&mut self.renderer);
        self.core.mark_rendered();
    }

    /// Check if redraw is needed
    pub fn needs_redraw(&self) -> bool {
        self.core.needs_redraw()
    }

    /// Get the underlying winit window
    pub fn window(&self) -> &Arc<Window> {
        &self.window
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
