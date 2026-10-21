//! Linux platform implementation (stub)
//!
//! This module provides a stub implementation of the Platform trait for Linux.
//! It serves as a placeholder for future native Linux integration using:
//!
//! - **Wayland**: Modern compositor protocol (preferred)
//! - **X11/Xlib**: Legacy X Window System support
//! - **Vulkan/wgpu**: GPU rendering
//! - **Tokio**: Already integrated for async executors
//! - **fontconfig/FreeType**: Font loading and text rendering
//!
//! # Current Status
//!
//! The platform itself is a stub: `LinuxPlatform::new` and most of the
//! [`Platform`] methods panic with `unimplemented!()`, and Linux windows come
//! from the winit backend (`WinitPlatform`). What exists natively is the
//! `display` source: the outputs of a Wayland compositor or an X
//! server, with their refresh rates and a notice when they change. The winit
//! backend reads its display snapshot from it, and [`LinuxPlatform::is_wayland`]
//! and [`LinuxPlatform::is_x11`] answer from the same protocol choice.
//!
//! # Implementation Roadmap
//!
//! ## Wayland Backend (Primary)
//!
//! 1. **Window Management**:
//!    - wayland-client for protocol handling
//!    - xdg-shell for window decorations
//!    - Multi-monitor support via wl_output
//!    - Fullscreen via xdg-toplevel
//!
//! 2. **Event Loop**:
//!    - wayland event queue integration
//!    - Input via wl_seat, wl_keyboard, wl_pointer
//!    - Event dispatch on main thread
//!
//! 3. **Rendering**:
//!    - wgpu surface from wayland surface
//!    - DMA-BUF for zero-copy buffers
//!    - HiDPI support via wl_output scale
//!
//! ## X11 Backend (Fallback)
//!
//! 1. **Window Management**:
//!    - Xlib or xcb for window creation
//!    - EWMH for window manager hints
//!    - Xrandr for multi-monitor
//!    - _NET_WM_STATE_FULLSCREEN for fullscreen
//!
//! 2. **Event Loop**:
//!    - XNextEvent for event polling
//!    - XInput2 for modern input devices
//!    - Event dispatch on main thread
//!
//! 3. **Rendering**:
//!    - wgpu surface from X11 window
//!    - DRI3/Present for efficient presentation
//!
//! ## Shared Services
//!
//! 4. **Platform Services**:
//!    - fontconfig for font discovery
//!    - FreeType for font rasterization
//!    - Clipboard via wayland-data-device or X11 CLIPBOARD
//!    - D-Bus for desktop integration
//!
//! # Usage
//!
//! Currently, attempting to use this platform will panic. For Linux
//! development, use the winit-based backend or wait for native implementation.
//!
//! ```rust,ignore
//! #[cfg(target_os = "linux")]
//! use flui_native::LinuxPlatform;
//!
//! // This will panic with "not implemented"
//! let platform = LinuxPlatform::new();
//! ```

#[cfg(all(target_os = "linux", feature = "a11y"))]
mod accessibility;
pub(crate) mod display;
#[cfg(feature = "winit-backend")]
pub(crate) mod host_notices;
#[cfg(feature = "winit-backend")]
pub(crate) mod lifecycle_model;
mod window_ext;
#[cfg(feature = "winit-backend")]
pub(crate) mod x11_window_probe;

use std::sync::Arc;

#[cfg(all(target_os = "linux", feature = "a11y"))]
pub use accessibility::UnixAccessibility;
pub use window_ext::*;

use self::display::protocol::{Environment, Protocol};
use crate::data_transfer::{DataTransferSource, NullDataTransferSource};
use crate::error::PlatformError;
use crate::traits::{
    Clipboard, DisplaySnapshot, HostWindow, OpenWindowError, Platform, PlatformExecutor,
    PlatformReadyCallback, WindowEvent, WindowId, WindowOptions,
};

/// Linux platform implementation (stub)
///
/// This is a placeholder for future native Linux support (Wayland + X11).
/// Most methods currently panic with `unimplemented!()`; `display_snapshot`
/// returns an error, `data_transfer` an inert source and `name` its label.
///
/// # Future Implementation
///
/// Will support both backends:
/// - **Wayland** (primary): wayland-client, xdg-shell, wl_seat
/// - **X11** (fallback): Xlib/xcb, XInput2, Xrandr
///
/// Plus shared services:
/// - **fontconfig** for font discovery
/// - **FreeType** for text rendering
/// - **Vulkan/wgpu** for GPU rendering
#[derive(Debug)]
pub struct LinuxPlatform;

impl LinuxPlatform {
    /// Create a new Linux platform instance (stub)
    ///
    /// # Panics
    ///
    /// Always panics with "Linux platform not yet implemented"
    #[expect(
        clippy::unimplemented,
        reason = "documented Linux platform stub (AGENTS.md platform-init exception); tracked in the module's own Implementation Roadmap, not a silent gap"
    )]
    pub fn new() -> Result<Self, PlatformError> {
        unimplemented!(
            "Linux platform not yet implemented - use winit backend or wait for native Wayland/X11 implementation"
        )
    }

    /// Whether this process will speak Wayland: `WAYLAND_DISPLAY` or
    /// `WAYLAND_SOCKET` is set and not empty. Wayland wins over X11 when both
    /// are present (WSLg offers both), as it does in winit.
    #[must_use]
    pub fn is_wayland() -> bool {
        Protocol::from_environment(&Environment::current()) == Some(Protocol::Wayland)
    }

    /// Whether this process will speak X11: only `DISPLAY` is set and not
    /// empty. A session with a Wayland socket speaks Wayland, so this is
    /// `false` there even though an X server may be running.
    #[must_use]
    pub fn is_x11() -> bool {
        Protocol::from_environment(&Environment::current()) == Some(Protocol::X11)
    }
}

impl Platform for LinuxPlatform {
    fn window_capabilities(&self) -> flui_platform_api::WindowCapabilities {
        flui_platform_api::WindowCapabilities::NONE
    }

    #[expect(
        clippy::unimplemented,
        reason = "documented Linux platform stub (AGENTS.md platform-init exception); tracked in the module's own Implementation Roadmap, not a silent gap"
    )]
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        unimplemented!("Linux Tokio executor integration not implemented")
    }

    #[expect(
        clippy::unimplemented,
        reason = "documented Linux platform stub (AGENTS.md platform-init exception); tracked in the module's own Implementation Roadmap, not a silent gap"
    )]
    fn run(
        self: Box<Self>,
        _on_finish_launching: PlatformReadyCallback,
    ) -> Result<(), PlatformError> {
        unimplemented!("Linux event loop (Wayland/X11) not implemented")
    }

    #[expect(
        clippy::unimplemented,
        reason = "documented Linux platform stub (AGENTS.md platform-init exception); tracked in the module's own Implementation Roadmap, not a silent gap"
    )]
    fn quit(&self) {
        unimplemented!("Linux quit not implemented")
    }

    #[expect(
        clippy::unimplemented,
        reason = "documented Linux platform stub (AGENTS.md platform-init exception); tracked in the module's own Implementation Roadmap, not a silent gap"
    )]
    fn active_window(&self) -> Option<WindowId> {
        unimplemented!("Linux active window query not implemented")
    }

    fn display_snapshot(&self) -> Result<DisplaySnapshot, PlatformError> {
        // This stub owns no display source: the one that exists runs under
        // the winit backend. An error, not an empty snapshot, which would
        // claim the machine has no display.
        Err(PlatformError::Displays {
            message: "the native Linux platform is not implemented; \
                      displays are read by the winit backend"
                .into(),
        })
    }

    #[expect(
        clippy::unimplemented,
        reason = "documented Linux platform stub (AGENTS.md platform-init exception); tracked in the module's own Implementation Roadmap, not a silent gap"
    )]
    fn open_window(&self, _options: WindowOptions) -> Result<Arc<dyn HostWindow>, OpenWindowError> {
        unimplemented!("Linux window creation (Wayland/X11) not implemented")
    }

    #[expect(
        clippy::unimplemented,
        reason = "documented Linux platform stub (AGENTS.md platform-init exception); tracked in the module's own Implementation Roadmap, not a silent gap"
    )]
    fn clipboard(&self) -> Arc<dyn Clipboard> {
        unimplemented!("Linux clipboard (wayland-data-device/X11 CLIPBOARD) not implemented")
    }

    fn data_transfer(&self) -> Arc<dyn DataTransferSource> {
        // No native Linux transport yet (ADR-0038): inert and honest.
        Arc::new(NullDataTransferSource)
    }

    fn name(&self) -> &'static str {
        "Linux (stub)"
    }

    #[expect(
        clippy::unimplemented,
        reason = "documented Linux platform stub (AGENTS.md platform-init exception); tracked in the module's own Implementation Roadmap, not a silent gap"
    )]
    fn on_quit(&self, _callback: Box<dyn FnMut() + Send>) {
        unimplemented!("Linux quit callback not implemented")
    }

    #[expect(
        clippy::unimplemented,
        reason = "documented Linux platform stub (AGENTS.md platform-init exception); tracked in the module's own Implementation Roadmap, not a silent gap"
    )]
    fn on_window_event(&self, _callback: Box<dyn FnMut(WindowEvent) + Send>) {
        unimplemented!("Linux window event callback not implemented")
    }

    #[expect(
        clippy::unimplemented,
        reason = "documented Linux platform stub (AGENTS.md platform-init exception); tracked in the module's own Implementation Roadmap, not a silent gap"
    )]
    fn app_path(&self) -> Result<std::path::PathBuf, PlatformError> {
        unimplemented!("Linux app path query (/proc/self/exe) not implemented")
    }
}

// TODO: Implement these when adding native Linux support:
//
// Wayland backend:
// - WaylandWindow wrapping wl_surface + xdg_toplevel
// - WaylandDisplay wrapping wl_output
// - Wayland event queue integration
//
// X11 backend:
// - X11Window wrapping X11 Window
// - X11Display wrapping RandR output
// - XNextEvent integration
//
// Shared:
// - FontconfigTextSystem using fontconfig + FreeType
// - TokioExecutor already available
// - VulkanSurface for wgpu
// - D-Bus integration for desktop services
