//! Common error types for FLUI rendering backends
//!
//! This module provides backend-agnostic error types that can be used
//! by any rendering backend (wgpu, skia, vello, software, etc.)
//!
//! # Design Principles
//!
//! 1. **Backend-agnostic**: Core error variants don't depend on specific backend types
//! 2. **Extensible**: `#[non_exhaustive]` allows adding variants without breaking changes
//! 3. **Composable**: Backend-specific errors wrap underlying errors via `source()`
//! 4. **Informative**: Each variant provides clear context about what went wrong

use std::error::Error;
use thiserror::Error;

/// Rendering errors that can occur in any backend
///
/// This enum is `#[non_exhaustive]` to allow adding new variants
/// in future versions without breaking existing code.
///
/// # Example
///
/// ```rust,ignore
/// use flui_engine::RenderError;
///
/// fn render_frame() -> Result<(), RenderError> {
///     // ... rendering code ...
///     Err(RenderError::SurfaceLost)
/// }
///
/// match render_frame() {
///     Ok(()) => println!("Frame rendered"),
///     Err(RenderError::SurfaceLost) => {
///         println!("Surface lost, will recover on next frame");
///     }
///     Err(e) => eprintln!("Render error: {}", e),
/// }
/// ```
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum RenderError {
    // ========================================================================
    // Surface/Window errors
    // ========================================================================
    /// Surface was lost and needs reconfiguration
    ///
    /// This typically happens when the window is minimized or the GPU driver
    /// is reset. The surface will be reconfigured automatically on the next frame.
    #[error("Surface was lost")]
    SurfaceLost,

    /// Surface is outdated and needs reconfiguration
    ///
    /// This happens when the surface size doesn't match the window size,
    /// typically after a resize event.
    #[error("Surface is outdated")]
    SurfaceOutdated,

    /// Surface acquisition timed out
    ///
    /// The GPU took too long to provide a new frame buffer.
    /// This is usually transient and resolves on the next frame.
    #[error("Surface acquisition timed out")]
    Timeout,

    // ========================================================================
    // Resource errors
    // ========================================================================
    /// Out of GPU memory
    ///
    /// The GPU ran out of memory. This is a serious error that may require
    /// releasing resources or reducing rendering quality.
    #[error("Out of GPU memory")]
    OutOfMemory,

    /// Failed to create a required resource
    ///
    /// Generic resource creation failure with description.
    #[error("Failed to create resource: {0}")]
    ResourceCreation(String),

    // ========================================================================
    // Initialization errors
    // ========================================================================
    /// Failed to create surface from window
    ///
    /// The rendering backend couldn't create a surface from the provided window.
    /// Contains backend-specific error as source.
    #[error("Failed to create surface: {0}")]
    SurfaceCreation(#[source] Box<dyn Error + Send + Sync>),

    /// No suitable GPU adapter found
    ///
    /// No GPU was found that meets the requirements (e.g., supports required features,
    /// is compatible with the surface).
    #[error("No suitable GPU adapter found")]
    NoAdapter,

    /// Failed to create GPU device
    ///
    /// The GPU adapter was found but device creation failed.
    /// Contains backend-specific error as source.
    #[error("Failed to create GPU device: {0}")]
    DeviceCreation(#[source] Box<dyn Error + Send + Sync>),

    // ========================================================================
    // Rendering errors
    // ========================================================================
    /// Error during painting operations
    ///
    /// An error occurred while executing paint commands.
    #[error("Painter error: {0}")]
    PainterError(String),

    /// Shader compilation or linking failed
    ///
    /// The shader source couldn't be compiled or linked.
    #[error("Shader error: {0}")]
    ShaderError(String),

    /// Pipeline creation failed
    ///
    /// Failed to create a rendering pipeline (combination of shaders, state, etc.)
    #[error("Pipeline error: {0}")]
    PipelineError(String),

    // ========================================================================
    // State errors
    // ========================================================================
    /// Invalid state for the requested operation
    ///
    /// The renderer is in a state that doesn't allow the requested operation.
    #[error("Invalid state: {0}")]
    InvalidState(String),

    /// Renderer was not properly initialized
    ///
    /// An operation was attempted before the renderer was fully initialized.
    #[error("Renderer not initialized")]
    NotInitialized,
}

// ============================================================================
// Convenience constructors
// ============================================================================

impl RenderError {
    /// Create a surface creation error from any error type
    #[must_use]
    pub fn surface_creation<E>(error: E) -> Self
    where
        E: Error + Send + Sync + 'static,
    {
        RenderError::SurfaceCreation(Box::new(error))
    }

    /// Create a device creation error from any error type
    #[must_use]
    pub fn device_creation<E>(error: E) -> Self
    where
        E: Error + Send + Sync + 'static,
    {
        RenderError::DeviceCreation(Box::new(error))
    }

    /// Create a painter error from a string
    #[must_use]
    pub fn painter<S: Into<String>>(msg: S) -> Self {
        RenderError::PainterError(msg.into())
    }

    /// Create a shader error from a string
    #[must_use]
    pub fn shader<S: Into<String>>(msg: S) -> Self {
        RenderError::ShaderError(msg.into())
    }

    /// Create a pipeline error from a string
    #[must_use]
    pub fn pipeline<S: Into<String>>(msg: S) -> Self {
        RenderError::PipelineError(msg.into())
    }

    /// Create a resource creation error from a string
    #[must_use]
    pub fn resource<S: Into<String>>(msg: S) -> Self {
        RenderError::ResourceCreation(msg.into())
    }

    /// Create an invalid state error from a string
    #[must_use]
    pub fn invalid_state<S: Into<String>>(msg: S) -> Self {
        RenderError::InvalidState(msg.into())
    }

    /// Check if this error is recoverable (will likely succeed on retry)
    #[must_use]
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            RenderError::SurfaceLost | RenderError::SurfaceOutdated | RenderError::Timeout
        )
    }

    /// Check if this error is fatal (requires restart or resource cleanup)
    #[must_use]
    pub fn is_fatal(&self) -> bool {
        matches!(
            self,
            RenderError::OutOfMemory
                | RenderError::NoAdapter
                | RenderError::DeviceCreation(_)
                | RenderError::NotInitialized
        )
    }
}

// ============================================================================
// Result type alias
// ============================================================================

/// A Result type alias for rendering operations
pub type RenderResult<T> = Result<T, RenderError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display() {
        assert_eq!(RenderError::SurfaceLost.to_string(), "Surface was lost");
        assert_eq!(
            RenderError::NoAdapter.to_string(),
            "No suitable GPU adapter found"
        );
        assert_eq!(
            RenderError::painter("test error").to_string(),
            "Painter error: test error"
        );
    }

    #[test]
    fn test_is_recoverable() {
        assert!(RenderError::SurfaceLost.is_recoverable());
        assert!(RenderError::SurfaceOutdated.is_recoverable());
        assert!(RenderError::Timeout.is_recoverable());
        assert!(!RenderError::OutOfMemory.is_recoverable());
        assert!(!RenderError::NoAdapter.is_recoverable());
    }

    #[test]
    fn test_is_fatal() {
        assert!(RenderError::OutOfMemory.is_fatal());
        assert!(RenderError::NoAdapter.is_fatal());
        assert!(RenderError::NotInitialized.is_fatal());
        assert!(!RenderError::SurfaceLost.is_fatal());
        assert!(!RenderError::Timeout.is_fatal());
    }
}
