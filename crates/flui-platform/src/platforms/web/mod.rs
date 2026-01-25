//! Web/WASM platform implementation (stub)
//!
//! This module provides a stub implementation of the Platform trait for Web browsers
//! via WebAssembly. It serves as a placeholder for future web integration using:
//!
//! - **wasm-bindgen**: Rust ↔ JavaScript interop
//! - **web-sys**: Web APIs bindings (DOM, Canvas, WebGL, etc.)
//! - **WebGPU**: Modern GPU API via wgpu
//! - **WebGL 2**: Fallback GPU rendering
//! - **Canvas API**: 2D rendering fallback
//!
//! # Current Status
//!
//! ⚠️ **NOT IMPLEMENTED** - This is a stub that returns `unimplemented!()` for all operations.
//!
//! # Implementation Roadmap
//!
//! ## Core Integration
//!
//! 1. **Browser Environment**:
//!    - wasm-bindgen for Rust/JS bridge
//!    - web-sys for DOM and Web API access
//!    - js-sys for JavaScript primitives
//!    - wasm-bindgen-futures for async/await
//!    - Console API for logging
//!
//! 2. **Window Management**:
//!    - Canvas element as rendering surface
//!    - Fullscreen API (Element.requestFullscreen)
//!    - Resize observer (ResizeObserver)
//!    - Viewport meta tag handling
//!    - Pointer lock API for games
//!
//! 3. **Input System**:
//!    - Mouse events (MouseEvent)
//!    - Touch events (TouchEvent) for mobile browsers
//!    - Keyboard events (KeyboardEvent)
//!    - Pointer events (PointerEvent) - unified input
//!    - Gamepad API for controllers
//!    - Wheel events for scrolling
//!    - Context menu prevention
//!
//! 4. **Rendering**:
//!    - WebGPU via wgpu (Chrome/Edge 113+, Firefox 121+)
//!    - WebGL 2 fallback (97% browser support)
//!    - OffscreenCanvas for worker rendering
//!    - High DPI/Retina support (devicePixelRatio)
//!    - Color space support (Display P3)
//!
//! ## Platform Services
//!
//! 5. **Display & Graphics**:
//!    - Screen API for display information
//!    - matchMedia for responsive design
//!    - Fullscreen API
//!    - Page Visibility API (document.hidden)
//!    - prefers-color-scheme (dark mode)
//!
//! 6. **Text System**:
//!    - Canvas measureText for text metrics
//!    - CSS fonts (@font-face)
//!    - Font Loading API
//!    - Emoji rendering via system fonts
//!    - Text input via hidden textarea trick
//!
//! 7. **System Integration**:
//!    - Clipboard API (navigator.clipboard)
//!    - Notifications API
//!    - File System Access API (Chrome)
//!    - Storage API (localStorage, IndexedDB)
//!    - Web Share API
//!    - Service Workers for offline
//!
//! 8. **Async & Threading**:
//!    - requestAnimationFrame for render loop
//!    - Web Workers for background tasks
//!    - wasm-bindgen-futures for Rust async
//!    - Promise integration
//!
//! ## Web-Specific Features
//!
//! 9. **Progressive Web App (PWA)**:
//!    - Service worker for offline support
//!    - Web app manifest
//!    - Install prompt
//!    - Push notifications
//!    - Background sync
//!
//! 10. **Browser APIs**:
//!     - WebRTC for peer-to-peer
//!     - Web Audio API for sound
//!     - WebSockets for networking
//!     - Fetch API for HTTP requests
//!     - WebAssembly.instantiateStreaming for fast loading
//!     - WebCodecs for video/audio
//!
//! 11. **Performance**:
//!     - Performance API for metrics
//!     - PerformanceObserver
//!     - Long Tasks API
//!     - Memory usage tracking
//!     - Bundle size optimization
//!     - Code splitting with dynamic imports
//!
//! # Browser Support
//!
//! **Minimum Requirements:**
//! - WebAssembly support (all modern browsers)
//! - WebGL 2.0 or WebGPU
//! - ES6 JavaScript (for wasm-bindgen output)
//!
//! **Target Browsers:**
//! - Chrome/Edge 90+ (WebGPU 113+)
//! - Firefox 78+ (WebGPU 121+)
//! - Safari 14+ (WebGPU 18+)
//! - Mobile browsers with similar versions
//!
//! # Usage
//!
//! Currently, attempting to use this platform will panic. For web development,
//! use the winit-based backend or wait for native implementation.
//!
//! ```rust,ignore
//! #[cfg(target_arch = "wasm32")]
//! use flui_platform::WebPlatform;
//!
//! // This will panic with "not implemented"
//! let platform = WebPlatform::new();
//! ```
//!
//! # Dependencies (Future)
//!
//! When implemented, will require:
//! - `wasm-bindgen = "0.2"` - Rust/JS interop
//! - `web-sys` - Web APIs (full features)
//! - `js-sys = "0.3"` - JavaScript types
//! - `wasm-bindgen-futures = "0.4"` - Async support
//! - `console_error_panic_hook` - Better panic messages
//! - `wee_alloc` - Small allocator for WASM

use crate::traits::*;
use anyhow::Result;
use std::sync::Arc;

/// Web/WASM platform implementation (stub)
///
/// This is a placeholder for future WebAssembly support. All methods
/// currently return `unimplemented!()`.
///
/// # Future Implementation
///
/// Will use Web APIs via wasm-bindgen:
/// - `Canvas` element for rendering
/// - `WebGPU` or `WebGL 2` for GPU access
/// - `requestAnimationFrame` for render loop
/// - `PointerEvent` for unified input
/// - Web APIs for clipboard, storage, etc.
///
/// # Target Environment
///
/// - Modern browsers with WASM + WebGPU/WebGL2
/// - Progressive Web App (PWA) capable
/// - Offline support via Service Workers
pub struct WebPlatform;

impl WebPlatform {
    /// Create a new Web platform instance (stub)
    ///
    /// # Panics
    ///
    /// Always panics with "Web platform not yet implemented"
    pub fn new() -> Result<Self> {
        unimplemented!("Web/WASM platform not yet implemented - use winit backend or wait for native web-sys implementation")
    }

    /// Initialize from canvas element ID
    ///
    /// # Panics
    ///
    /// Always panics (stub implementation)
    pub fn from_canvas(_canvas_id: &str) -> Result<Self> {
        unimplemented!("Web canvas initialization not implemented")
    }

    /// Set panic hook for better error messages
    ///
    /// # Panics
    ///
    /// Always panics (stub implementation)
    pub fn set_panic_hook() {
        unimplemented!("Web panic hook not implemented")
    }
}

impl Platform for WebPlatform {
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        unimplemented!("Web Worker executor not implemented")
    }

    fn foreground_executor(&self) -> Arc<dyn PlatformExecutor> {
        unimplemented!("Web requestAnimationFrame executor not implemented")
    }

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        unimplemented!("Web Canvas text measurement not implemented")
    }

    fn run(&self, _on_finish_launching: Box<dyn FnOnce()>) {
        unimplemented!("Web requestAnimationFrame loop not implemented")
    }

    fn quit(&self) {
        unimplemented!("Web quit (close tab) not implemented")
    }

    fn request_frame(&self) {
        unimplemented!("Web requestAnimationFrame not implemented")
    }

    fn active_window(&self) -> Option<WindowId> {
        unimplemented!("Web active window query not implemented")
    }

    fn displays(&self) -> Vec<Arc<dyn PlatformDisplay>> {
        unimplemented!("Web Screen API enumeration not implemented")
    }

    fn primary_display(&self) -> Option<Arc<dyn PlatformDisplay>> {
        unimplemented!("Web primary screen query not implemented")
    }

    fn open_window(&self, _options: WindowOptions) -> Result<Box<dyn PlatformWindow>> {
        unimplemented!("Web canvas/window creation not implemented")
    }

    fn clipboard(&self) -> Arc<dyn Clipboard> {
        unimplemented!("Web Clipboard API not implemented")
    }

    fn capabilities(&self) -> &dyn PlatformCapabilities {
        unimplemented!("Web capabilities not implemented")
    }

    fn name(&self) -> &'static str {
        "Web (WASM stub)"
    }

    fn on_quit(&self, _callback: Box<dyn FnMut() + Send>) {
        unimplemented!("Web quit callback not implemented")
    }

    fn on_window_event(&self, _callback: Box<dyn FnMut(WindowEvent) + Send>) {
        unimplemented!("Web window event callback not implemented")
    }

    fn app_path(&self) -> Result<std::path::PathBuf> {
        unimplemented!("Web app path (origin URL) query not implemented")
    }
}

// TODO: Implement these when adding native Web support:
//
// Core:
// - WebWindow wrapping Canvas element
// - WebDisplay wrapping Screen API
// - WebWorkerExecutor using Web Workers
// - WebTextSystem using Canvas text metrics
//
// Input:
// - PointerEvent handling (mouse + touch unified)
// - KeyboardEvent with proper key codes
// - Gamepad API integration
// - Wheel/scroll events
//
// Rendering:
// - WebGPU surface via wgpu
// - WebGL 2 fallback
// - requestAnimationFrame loop
// - High DPI handling
//
// Services:
// - Clipboard API via navigator.clipboard
// - LocalStorage / IndexedDB
// - Fetch API for networking
// - File System Access API
//
// Integration:
// - wasm-bindgen bridge setup
// - web-sys API bindings
// - Service Worker registration
// - PWA manifest handling
//
// Performance:
// - Bundle size optimization
// - Code splitting
// - Lazy loading
// - WebAssembly streaming compilation
