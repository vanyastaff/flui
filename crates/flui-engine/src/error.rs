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
//!
//! # Host-crash invariant
//!
//! wgpu shader and pipeline creation is **infallible at runtime** in this
//! engine: a shader/pipeline that compiled once will keep compiling for the
//! lifetime of the process, and validation failures surface through wgpu's
//! `on_uncaptured_error` host-level handler (which logs and aborts) rather
//! than a `Result` the engine propagates. There is therefore no typed
//! shader/pipeline error variant to wrap. Resource I/O failures (font/shader
//! file loads) use [`EngineError::ResourceIo`]; GPU-side init failures use
//! [`EngineError::SurfaceCreation`] / [`EngineError::DeviceCreation`] /
//! [`EngineError::AdapterRequest`].

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
/// use flui_engine::EngineError;
///
/// fn render_frame() -> Result<(), EngineError> {
///     // ... rendering code ...
///     Err(EngineError::SurfaceLost)
/// }
///
/// match render_frame() {
///     Ok(()) => println!("Frame rendered"),
///     Err(EngineError::SurfaceLost) => {
///         println!("Surface lost, will recover on next frame");
///     }
///     Err(e) => eprintln!("Render error: {}", e),
/// }
/// ```
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum EngineError {
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

    /// GPU device was lost and cannot be recovered by surface reconfiguration.
    ///
    /// This happens on TDR (Timeout Detection and Recovery), driver crashes,
    /// or GPU hardware failures. The caller must recreate the entire renderer
    /// to recover.
    #[error("GPU device lost")]
    DeviceLost,

    /// Surface acquisition timed out
    ///
    /// The GPU took too long to provide a new frame buffer.
    /// This is usually transient and resolves on the next frame.
    #[error("Surface acquisition timed out")]
    Timeout,

    /// Surface texture acquisition failed wgpu validation.
    ///
    /// wgpu's `CurrentSurfaceTexture::Validation` carries no diagnostic
    /// payload — it signals a surface misconfiguration (format/usage/present
    /// mode incompatibility) that **cannot** be resolved by retrying
    /// `get_current_texture`; the surface must be reconfigured before the
    /// next acquire. Retrying without reconfiguring loops forever, so this
    /// is classified [`Recoverability::Unrecoverable`]: the caller logs and
    /// drops the frame, then reconfigures on the next pass.
    #[error("Surface texture validation error")]
    SurfaceValidation,

    // ========================================================================
    // Resource errors
    // ========================================================================
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
    /// (e.g. the future resolved to `None` semantically). For wgpu 29.x
    /// `Result<Adapter, RequestAdapterError>` returns prefer
    /// [`EngineError::AdapterRequest`] which preserves the wgpu diagnostic
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
    /// `EnvNotSet`, ...). Use this in preference to [`EngineError::NoAdapter`]
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
    // Text rendering errors (glyphon)
    // ========================================================================
    /// glyphon text atlas preparation failed.
    ///
    /// Boxes the underlying `glyphon::PrepareError` via `#[source]` so the
    /// diagnostic chain survives. Use [`EngineError::text_prepare`] to
    /// construct it from any `Error + Send + Sync + 'static`.
    #[error("Text prepare error: {0}")]
    TextPrepare(#[source] Box<dyn Error + Send + Sync>),

    /// glyphon text render pass failed.
    ///
    /// Boxes the underlying `glyphon::RenderError` via `#[source]` so the
    /// diagnostic chain survives. Use [`EngineError::text_render`] to
    /// construct it from any `Error + Send + Sync + 'static`.
    #[error("Text render error: {0}")]
    TextRender(#[source] Box<dyn Error + Send + Sync>),

    // ========================================================================
    // State errors
    // ========================================================================
    /// Renderer was not properly initialized
    ///
    /// An operation was attempted before the renderer was fully initialized.
    #[error("Renderer not initialized")]
    NotInitialized,
}

// ============================================================================
// Recoverability classification
// ============================================================================

/// Coarse recovery classification for an [`EngineError`].
///
/// Replaces the previous pair of `bool` classifiers (`is_recoverable` /
/// `is_fatal`) which silently dropped variants into an undocumented third
/// bucket. This enum makes the third bucket explicit and the internal match
/// in [`EngineError::recoverability`] exhaustive, so adding a variant to
/// `EngineError` without a classification arm is a compile error — closing
/// the silent-third-bucket hole.
///
/// - [`Recoverability::Recoverable`] — retry the same operation on the next frame; no rebuild.
/// - [`Recoverability::Fatal`] — the renderer must be recreated; surface reconfiguration
///   cannot recover.
/// - [`Recoverability::Unrecoverable`] — the frame is dropped and logged; reconfiguration or
///   operator intervention may be required (e.g. a surface misconfig), but
///   blindly retrying the same call loops forever.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Recoverability {
    /// Retry the same operation next frame; no rebuild needed.
    Recoverable,
    /// The renderer must be recreated; surface reconfiguration cannot recover.
    Fatal,
    /// Drop the frame and log; do not blindly retry (would loop forever).
    Unrecoverable,
}

impl EngineError {
    /// Classify this error's recovery posture.
    ///
    /// The internal `match` is exhaustive — a future variant added to
    /// [`EngineError`] cannot compile without a classification arm here, so
    /// no variant can silently fall into an undocumented bucket.
    #[must_use]
    pub fn recoverability(&self) -> Recoverability {
        match self {
            Self::SurfaceLost | Self::Timeout => Recoverability::Recoverable,
            Self::DeviceLost
            | Self::SurfaceCreation(_)
            | Self::NoAdapter
            | Self::AdapterRequest(_)
            | Self::DeviceCreation(_)
            | Self::NotInitialized => Recoverability::Fatal,
            Self::SurfaceValidation
            | Self::ResourceIo { .. }
            | Self::TextPrepare(_)
            | Self::TextRender(_) => Recoverability::Unrecoverable,
        }
    }
}

// ============================================================================
// Convenience constructors
// ============================================================================

impl EngineError {
    /// Create a surface creation error from any error type
    #[must_use]
    pub fn surface_creation<E>(error: E) -> Self
    where
        E: Error + Send + Sync + 'static,
    {
        EngineError::SurfaceCreation(Box::new(error))
    }

    /// Create a device creation error from any error type
    #[must_use]
    pub fn device_creation<E>(error: E) -> Self
    where
        E: Error + Send + Sync + 'static,
    {
        EngineError::DeviceCreation(Box::new(error))
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
        EngineError::AdapterRequest(Box::new(error))
    }

    /// Create a filesystem-resource-load error from an [`std::io::Error`].
    ///
    /// `context` is a caller-supplied free-form description (e.g.
    /// `"font load /path/to/font.ttf"`); the underlying `io::Error` is
    /// preserved via `#[source]` so callers can match on
    /// [`std::io::ErrorKind`].
    #[must_use]
    pub fn resource_io<S: Into<String>>(context: S, source: std::io::Error) -> Self {
        EngineError::ResourceIo {
            context: context.into(),
            source,
        }
    }

    /// Create a text-prepare error from any error type.
    ///
    /// Boxes the underlying `glyphon::PrepareError` (or equivalent) via
    /// `#[source]` so the diagnostic chain survives. Follows the same
    /// `Error + Send + Sync + 'static` ctor pattern as
    /// [`EngineError::surface_creation`].
    #[must_use]
    pub fn text_prepare<E>(error: E) -> Self
    where
        E: Error + Send + Sync + 'static,
    {
        EngineError::TextPrepare(Box::new(error))
    }

    /// Create a text-render error from any error type.
    ///
    /// Boxes the underlying `glyphon::RenderError` (or equivalent) via
    /// `#[source]` so the diagnostic chain survives. Follows the same
    /// `Error + Send + Sync + 'static` ctor pattern as
    /// [`EngineError::surface_creation`].
    #[must_use]
    pub fn text_render<E>(error: E) -> Self
    where
        E: Error + Send + Sync + 'static,
    {
        EngineError::TextRender(Box::new(error))
    }
}

// ============================================================================
// Result type alias
// ============================================================================

/// A Result type alias for engine operations.
pub type EngineResult<T> = Result<T, EngineError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display() {
        assert_eq!(EngineError::SurfaceLost.to_string(), "Surface was lost");
        assert_eq!(
            EngineError::NoAdapter.to_string(),
            "No suitable GPU adapter found"
        );
        assert_eq!(
            EngineError::SurfaceValidation.to_string(),
            "Surface texture validation error"
        );
    }

    #[test]
    fn test_recoverability_surface_validation() {
        // The bug fix this refactor rides on: a wgpu surface-validation
        // error must NOT be classified as Recoverable (the pre-refactor code
        // mapped `wgpu::CurrentSurfaceTexture::Validation` to `SurfaceLost`,
        // causing an infinite retry loop on a misconfigured surface).
        assert_eq!(
            EngineError::SurfaceValidation.recoverability(),
            Recoverability::Unrecoverable
        );
    }

    #[test]
    fn test_recoverability_table() {
        // Full classification table across the three buckets. Asserts the
        //exact value so a misclassification fails the test rather than
        // silently passing via a bool.
        assert_eq!(
            EngineError::SurfaceLost.recoverability(),
            Recoverability::Recoverable
        );
        assert_eq!(
            EngineError::Timeout.recoverability(),
            Recoverability::Recoverable
        );
        assert_eq!(
            EngineError::DeviceLost.recoverability(),
            Recoverability::Fatal
        );
        assert_eq!(
            EngineError::NoAdapter.recoverability(),
            Recoverability::Fatal
        );
        assert_eq!(
            EngineError::surface_creation(std::io::Error::other("test")).recoverability(),
            Recoverability::Fatal
        );
        assert_eq!(
            EngineError::adapter_request(std::io::Error::other("test")).recoverability(),
            Recoverability::Fatal
        );
        assert_eq!(
            EngineError::device_creation(std::io::Error::other("test")).recoverability(),
            Recoverability::Fatal
        );
        assert_eq!(
            EngineError::NotInitialized.recoverability(),
            Recoverability::Fatal
        );
        assert_eq!(
            EngineError::SurfaceValidation.recoverability(),
            Recoverability::Unrecoverable
        );
        assert_eq!(
            EngineError::resource_io("font load", std::io::Error::other("boom")).recoverability(),
            Recoverability::Unrecoverable
        );
        assert_eq!(
            EngineError::text_prepare(std::io::Error::other("prepare boom")).recoverability(),
            Recoverability::Unrecoverable
        );
        assert_eq!(
            EngineError::text_render(std::io::Error::other("render boom")).recoverability(),
            Recoverability::Unrecoverable
        );
    }

    #[cfg(feature = "wgpu-backend")]
    #[test]
    fn test_text_prepare_source_chain() {
        // Red pre-refactor: `text_prepare` / `TextPrepare` / typed `#[source]`
        // did not exist; the old code stringified via
        // `text_render(format!("prepare: {e:?}"))` into `TextRender(String)`,
        // whose `source()` is `None` -> `unwrap()` would panic. Post-refactor
        // the typed ctor boxes the glyphon error and `source()` downcasts back
        // to the original `PrepareError`.
        let e = EngineError::text_prepare(glyphon::PrepareError::AtlasFull);
        assert_eq!(
            e.source()
                .unwrap()
                .downcast_ref::<glyphon::PrepareError>()
                .unwrap(),
            &glyphon::PrepareError::AtlasFull
        );
    }

    #[cfg(feature = "wgpu-backend")]
    #[test]
    fn test_text_render_source_chain() {
        // Red pre-refactor: `text_render` took `Into<String>` and the
        // `TextRender(String)` variant's `source()` is `None`. Post-refactor
        // the typed ctor boxes the glyphon error and `source()` downcasts back
        // to the original `RenderError`.
        let e = EngineError::text_render(glyphon::RenderError::RemovedFromAtlas);
        assert_eq!(
            e.source()
                .unwrap()
                .downcast_ref::<glyphon::RenderError>()
                .unwrap(),
            &glyphon::RenderError::RemovedFromAtlas
        );
    }

    #[cfg(feature = "wgpu-backend")]
    #[test]
    fn test_handle_error_preserved() {
        // Red pre-refactor: the call site stringified the `HandleError` via
        // `std::io::Error::other(e.to_string())` and boxed the `io::Error`,
        // so `source().downcast_ref::<HandleError>()` returned `None`.
        // Post-refactor (with the raw-window-handle `std` feature enabled)
        // `HandleError` impls `std::error::Error` and is boxed directly,
        // preserving the original error type in the source chain.
        let e = EngineError::surface_creation(raw_window_handle::HandleError::Unavailable);
        assert!(
            e.source()
                .unwrap()
                .downcast_ref::<raw_window_handle::HandleError>()
                .is_some()
        );
    }
}
