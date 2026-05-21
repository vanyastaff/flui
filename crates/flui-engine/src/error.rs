//! Common error types for FLUI rendering backends
//!
//! This module provides backend-agnostic error types that can be used
//! by any rendering backend (wgpu, skia, vello, software, etc.)
//!
//! # Design Principles
//!
//! 1. **Backend-agnostic**: Core error variants don't depend on specific
//!    backend types
//! 2. **Extensible**: `#[non_exhaustive]` allows adding variants without
//!    breaking changes
//! 3. **Composable**: Backend-specific errors wrap underlying errors via
//!    `source()`
//! 4. **Informative**: Each variant provides clear context about what went
//!    wrong

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
    /// is reset. The surface will be reconfigured automatically on the next
    /// frame.
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

    /// Filesystem-backed resource (font, shader file, asset) failed to load.
    ///
    /// Preserves the underlying `std::io::Error` via `#[source]` so callers can
    /// match on `io::ErrorKind::{NotFound, PermissionDenied, ...}` without
    /// re-parsing the formatted message.
    #[error("Resource I/O failure ({context})")]
    ResourceIo {
        /// Caller-supplied context (e.g. `"font load /path/to/font.ttf"`).
        context: String,
        /// Underlying `std::io::Error`.
        #[source]
        source: std::io::Error,
    },

    // ========================================================================
    // Initialization errors
    // ========================================================================
    /// Failed to create surface from window
    ///
    /// The rendering backend couldn't create a surface from the provided
    /// window. Contains backend-specific error as source.
    #[error("Failed to create surface: {0}")]
    SurfaceCreation(#[source] Box<dyn Error + Send + Sync>),

    /// No suitable GPU adapter found (sentinel; carries no underlying error).
    ///
    /// Use this variant when `request_adapter` returns no underlying error
    /// (e.g. the future resolved to `None` semantically). For wgpu 25.x+
    /// `Result<Adapter, RequestAdapterError>` returns prefer
    /// [`RenderError::AdapterRequest`] which preserves the wgpu diagnostic
    /// (`NotFound { active_backends, requested_backends, supported_backends,
    /// no_fallback_backends, no_adapter_backends, incompatible_surface_backends }`)
    /// via `#[source]`.
    #[error("No suitable GPU adapter found")]
    NoAdapter,

    /// Adapter request failed with a backend-specific diagnostic payload.
    ///
    /// Wraps wgpu's `RequestAdapterError` (or any other backend-specific
    /// adapter-acquisition error) via `#[source]` so operators get the full
    /// diagnostic context (`NotFound { active_backends, ... }`,
    /// `EnvNotSet`, ...). Use this in preference to [`RenderError::NoAdapter`]
    /// when the underlying API exposes structured diagnostics.
    #[error("GPU adapter request failed: {0}")]
    AdapterRequest(#[source] Box<dyn Error + Send + Sync>),

    /// Failed to create GPU device
    ///
    /// The GPU adapter was found but device creation failed.
    /// Contains backend-specific error as source.
    #[error("Failed to create GPU device: {0}")]
    DeviceCreation(#[source] Box<dyn Error + Send + Sync>),

    // ========================================================================
    // Rendering errors
    // ========================================================================
    /// Shader compilation or linking failed
    ///
    /// The shader source couldn't be compiled or linked.
    #[error("Shader error: {0}")]
    ShaderError(String),

    /// Pipeline creation failed
    ///
    /// Failed to create a rendering pipeline (combination of shaders, state,
    /// etc.)
    #[error("Pipeline error: {0}")]
    PipelineError(String),

    /// Text rendering (glyphon prepare/render) failed.
    ///
    /// Carries the underlying glyphon error message via String because
    /// `glyphon::PrepareError` and `glyphon::RenderError` are private
    /// implementation types in older glyphon releases; we preserve the
    /// formatted error context.
    #[error("Text render error: {0}")]
    TextRender(String),

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

    /// Create an adapter-request error from any error type.
    ///
    /// Wraps the underlying `RequestAdapterError` (or equivalent) via
    /// `#[source]` so the diagnostic chain is preserved.
    #[must_use]
    pub fn adapter_request<E>(error: E) -> Self
    where
        E: Error + Send + Sync + 'static,
    {
        RenderError::AdapterRequest(Box::new(error))
    }

    /// Create a filesystem-resource-load error from an [`std::io::Error`].
    ///
    /// `context` is a caller-supplied free-form description (e.g.
    /// `"font load /path/to/font.ttf"`); the underlying `io::Error` is
    /// preserved via `#[source]` so callers can match on
    /// [`std::io::ErrorKind`].
    #[must_use]
    pub fn resource_io<S: Into<String>>(context: S, source: std::io::Error) -> Self {
        RenderError::ResourceIo {
            context: context.into(),
            source,
        }
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

    /// Create a text-render error from a string.
    #[must_use]
    pub fn text_render<S: Into<String>>(msg: S) -> Self {
        RenderError::TextRender(msg.into())
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
                | RenderError::AdapterRequest(_)
                | RenderError::DeviceCreation(_)
                | RenderError::SurfaceCreation(_)
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
        assert!(RenderError::surface_creation(std::io::Error::other("test")).is_fatal());
        assert!(!RenderError::SurfaceLost.is_fatal());
        assert!(!RenderError::Timeout.is_fatal());
    }
}
