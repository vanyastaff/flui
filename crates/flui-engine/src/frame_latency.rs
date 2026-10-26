//! How many frames the swapchain lets the application run ahead of the display.

/// The most frames a window's swapchain holds between the application and the screen
/// (ADR-0257).
///
/// The present mode stays `Fifo`, which blocks the producer when the swapchain is full; this
/// sets how full that is. A backend that does not honour the bound (wgpu on GL) keeps ignoring
/// it.
///
/// # Examples
///
/// ```
/// use flui_engine::FrameLatency;
///
/// assert_eq!(FrameLatency::default(), FrameLatency::Balanced);
/// assert_eq!(FrameLatency::Balanced.frames(), 2);
/// assert_eq!(FrameLatency::Low.frames(), 1);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FrameLatency {
    /// Two frames in flight: a frame whose own work is longer than what is left of the display
    /// period after the previous frame's release still makes the next vsync. Throughput over
    /// input latency, and the measured default.
    #[default]
    Balanced,
    /// One frame in flight: the displayed frame is the newest one, at the cost of missing every
    /// other vsync when a frame's work exceeds the display period.
    Low,
}

impl FrameLatency {
    /// The `desired_maximum_frame_latency` this setting asks the swapchain for.
    #[must_use]
    pub const fn frames(self) -> u32 {
        match self {
            Self::Balanced => 2,
            Self::Low => 1,
        }
    }
}
