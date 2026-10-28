//! A presentation's frame clock keeps the platform's newest report that one of its frames reached
//! the screen, and places the next refresh on the grid that report starts.

use std::time::Duration;

use flui_foundation::{ManualClock, MonotonicClock as _};
use flui_scheduler::{ClockSource, FrameClock, PRESENTATION_GRID_HORIZON, PresentationReport};

const REFRESH: Duration = Duration::from_millis(10);

fn clock() -> (FrameClock, web_time::Instant) {
    let manual = ManualClock::new();
    let start = manual.now();
    (FrameClock::with_source(ClockSource::Manual(manual)), start)
}

/// The next refresh after a reading lies on the report's grid, strictly after the reading; a
/// report still ahead of the reading is itself the next.
fn the_next_refresh_lies_on_the_grid_the_report_starts() {
    let (clock, start) = clock();
    let shown = start + Duration::from_millis(100);
    clock.record_presentation(PresentationReport::new(shown).with_refresh(REFRESH));
    let ms = Duration::from_millis;
    assert_eq!(
        clock.next_presentation_after(shown + ms(25)),
        Some(shown + ms(30))
    );
    assert_eq!(
        clock.next_presentation_after(shown + ms(30)),
        Some(shown + ms(40)),
        "a reading on a refresh names the one after it"
    );
    assert_eq!(clock.next_presentation_after(shown), Some(shown + ms(10)));
    assert_eq!(
        clock.next_presentation_after(start),
        Some(shown),
        "the report lies after the reading"
    );
}

/// Without a report, without an interval in it or past the horizon, no refresh is named.
fn no_refresh_is_named_without_an_interval_or_past_the_horizon() {
    let (clock, start) = clock();
    assert_eq!(clock.next_presentation_after(start), None, "no report");

    clock.record_presentation(PresentationReport::new(start));
    assert_eq!(
        clock.next_presentation_after(start + REFRESH),
        None,
        "no refresh interval"
    );

    let later = start + REFRESH;
    clock.record_presentation(PresentationReport::new(later).with_refresh(REFRESH));
    let last_on_grid = later + REFRESH * PRESENTATION_GRID_HORIZON;
    let just_before = last_on_grid
        .checked_sub(Duration::from_nanos(1))
        .expect("after the start");
    assert_eq!(
        clock.next_presentation_after(just_before),
        Some(last_on_grid)
    );
    assert_eq!(
        clock.next_presentation_after(last_on_grid),
        None,
        "past the horizon"
    );
    assert_eq!(
        PresentationReport::new(later)
            .with_refresh(Duration::ZERO)
            .refresh,
        None,
        "a zero interval is an unknown one"
    );
}

/// The newest report is kept: a repeat or an older one does not replace it.
fn an_older_report_does_not_replace_a_newer_one() {
    let (clock, start) = clock();
    let newer = PresentationReport::new(start + REFRESH * 3).with_refresh(REFRESH);
    clock.record_presentation(newer);
    clock.record_presentation(PresentationReport::new(start + REFRESH));
    clock.record_presentation(PresentationReport::new(newer.at));
    assert_eq!(clock.last_presentation(), Some(newer));
}

#[test]
fn presentation_feedback() {
    crate::run_table(
        "presentation_feedback",
        &[
            (
                "the_next_refresh_lies_on_the_grid_the_report_starts",
                the_next_refresh_lies_on_the_grid_the_report_starts as fn(),
            ),
            (
                "no_refresh_is_named_without_an_interval_or_past_the_horizon",
                no_refresh_is_named_without_an_interval_or_past_the_horizon as fn(),
            ),
            (
                "an_older_report_does_not_replace_a_newer_one",
                an_older_report_does_not_replace_a_newer_one as fn(),
            ),
        ],
    );
}
