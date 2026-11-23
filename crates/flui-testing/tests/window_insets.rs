//! What covers a window reaches its presentation's `MediaQuery`: the safe
//! area as `view_padding`, the system gesture regions as
//! `system_gesture_insets`, a docked keyboard as `view_insets` sampled at each
//! frame's time until it settles, after which no frame is asked for, and a
//! floating keyboard's frame beside it, as `MediaQuery::floating_keyboard_of`
//! reads it.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::geometry::{EdgeInsets, Rect};
use flui_platform::{EventTime, InsetCurve, KeyboardPlacement, SoftKeyboard, WindowInsets};
use flui_testing::host::{HeadlessHost, HeadlessWindow};
use flui_view::environment::{MediaQuery, MediaQueryData};
use flui_widgets::SizedBox;

const FRAME: Duration = Duration::from_millis(16);

/// What one build of the reader read: the `MediaQuery`, and the frame of the
/// keyboard floating over the window.
#[derive(Clone)]
struct Read {
    media: MediaQueryData,
    floating_keyboard: Option<Rect>,
}

/// Records what each of its builds reads.
#[derive(Clone, flui_view::prelude::StatelessView)]
struct MediaQueryReader {
    seen: Rc<RefCell<Vec<Read>>>,
}

impl flui_view::prelude::StatelessView for MediaQueryReader {
    fn build(
        &self,
        ctx: &dyn flui_view::prelude::BuildContext,
    ) -> impl flui_view::prelude::IntoView {
        self.seen.borrow_mut().push(Read {
            media: MediaQuery::of(ctx),
            floating_keyboard: MediaQuery::floating_keyboard_of(ctx),
        });
        SizedBox::new(10.0, 10.0)
    }
}

/// A 400 × 800 window showing a reader, after its first frame.
fn reading_host() -> (HeadlessHost, Rc<RefCell<Vec<Read>>>) {
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

fn latest(seen: &Rc<RefCell<Vec<Read>>>) -> Read {
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
        KeyboardPlacement::Docked { inset: target },
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
        let shown = latest(&seen).media.view_insets;
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
        latest(&seen).media.view_insets,
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
            SoftKeyboard::try_at_rest(KeyboardPlacement::Docked { inset: keyboard })
                .expect("a valid keyboard"),
            gestures,
        )
        .expect("valid insets"),
    );
    let _ = host.pump(FRAME);
    let data = latest(&seen).media;
    assert_eq!(data.view_padding, safe_area);
    assert_eq!(data.view_insets, keyboard);
    assert_eq!(data.system_gesture_insets, gestures);
    assert_eq!(
        host.time_to_next_frame(),
        None,
        "a keyboard at rest asks for no frame"
    );
}

/// A keyboard that comes to float over the window publishes its frame as the
/// `MediaQuery`'s floating keyboard and moves its inset to none, so the
/// layout keeps its height; hiding it clears the frame.
pub(crate) fn a_floating_keyboard_reaches_the_media_query_as_a_frame() {
    let (mut host, seen) = reading_host();
    let docked = EdgeInsets::new(0.0, 0.0, 291.0, 0.0);
    host.set_window_insets(
        host.primary_window(),
        WindowInsets::ZERO.with_keyboard(
            SoftKeyboard::try_at_rest(KeyboardPlacement::Docked { inset: docked })
                .expect("a valid keyboard"),
        ),
    );
    let _ = host.pump(FRAME);
    assert_eq!(latest(&seen).media.view_insets, docked);
    assert_eq!(latest(&seen).floating_keyboard, None);

    let frame = Rect::from_xywh(40.0, 300.0, 320.0, 240.0);
    let keyboard = host.move_keyboard(
        host.primary_window(),
        KeyboardPlacement::Floating { frame },
        Duration::from_millis(250),
        InsetCurve::SystemKeyboard,
    );
    let _ = host.pump(FRAME);
    let undocking = latest(&seen);
    assert_eq!(
        undocking.floating_keyboard,
        Some(frame),
        "the frame is published when the keyboard is reported"
    );
    assert_eq!(undocking.media.view_insets, keyboard.at(now(&host)));
    let end = keyboard.settles_at().expect("a moving keyboard settles");
    while now(&host) < end {
        let _ = host.pump(FRAME);
    }
    assert_eq!(
        latest(&seen).media.view_insets,
        EdgeInsets::ZERO,
        "a floating keyboard covers no edge"
    );

    // The user drags it: nothing the `MediaQuery` holds changes, and the
    // frame alone brings the next build.
    let builds = seen.borrow().len();
    let dropped = Rect::from_xywh(60.0, 120.0, 320.0, 240.0);
    host.set_window_insets(
        host.primary_window(),
        WindowInsets::ZERO.with_keyboard(
            SoftKeyboard::try_at_rest(KeyboardPlacement::Floating { frame: dropped })
                .expect("a valid keyboard"),
        ),
    );
    let _ = host.pump(FRAME);
    assert_eq!(latest(&seen).floating_keyboard, Some(dropped));
    assert_eq!(seen.borrow().len(), builds + 1, "one build for the move");

    host.set_window_insets(host.primary_window(), WindowInsets::ZERO);
    let _ = host.pump(FRAME);
    assert_eq!(latest(&seen).floating_keyboard, None);
}

/// A window opened while a keyboard floats over it shows the frame in its
/// first build: the presentation is made from what the window reports as it
/// opens, with no change to hear of.
pub(crate) fn a_window_opened_under_a_floating_keyboard_reads_its_frame() {
    let mut host = HeadlessHost::new(HeadlessWindow::new(400, 800));
    let frame = Rect::from_xywh(40.0, 300.0, 320.0, 240.0);
    let second = host.open_window(
        HeadlessWindow::new(400, 800).with_insets(
            WindowInsets::ZERO.with_keyboard(
                SoftKeyboard::try_at_rest(KeyboardPlacement::Floating { frame })
                    .expect("a valid keyboard"),
            ),
        ),
    );
    let seen = Rc::new(RefCell::new(Vec::new()));
    host.attach_to(
        second,
        &MediaQueryReader {
            seen: Rc::clone(&seen),
        },
    )
    .expect("a fresh window has no root yet");
    let _ = host.pump(FRAME);
    let first = seen.borrow().first().cloned().expect("the reader built");
    assert_eq!(first.floating_keyboard, Some(frame));
    assert_eq!(first.media.view_insets, EdgeInsets::ZERO);
}

/// Records the floating keyboard each of its builds reads, and nothing else.
#[derive(Clone, flui_view::prelude::StatelessView)]
struct FloatingKeyboardReader {
    seen: Rc<RefCell<Vec<Option<Rect>>>>,
}

impl flui_view::prelude::StatelessView for FloatingKeyboardReader {
    fn build(
        &self,
        ctx: &dyn flui_view::prelude::BuildContext,
    ) -> impl flui_view::prelude::IntoView {
        self.seen
            .borrow_mut()
            .push(MediaQuery::floating_keyboard_of(ctx));
        SizedBox::new(10.0, 10.0)
    }
}

/// A widget that reads only the floating keyboard hears it come, move and
/// go: the root publishes the frame only while a keyboard floats, and a
/// widget that found none rebuilds when one starts.
pub(crate) fn a_widget_reading_only_the_floating_keyboard_hears_it_come_and_go() {
    let mut host = HeadlessHost::new(HeadlessWindow::new(400, 800));
    let seen = Rc::new(RefCell::new(Vec::new()));
    host.attach(&FloatingKeyboardReader {
        seen: Rc::clone(&seen),
    })
    .expect("a fresh host has no root yet");
    let _ = host.pump(FRAME);
    let floating_at = |frame: Rect| {
        WindowInsets::ZERO.with_keyboard(
            SoftKeyboard::try_at_rest(KeyboardPlacement::Floating { frame })
                .expect("a valid keyboard"),
        )
    };
    let frame = Rect::from_xywh(40.0, 300.0, 320.0, 240.0);
    let dropped = Rect::from_xywh(60.0, 120.0, 320.0, 240.0);
    for insets in [floating_at(frame), floating_at(dropped), WindowInsets::ZERO] {
        host.set_window_insets(host.primary_window(), insets);
        let _ = host.pump(FRAME);
    }
    assert_eq!(
        *seen.borrow(),
        [None, Some(frame), Some(dropped), None],
        "one build for each change of the frame"
    );
}
