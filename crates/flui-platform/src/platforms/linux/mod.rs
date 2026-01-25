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
//! ⚠️ **NOT IMPLEMENTED** - This is a stub that returns `unimplemented!()` for all operations.
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
//! Currently, attempting to use this platform will panic. For Linux development,
//! use the winit-based backend or wait for native implementation.
//!
//! ```rust,ignore
//! #[cfg(target_os = "linux")]
//! use flui_platform::LinuxPlatform;
//!
//! // This will panic with "not implemented"
//! let platform = LinuxPlatform::new();
//! ```

use crate::traits::*;
use anyhow::Result;
use std::sync::Arc;

/// Linux platform implementation (stub)
///
/// This is a placeholder for future native Linux support (Wayland + X11).
/// All methods currently return `unimplemented!()`.
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
pub struct LinuxPlatform;

impl LinuxPlatform {
    /// Create a new Linux platform instance (stub)
    ///
    /// # Panics
    ///
    /// Always panics with "Linux platform not yet implemented"
    pub fn new() -> Result<Self> {
        unimplemented!("Linux platform not yet implemented - use winit backend or wait for native Wayland/X11 implementation")
    }

    /// Check if running on Wayland
    ///
    /// # Panics
    ///
    /// Always panics (stub implementation)
    pub fn is_wayland() -> bool {
        unimplemented!("Wayland detection not implemented")
    }

    /// Check if running on X11
    ///
    /// # Panics
    ///
    /// Always panics (stub implementation)
    pub fn is_x11() -> bool {
        unimplemented!("X11 detection not implemented")
    }
}

impl Platform for LinuxPlatform {
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        unimplemented!("Linux Tokio executor integration not implemented")
    }

    fn foreground_executor(&self) -> Arc<dyn PlatformExecutor> {
        unimplemented!("Linux main thread executor not implemented")
    }

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        unimplemented!("Linux fontconfig/FreeType system not implemented")
    }

    fn run(&self, _on_finish_launching: Box<dyn FnOnce()>) {
        unimplemented!("Linux event loop (Wayland/X11) not implemented")
    }

    fn quit(&self) {
        unimplemented!("Linux quit not implemented")
    }

    fn request_frame(&self) {
        unimplemented!("Linux frame request not implemented")
    }

    fn active_window(&self) -> Option<WindowId> {
        unimplemented!("Linux active window query not implemented")
    }

    fn displays(&self) -> Vec<Arc<dyn PlatformDisplay>> {
        unimplemented!("Linux display enumeration (wl_output/Xrandr) not implemented")
    }

    fn primary_display(&self) -> Option<Arc<dyn PlatformDisplay>> {
        unimplemented!("Linux primary display query not implemented")
    }

    fn open_window(&self, _options: WindowOptions) -> Result<Box<dyn PlatformWindow>> {
        unimplemented!("Linux window creation (Wayland/X11) not implemented")
    }

    fn clipboard(&self) -> Arc<dyn Clipboard> {
        unimplemented!("Linux clipboard (wayland-data-device/X11 CLIPBOARD) not implemented")
    }

    fn capabilities(&self) -> &dyn PlatformCapabilities {
        unimplemented!("Linux capabilities not implemented")
    }

    fn name(&self) -> &'static str {
        "Linux (stub)"
    }

    fn on_quit(&self, _callback: Box<dyn FnMut() + Send>) {
        unimplemented!("Linux quit callback not implemented")
    }

    fn on_window_event(&self, _callback: Box<dyn FnMut(WindowEvent) + Send>) {
        unimplemented!("Linux window event callback not implemented")
    }

    fn app_path(&self) -> Result<std::path::PathBuf> {
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
