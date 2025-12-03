//! iOS embedder (placeholder)
//!
//! TODO: Implement iOS-specific embedder with Metal backend
//! and iOS lifecycle management.

use crate::{
    core::EmbedderCore,
    traits::{
        MobileCapabilities, PlatformCapabilities, PlatformEmbedder, PlatformWindow, WinitWindow,
    },
    PlatformError, Result,
};

/// iOS embedder (placeholder)
///
/// Will handle iOS-specific lifecycle and Metal rendering.
pub struct IosEmbedder {
    // TODO: Implement
    _placeholder: (),
}

impl IosEmbedder {
    /// Create a new iOS embedder
    pub async fn new() -> Result<Self> {
        Err(PlatformError::WindowCreation(
            "iOS embedder not yet implemented".to_string(),
        ))
    }
}

// Note: PlatformEmbedder impl will be added when iOS support is implemented
