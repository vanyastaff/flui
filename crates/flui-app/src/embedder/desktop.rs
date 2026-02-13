//! Desktop embedder - Windows, macOS, Linux
//!
//! Platform embedder for desktop environments using flui-platform + wgpu.
//! Platform delivers `PlatformInput` events directly via per-window callbacks.

use crate::app::AppBinding;
use flui_engine::wgpu::Renderer;
use flui_foundation::HasInstance;
use flui_interaction::events::{PointerEvent, ScrollEventData};
use flui_platform::traits::{
    DispatchEventResult, KeyboardEvent as PlatformKeyboardEvent, PlatformInput,
};
use flui_platform::PlatformWindow;
use flui_scheduler::Scheduler;
use flui_types::geometry::{delta_px, PixelDelta, Pixels, Size};
use flui_types::Offset;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};
use std::time::Instant;

// ============================================================================
// PlatformWindowHandle adapter
// ============================================================================

/// Adapter bridging `dyn PlatformWindow` to `HasWindowHandle + HasDisplayHandle`.
///
/// `Renderer::new()` requires `W: HasWindowHandle + HasDisplayHandle`.
/// `PlatformWindow` has the methods but traits can't be implemented on `dyn`.
/// This zero-cost wrapper bridges the gap.
struct PlatformWindowHandle<'a>(&'a dyn PlatformWindow);

impl HasWindowHandle for PlatformWindowHandle<'_> {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        self.0.window_handle()
    }
}

impl HasDisplayHandle for PlatformWindowHandle<'_> {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        self.0.display_handle()
    }
}

// ============================================================================
// DesktopEmbedder
// ============================================================================

/// Desktop embedder for Windows, macOS, Linux
///
/// Thin wrapper that connects platform (flui-platform/wgpu) to AppBinding.
/// All framework logic is in AppBinding, this only handles:
/// - Window reference (for size queries and redraw requests)
/// - GPU rendering
///
/// # Architecture
///
/// ```text
/// DesktopEmbedder (thin platform wrapper)
///   ├─ window: Box<dyn PlatformWindow>
///   └─ renderer: Renderer (GPU rendering)
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
    window: Box<dyn PlatformWindow>,

    /// GPU renderer
    renderer: Renderer,
}

impl DesktopEmbedder {
    /// Create a new desktop embedder from a platform window.
    ///
    /// Creates a wgpu `Renderer` using the window's raw-window-handle.
    pub async fn new(window: Box<dyn PlatformWindow>) -> Result<Self, EmbedderError> {
        let _init_span = tracing::info_span!("init_embedder").entered();

        // Initialize GPU renderer via PlatformWindowHandle adapter
        let phys_size = window.physical_size();
        let mut renderer = {
            let _span = tracing::info_span!("init_gpu").entered();
            let handle = PlatformWindowHandle(window.as_ref());
            Renderer::new(&handle)
                .await
                .map_err(|e| EmbedderError::GpuInitialization(format!("{:?}", e)))?
        };
        renderer.resize(phys_size.width.0 as u32, phys_size.height.0 as u32);

        tracing::info!(
            width = phys_size.width.0,
            height = phys_size.height.0,
            "Embedder created with platform window"
        );

        Ok(Self { window, renderer })
    }

    /// Render a frame.
    ///
    /// Delegates to AppBinding for all framework logic, only handles GPU rendering.
    /// Also invokes scheduler callbacks for animations.
    pub fn render_frame(&mut self) {
        let now = Instant::now();

        // 1. Handle scheduler frame callbacks (animations, etc.)
        let scheduler = Scheduler::instance();
        let frame_id = scheduler.handle_begin_frame(now);
        tracing::trace!(
            "render_frame: handle_begin_frame completed, frame_id={:?}",
            frame_id
        );
        scheduler.handle_draw_frame();
        tracing::trace!("render_frame: handle_draw_frame completed");

        // 2. Also tick the Arc scheduler singleton (used by AnimationController)
        let arc_scheduler = Scheduler::arc_instance();
        let arc_frame_id = arc_scheduler.handle_begin_frame(now);
        tracing::trace!("render_frame: arc_scheduler frame_id={:?}", arc_frame_id);
        arc_scheduler.handle_draw_frame();

        // 3. Render the frame via AppBinding
        let binding = AppBinding::instance();
        let _scene = binding.render_frame(&mut self.renderer);
    }

    /// Resize the renderer surface.
    pub fn resize(&mut self, size: Size<Pixels>, scale_factor: f32) {
        let w = (size.width.0 * scale_factor) as u32;
        let h = (size.height.0 * scale_factor) as u32;
        self.renderer.resize(w, h);
        AppBinding::instance().request_redraw();
    }

    /// Check if redraw is needed.
    pub fn needs_redraw(&self) -> bool {
        if AppBinding::instance().needs_redraw() {
            return true;
        }
        let arc_scheduler = Scheduler::arc_instance();
        arc_scheduler.is_frame_scheduled()
    }

    /// Get window physical size as `(width, height)`.
    pub fn size(&self) -> (u32, u32) {
        let size = self.window.physical_size();
        (size.width.0 as u32, size.height.0 as u32)
    }

    /// Request a redraw from the platform window.
    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }
}

// ============================================================================
// Input event handling
// ============================================================================

/// Handle a `PlatformInput` event by routing it to AppBinding.
///
/// Called from the `on_input` callback registered on the platform window.
pub fn handle_platform_input(input: PlatformInput) -> DispatchEventResult {
    match input {
        PlatformInput::Pointer(pointer_event) => {
            handle_pointer_event(pointer_event);
            DispatchEventResult::default()
        }
        PlatformInput::Keyboard(keyboard_event) => {
            let ui_event = bridge_keyboard_event(&keyboard_event);
            AppBinding::instance().handle_key_event(ui_event);
            DispatchEventResult {
                propagate: false,
                default_prevented: true,
            }
        }
    }
}

/// Handle a pointer event from the platform.
fn handle_pointer_event(event: PointerEvent) {
    use flui_interaction::events::PointerEventData;
    use flui_interaction::events::ScrollDelta;

    let binding = AppBinding::instance();

    if let Some(data) = PointerEventData::from_pointer_event(&event) {
        let position = data.position;
        let device = data.device_kind;

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
                let delta: Offset<PixelDelta> = match scroll.delta {
                    ScrollDelta::LineDelta(x, y) | ScrollDelta::PageDelta(x, y) => {
                        Offset::new(delta_px(x), delta_px(y))
                    }
                    ScrollDelta::PixelDelta(pos) => {
                        Offset::new(delta_px(pos.x as f32), delta_px(pos.y as f32))
                    }
                };
                let scroll_event = ScrollEventData {
                    position,
                    delta,
                    modifiers: scroll.state.modifiers,
                };
                binding.handle_scroll_event(scroll_event);
            }
            PointerEvent::Gesture(_) => {
                // TODO: Implement gesture event handling
            }
            _ => {}
        }
    }
}

/// Bridge `flui_platform::KeyboardEvent` to `flui_interaction::KeyboardEvent` (ui-events).
///
/// The platform layer uses a simple custom `KeyboardEvent` struct,
/// while the interaction layer uses `ui_events::keyboard::KeyboardEvent`.
fn bridge_keyboard_event(
    platform_event: &PlatformKeyboardEvent,
) -> flui_interaction::events::KeyboardEvent {
    use flui_interaction::events::keyboard::{Code, KeyState, Location};

    flui_interaction::events::KeyboardEvent {
        state: if platform_event.is_down {
            KeyState::Down
        } else {
            KeyState::Up
        },
        key: platform_event.key.clone(),
        code: Code::Unidentified,
        location: Location::Standard,
        modifiers: platform_event.modifiers,
        repeat: platform_event.is_repeat,
        is_composing: false,
    }
}

// ============================================================================
// Errors
// ============================================================================

/// Embedder error types.
#[derive(Debug, thiserror::Error)]
pub enum EmbedderError {
    /// Failed to create window
    #[error("Failed to create window: {0}")]
    WindowCreation(String),

    /// Failed to initialize GPU
    #[error("Failed to initialize GPU: {0}")]
    GpuInitialization(String),
}
