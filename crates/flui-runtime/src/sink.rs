//! The frame sink: the seam a UI runtime's frame transaction submits through.
//!
//! A [`FrameSink`] answers the two questions a frame asks of whatever puts it
//! on screen: the surface size layout must use, and what happened to the
//! composited scene it was handed. The answer to the second is a
//! [`SubmitVerdict`], which the UI runtime classifies into retry, device-loss and
//! not-shown handling (ADR-0068). The host implements the trait; this crate
//! names no backend, surface or GPU type.

use flui_foundation::{FrameEpoch, PresentationAddress};
use flui_layer::ScenePublication;

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
///         SubmitVerdict::Queued => false,
///     }
/// }
///
/// assert!(arms_a_retry(SubmitVerdict::SurfaceStale));
/// assert!(!arms_a_retry(SubmitVerdict::Presented));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a host paces or retries from each verdict, and a `_` arm would lose a frame or a retry"
)]
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
    /// as done. The engine's raster executor draws the same
    /// distinction at the slot boundary with its `PresentDisposition`.
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
    /// retries the frame after the dropped-frame backoff and retains its input
    /// epochs (ADR-0267).
    DeviceLost,
    /// The frame failed in a way no retry can fix this frame (a generic
    /// render error, or a refused submit). No retry is armed.
    Failed,
    /// The sink took the frame and settles it later: the raster side answers with a
    /// [`FrameSettlement`] naming the frame's address and epoch, which the host hands to
    /// `UiRuntime::settle_frame`. Until then the runtime keeps what the frame commits, captured
    /// when it was submitted (ADR-0305 §4). A settlement never carries this verdict.
    Queued,
}

/// The verdict of a frame a sink answered [`SubmitVerdict::Queued`], named by the frame's stamp
/// (ADR-0305 §4). The host drains it from the raster side and hands it to
/// `UiRuntime::settle_frame` on the owner thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct FrameSettlement {
    /// The presentation whose frame settles.
    pub address: PresentationAddress,
    /// The epoch the frame was submitted with.
    pub epoch: FrameEpoch,
    /// What became of the frame: any verdict but [`SubmitVerdict::Queued`].
    pub verdict: SubmitVerdict,
}

impl FrameSettlement {
    /// The settlement of `address`'s frame stamped `epoch`.
    #[must_use]
    pub const fn new(
        address: PresentationAddress,
        epoch: FrameEpoch,
        verdict: SubmitVerdict,
    ) -> Self {
        Self {
            address,
            epoch,
            verdict,
        }
    }
}

/// What `UiRuntime::settle_frame` did with a [`FrameSettlement`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a settlement either names a pending frame or does not; there is no third answer"
)]
#[must_use = "a stale settlement is a host that drained another frame's answer"]
pub enum Settled {
    /// The settlement named a pending frame, which committed what its verdict owes.
    Applied,
    /// No pending frame has the settlement's address and epoch (another runtime's, an earlier
    /// life's, one already settled): it was traced and ignored.
    Stale,
}

/// The seam a UI runtime's frame transaction submits through: the surface size
/// layout must use, and the submit itself.
///
/// Implemented by the host: `flui-app`'s raster executor host, and `flui-testing`'s headless
/// sink. Every implementation feeds the
/// same UI runtime-side classification arms via [`SubmitVerdict`], so the
/// retry/telemetry semantics cannot drift between them.
///
/// The trait is object-safe: the UI runtime is generic over its sink today, and
/// will drive it as `&mut dyn FrameSink` through the proposed `Runtime::pump`
/// (ADR-0083).
///
/// # Damage
///
/// A [`ScenePublication`] carries the whole retained scene version and the
/// damage since the version the runtime last heard was shown (ADR-0304 §7).
/// The runtime hears it when `submit` answers [`SubmitVerdict::Presented`] or
/// [`SubmitVerdict::NoPresent`]; any other verdict leaves the damage owed, so
/// the next publication covers this frame's changes too. A sink that keeps an
/// image applies the damage only to an image of the publication's `base` or a
/// later version of the same lineage on a surface of the same size, and draws
/// in full otherwise: `flui_layer::RasterBase` holds that rule.
pub trait FrameSink {
    /// Physical surface size in pixels, as layout's root constraints input.
    fn surface_size(&mut self) -> (u32, u32);

    /// Submit one composited scene for rasterization and classify what
    /// happened.
    ///
    /// `epoch` is the frame's place in its UI runtime's sequence, which the
    /// runtime mints for each frame any of its presentations submits, so the
    /// epochs a sink is handed only grow, whichever presentation produced
    /// them: the host stamps the frame with it, so what the frame becomes is
    /// matched to it by stamp, not by being the latest (ADR-0305 §4).
    fn submit(&mut self, epoch: FrameEpoch, publication: ScenePublication) -> SubmitVerdict;
}
