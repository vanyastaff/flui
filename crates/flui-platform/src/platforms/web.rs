//! Web embedder (placeholder)
//!
//! **Status: Not Yet Implemented**
//!
//! This module will provide WebAssembly/browser platform integration when implemented.
//!
//! # Planned Architecture
//!
//! ```text
//! WebEmbedder
//!   ├─ core: EmbedderCore (90% reuse from other platforms)
//!   ├─ canvas: HtmlCanvasElement (browser canvas)
//!   ├─ renderer: GpuRenderer (WebGPU backend via wgpu)
//!   ├─ capabilities: WebCapabilities
//!   └─ event listeners: browser event handlers
//! ```
//!
//! # Key Differences from Desktop
//!
//! - **Platform**: WebAssembly running in browser
//! - **GPU Backend**: WebGPU (web standard graphics API)
//! - **Window Management**: Single canvas element (no native windows)
//! - **Input**: Mouse, touch, and keyboard via browser events
//! - **Lifecycle**: Page Visibility API (visible/hidden)
//! - **Build Target**: `wasm32-unknown-unknown`
//!
//! # Implementation Roadmap
//!
//! 1. **Phase 1**: Basic rendering
//!    - Canvas element integration
//!    - WebGPU surface initialization via wgpu
//!    - Mouse and touch event handling
//!    - Keyboard input
//!
//! 2. **Phase 2**: Browser integration
//!    - Page Visibility API (pause when tab hidden)
//!    - Resize handling (canvas size changes)
//!    - High DPI/Retina display support
//!    - Request Animation Frame integration
//!
//! 3. **Phase 3**: Web-specific features
//!    - URL routing integration
//!    - Local storage persistence
//!    - Clipboard API
//!    - File upload/download
//!
//! # Build Instructions
//!
//! When implemented, use:
//!
//! ```bash
//! # Install wasm target
//! rustup target add wasm32-unknown-unknown
//!
//! # Build for web
//! cargo build --target wasm32-unknown-unknown --release
//!
//! # Or use trunk for development
//! trunk serve
//! ```
//!
//! # References
//!
//! - Desktop implementation: `desktop.rs` (base architecture)
//! - wgpu web examples: https://github.com/gfx-rs/wgpu/tree/trunk/examples

use crate::{traits::WebCapabilities, PlatformError, Result};

/// Web embedder (placeholder)
///
/// This will provide WebAssembly/browser platform integration following the same
/// pattern as `DesktopEmbedder` and `AndroidEmbedder`, with ~90% code reuse via `EmbedderCore`.
///
/// # Platform-Specific Code
///
/// Only Web-specific logic will be implemented here:
/// - HTML Canvas element binding
/// - Browser event listener setup (mouse, touch, keyboard, resize)
/// - WebGPU surface creation (via wgpu)
/// - Page Visibility API integration
/// - Request Animation Frame loop
///
/// # Current Status
///
/// **Not implemented.** Attempting to create a `WebEmbedder` will return an error.
///
/// # Usage Example (Future)
///
/// ```rust,ignore
/// use wasm_bindgen::prelude::*;
///
/// #[wasm_bindgen(start)]
/// pub async fn start() -> Result<(), JsValue> {
///     let embedder = WebEmbedder::new("canvas-id").await?;
///     embedder.run().await;
///     Ok(())
/// }
/// ```
#[doc = "⚠️ **NOT YET IMPLEMENTED** - Web support is planned but not available yet"]
pub struct WebEmbedder {
    // TODO: Implement with:
    // - core: EmbedderCore
    // - canvas: HtmlCanvasElement
    // - renderer: GpuRenderer
    // - capabilities: WebCapabilities
    // - event_listeners: browser event handlers
    _placeholder: (),
}

impl WebEmbedder {
    /// Create a new Web embedder
    ///
    /// # Arguments
    ///
    /// * `canvas_id` - ID of the HTML canvas element to render to
    ///
    /// # Errors
    ///
    /// Currently always returns `PlatformError::WindowCreation` as Web
    /// support is not yet implemented.
    ///
    /// # Future API
    ///
    /// When implemented, the signature will be:
    ///
    /// ```rust,ignore
    /// pub async fn new(
    ///     pipeline_owner: Arc<RwLock<PipelineOwner>>,
    ///     needs_redraw: Arc<AtomicBool>,
    ///     scheduler: Arc<Scheduler>,
    ///     event_router: Arc<RwLock<EventRouter>>,
    ///     canvas_id: &str,
    /// ) -> Result<Self>
    /// ```
    pub async fn new(_canvas_id: &str) -> Result<Self> {
        Err(PlatformError::WindowCreation(
            "Web embedder not yet implemented. See platforms/web.rs documentation for roadmap."
                .to_string(),
        ))
    }
}

// Note: PlatformEmbedder impl will be added when Web support is implemented
