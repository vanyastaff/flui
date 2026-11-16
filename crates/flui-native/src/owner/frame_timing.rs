//! A frame's times on the event clock: the frame request a host stamps when
//! the platform asks for a frame, a platform's own stamp rebased onto the event
//! clock, and the presentation reports a window's next request carries.

use std::sync::Arc;

use flui_foundation::{FrameEpoch, FrameStamp, PresentationAddress, SurfaceGeneration};
use flui_platform::{EventTime, FramePresented, FrameRequest};
use parking_lot::Mutex;

/// A frame request for a backend whose platform reports no frame timestamp: the vsync is this
/// reading of the event clock, and the target present time is unknown (ADR-0347).
#[must_use]
pub fn frame_request_now() -> FrameRequest {
    frame_request_aged(std::time::Duration::ZERO)
}

/// A frame request whose vsync the platform stamped `age` before this reading of the event clock,
/// for a platform that reports its frame time on its own clock (the browser's animation frame
/// timestamp): the age is the one quantity both clocks agree on, so no epoch has to be aligned.
/// An age past the start of the timeline is the start of it.
#[must_use]
pub fn frame_request_aged(age: std::time::Duration) -> FrameRequest {
    frame_request_timed(age, None)
}

/// [`frame_request_aged`] for a platform that also knows when the frame will next reach the
/// screen, `until_present` after this reading.
#[must_use]
pub fn frame_request_timed(
    age: std::time::Duration,
    until_present: Option<std::time::Duration>,
) -> FrameRequest {
    frame_request_at(EventTime::now().as_nanos(), age, until_present)
}

fn frame_request_at(
    now_nanos: u64,
    age: std::time::Duration,
    until_present: Option<std::time::Duration>,
) -> FrameRequest {
    let nanos = |d: std::time::Duration| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX);
    let request = FrameRequest::new(flui_platform::EventTime::from_nanos(
        now_nanos.saturating_sub(nanos(age)),
    ));
    match until_present {
        Some(wait) => request.target_present(flui_platform::EventTime::from_nanos(
            now_nanos.saturating_add(nanos(wait)),
        )),
        None => request,
    }
}

/// The event-clock time of a stamp the platform took on its own monotonic clock, given a reading
/// of that clock taken now (`platform_now`): the stamp lies as far before or after this reading of
/// the event clock as it lies from `platform_now`, so no epoch has to be aligned. A stamp further
/// back than the timeline's start is its start.
#[cfg(any(windows, wayland_host, target_os = "android"))]
#[must_use]
pub(crate) fn rebased(platform_now: std::time::Duration, stamp: std::time::Duration) -> EventTime {
    rebased_at(EventTime::now().as_nanos(), platform_now, stamp)
}

#[cfg(any(
    windows,
    wayland_host,
    target_os = "macos",
    target_os = "android",
    test
))]
pub(crate) fn rebased_at(
    event_now: u64,
    platform_now: std::time::Duration,
    stamp: std::time::Duration,
) -> EventTime {
    let nanos = |d: std::time::Duration| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX);
    // One of the two is zero: the stamp lies before the reading or after it.
    let (before, after) = (
        platform_now.saturating_sub(stamp),
        stamp.saturating_sub(platform_now),
    );
    EventTime::from_nanos(
        event_now
            .saturating_sub(nanos(before))
            .saturating_add(nanos(after)),
    )
}

/// What a window's platform reported about frames that reached the screen, kept until a frame
/// request of the window carries it (`FrameRequest::presented`).
///
/// Any thread reports: the presenting thread (a swap chain's statistics), the platform's own
/// callback thread (Metal's presented handler) or the owner (a compositor's event). Each report
/// names the frame it describes (`FramePresented::frame`), and that name decides what it is:
///
/// - a frame of a presentation or a surface generation other than those of the frame the window's
///   newest present carried ([`presenting`](Self::presenting)) belongs to what the window no
///   longer shows, and the report is dropped; a window that presented nothing reports nothing. So
///   a report that arrives late, after the window moved to another presentation or its surface
///   was replaced, is refused by the identity its frame carries, whatever order the reports of
///   the two surfaces arrive in;
/// - a frame no newer than the frame of a report kept or already carried (a repeat, or a report
///   that arrived after a later frame's) is dropped, and so is a frame shown before the time of
///   such a report, so neither the frames nor the times a window's requests carry run backwards;
/// - the newest report wins.
///
/// A stamp ahead of the event clock's reading when the report arrives is measured with error,
/// and how far decides what it is:
///
/// - within [`REPORT_LEAD`] it is the frame's time as the platform measured it (the two readings
///   a rebase takes, a compositor that adds its display's latency), and the report is carried
///   with that stamp: the stamp is the phase of the display's refresh grid, which the frame clock
///   plans on, and a report held back until the clock reached it would be replaced by the next
///   one, so a host whose every stamp leads would report nothing while it presents;
/// - further ahead it is no time of this frame (a stamp left on the platform's own epoch, a
///   rebase saturated at the timeline's end), and the report is dropped.
///
/// So the next request carries the newest report that arrived before it, with its own stamp.
#[derive(Clone, Debug, Default)]
pub(crate) struct PresentationReports(Arc<Mutex<Reported>>);

/// How far ahead of the event clock's reading a report's stamp may lie and still be the time of
/// a frame: milliseconds separate the two for a rebase's error or a display's latency, hours or
/// more for a stamp on another clock. A second lies between the two.
const REPORT_LEAD: std::time::Duration = std::time::Duration::from_secs(1);

#[derive(Debug, Default)]
struct Reported {
    /// The presentation and surface generation of the frame the window's newest present carried.
    presenting: Option<(PresentationAddress, SurfaceGeneration)>,
    /// The newest report no request has carried yet.
    pending: Option<FramePresented>,
    /// The frame and time of the newest report kept or carried, of the presentation the window
    /// presents.
    newest: Option<(FrameEpoch, EventTime)>,
}

/// Why a report was dropped, named by the trace that says so once the lock is released.
#[derive(Clone, Copy, Debug)]
enum Dropped {
    /// Of a presentation or a surface the window does not present now.
    Elsewhere,
    /// Of a frame no newer than one reported, or shown before it.
    Stale,
}

/// The presentation and the surface generation `frame` was drawn for: what a window presents.
fn surface_of(frame: FrameStamp) -> (PresentationAddress, SurfaceGeneration) {
    (frame.address, frame.surface_generation)
}

impl PresentationReports {
    /// The window is about to present `frame`, `None` for a present that shows no frame of the
    /// window. A frame of another presentation or another surface generation than the reports
    /// kept so far were of moves the window to it: what was kept of the old one is forgotten, and
    /// a report of the old one that arrives later is refused by its frame's identity.
    pub(crate) fn presenting(&self, frame: Option<FrameStamp>) {
        let Some(frame) = frame else {
            return;
        };
        let mut reported = self.0.lock();
        if reported.presenting != Some(surface_of(frame)) {
            *reported = Reported {
                presenting: Some(surface_of(frame)),
                ..Reported::default()
            };
        }
    }

    /// The platform showed a frame.
    pub(crate) fn report(&self, presented: FramePresented) {
        self.report_read_at(presented, EventTime::now());
    }

    /// [`Self::report`], arriving when the event clock reads `now`.
    fn report_read_at(&self, presented: FramePresented, now: EventTime) {
        if presented.at.saturating_duration_since(now) > REPORT_LEAD {
            // Outside the lock: a subscriber is user code.
            tracing::debug!(
                target: "flui.present",
                at_ns = presented.at.as_nanos(),
                now_ns = now.as_nanos(),
                "a presentation stamped far ahead of the event clock is dropped"
            );
            return;
        }
        let dropped = {
            let mut reported = self.0.lock();
            if reported.presenting != Some(surface_of(presented.frame)) {
                Some(Dropped::Elsewhere)
            } else if reported
                .newest
                .is_some_and(|(epoch, at)| presented.frame.epoch <= epoch || presented.at < at)
            {
                Some(Dropped::Stale)
            } else {
                reported.pending = Some(presented);
                reported.newest = Some((presented.frame.epoch, presented.at));
                None
            }
        };
        if let Some(dropped) = dropped {
            tracing::trace!(
                target: "flui.present",
                epoch = presented.frame.epoch.get(),
                at_ns = presented.at.as_nanos(),
                ?dropped,
                "a presentation report is dropped"
            );
        }
    }

    /// `request`, carrying the newest report no request carried yet.
    pub(crate) fn carried_by(&self, request: FrameRequest) -> FrameRequest {
        let presented = self.0.lock().pending.take();
        match presented {
            Some(presented) => request.presented(presented),
            None => request,
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    /// A platform stamp lands as far from the event reading as it lies from the platform's own
    /// reading, on either side, and never before the timeline's start.
    #[test]
    fn a_platform_stamp_is_rebased_by_its_distance_from_the_reading() {
        let ms = std::time::Duration::from_millis;
        let at = |platform_now, stamp| rebased_at(50_000_000, platform_now, stamp).as_nanos();
        assert_eq!(at(ms(900), ms(880)), 30_000_000, "20 ms before");
        assert_eq!(at(ms(900), ms(905)), 55_000_000, "5 ms after");
        assert_eq!(at(ms(900), ms(900)), 50_000_000);
        assert_eq!(at(ms(900), ms(0)), 0, "before the timeline's start");
        assert_eq!(
            rebased_at(u64::MAX - 1, ms(0), ms(1)).as_nanos(),
            u64::MAX,
            "past the timeline's end"
        );
    }
    /// Frame `epoch` of presentation `presentation`.
    fn frame(presentation: usize, epoch: u64) -> FrameStamp {
        FrameStamp::new(
            PresentationAddress {
                ui_runtime_id: flui_foundation::UiRuntimeId::new(1),
                presentation_id: flui_foundation::PresentationId::new(presentation),
            },
            FrameEpoch::from_raw(epoch),
            flui_foundation::SurfaceGeneration::ZERO.next(),
        )
    }

    /// The reports of a window whose newest present carried a frame of presentation 1.
    fn presenting_first() -> PresentationReports {
        let reports = PresentationReports::default();
        reports.presenting(Some(frame(1, 0)));
        reports
    }

    /// The frame and stamp the request after a report carries.
    fn carried(reports: &PresentationReports) -> Option<(u64, u64)> {
        reports
            .carried_by(FrameRequest::new(EventTime::from_nanos(1)))
            .presented
            .map(|presented| (presented.frame.epoch.get(), presented.at.as_nanos()))
    }

    /// The newest report reaches the next request, once, with its own frame and stamp; a repeat,
    /// a report of an older frame and one shown before a report kept do not.
    #[test]
    fn a_presentation_report_rides_the_next_request() {
        let at = EventTime::from_nanos;
        let shown = |epoch, nanos| FramePresented::new(frame(1, epoch), at(nanos));

        let reports = presenting_first();
        assert_eq!(carried(&reports), None, "nothing reported");

        reports.report_read_at(shown(1, 20), at(30));
        reports.report_read_at(shown(3, 30), at(30));
        reports.report_read_at(shown(2, 25), at(30));
        assert_eq!(carried(&reports), Some((3, 30)), "the newest frame");
        assert_eq!(carried(&reports), None, "carried once");

        reports.report_read_at(shown(3, 30), at(40));
        reports.report_read_at(shown(2, 35), at(40));
        reports.report_read_at(shown(4, 29), at(40));
        assert_eq!(
            carried(&reports),
            None,
            "a repeat, an older frame, and a newer frame shown before the one carried"
        );

        reports.report_read_at(shown(5, 30), at(40));
        assert_eq!(
            carried(&reports),
            Some((5, 30)),
            "a later frame shown at the refresh of the one carried: the earlier was replaced"
        );

        reports.report_read_at(shown(6, 45), at(40));
        assert_eq!(
            carried(&reports),
            Some((6, 45)),
            "a stamp ahead of the reading, carried as it is"
        );
    }

    /// A report names the presentation whose frame it was: one of a presentation the window no
    /// longer presents is dropped, and so is any report before the window presented a frame;
    /// moving to another presentation forgets what was kept of the old one.
    #[test]
    fn a_report_of_a_presentation_the_window_left_is_dropped() {
        let at = EventTime::from_nanos;
        let reports = PresentationReports::default();
        reports.report_read_at(FramePresented::new(frame(1, 1), at(10)), at(20));
        assert_eq!(carried(&reports), None, "nothing presented yet");

        reports.presenting(Some(frame(1, 1)));
        reports.presenting(None);
        reports.report_read_at(FramePresented::new(frame(1, 7), at(10)), at(20));
        reports.presenting(Some(frame(2, 1)));
        assert_eq!(carried(&reports), None, "the old presentation's report");

        reports.report_read_at(FramePresented::new(frame(1, 8), at(30)), at(40));
        reports.report_read_at(FramePresented::new(frame(2, 1), at(25)), at(40));
        assert_eq!(
            carried(&reports),
            Some((1, 25)),
            "the new presentation's first frame, though older than the old one's"
        );
    }

    /// A report names the surface its frame was drawn for: once the window presents a frame of a
    /// new surface generation, a report of the old surface's frame is refused, whether it arrives
    /// before or after the new surface's first report.
    #[test]
    fn a_report_of_a_replaced_surface_is_dropped() {
        let at = EventTime::from_nanos;
        let on = |epoch, surface| {
            FrameStamp::new(
                frame(1, epoch).address,
                FrameEpoch::from_raw(epoch),
                SurfaceGeneration::from_raw(surface),
            )
        };
        for (old_first, why) in [
            (true, "the old report first"),
            (false, "the old report last"),
        ] {
            let reports = PresentationReports::default();
            reports.presenting(Some(on(1, 1)));
            reports.presenting(Some(on(2, 2)));
            let old = FramePresented::new(on(1, 1), at(30));
            let new = FramePresented::new(on(2, 2), at(20));
            if old_first {
                reports.report_read_at(old, at(40));
                reports.report_read_at(new, at(40));
            } else {
                reports.report_read_at(new, at(40));
                reports.report_read_at(old, at(40));
            }
            assert_eq!(carried(&reports), Some((2, 20)), "{why}");
        }
    }

    /// A report stamped more than the lead ahead of the reading it arrives at is dropped, so the
    /// reports after it are carried; one at the lead is carried with its stamp.
    #[test]
    fn a_report_stamped_far_ahead_does_not_outrank_the_reports_after_it() {
        const SECOND: u64 = 1_000_000_000;
        let at = EventTime::from_nanos;
        let shown = |epoch, nanos| FramePresented::new(frame(1, epoch), at(nanos));
        let now = 5 * SECOND;

        for (stamp, why) in [
            (u64::MAX, "a rebase that saturated at the timeline's end"),
            (now + 3600 * SECOND, "a stamp on another clock's epoch"),
            (now + SECOND + 1, "just past the lead"),
        ] {
            let reports = presenting_first();
            reports.report_read_at(shown(1, stamp), at(now));
            reports.report_read_at(shown(2, now - 1_000), at(now));
            assert_eq!(carried(&reports), Some((2, now - 1_000)), "{why}");
        }

        let reports = presenting_first();
        reports.report_read_at(shown(1, now + SECOND), at(now));
        assert_eq!(
            carried(&reports),
            Some((1, now + SECOND)),
            "at the lead: carried with its stamp"
        );
    }

    /// A host whose every stamp leads the clock by two refreshes (a compositor that adds its
    /// display's latency) still has each request carry a report, with the stamp the platform
    /// gave: no report waits for the clock while the next one replaces it, and the refresh grid
    /// the frame clock plans on keeps the display's phase.
    #[test]
    fn a_stream_of_stamps_that_lead_the_clock_reaches_every_request() {
        const REFRESH: u64 = 16_666_667;
        let at = EventTime::from_nanos;
        let reports = presenting_first();
        for epoch in 1..=5_u64 {
            let arrives = 1_000_000_000 + epoch * REFRESH;
            let stamp = arrives + 2 * REFRESH;
            reports.report_read_at(FramePresented::new(frame(1, epoch), at(stamp)), at(arrives));
            assert_eq!(carried(&reports), Some((epoch, stamp)), "frame {epoch}");
        }
    }

    /// A platform's frame stamp, given as an age, lands that long before the reading, and an age
    /// longer than the timeline is its start.
    #[test]
    fn an_aged_frame_request_is_stamped_before_the_reading() {
        use std::time::Duration;

        let at = |now, age| frame_request_at(now, age, None).vsync.as_nanos();
        assert_eq!(at(5_000_000, Duration::from_millis(2)), 3_000_000);
        assert_eq!(at(5_000_000, Duration::ZERO), 5_000_000);
        assert_eq!(at(1_000, Duration::from_secs(1)), 0);
        assert_eq!(at(1_000, Duration::MAX), 0);

        let request = frame_request_at(
            5_000_000,
            Duration::from_millis(2),
            Some(Duration::from_millis(3)),
        );
        assert_eq!(request.vsync.as_nanos(), 3_000_000);
        assert_eq!(
            request
                .target_present
                .map(flui_platform::EventTime::as_nanos),
            Some(8_000_000)
        );
    }
}
