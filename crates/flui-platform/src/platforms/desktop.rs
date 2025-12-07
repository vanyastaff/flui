//! Desktop embedder - Windows, macOS, Linux
//!
//! This embedder is now a thin wrapper around `EmbedderCore`,
//! containing only desktop-specific logic.

use crate::{
    core::EmbedderCore,
    traits::{DesktopCapabilities, PlatformEmbedder, PlatformWindow, WinitWindow},
    PlatformError, Result,
};
use flui_core::pipeline::PipelineOwner;
use flui_engine::GpuRenderer;
use flui_interaction::EventRouter;
use flui_scheduler::Scheduler;
use flui_types::{
    events::{MouseCursor, PointerButton, PointerDeviceKind, SystemMouseCursor},
    Offset,
};
use parking_lot::RwLock;
use std::sync::{atomic::AtomicBool, Arc};
use winit::{
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::ActiveEventLoop,
    keyboard::ModifiersState,
    window::{CursorIcon, Window},
};

/// Desktop embedder for Windows, macOS, Linux
///
/// This is now a thin wrapper around `EmbedderCore` (~120 lines vs 318 before).
/// All shared logic is delegated to the core.
///
/// # Architecture
///
/// ```text
/// DesktopEmbedder (thin wrapper)
///   ├─ core: EmbedderCore (90% of logic)
///   ├─ window: WinitWindow (platform window)
///   ├─ renderer: GpuRenderer (GPU rendering)
///   └─ capabilities: DesktopCapabilities
/// ```
///
/// # Example
///
/// ```rust,ignore
/// // In ApplicationHandler::resumed()
/// let embedder = DesktopEmbedder::new(
///     pipeline, needs_redraw, scheduler, event_router, event_loop
/// ).await?;
///
/// // In ApplicationHandler::window_event()
/// embedder.handle_window_event(event, elwt);
///
/// // In RedrawRequested
/// embedder.render_frame();
/// ```
pub struct DesktopEmbedder {
    /// Shared embedder core (90% of logic)
    core: EmbedderCore,

    /// Platform window
    window: WinitWindow,

    /// GPU renderer
    renderer: GpuRenderer,

    /// Platform capabilities
    capabilities: DesktopCapabilities,

    /// Current keyboard modifiers state
    modifiers: ModifiersState,
}

impl DesktopEmbedder {
    /// Create a new desktop embedder
    ///
    /// # Arguments
    ///
    /// * `pipeline_owner` - Shared pipeline from AppBinding
    /// * `needs_redraw` - Shared redraw flag from AppBinding
    /// * `scheduler` - Scheduler from SchedulerBinding
    /// * `event_router` - Event router from GestureBinding
    /// * `event_loop` - Active event loop for window creation
    pub async fn new(
        pipeline_owner: Arc<RwLock<PipelineOwner>>,
        needs_redraw: Arc<AtomicBool>,
        scheduler: Arc<Scheduler>,
        event_router: Arc<RwLock<EventRouter>>,
        event_loop: &ActiveEventLoop,
    ) -> Result<Self> {
        // 1. Create window using ActiveEventLoop (winit 0.30+ API)
        let window_attributes = Window::default_attributes()
            .with_title("FLUI App")
            .with_inner_size(winit::dpi::PhysicalSize::new(800, 600));

        let window = Arc::new(
            event_loop
                .create_window(window_attributes)
                .map_err(|e| PlatformError::WindowCreation(e.to_string()))?,
        );

        let size = window.inner_size();

        // 2. Initialize GPU renderer
        let renderer =
            GpuRenderer::new_async_with_window(Arc::clone(&window), size.width, size.height).await;

        tracing::info!(
            width = size.width,
            height = size.height,
            format = ?renderer.format(),
            "Desktop embedder initialized"
        );

        // 3. Create embedder core (shared logic)
        let core = EmbedderCore::new(pipeline_owner, needs_redraw, scheduler, event_router);

        Ok(Self {
            core,
            window: WinitWindow::new(window),
            renderer,
            capabilities: DesktopCapabilities,
            modifiers: ModifiersState::empty(),
        })
    }

    /// Handle window event
    pub fn handle_window_event(&mut self, event: WindowEvent, elwt: &ActiveEventLoop) {
        // 1. Update lifecycle state
        match &event {
            WindowEvent::Focused(focused) => {
                self.core.handle_focus_changed(*focused);
                self.window.set_focused(*focused);
            }
            WindowEvent::Occluded(occluded) => {
                self.core.handle_visibility_changed(!occluded);
                self.window.set_visible(!occluded);
            }
            _ => {}
        }

        // 2. Handle specific events
        match event {
            WindowEvent::CloseRequested => {
                tracing::info!("Window close requested");
                elwt.exit();
            }

            WindowEvent::Resized(size) => {
                self.core
                    .handle_resize(&mut self.renderer, size.width, size.height);
            }

            WindowEvent::CursorMoved { position, .. } => {
                let offset = Offset::new(position.x as f32, position.y as f32);
                self.core
                    .handle_pointer_move(offset, PointerDeviceKind::Mouse);
            }

            WindowEvent::MouseInput { state, button, .. } => {
                // Use tracked position from pointer state in core
                let position = self.core.last_pointer_position();

                self.core.handle_pointer_button(
                    position,
                    PointerDeviceKind::Mouse,
                    convert_mouse_button(button),
                    state == ElementState::Pressed,
                );
            }

            WindowEvent::ModifiersChanged(new_modifiers) => {
                self.modifiers = new_modifiers.state();
            }

            WindowEvent::KeyboardInput {
                event: key_event, ..
            } => {
                // Convert winit keyboard event to FLUI event
                let flui_event = crate::conversions::convert_key_event(&key_event, self.modifiers);
                self.core.handle_key_event(flui_event);
            }

            _ => {
                // Other events not handled yet
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

    /// Get the underlying winit window Arc
    pub fn winit_window(&self) -> &Arc<Window> {
        self.window.inner()
    }

    /// Set the mouse cursor
    ///
    /// Converts FLUI's MouseCursor to winit's CursorIcon and applies it.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// embedder.set_cursor(MouseCursor::CLICK);
    /// ```
    pub fn set_cursor(&self, cursor: MouseCursor) {
        let icon = convert_mouse_cursor(cursor);
        self.window.inner().set_cursor(icon);
    }

    /// Set cursor visibility
    ///
    /// Use this with `SystemMouseCursor::None` to hide the cursor.
    pub fn set_cursor_visible(&self, visible: bool) {
        self.window.inner().set_cursor_visible(visible);
    }
}

impl PlatformEmbedder for DesktopEmbedder {
    type Window = WinitWindow;
    type Capabilities = DesktopCapabilities;

    fn window(&self) -> &Self::Window {
        &self.window
    }

    fn capabilities(&self) -> &Self::Capabilities {
        &self.capabilities
    }

    fn request_redraw(&self) {
        self.window.request_redraw();
    }
}

/// Convert winit mouse button to FLUI pointer button
fn convert_mouse_button(button: MouseButton) -> PointerButton {
    match button {
        MouseButton::Left => PointerButton::Primary,
        MouseButton::Right => PointerButton::Secondary,
        MouseButton::Middle => PointerButton::Middle,
        MouseButton::Back => PointerButton::Other(3),
        MouseButton::Forward => PointerButton::Other(4),
        MouseButton::Other(n) => PointerButton::Other(n as u8),
    }
}

/// Convert FLUI MouseCursor to winit CursorIcon
///
/// Maps FLUI's Flutter-compatible cursor types to winit's cursor icons.
/// For `MouseCursor::Defer` and `MouseCursor::Uncontrolled`, returns `CursorIcon::Default`.
pub fn convert_mouse_cursor(cursor: MouseCursor) -> CursorIcon {
    match cursor {
        MouseCursor::System(system) => convert_system_cursor(system),
        MouseCursor::Defer | MouseCursor::Uncontrolled => CursorIcon::Default,
    }
}

/// Convert FLUI SystemMouseCursor to winit CursorIcon
fn convert_system_cursor(cursor: SystemMouseCursor) -> CursorIcon {
    match cursor {
        SystemMouseCursor::None => CursorIcon::Default, // winit doesn't have hidden cursor via icon
        SystemMouseCursor::Basic => CursorIcon::Default,
        SystemMouseCursor::Click => CursorIcon::Pointer,
        SystemMouseCursor::Wait => CursorIcon::Wait,
        SystemMouseCursor::Progress => CursorIcon::Progress,
        SystemMouseCursor::Forbidden => CursorIcon::NotAllowed,
        SystemMouseCursor::Text => CursorIcon::Text,
        SystemMouseCursor::VerticalText => CursorIcon::VerticalText,
        SystemMouseCursor::Precise => CursorIcon::Crosshair,
        SystemMouseCursor::ContextMenu => CursorIcon::ContextMenu,
        SystemMouseCursor::Grab => CursorIcon::Grab,
        SystemMouseCursor::Grabbing => CursorIcon::Grabbing,
        SystemMouseCursor::Move => CursorIcon::Move,
        SystemMouseCursor::AllScroll => CursorIcon::AllScroll,
        SystemMouseCursor::Copy => CursorIcon::Copy,
        SystemMouseCursor::Alias => CursorIcon::Alias,
        SystemMouseCursor::NoDrop => CursorIcon::NoDrop,
        SystemMouseCursor::Cell => CursorIcon::Cell,
        SystemMouseCursor::ResizeUp => CursorIcon::NResize,
        SystemMouseCursor::ResizeDown => CursorIcon::SResize,
        SystemMouseCursor::ResizeLeft => CursorIcon::WResize,
        SystemMouseCursor::ResizeRight => CursorIcon::EResize,
        SystemMouseCursor::ResizeUpLeft => CursorIcon::NwResize,
        SystemMouseCursor::ResizeUpRight => CursorIcon::NeResize,
        SystemMouseCursor::ResizeDownLeft => CursorIcon::SwResize,
        SystemMouseCursor::ResizeDownRight => CursorIcon::SeResize,
        SystemMouseCursor::ResizeUpDown => CursorIcon::NsResize,
        SystemMouseCursor::ResizeLeftRight => CursorIcon::EwResize,
        SystemMouseCursor::ResizeUpLeftDownRight => CursorIcon::NwseResize,
        SystemMouseCursor::ResizeUpRightDownLeft => CursorIcon::NeswResize,
        SystemMouseCursor::ResizeColumn => CursorIcon::ColResize,
        SystemMouseCursor::ResizeRow => CursorIcon::RowResize,
        SystemMouseCursor::ZoomIn => CursorIcon::ZoomIn,
        SystemMouseCursor::ZoomOut => CursorIcon::ZoomOut,
        SystemMouseCursor::Help => CursorIcon::Help,
        // Fallback for any new cursor types
        _ => CursorIcon::Default,
    }
}
