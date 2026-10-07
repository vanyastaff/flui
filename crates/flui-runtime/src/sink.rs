//! The frame sink: the seam a UI runtime's frame transaction submits through.
//!
//! A [`FrameSink`] answers the two questions a frame asks of whatever puts it
//! on screen: the surface size layout must use, and what happened to the
//! composited scene it was handed. The answer to the second is a
//! [`SubmitVerdict`], which the UI runtime classifies into retry, device-loss and
//! not-shown handling (ADR-0068). The host implements the trait; this crate
//! names no backend, surface or GPU type.

use flui_layer::Scene;

/// What one submitted frame did, as the UI runtime's frame transaction needs to
/// classify it: the behavioral buckets the UI runtime's submit arms distinguish,
/// produced uniformly by every [`FrameSink`].
///
/// The enum is deliberately exhaustive: adding a variant must make the
/// compiler name the UI runtime's one match site (ADR-0068), and that match lives
/// in another crate, where `#[non_exhaustive]` would force a wildcard arm and
/// silently swallow the new variant.
///
/// ```
/// use flui_runtime::sink::SubmitVerdict;
///
/// // Matched from outside this crate with no wildcard arm: this stops
/// // compiling (E0004) if the enum is ever marked `#[non_exhaustive]`.
/// fn arms_a_retry(verdict: SubmitVerdict) -> bool {
///     match verdict {
///         SubmitVerdict::Presented => false,
///         SubmitVerdict::NoPresent => false,
///         SubmitVerdict::NotShown => false,
///         SubmitVerdict::Retry => true,
///         SubmitVerdict::SurfaceStale => true,
///         SubmitVerdict::DeviceLost => true,
///         SubmitVerdict::Failed => false,
///     }
/// }
///
/// assert!(arms_a_retry(SubmitVerdict::SurfaceStale));
/// assert!(!arms_a_retry(SubmitVerdict::Presented));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitVerdict {
    /// The frame rendered and reached `present()`.
    Presented,
    /// The frame rendered successfully but had nothing to present — the
    /// backend reported no damage — so no vsync block happened and the
    /// caller's no-present fallback pacing applies. The work was genuinely
    /// finished; nothing is left to retry.
    NoPresent,
    /// The frame rendered successfully and then could not be shown: the
    /// backend owed content it had nowhere to put on screen (an occluded
    /// surface, or one its owner had released). Also no vsync block, but
    /// unlike [`Self::NoPresent`] the work was CONSUMED and never reached
    /// the screen — so the caller retains the frame rather than counting it
    /// as done. The engine's `RasterBackend::render_scene` draws the same
    /// distinction at the backend boundary with its `PresentDisposition`.
    NotShown,
    /// Rendering was temporarily deferred. Retry on the ordinary paced frame
    /// wake, retaining input epochs; no surface restamp or device rebuild is owed.
    Retry,
    /// The surface this frame was produced against is gone, outdated, or
    /// misconfigured (surface lost, validation failure, or a stale
    /// [`flui_foundation::SurfaceGeneration`] stamp). A retry against the
    /// reconfigured/restamped surface can succeed, so the caller arms one
    /// and retains the frame's input epochs.
    SurfaceStale,
    /// The GPU device was lost. Recovery is the runner's job; the caller
    /// arms a retry and retains the frame's input epochs.
    DeviceLost,
    /// The frame failed in a way no retry can fix this frame (a generic
    /// render error, or a refused submit). No retry is armed.
    Failed,
}

/// The seam a UI runtime's frame transaction submits through: the surface size
/// layout must use, and the submit itself.
///
/// Implemented by the host: `flui-app`'s raster lane and its direct sink; a
/// headless sink arrives with the UI runtime core. Every implementation feeds the
/// same UI runtime-side classification arms via [`SubmitVerdict`], so the
/// retry/telemetry semantics cannot drift between them.
///
/// The trait is object-safe: the UI runtime is generic over its sink today, and
/// will drive it as `&mut dyn FrameSink` through the proposed `Runtime::pump`
/// (ADR-0083).
///
/// # Damage
///
/// A submitted scene is always the whole frame; the UI runtime computes no damage.
/// A host sink that wants partial repaint owns the comparison: `flui-app`'s
/// raster lane runs a `flui_layer::LayerDiffer` over each scene it is handed
/// and stamps the result on the frame (ADR-0087 §3). That damage is relative
/// to the scene submitted before it, not to the one last presented, so the
/// backend accumulates the damage of every frame it has not yet presented.
pub trait FrameSink {
    /// Physical surface size in pixels, as layout's root constraints input.
    fn surface_size(&mut self) -> (u32, u32);

    /// Submit one composited scene for rasterization and classify what
    /// happened.
    fn submit(&mut self, scene: Scene) -> SubmitVerdict;
}
