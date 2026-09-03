//! Event-timestamp epoch and pointer identity shared by the native
//! event-conversion backends (winit, Win32, AppKit).
//!
//! Besides removing three per-backend copies, a single process-wide
//! `PROCESS_START` closes a latent skew: each backend used to lazily
//! initialize its own epoch at first use, so in a binary compiling more than
//! one backend, timestamps from different backends measured from different
//! zero points. One shared epoch makes every backend's `time` field directly
//! comparable.

use std::{sync::LazyLock, time::Instant};

use ui_events::pointer::{PointerId, PointerInfo, PointerType};

/// Process-start epoch for monotonic event timestamps.
static PROCESS_START: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Get monotonic timestamp in nanoseconds since process start.
#[inline]
pub fn event_timestamp_ns() -> u64 {
    // Upstream ui-events documents PointerState.time as NANOSECONDS
    // ("u64 nanoseconds real time"); a millisecond stamp here silently
    // broke the unit for every consumer comparing across devices.
    #[expect(clippy::cast_possible_truncation)] // ~584 years of nanoseconds fit u64
    {
        PROCESS_START.elapsed().as_nanos() as u64
    }
}

/// Create a `PointerInfo` for the primary mouse pointer.
#[inline]
#[must_use]
pub fn primary_mouse_info() -> PointerInfo {
    PointerInfo {
        pointer_id: Some(PointerId::PRIMARY),
        pointer_type: PointerType::Mouse,
        persistent_device_id: None,
    }
}
