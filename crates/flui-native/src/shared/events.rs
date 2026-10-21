//! Event-timestamp epoch and pointer identity shared by the native
//! event-conversion backends (winit, Win32, AppKit).
//!
//! Every backend stamps against the one process epoch,
//! [`flui_foundation::event_epoch`], which the interaction clock also counts
//! from (ADR-0207 §2): timestamps from different backends, and the deadlines
//! the arena arms from them, share one timeline.

use flui_platform_api::pointer::{PointerId, PointerInfo, PointerKind, PointerRole};

/// Nanoseconds since the process event epoch.
#[inline]
pub fn event_timestamp_ns() -> u64 {
    // EventTime is nanoseconds; every producer shares this epoch.
    #[expect(clippy::cast_possible_truncation)] // ~584 years of nanoseconds fit u64
    {
        flui_foundation::event_epoch().elapsed().as_nanos() as u64
    }
}

/// Create a `PointerInfo` for the primary mouse pointer.
#[inline]
#[must_use]
pub fn primary_mouse_info() -> PointerInfo {
    PointerInfo::new(
        PointerId::try_from(1_u64).expect("BUG: primary mouse identity is nonzero"),
        PointerKind::Mouse,
    )
    .with_role(PointerRole::Primary)
}
