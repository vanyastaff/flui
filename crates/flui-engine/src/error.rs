//! Error types for FLUI's wgpu rendering engine
//!
//! This module describes engine failures and their recovery policy. GPU errors
//! retain wgpu types and diagnostics across its platform backends.
//!
//! # Design Principles
//!
//! 1. **Typed diagnostics**: GPU variants preserve wgpu configuration and
//!    failure details
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

/// Geometry rejected before recording or GPU packing.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum GeometryError {
    /// A geometry input or accumulated result is not finite.
    #[error("non-finite geometry: {context}")]
    NonFinite {
        /// The failed operation.
        context: &'static str,
    },
    /// The current renderer admits only two-dimensional affine transforms.
    #[error("only two-dimensional affine transforms are supported")]
    UnsupportedTransform,
    /// An invertible transform could not produce a finite inverse.
    #[error("geometry inverse cannot be represented")]
    NonFiniteInverse,
    /// Rectangle edges are reversed or their difference overflows.
    #[error("invalid clip rectangle extent")]
    InvalidExtent,
    /// Corner radii must be finite and nonnegative.
    #[error("invalid clip corner radius")]
    InvalidRadius,
    /// A path exceeds its input command allowance.
    #[error("clip path has {requested} commands, limit {limit}")]
    PathCommandLimit {
        /// Input command count.
        requested: usize,
        /// Admitted command count.
        limit: usize,
    },
    /// Finite geometry cannot be represented by the GPU payload.
    #[error("geometry cannot be packed: {context}")]
    Unrepresentable {
        /// The failed payload.
        context: &'static str,
    },
}

/// Typed failures of trusted external registration and draw validation.
#[derive(Debug, Clone, Error)]
#[non_exhaustive]
pub enum ExternalTextureError {
    /// The logical ID is not registered.
    #[error("external texture {id} is not registered")]
    UnknownTexture {
        /// Logical texture ID.
        id: u64,
    },
    /// Register cannot implicitly replace a live allocation.
    #[error("external texture {id} is already registered")]
    DuplicateTexture {
        /// Logical texture ID.
        id: u64,
    },
    /// Only two-dimensional textures are supported.
    #[error("external texture dimension {actual:?} is unsupported")]
    InvalidTextureDimension {
        /// Actual texture dimension.
        actual: wgpu::TextureDimension,
    },
    /// Array/cube allocations are unsupported.
    #[error("external texture has {actual} layers; one required")]
    InvalidTextureLayers {
        /// Actual layer count.
        actual: u32,
    },
    /// Multisampled sampling is unsupported.
    #[error("external texture has {actual} samples; one required")]
    InvalidTextureSamples {
        /// Actual sample count.
        actual: u32,
    },
    /// The allocation must be sampleable.
    #[error("external texture lacks TEXTURE_BINDING usage")]
    MissingTextureBindingUsage,
    /// Only encoded-SDR RGBA8/BGRA8 unorm views are supported.
    #[error("external texture format {format:?} is unsupported")]
    UnsupportedTextureFormat {
        /// Actual format, without reinterpretation.
        format: wgpu::TextureFormat,
    },
    /// Dimensions exceed the receiving device's supported extent.
    #[error("external texture size {width}x{height} is invalid")]
    InvalidTextureSize {
        /// Actual width in device pixels.
        width: u32,
        /// Actual height in device pixels.
        height: u32,
    },
    /// Update must preserve allocation interpretation and extent.
    #[error(
        "external replacement {actual_size:?}/{actual_format:?} differs from {expected_size:?}/{expected_format:?}"
    )]
    IncompatibleReplacement {
        /// Previously admitted extent.
        expected_size: (u32, u32),
        /// Replacement extent.
        actual_size: (u32, u32),
        /// Previously admitted format.
        expected_format: wgpu::TextureFormat,
        /// Replacement format.
        actual_format: wgpu::TextureFormat,
    },
    /// Registry generation arithmetic cannot wrap.
    #[error("external allocation generation exhausted")]
    GenerationExhausted,
    /// Draw opacity must be finite and within zero to one.
    #[error("external texture opacity is invalid")]
    InvalidOpacity,
    /// Destination geometry must be finite and have positive extent.
    #[error("external texture destination is invalid")]
    InvalidDestination,
    /// Crop geometry must be finite, positive and inside the allocation.
    #[error("external texture source rectangle is invalid")]
    InvalidSourceRect,
    /// The captured engine lease belongs to another expected device owner.
    #[error("external texture lease belongs to another device domain")]
    ForeignOwner,
}

/// Rendering errors that can occur in any backend
///
/// This enum is `#[non_exhaustive]` to allow adding new variants
/// in future versions without breaking existing code.
///
/// # Example
///
/// ```rust,no_run
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
    /// A direct draw cannot separate paint alpha from soft clip coverage on this device.
    #[error(
        "direct {mode:?} drawing under antialiased coverage requires dual-source blending or a supported sampleable destination"
    )]
    UnsupportedCoverageBlend {
        /// Destination-sensitive blend mode whose coverage cannot be represented.
        mode: flui_painting::paint::BlendMode,
    },
    /// Coverage-correct compositing requires a sampleable destination.
    #[error("compositing requires a sampleable destination target")]
    CompositeBackdropUnavailable,
    /// A supplied render target violates the required attachment contract.
    #[error("invalid render target: {reason}")]
    InvalidRenderTarget {
        /// The violated attachment requirement.
        reason: &'static str,
    },
    /// Geometry violated the admitted rendering contract.
    #[error(transparent)]
    InvalidGeometry(#[from] GeometryError),
    /// External registration or drawing violated its explicit contract.
    #[error(transparent)]
    ExternalTexture(#[from] ExternalTextureError),
    /// Preparing a frame would exceed an explicit resource or device limit.
    #[error("prepared {resource} exceeds limit: requested {requested}, limit {limit}")]
    PreparedResourceLimit {
        /// The resource whose admission failed.
        resource: &'static str,
        /// Required amount, including resources already charged where applicable.
        requested: usize,
        /// Maximum admitted amount.
        limit: usize,
    },
    /// A fallible prepared metadata allocation was rejected by the allocator.
    #[error("prepared {resource} allocation failed: {source}")]
    PreparedResourceAllocation {
        /// The owner whose metadata could not grow.
        resource: &'static str,
        /// The allocator/capacity error.
        #[source]
        source: std::collections::TryReserveError,
    },
    /// A resource size cannot be represented without arithmetic overflow.
    #[error("prepared resource size overflow")]
    PreparedResourceOverflow,
    /// A prepared submission or reservation belongs to a different device owner.
    #[error("prepared submission belongs to another device domain")]
    DeviceDomainMismatch,
    /// The device owner is closing or no longer accepts preparation.
    #[error("GPU preparation is unavailable")]
    GpuUnavailable,
    /// The caller tried to open another frame before finishing the active one.
    #[error("a frame is already active on this device domain")]
    FrameAlreadyActive,
    /// Partial rendering needs a valid committed image; retry with a full frame.
    #[error("partial frame has no compatible committed retained source")]
    MissingRetainedSource,
    /// Outstanding submissions must complete before another may be admitted.
    #[error("GPU submission capacity exhausted")]
    GpuBackpressure,
    /// Nonblocking progress on the device failed.
    #[error("GPU progress failed: {source}")]
    GpuProgress {
        /// The wgpu progress failure.
        #[source]
        source: wgpu::PollError,
    },
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
    /// payload. The renderer reconfigures the surface and retries once before
    /// returning this error. A repeated validation failure is classified
    /// [`Recoverability::Unrecoverable`], so the caller logs and drops the
    /// frame instead of entering an unbounded retry loop.
    #[error("Surface texture validation error")]
    SurfaceValidation,

    /// No advertised format/color-space pair accepts encoded sRGB shader output.
    /// Retrying the same surface cannot repair this unsupported configuration.
    #[error("Surface requires an SDR UNorm/Srgb presentation pair; advertised: {supported:?}")]
    UnsupportedSurfaceColorConfiguration {
        /// Per-format presentation color spaces reported by the adapter.
        supported: Vec<wgpu::SurfaceFormatCapabilities>,
    },

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

    /// The window target's owner reports its native handle is gone or
    /// suspended (a destroyed window, a torn-down Wayland surface, an
    /// Android window between `MainEvent::TerminateWindow` and the next
    /// `MainEvent::InitWindow`).
    ///
    /// Distinguished from [`EngineError::SurfaceCreation`] on purpose: wgpu's
    /// `CreateSurfaceError` boxes the `raw_window_handle::HandleError` it
    /// hits internally but does not expose it via `source()` — only the
    /// formatted `Display` text survives. Verified against
    /// `wgpu-30.0.1/src/api/surface.rs`'s `CreateSurfaceError::source()`:
    /// its `CreateSurfaceErrorKind::RawHandle` arm returns `None` directly
    /// when wgpu's own `std` feature is off (this workspace's resolved
    /// feature set — confirmed via `cargo metadata`, wgpu carries no `std`
    /// feature here); with that feature on it instead forwards to
    /// `HandleError::source()`, which is `None` too (raw-window-handle's
    /// `impl std::error::Error for HandleError {}` has no override) — either
    /// way the `HandleError` itself never survives the `source()` chain.
    /// Without probing the target directly first, "the owner says the
    /// window is gone/suspended"
    /// and "the GPU driver refused for some unrelated reason" would collapse
    /// into one undifferentiated variant that a caller cannot tell apart —
    /// which matters because the first is often transient (wait for the next
    /// resume) and the second usually is not. Use
    /// [`EngineError::surface_target_unavailable`] to construct this from the
    /// original, typed [`raw_window_handle::HandleError`].
    #[error(
        "surface target unavailable: the window owner reports its native handle is gone or \
         suspended: {source}"
    )]
    SurfaceTargetUnavailable {
        /// The owner's report — `Unavailable` for a destroyed/suspended
        /// window, `NotSupported` when the owner never implements this
        /// handle kind at all.
        #[source]
        source: raw_window_handle::HandleError,
    },

    /// Adapter request failed with a backend-specific diagnostic payload.
    ///
    /// Wraps wgpu's `RequestAdapterError` (or any other backend-specific
    /// adapter-acquisition error) via `#[source]` so operators get the full
    /// diagnostic context (`NotFound { active_backends, ... }`,
    /// `EnvNotSet`, ...). The structured payload is the reason this variant
    /// exists rather than a sentinel: wgpu 30's `request_adapter` returns a
    /// `Result` whose error names which backends were tried and why each was
    /// rejected.
    #[error("GPU adapter request failed: {0}")]
    AdapterRequest(#[source] Box<dyn Error + Send + Sync>),

    /// Failed to create GPU device
    ///
    /// The GPU adapter was found but device creation failed.
    /// Contains backend-specific error as source.
    #[error("Failed to create GPU device: {0}")]
    DeviceCreation(#[source] Box<dyn Error + Send + Sync>),

    // ========================================================================
    // State errors
    // ========================================================================
    /// Renderer was not properly initialized
    ///
    /// An operation was attempted before the renderer was fully initialized.
    #[error("Renderer not initialized")]
    NotInitialized,

    /// A capture or render target was requested at a zero-sized extent.
    ///
    /// wgpu rejects a zero-byte buffer and a zero-sized texture, and its own
    /// rejection path is `BufferSize::new(..).unwrap()` inside `wgpu-core` —
    /// a panic, not a validation error a caller can match on. This variant is
    /// the typed rejection: a caller that derived the size from user input
    /// (a `-- <w> <h>` CLI argument, a window that has not been laid out yet)
    /// gets a `Result` instead of taking the process down.
    #[error("invalid render target size: {width}x{height} (both axes must be non-zero)")]
    InvalidTargetSize {
        /// The requested width in device pixels.
        width: u32,
        /// The requested height in device pixels.
        height: u32,
    },

    /// A pixel readback did not complete within its bound.
    ///
    /// The GPU never signalled the copy's completion: a stalled device or
    /// driver. Waiting without a bound turned that into a process that never
    /// returns; this is the error a caller sees instead.
    #[error("GPU readback did not complete within {waited:?}")]
    ReadbackTimedOut {
        /// How long the readback waited.
        waited: std::time::Duration,
    },
}

// ============================================================================
// Recoverability classification
// ============================================================================

/// Coarse recovery classification for an [`EngineError`].
///
/// Three postures, each a different caller action; the enum is closed (no
/// `#[non_exhaustive]`) so every consumer's `match` stays exhaustive and a
/// fourth posture is a compile error at every decision site rather than a
/// silent fall-through into a `_` arm.
///
/// - [`Recoverability::Recoverable`] — retry the same operation on the next frame; no rebuild.
/// - [`Recoverability::Fatal`] — the renderer must be recreated; surface reconfiguration
///   cannot recover.
/// - [`Recoverability::Unrecoverable`] — the frame is dropped and logged; reconfiguration or
///   operator intervention may be required (e.g. a surface misconfig), but
///   blindly retrying the same call loops forever.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
            Self::SurfaceLost
            | Self::Timeout
            | Self::GpuBackpressure
            | Self::MissingRetainedSource => Recoverability::Recoverable,
            // `raw_window_handle::HandleError` is itself `#[non_exhaustive]`,
            // so this inner match's wildcard is deliberate: a variant this
            // crate has not classified yet is treated as `Fatal` rather than
            // silently `Recoverable`. `Unavailable` means "wait and retry"
            // (a suspended surface); `NotSupported` means the owner can
            // never answer this handle kind, which retrying cannot fix.
            Self::SurfaceTargetUnavailable { source } => match source {
                raw_window_handle::HandleError::Unavailable => Recoverability::Recoverable,
                // `NotSupported` is named explicitly for the reader even
                // though it shares `_`'s outcome: retrying cannot fix an
                // owner that can never answer this handle kind, and any
                // variant this crate has not seen yet gets the same
                // conservative answer.
                raw_window_handle::HandleError::NotSupported | _ => Recoverability::Fatal,
            },
            Self::DeviceLost
            | Self::GpuProgress { .. }
            | Self::GpuUnavailable
            | Self::SurfaceCreation(_)
            | Self::AdapterRequest(_)
            | Self::DeviceCreation(_)
            | Self::InvalidTargetSize { .. }
            | Self::ReadbackTimedOut { .. }
            | Self::NotInitialized => Recoverability::Fatal,
            Self::UnsupportedCoverageBlend { .. }
            | Self::CompositeBackdropUnavailable
            | Self::InvalidRenderTarget { .. }
            | Self::InvalidGeometry(_)
            | Self::ExternalTexture(_)
            | Self::SurfaceValidation
            | Self::PreparedResourceLimit { .. }
            | Self::PreparedResourceOverflow
            | Self::PreparedResourceAllocation { .. }
            | Self::DeviceDomainMismatch
            | Self::FrameAlreadyActive
            | Self::ResourceIo { .. }
            | Self::UnsupportedSurfaceColorConfiguration { .. } => Recoverability::Unrecoverable,
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

    /// Create a surface-target-unavailable error from the window owner's
    /// reported [`raw_window_handle::HandleError`].
    #[must_use]
    pub fn surface_target_unavailable(source: raw_window_handle::HandleError) -> Self {
        EngineError::SurfaceTargetUnavailable { source }
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
}

// ============================================================================
// Result type alias
// ============================================================================

/// A Result type alias for engine operations.
pub type EngineResult<T> = Result<T, EngineError>;

impl From<crate::device_domain::DomainError> for EngineError {
    fn from(error: crate::device_domain::DomainError) -> Self {
        use crate::device_domain::DomainError;
        match error {
            DomainError::Overflow => Self::PreparedResourceOverflow,
            DomainError::Budget {
                requested,
                used,
                limit,
            } => {
                let dimensions = [
                    (
                        "GPU payload bytes",
                        requested.gpu_bytes,
                        used.gpu_bytes,
                        limit.gpu_bytes,
                    ),
                    (
                        "CPU preparation bytes",
                        requested.cpu_bytes,
                        used.cpu_bytes,
                        limit.cpu_bytes,
                    ),
                    (
                        "prepared objects",
                        requested.objects,
                        used.objects,
                        limit.objects,
                    ),
                ];
                let (resource, requested, limit) = dimensions
                    .into_iter()
                    .find_map(|(name, requested, used, limit)| {
                        let total = requested.saturating_add(used);
                        (total > limit).then_some((name, total, limit))
                    })
                    .expect("BUG: budget rejection must exceed a resource limit");
                Self::PreparedResourceLimit {
                    resource,
                    requested,
                    limit,
                }
            }
            DomainError::ForeignOwner => Self::DeviceDomainMismatch,
            DomainError::FrameSubmissionBudget { limit } => Self::PreparedResourceLimit {
                resource: "frame submissions",
                requested: limit.saturating_add(1),
                limit,
            },
            DomainError::Unavailable => Self::GpuUnavailable,
            DomainError::FrameAlreadyActive => Self::FrameAlreadyActive,
            DomainError::SubmissionBudget | DomainError::Backpressure => Self::GpuBackpressure,
            DomainError::Poll(source) => Self::GpuProgress { source },
        }
    }
}
