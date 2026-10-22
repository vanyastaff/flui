//! How often a modal loop asks the owner for frames.
//!
//! While a modal loop stands in for the owner's message loop (OLE's
//! `DoDragDrop`, or `DefWindowProc`'s while a window is moved or sized), the
//! owner delivers the wake deadlines and paints that loop does not
//! (`owner::windows::modal_loop::ModalFrames`), each time the modal loop asks.
//! This is how long it waits between two asks: a pure rule, so every host
//! checks it.

#![forbid(unsafe_code)]

/// The longest a modal loop waits before asking again whether a frame is due:
/// one 60 Hz frame.
pub(crate) const MODAL_FRAME_PACE_MS: u32 = 16;

/// How long a modal loop's frame timer waits: until `deadline`, rounded up to a
/// whole millisecond and at least one, but never longer than
/// [`MODAL_FRAME_PACE_MS`], so a deadline set while it waits is seen within a
/// frame.
#[must_use]
pub(crate) fn modal_frame_wait_ms(
    deadline: Option<web_time::Instant>,
    now: web_time::Instant,
) -> u32 {
    deadline.map_or(MODAL_FRAME_PACE_MS, |deadline| {
        let remaining = deadline.saturating_duration_since(now).as_nanos();
        let millis = remaining.div_ceil(1_000_000);
        u32::try_from(millis.clamp(1, u128::from(MODAL_FRAME_PACE_MS)))
            .unwrap_or(MODAL_FRAME_PACE_MS)
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{MODAL_FRAME_PACE_MS, modal_frame_wait_ms};

    /// The modal loop asks for frames by the next deadline, at least every
    /// frame and never in a busy loop.
    #[test]
    fn the_modal_loop_waits_for_the_next_deadline_within_a_frame() {
        let now = web_time::Instant::now();
        let rows = [
            (None, MODAL_FRAME_PACE_MS),
            (Some(now), 1),
            (Some(now + Duration::from_micros(200)), 1),
            (Some(now + Duration::from_micros(4_200)), 5),
            (Some(now + Duration::from_secs(3)), MODAL_FRAME_PACE_MS),
        ];
        for (deadline, wait) in rows {
            assert_eq!(modal_frame_wait_ms(deadline, now), wait, "{deadline:?}");
        }
        let past = now
            .checked_sub(Duration::from_millis(5))
            .expect("past instant");
        assert_eq!(modal_frame_wait_ms(Some(past), now), 1);
    }
}
