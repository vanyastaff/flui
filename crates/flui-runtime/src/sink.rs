//! The frame sink: the seam a realm's frame transaction submits through.
//!
//! A [`FrameSink`] answers the two questions a frame asks of whatever puts it
//! on screen: the surface size layout must use, and what happened to the
//! composited scene it was handed. The answer to the second is a
//! [`SubmitVerdict`], which the realm classifies into retry, device-loss and
//! not-shown handling (ADR-0068). The host implements the trait; this crate
//! names no backend, surface or GPU type.

use flui_layer::Scene;

/// What one submitted frame did, as the realm's frame transaction needs to
/// classify it: the behavioral buckets the realm's submit arms distinguish,
/// produced uniformly by every [`FrameSink`].
///
/// The enum is deliberately exhaustive: adding a variant must make the
/// compiler name the realm's one match site (ADR-0068), and that match lives
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

/// The seam a realm's frame transaction submits through: the surface size
/// layout must use, and the submit itself.
///
/// Implemented by the host: `flui-app`'s raster lane and its direct sink; a
/// headless sink arrives with the realm core. Every implementation feeds the
/// same realm-side classification arms via [`SubmitVerdict`], so the
/// retry/telemetry semantics cannot drift between them.
///
/// The trait is object-safe: a realm drives it as `&mut dyn FrameSink`
/// (ADR-0083 §1).
pub trait FrameSink {
    /// Physical surface size in pixels, as layout's root constraints input.
    fn surface_size(&mut self) -> (u32, u32);

    /// Submit one composited scene for rasterization and classify what
    /// happened.
    fn submit(&mut self, scene: Scene) -> SubmitVerdict;
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use flui_layer::{CanvasLayer, Layer, LayerTree};

    use super::*;

    /// A host sink that reports a fixed size and hands back scripted verdicts.
    struct RecordingSink {
        size: (u32, u32),
        verdicts: VecDeque<SubmitVerdict>,
        submitted: usize,
    }

    impl FrameSink for RecordingSink {
        fn surface_size(&mut self) -> (u32, u32) {
            self.size
        }

        fn submit(&mut self, _scene: Scene) -> SubmitVerdict {
            self.submitted += 1;
            self.verdicts
                .pop_front()
                .expect("BUG: the test scripts one verdict per submit")
        }
    }

    fn empty_scene() -> Scene {
        Scene::new(LayerTree::new(Layer::from(CanvasLayer::new())))
    }

    #[test]
    fn a_host_sink_is_driven_through_dyn_frame_sink() {
        let mut recording = RecordingSink {
            size: (640, 480),
            verdicts: VecDeque::from([SubmitVerdict::Presented, SubmitVerdict::SurfaceStale]),
            submitted: 0,
        };

        // The realm drives its sink as a trait object (ADR-0083 §1); a
        // generic method on the trait would stop this from compiling.
        let sink: &mut dyn FrameSink = &mut recording;
        assert_eq!(sink.surface_size(), (640, 480));
        assert_eq!(sink.submit(empty_scene()), SubmitVerdict::Presented);
        assert_eq!(sink.submit(empty_scene()), SubmitVerdict::SurfaceStale);

        assert_eq!(recording.submitted, 2);
    }
}
