//! When the compositor showed a window's commit, as
//! `wp_presentation_feedback.presented` carries it: a time on the clock
//! `wp_presentation.clock_id` named, and the output's refresh interval.
//! The window asks for one feedback per commit it presents (its surface
//! target's `pre_present_notify`) and reports each shown commit on the
//! event timeline; a discarded commit reports nothing.

#![forbid(unsafe_code)]

use std::collections::VecDeque;
use std::time::Duration;

use flui_foundation::{FrameStamp, SurfaceGeneration};
use flui_platform::{CapabilityAbsence, WindowCapabilities, WindowCapability};

/// How many feedbacks a surface keeps the frame of until the compositor
/// answers: more than a compositor holds commits of one surface.
const FEEDBACKS_KEPT: usize = 16;

/// The feedbacks a window's surface asked for, numbered in the order it
/// asked, until the compositor answers each.
///
/// A feedback and a frame callback asked before a present bind to the
/// surface's next commit, whichever present makes it. So a present that
/// failed to commit leaves its feedback to describe the next commit, with
/// another frame. A feedback counts as committed once a frame callback
/// armed with its present or a later one fired (the compositor shows only
/// committed content); when the next present asks, every earlier feedback
/// not confirmed is in doubt, and is forgotten: its answer, whenever it
/// comes, names no frame. A present of another surface generation
/// forgets every feedback of the old surface. Requests are numbered from
/// 1 by a `u64`, and a surface that used every number asks no more.
#[derive(Debug, Default)]
pub(crate) struct CommitFeedbacks {
    /// The number of the newest request.
    newest: u64,
    /// The feedbacks asked and not yet answered or forgotten, oldest first.
    outstanding: VecDeque<(u64, FrameStamp)>,
    /// The newest request a fired frame callback confirmed committed.
    confirmed: u64,
    /// The surface generation of the frames asked for.
    generation: Option<SurfaceGeneration>,
}

impl CommitFeedbacks {
    /// A present of `frame` is about to commit (`None` for a present that
    /// shows no frame): the earlier feedbacks it puts in doubt are
    /// forgotten, and the number the present's feedback carries, `None`
    /// when it asks for none.
    pub(crate) fn request(&mut self, frame: Option<FrameStamp>) -> Option<u64> {
        let confirmed = self.confirmed;
        self.outstanding.retain(|&(number, _)| number <= confirmed);
        if let Some(frame) = frame
            && self.generation != Some(frame.surface_generation)
        {
            self.outstanding.clear();
            self.generation = Some(frame.surface_generation);
        }
        self.newest = self.newest.checked_add(1)?;
        let frame = frame?;
        if self.outstanding.len() == FEEDBACKS_KEPT {
            self.outstanding.pop_front();
        }
        self.outstanding.push_back((self.newest, frame));
        Some(self.newest)
    }

    /// A frame callback is armed with the present about to commit: the
    /// request it confirms when it fires, `None` before any request.
    pub(crate) fn arm(&self) -> Option<u64> {
        (self.newest > 0).then_some(self.newest)
    }

    /// A frame callback armed at request `through` fired: the commits of
    /// that request and every earlier one happened.
    pub(crate) fn confirm(&mut self, through: u64) {
        self.confirmed = self.confirmed.max(through);
    }

    /// Feedback `number` was presented: the frame its present carried,
    /// once; `None` for a feedback forgotten.
    pub(crate) fn presented(&mut self, number: u64) -> Option<FrameStamp> {
        let position = self
            .outstanding
            .iter()
            .position(|&(kept, _)| kept == number)?;
        self.outstanding.remove(position).map(|(_, frame)| frame)
    }

    /// Feedback `number` was discarded: no answer will name its frame.
    pub(crate) fn discarded(&mut self, number: u64) {
        self.outstanding.retain(|&(kept, _)| kept != number);
    }
}

/// The time a `presented` event carries, from its three words: the
/// seconds split into high and low halves, and the nanoseconds.
/// Nanoseconds of a second or more are not a time and give `None`.
pub(crate) fn shown_at(tv_sec_hi: u32, tv_sec_lo: u32, tv_nsec: u32) -> Option<Duration> {
    if tv_nsec >= 1_000_000_000 {
        return None;
    }
    let seconds = (u64::from(tv_sec_hi) << 32) | u64::from(tv_sec_lo);
    Some(Duration::new(seconds, tv_nsec))
}

/// `capabilities` with the presentation report the compositor makes:
/// where it offers no `wp_presentation`, a window learns nothing of when
/// its frames are shown.
pub(crate) const fn reported(
    capabilities: WindowCapabilities,
    offered: bool,
) -> WindowCapabilities {
    let mut capabilities = capabilities.into_parts();
    capabilities.reports_presentation = if offered {
        WindowCapability::Native
    } else {
        WindowCapability::Absent(CapabilityAbsence::NOT_ON_THIS_DEVICE)
    };
    capabilities.into_capabilities()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `presented` event's words make the time it names; nanoseconds
    /// past a second do not.
    #[test]
    fn a_presented_event_names_its_time() {
        assert_eq!(
            shown_at(0, 12, 500_000_000),
            Some(Duration::from_millis(12_500))
        );
        assert_eq!(
            shown_at(1, 0, 0),
            Some(Duration::from_secs(1 << 32)),
            "the high word holds the upper seconds"
        );
        assert_eq!(shown_at(0, 1, 1_000_000_000), None);
    }

    /// A compositor without `wp_presentation` leaves a window without the
    /// report, for want of the device's compositor, not the platform.
    #[test]
    fn the_report_follows_the_compositor() {
        let offered = reported(WindowCapabilities::NONE, true);
        assert_eq!(offered.reports_presentation(), WindowCapability::Native);
        let missing = reported(WindowCapabilities::NONE, false);
        assert_eq!(
            missing.reports_presentation(),
            WindowCapability::Absent(CapabilityAbsence::NOT_ON_THIS_DEVICE)
        );
    }

    fn frame(epoch: u64, surface: u64) -> FrameStamp {
        FrameStamp::new(
            flui_foundation::PresentationAddress {
                ui_runtime_id: flui_foundation::UiRuntimeId::new(1),
                presentation_id: flui_foundation::PresentationId::new(1),
            },
            flui_foundation::FrameEpoch::from_raw(epoch),
            SurfaceGeneration::from_raw(surface),
        )
    }

    /// A feedback reports the frame of the commit it was asked with: one
    /// whose present was never confirmed committed when the next present
    /// asks is forgotten, so it never claims the next commit; one
    /// confirmed is reported once.
    #[test]
    fn a_feedback_whose_present_did_not_commit_reports_nothing() {
        let mut feedbacks = CommitFeedbacks::default();
        let first = feedbacks.request(Some(frame(1, 1))).expect("asked");
        let armed = feedbacks.arm().expect("a callback armed");
        // The present failed: nothing committed, the callback never fired.
        let second = feedbacks.request(Some(frame(2, 1))).expect("asked");
        assert_eq!(
            feedbacks.presented(first),
            None,
            "the failed present's feedback claims the next commit"
        );
        assert_eq!(feedbacks.presented(second), Some(frame(2, 1)));
        assert_eq!(feedbacks.presented(second), None, "reported once");

        // A callback armed with a present confirms its commit: that
        // feedback outlives the next present.
        let third = feedbacks.request(Some(frame(3, 1))).expect("asked");
        let callback = feedbacks.arm().expect("a callback armed");
        assert!(callback > armed);
        feedbacks.confirm(callback);
        let fourth = feedbacks.request(Some(frame(4, 1))).expect("asked");
        assert_eq!(feedbacks.presented(third), Some(frame(3, 1)));
        feedbacks.discarded(fourth);
        assert_eq!(feedbacks.presented(fourth), None, "discarded");
    }

    /// A present of another surface generation forgets every feedback of
    /// the old surface, confirmed or not, and a present that shows no
    /// frame asks for none.
    #[test]
    fn a_new_surface_forgets_the_old_surfaces_feedbacks() {
        let mut feedbacks = CommitFeedbacks::default();
        let old = feedbacks.request(Some(frame(1, 1))).expect("asked");
        let callback = feedbacks.arm().expect("a callback armed");
        feedbacks.confirm(callback);
        let new = feedbacks.request(Some(frame(2, 2))).expect("asked");
        assert_eq!(feedbacks.presented(old), None, "the old surface's");
        assert_eq!(feedbacks.presented(new), Some(frame(2, 2)));
        assert_eq!(feedbacks.request(None), None, "a lent present asks nothing");
    }
}
