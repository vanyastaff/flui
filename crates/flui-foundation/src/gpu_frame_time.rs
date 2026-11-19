//! [`GpuFrameTime`]: how long one frame ran on the GPU, as the raster side
//! hands it back to the runtime that produced the frame (ADR-0388).
//!
//! It lives here, beside [`FrameStamp`](crate::FrameStamp), because it
//! crosses the same boundary in the other direction: the engine measures it
//! and the runtime, which has no GPU edge, records it on the frame's
//! telemetry.

use std::time::Duration;

use crate::epoch::FrameEpoch;

/// How long one frame ran on the GPU, read back after the frame finished.
///
/// It arrives after the frame's settlement, at a later frame of the same
/// surface, so it names the frame by its epoch. A device without timestamp
/// queries (WebGL2, some mobile GPUs) times no frame, and a frame the GPU had
/// not answered by then is not timed either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct GpuFrameTime {
    /// The epoch of the frame that was timed.
    pub epoch: FrameEpoch,
    /// The time from the start of the frame's first GPU pass to the end of
    /// its last one.
    pub gpu: Duration,
}

impl GpuFrameTime {
    /// The time `gpu` of the frame stamped `epoch`.
    #[must_use]
    pub const fn new(epoch: FrameEpoch, gpu: Duration) -> Self {
        Self { epoch, gpu }
    }
}
