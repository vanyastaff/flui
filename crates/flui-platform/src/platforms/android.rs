//! Android embedder
//!
//! Handles Android-specific lifecycle (Resumed/Suspended) and
//! manages GPU resources for battery optimization.

use crate::{
    core::EmbedderCore,
    traits::{
        AndroidEvent, MobileCapabilities, PlatformCapabilities, PlatformEmbedder,
        PlatformSpecificEvent, PlatformWindow, WinitWindow,
    },
    PlatformError, Result,
};
use flui_core::pipeline::PipelineOwner;
use flui_engine::GpuRenderer;
use flui_interaction::EventRouter;
use flui_scheduler::Scheduler;
use flui_types::{
    events::{PointerButton, PointerDeviceKind, ScrollDelta, ScrollEventData},
    Offset,
};
use parking_lot::RwLock;
use std::sync::{atomic::AtomicBool, Arc};
use winit::{
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::ActiveEventLoop,
    keyboard::ModifiersState,
    window::Window,
};

/// Android embedder
///
/// Adds Android-specific lifecycle management to the shared core.
/// Handles suspend/resume for battery optimization.
///
/// # Architecture
///
/// ```text
/// AndroidEmbedder (thin wrapper)
///   ├─ core: EmbedderCore (90% of logic)
///   ├─ window: WinitWindow (Android window)
///   ├─ renderer: GpuRenderer (Vulkan backend)
///   ├─ capabilities: MobileCapabilities
///   └─ is_suspended: bool (Android lifecycle)
/// ```
///
/// # Lifecycle
///
/// ```text
/// Created → Resumed (GPU active) ⇄ Suspended (GPU paused)
/// ```
pub struct AndroidEmbedder {
    /// Shared embedder core
    core: EmbedderCore,

    /// Platform window
    window: WinitWindow,

    /// GPU renderer (Vulkan on Android)
    renderer: GpuRenderer,

    /// Platform capabilities
    capabilities: MobileCapabilities,

    /// Android lifecycle state - true when app is in background
    is_suspended: bool,

    /// Current keyboard modifiers state
    modifiers: ModifiersState,
}

impl AndroidEmbedder {
    /// Create a new Android embedder
    pub async fn new(
        pipeline_owner: Arc<RwLock<PipelineOwner>>,
        needs_redraw: Arc<AtomicBool>,
        scheduler: Arc<Scheduler>,
        event_router: Arc<RwLock<EventRouter>>,
        event_loop: &ActiveEventLoop,
    ) -> Result<Self> {
        tracing::info!("Initializing Android embedder");

        // 1. Create Android window
        let window_attributes = Window::default_attributes()
            .with_title("FLUI App")
            .with_inner_size(winit::dpi::PhysicalSize::new(800, 600));

        let window = Arc::new(
            event_loop
                .create_window(window_attributes)
                .map_err(|e| PlatformError::WindowCreation(e.to_string()))?,
        );

        let size = window.inner_size();

        // 2. Initialize GPU renderer (Vulkan on Android)
        let renderer =
            GpuRenderer::new_async_with_window(Arc::clone(&window), size.width, size.height).await;

        tracing::info!(
            width = size.width,
            height = size.height,
            format = ?renderer.format(),
            backend = "Vulkan",
            "Android embedder initialized"
        );

        // 3. Create embedder core
        let core = EmbedderCore::new(pipeline_owner, needs_redraw, scheduler, event_router);

        Ok(Self {
            core,
            window: WinitWindow::new(window),
            renderer,
            capabilities: MobileCapabilities::android(),
            is_suspended: false,
            modifiers: ModifiersState::empty(),
        })
    }

    /// Mark embedder as suspended (app backgrounded)
    ///
    /// Stops rendering to save battery.
    pub fn suspend(&mut self) {
        if self.is_suspended {
            tracing::warn!("suspend() called but already suspended");
            return;
        }

        self.is_suspended = true;
        self.core.handle_visibility_changed(false);
        tracing::info!("Android embedder suspended (rendering stopped)");
    }

    /// Mark embedder as resumed (app foregrounded)
    pub fn resume(&mut self) {
        if !self.is_suspended {
            tracing::warn!("resume() called but not suspended");
            return;
        }

        self.is_suspended = false;
        self.core.handle_visibility_changed(true);
        tracing::info!("Android embedder resumed (rendering active)");
    }

    /// Check if embedder is suspended
    pub fn is_suspended(&self) -> bool {
        self.is_suspended
    }

    /// Handle window event
    pub fn handle_window_event(&mut self, event: WindowEvent, _elwt: &ActiveEventLoop) {
        // Update lifecycle state
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

        // Handle specific events
        match event {
            WindowEvent::Resized(size) => {
                self.core
                    .handle_resize(&mut self.renderer, size.width, size.height);
            }

            WindowEvent::CursorMoved { position, .. } => {
                // Touch events on Android
                let offset = Offset::new(position.x as f32, position.y as f32);
                self.core
                    .handle_pointer_move(offset, PointerDeviceKind::Touch);
            }

            WindowEvent::MouseInput { state, button, .. } => {
                // On Android, mouse input = touch
                let position = Offset::ZERO; // Would get from tracked state

                self.core.handle_pointer_button(
                    position,
                    PointerDeviceKind::Touch,
                    convert_mouse_button(button),
                    state == ElementState::Pressed,
                );
            }

            WindowEvent::Touch(touch) => {
                // Native touch events
                let offset = Offset::new(touch.location.x as f32, touch.location.y as f32);

                match touch.phase {
                    winit::event::TouchPhase::Started => {
                        self.core.handle_pointer_button(
                            offset,
                            PointerDeviceKind::Touch,
                            PointerButton::Primary,
                            true,
                        );
                    }
                    winit::event::TouchPhase::Moved => {
                        self.core
                            .handle_pointer_move(offset, PointerDeviceKind::Touch);
                    }
                    winit::event::TouchPhase::Ended => {
                        self.core.handle_pointer_button(
                            offset,
                            PointerDeviceKind::Touch,
                            PointerButton::Primary,
                            false,
                        );
                    }
                    winit::event::TouchPhase::Cancelled => {
                        // Handle cancel
                    }
                }
            }

            WindowEvent::ModifiersChanged(new_modifiers) => {
                self.modifiers = new_modifiers.state();
            }

            WindowEvent::KeyboardInput {
                event: key_event, ..
            } => {
                // Android hardware keyboard support (Bluetooth, USB)
                let flui_event = crate::conversions::convert_key_event(&key_event, self.modifiers);
                self.core.handle_key_event(flui_event);
            }

            WindowEvent::MouseWheel { delta, .. } => {
                // Scroll events on Android (e.g., Bluetooth mouse, trackpad)
                let position = self.core.last_pointer_position();
                let scroll_delta = convert_mouse_wheel_delta(delta);
                let modifiers = crate::conversions::convert_modifiers(self.modifiers);

                let scroll_event = ScrollEventData {
                    position,
                    delta: scroll_delta,
                    modifiers,
                };

                self.core.handle_scroll_event(scroll_event);
            }

            _ => {}
        }
    }

    /// Render a frame
    ///
    /// Skips rendering if suspended to save battery.
    pub fn render_frame(&mut self) {
        if self.is_suspended {
            tracing::trace!("Skipping render (suspended)");
            return;
        }

        let _scene = self.core.render_frame(&mut self.renderer);
        self.core.mark_rendered();
    }

    /// Check if redraw is needed
    pub fn needs_redraw(&self) -> bool {
        !self.is_suspended && self.core.needs_redraw()
    }

    /// Get the underlying winit window Arc
    pub fn winit_window(&self) -> &Arc<Window> {
        self.window.inner()
    }
}

impl PlatformEmbedder for AndroidEmbedder {
    type Window = WinitWindow;
    type Capabilities = MobileCapabilities;

    fn window(&self) -> &Self::Window {
        &self.window
    }

    fn capabilities(&self) -> &Self::Capabilities {
        &self.capabilities
    }

    fn request_redraw(&self) {
        if !self.is_suspended {
            self.window.request_redraw();
        }
    }

    fn handle_platform_event(&mut self, event: PlatformSpecificEvent) {
        match event {
            PlatformSpecificEvent::Android(android_event) => match android_event {
                AndroidEvent::Resumed => self.resume(),
                AndroidEvent::Suspended => self.suspend(),
                AndroidEvent::LowMemory => {
                    // Clear caches
                    self.core.scene_cache().clear();
                    tracing::warn!("Android low memory warning - caches cleared");
                }
                AndroidEvent::ConfigurationChanged => {
                    tracing::debug!("Android configuration changed");
                }
            },
            _ => {
                tracing::debug!("Non-Android platform event ignored");
            }
        }
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

/// Convert winit MouseScrollDelta to FLUI ScrollDelta
///
/// Used for external input devices connected to Android (Bluetooth mouse, trackpad, etc.)
fn convert_mouse_wheel_delta(delta: MouseScrollDelta) -> ScrollDelta {
    match delta {
        MouseScrollDelta::LineDelta(x, y) => ScrollDelta::Lines { x, y },
        MouseScrollDelta::PixelDelta(pos) => ScrollDelta::Pixels {
            x: pos.x as f32,
            y: pos.y as f32,
        },
    }
}
