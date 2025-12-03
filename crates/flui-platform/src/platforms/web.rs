//! Web embedder (placeholder)
//!
//! TODO: Implement Web-specific embedder with WebGPU backend
//! and browser event handling.

use crate::{traits::WebCapabilities, PlatformError, Result};

/// Web embedder (placeholder)
///
/// Will render to HTML Canvas using WebGPU.
pub struct WebEmbedder {
    // TODO: Implement
    _placeholder: (),
}

impl WebEmbedder {
    /// Create a new Web embedder
    pub async fn new(_canvas_id: &str) -> Result<Self> {
        Err(PlatformError::WindowCreation(
            "Web embedder not yet implemented".to_string(),
        ))
    }
}

// Note: PlatformEmbedder impl will be added when Web support is implemented
