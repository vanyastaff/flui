//! What covers a window reaches its presentation's `MediaQuery`: the safe
//! area as `view_padding`, the system gesture regions as
//! `system_gesture_insets`, and a moving keyboard as `view_insets` sampled at
//! each frame's time until it settles, after which no frame is asked for.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::geometry::EdgeInsets;
use flui_platform::{EventTime, InsetCurve, KeyboardInset, WindowInsets};
use flui_testing::{HeadlessHost, HeadlessWindow};
use flui_widgets::{MediaQuery, MediaQueryData, SizedBox};

const FRAME: Duration = Duration::from_millis(16);

/// Records the `MediaQuery` each of its builds reads.
#[derive(Clone, flui_view::prelude::StatelessView)]
struct MediaQueryReader {
    seen: Rc<RefCell<Vec<MediaQueryData>>>,
}

impl flui_view::prelude::StatelessView for MediaQueryReader {
    fn build(
        &self,
        ctx: &dyn flui_view::prelude::BuildContext,
    ) -> impl flui_view::prelude::IntoView {
        self.seen.borrow_mut().push(MediaQuery::of(ctx));
        SizedBox::new(10.0, 10.0)
    }
}

/// A 400 × 800 window showing a reader, after its first frame.
fn reading_host() -> (HeadlessHost, Rc<RefCell<Vec<MediaQueryData>>>) {
    let mut host = HeadlessHost::new(HeadlessWindow::new(400, 800));
    let seen = Rc::new(RefCell::new(Vec::new()));
    host.attach(&MediaQueryReader {
        seen: Rc::clone(&seen),
    })
    .expect("a fresh host has no root yet");
    let _ = host.pump(FRAME);
    (host, seen)
}

/// The instant the next frame's vsync carries: the host's virtual clock.
fn now(host: &HeadlessHost) -> EventTime {
    EventTime::from_nanos(
        u64::try_from(host.clock().elapsed().as_nanos()).expect("a short test's time fits"),
    )
}

fn latest(seen: &Rc<RefCell<Vec<MediaQueryData>>>) -> MediaQueryData {
    seen.borrow().last().cloned().expect("the reader built")
}

/// Each frame of a 250 ms keyboard motion builds with the occlusion the
/// motion names at that frame's vsync, the last at the target; once it
/// settles the window asks for no more frames.
pub(crate) fn a_keyboard_motion_lays_out_each_frame_at_its_time() {
    let (mut host, seen) = reading_host();
    let target = EdgeInsets::new(0.0, 0.0, 336.0, 0.0);
    let keyboard = host.move_keyboard(
        host.primary_window(),
        target,
        Duration::from_millis(250),
        InsetCurve::SystemKeyboard,
    );
    let end = keyboard.settles_at().expect("a moving keyboard settles");

    let mut frames = 0;
    let mut previous = 0.0;
    while now(&host) < end {
        assert!(frames < 32, "the motion settles within its duration");
        let _ = host.pump(FRAME);
        frames += 1;
        let vsync = now(&host);
        let shown = latest(&seen).view_insets;
        assert_eq!(
            shown,
            keyboard.at(vsync),
            "frame {frames} builds with the occlusion at its vsync"
        );
        assert!(
            shown.bottom >= previous,
            "a showing keyboard only rises, frame {frames} read {}",
            shown.bottom
        );
        previous = shown.bottom;
    }
    assert!(frames > 2, "the motion spans several frames, ran {frames}");
    assert_eq!(
        latest(&seen).view_insets,
        target,
        "the last frame is at rest"
    );
    assert_eq!(
        host.time_to_next_frame(),
        None,
        "a settled keyboard asks for no frame"
    );
}

/// A keyboard at rest, the safe area and the system gesture regions each
/// reach the field the `MediaQuery` names for them in the next frame.
pub(crate) fn reported_insets_reach_their_media_query_fields() {
    let (mut host, seen) = reading_host();
    let safe_area = EdgeInsets::new(47.0, 0.0, 34.0, 0.0);
    let keyboard = EdgeInsets::new(0.0, 0.0, 291.0, 0.0);
    let gestures = EdgeInsets::new(0.0, 24.0, 32.0, 24.0);
    host.set_window_insets(
        host.primary_window(),
        WindowInsets::try_new(
            safe_area,
            KeyboardInset::try_at_rest(keyboard).expect("a valid keyboard"),
            gestures,
        )
        .expect("valid insets"),
    );
    let _ = host.pump(FRAME);
    let data = latest(&seen);
    assert_eq!(data.view_padding, safe_area);
    assert_eq!(data.view_insets, keyboard);
    assert_eq!(data.system_gesture_insets, gestures);
    assert_eq!(
        host.time_to_next_frame(),
        None,
        "a keyboard at rest asks for no frame"
    );
}
