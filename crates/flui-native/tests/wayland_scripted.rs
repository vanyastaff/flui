//! The native Wayland host against a compositor the test scripts
//! (`flui_native::testing::wayland::compositor`): the wire protocol over a
//! socket of the case's own, to the unmodified host, for what no installed
//! compositor does on demand: bounds on a window's size, a scale change while
//! the host runs, a seat too old to tell its repeat, a strict buffer-scale
//! rule on cursors, frame callbacks it never answers, an output removed under
//! a window, a configure storm, and a connection closed in the middle of a
//! request.
//!
//! Its own harness: the cursor theme the host draws from is the harness's own
//! (one written into a private directory and named in the process's
//! environment before any thread starts), so a case knows the size of every
//! image the theme gives. Nothing else is needed: the compositor is a thread
//! of the case, and libwayland-client and libxkbcommon are loaded as the host
//! loads them.

fn main() {
    #[cfg(target_os = "linux")]
    cases::main();
}

#[cfg(target_os = "linux")]
mod cases {
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use flui_foundation::geometry::Size;
    use flui_native::testing::wayland::compositor::{
        Configure, HangUp, KEY_A, Offer, OutputOffer, ScriptedCompositor, Step, ToplevelState,
    };
    use flui_native::testing::wayland::{
        compositor_report, cursor_report, present, run_for,
    };
    use flui_native::{HostWindow, Platform as _, WaylandPlatform};
    use flui_platform::keyboard::{KeyPress, KeyState};
    use flui_platform::{PlatformInput, WindowOptions};
    use flui_testing::window_handler::{HandlerCall, RecordingWindowHandler};

    /// The cursor theme the harness writes and names.
    const THEME: &str = "flui-scripted";

    /// How long a case waits for what it expects before it fails.
    const PATIENCE: Duration = Duration::from_secs(10);

    pub(super) fn main() {
        let theme = tempfile::tempdir().expect("a directory for the cursor theme");
        write_theme(theme.path());
        name_theme(theme.path());
        let arguments = libtest_mimic::Arguments::from_args();
        let trials = [
            (
                "a_window_opens_within_the_compositors_bounds",
                a_window_opens_within_the_compositors_bounds as fn(),
            ),
            (
                "a_theme_cursor_is_drawn_again_when_its_window_changes_scale",
                a_theme_cursor_is_drawn_again_when_its_window_changes_scale,
            ),
            (
                "a_theme_cursor_its_scale_does_not_divide_goes_through_a_viewport",
                a_theme_cursor_its_scale_does_not_divide_goes_through_a_viewport,
            ),
            (
                "a_seat_too_old_to_tell_its_repeat_repeats_a_held_key",
                a_seat_too_old_to_tell_its_repeat_repeats_a_held_key,
            ),
            (
                "a_window_the_compositor_never_draws_keeps_the_loop_turning",
                a_window_the_compositor_never_draws_keeps_the_loop_turning,
            ),
            (
                "an_output_removed_under_a_window_leaves_it_on_the_others",
                an_output_removed_under_a_window_leaves_it_on_the_others,
            ),
            (
                "a_configure_storm_settles_on_the_last_configure",
                a_configure_storm_settles_on_the_last_configure,
            ),
            (
                "a_compositor_that_hangs_up_while_a_window_opens_fails_the_open",
                a_compositor_that_hangs_up_while_a_window_opens_fails_the_open,
            ),
            (
                "a_compositor_that_hangs_up_on_a_frame_ends_the_loop_with_an_error",
                a_compositor_that_hangs_up_on_a_frame_ends_the_loop_with_an_error,
            ),
        ]
        .into_iter()
        .map(|(name, case)| {
            libtest_mimic::Trial::test(name, move || {
                case();
                Ok(())
            })
        })
        .collect();
        let conclusion = libtest_mimic::run(&arguments, trials);
        drop(theme);
        conclusion.exit();
    }

    /// Name `directory`'s theme for the process: its path, its name and its
    /// size at scale one.
    #[expect(
        unsafe_code,
        reason = "the harness names its cursor theme in the environment before any thread starts"
    )]
    fn name_theme(directory: &std::path::Path) {
        // SAFETY: `main` calls this first, before the process starts any
        // thread, so nothing reads or writes the environment meanwhile.
        unsafe {
            std::env::set_var("XCURSOR_PATH", directory);
            std::env::set_var("XCURSOR_THEME", THEME);
            std::env::set_var("XCURSOR_SIZE", "24");
        }
    }

    /// The images of the theme's arrow, as `(nominal size, side)`: a theme
    /// cursor is drawn at the window's scale rounded up, and its theme asked
    /// for 24 times that. At 2 it gives an image of 45 pixels, which 2 does
    /// not divide, as Adwaita's 64 is not divided by 3.
    const IMAGES: [(u32, u32); 3] = [(24, 24), (48, 45), (72, 72)];

    /// Write the theme: its arrow, under every name the default cursor goes
    /// by, as an Xcursor file of [`IMAGES`].
    fn write_theme(directory: &std::path::Path) {
        let cursors = directory.join(THEME).join("cursors");
        std::fs::create_dir_all(&cursors).expect("the theme's directory is made");
        let file = xcursor(&IMAGES);
        for name in ["default", "left_ptr", "arrow"] {
            std::fs::write(cursors.join(name), &file).expect("the theme's cursor is written");
        }
    }

    /// An Xcursor file of one square, opaque image per `(nominal, side)`,
    /// its hotspot in the middle.
    fn xcursor(images: &[(u32, u32)]) -> Vec<u8> {
        const IMAGE: u32 = 0xfffd_0002;
        const FILE_HEADER: u32 = 16;
        const IMAGE_HEADER: u32 = 36;
        let count = u32::try_from(images.len()).expect("a few images");
        let mut file = Vec::new();
        let word = |file: &mut Vec<u8>, value: u32| file.extend(value.to_le_bytes());
        file.extend(b"Xcur");
        word(&mut file, FILE_HEADER);
        word(&mut file, 0x0001_0000);
        word(&mut file, count);
        let mut position = FILE_HEADER + count * 12;
        for &(nominal, side) in images {
            word(&mut file, IMAGE);
            word(&mut file, nominal);
            word(&mut file, position);
            position += IMAGE_HEADER + side * side * 4;
        }
        for &(nominal, side) in images {
            for value in [IMAGE_HEADER, IMAGE, nominal, 1, side, side, side / 2, side / 2, 0] {
                word(&mut file, value);
            }
            for _ in 0..side * side {
                word(&mut file, 0xff00_0000);
            }
        }
        file
    }

    /// A compositor, the host on it, and one window.
    struct Session {
        window: Rc<dyn HostWindow>,
        handler: Rc<RecordingWindowHandler>,
        platform: WaylandPlatform,
        compositor: ScriptedCompositor,
    }

    impl Session {
        /// A compositor offering `offer`, the host on it, and a window of
        /// `options` on that.
        fn open(offer: Offer, options: WindowOptions) -> Self {
            let compositor = ScriptedCompositor::start(offer).expect("the compositor starts");
            let platform = WaylandPlatform::try_new_at(compositor.socket())
                .expect("the host connects to the compositor");
            let handler = RecordingWindowHandler::new();
            let window = platform
                .open_window(options, Rc::clone(&handler) as _)
                .expect("a window opens");
            Self {
                window,
                handler,
                platform,
                compositor,
            }
        }

        /// Turn the loop, presenting a frame on each turn as a renderer
        /// would, until `done` holds, bounded at [`PATIENCE`].
        fn turn_until(&self, what: &str, done: impl Fn(&Self) -> bool) {
            let deadline = Instant::now() + PATIENCE;
            loop {
                run_for(&self.platform, Duration::from_millis(5)).expect("the loop turns");
                self.draw();
                if done(self) {
                    return;
                }
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for {what}; the compositor heard {:?}",
                    self.compositor.heard()
                );
            }
        }

        /// Present one frame, as the raster side does.
        fn draw(&self) {
            self.window
                .surface_target()
                .pre_present_notify(None, flui_platform::SwapChainStatistics::None);
            let _ = present(self.window.as_ref());
        }

        /// The window's scale, as its snapshot publishes it.
        fn scale(&self) -> Option<f64> {
            self.window
                .state()
                .metrics
                .map(|metrics| metrics.scale().get())
        }

        /// The cursor's scale as the host last showed it.
        fn cursor_scale(&self) -> Option<i32> {
            cursor_report(&self.platform).map(|cursor| cursor.scale)
        }

        /// The keys the window heard, in order.
        fn keys(&self) -> Vec<KeyState> {
            self.handler
                .calls()
                .into_iter()
                .filter_map(|call| match call {
                    HandlerCall::Input(PlatformInput::Keyboard(key)) => Some(key.state),
                    _ => None,
                })
                .collect()
        }
    }

    /// A window asked for larger than the compositor's bounds opens within
    /// them: the configure that leaves the size to it comes after
    /// `configure_bounds` (`xdg_wm_base` version 4).
    fn a_window_opens_within_the_compositors_bounds() {
        let offer = Offer::default().shell(4).first_configure(Some(
            Configure::default()
                .bounds(1366, 740)
                .state(ToplevelState::Activated),
        ));
        let session = Session::open(
            offer,
            WindowOptions::default()
                .title("bounded")
                .size(Size::new(1600.0, 1000.0)),
        );
        session.turn_until("the window's first frame", |session| {
            session.compositor.heard().frames > 0
        });
        assert_eq!(
            session.window.state().content_size.get(),
            Size::new(1366.0, 740.0),
            "the window opened within the bounds"
        );
        assert_eq!(
            session.compositor.heard().buffer,
            Some((1366, 740)),
            "its first frame is the bounded size"
        );
    }

    /// A theme cursor over a window is drawn again when the window's scale
    /// changes, for the new scale: at 1 the theme's 24-pixel image, at 3
    /// its 72-pixel one at buffer scale 3.
    fn a_theme_cursor_is_drawn_again_when_its_window_changes_scale() {
        let session = Session::open(
            Offer::default().seat(7),
            WindowOptions::default().title("a cursor"),
        );
        session.turn_until("the window's first frame", |session| {
            session.compositor.heard().frames > 0
        });
        session
            .compositor
            .play(Step::PointerEnter { x: 20.0, y: 20.0 });
        session.turn_until("a cursor at scale 1", |session| {
            session.cursor_scale() == Some(1)
        });
        session.compositor.play(Step::PreferBufferScale(3));
        session.turn_until("a cursor at scale 3", |session| {
            session.cursor_scale() == Some(3)
        });
        let cursor = session.compositor.heard().cursor;
        let last = cursor.last().expect("a cursor buffer was committed");
        assert_eq!(
            (last.size, last.scale, last.destination),
            ((72, 72), 3, None),
            "the cursor drawn again for scale 3; every cursor buffer: {cursor:?}"
        );
        assert_eq!(cursor[0].size, (24, 24), "first drawn for scale 1");
    }

    /// A theme cursor whose image its scale does not divide (45 pixels at
    /// 2) goes on its surface through a viewport at buffer scale one, so a
    /// compositor that holds cursor buffers to the buffer-scale rule
    /// (`wl_surface.invalid_size`, which ends the connection) keeps the
    /// connection open.
    fn a_theme_cursor_its_scale_does_not_divide_goes_through_a_viewport() {
        let session = Session::open(
            Offer::default().seat(7),
            WindowOptions::default().title("an odd cursor"),
        );
        session.compositor.play(Step::PreferBufferScale(2));
        session.turn_until("the window's first frame at scale 2", |session| {
            session.compositor.heard().frames > 0 && session.scale() == Some(2.0)
        });
        session
            .compositor
            .play(Step::PointerEnter { x: 20.0, y: 20.0 });
        session.turn_until("a cursor at scale 2", |session| {
            session.cursor_scale() == Some(2)
        });
        let heard = session.compositor.heard();
        assert_eq!(heard.errors, [], "the compositor posted no protocol error");
        let last = heard.cursor.last().expect("a cursor buffer was committed");
        assert_eq!(
            (last.size, last.scale, last.destination),
            ((45, 45), 1, Some((23, 23))),
            "the 45-pixel image at buffer scale one through a 23-point viewport"
        );
        run_for(&session.platform, Duration::from_millis(50))
            .expect("the connection is still open");
    }

    /// A held key repeats on a seat older than `repeat_info` (version 4),
    /// which never tells the repeat: at 25 a second after 600 ms.
    fn a_seat_too_old_to_tell_its_repeat_repeats_a_held_key() {
        let session = Session::open(
            Offer::default().seat(3),
            WindowOptions::default().title("an old seat"),
        );
        session.turn_until("the window's first frame", |session| {
            session.compositor.heard().frames > 0
        });
        session.compositor.play(Step::KeyboardEnter);
        session.compositor.play(Step::Key {
            code: KEY_A,
            down: true,
        });
        session.turn_until("the press", |session| {
            session.keys().contains(&KeyState::Down(KeyPress::First))
        });
        let pressed = Instant::now();
        let repeats = |session: &Session| {
            session
                .keys()
                .iter()
                .filter(|key| **key == KeyState::Down(KeyPress::AutoRepeat))
                .count()
        };
        session.turn_until("the first repeat", |session| repeats(session) > 0);
        let first = pressed.elapsed();
        let held = Duration::from_millis(1_600);
        while pressed.elapsed() < held {
            run_for(&session.platform, Duration::from_millis(5)).expect("the loop turns");
        }
        session.compositor.play(Step::Key {
            code: KEY_A,
            down: false,
        });
        session.turn_until("the release", |session| {
            session.keys().contains(&KeyState::Up)
        });
        assert!(
            first >= Duration::from_millis(550),
            "the first repeat came {first:?} after the press, before the 600 ms delay"
        );
        // 1 000 ms of repeats at 25 a second, give or take the turns.
        let count = repeats(&session);
        assert!(
            (18..=28).contains(&count),
            "{count} repeats in {held:?} held: not 25 a second after 600 ms"
        );
    }

    /// A window the compositor never draws (it answers no frame callback, as
    /// for a hidden surface) holds the loop for no frame: each turn returns
    /// in its time, and the window is asked for a frame again only once the
    /// callback's grace has run out.
    fn a_window_the_compositor_never_draws_keeps_the_loop_turning() {
        let session = Session::open(
            Offer::default().withhold_frame_callbacks(),
            WindowOptions::default().title("never drawn"),
        );
        let frames = |session: &Session| {
            session
                .handler
                .calls()
                .iter()
                .filter(|call| matches!(call, HandlerCall::FrameRequested(_)))
                .count()
        };
        let started = Instant::now();
        let span = Duration::from_millis(3_500);
        let mut drawn = 0;
        let mut longest = Duration::ZERO;
        while started.elapsed() < span {
            session.window.request_redraw();
            let turn = Instant::now();
            run_for(&session.platform, Duration::from_millis(10)).expect("the loop turns");
            longest = longest.max(turn.elapsed());
            if frames(&session) > drawn {
                drawn = frames(&session);
                session.draw();
            }
        }
        let heard = session.compositor.heard();
        assert!(heard.withheld > 0, "the compositor held back the callbacks");
        assert!(
            longest < Duration::from_millis(500),
            "a turn of 10 ms took {longest:?}"
        );
        // The first frame, then one per second's grace.
        assert!(
            (2..=5).contains(&drawn),
            "{drawn} frames in {span:?} with every callback held back"
        );
        // Once the compositor draws the window, frames come at its pace.
        session.compositor.play(Step::WithholdFrameCallbacks(false));
        let before = frames(&session);
        let deadline = Instant::now() + Duration::from_secs(1);
        while frames(&session) < before + 10 {
            session.window.request_redraw();
            run_for(&session.platform, Duration::from_millis(10)).expect("the loop turns");
            session.draw();
            assert!(
                Instant::now() < deadline,
                "{} frames in a second once the compositor draws",
                frames(&session) - before
            );
        }
    }

    /// An output the compositor removes while the window's surface is on it,
    /// with no `leave`, no longer sets the window's scale or holds it: the
    /// window takes the scale of the output it is still on.
    fn an_output_removed_under_a_window_leaves_it_on_the_others() {
        let offer = Offer::default()
            .compositor(5)
            .outputs([
                OutputOffer::new(1920, 1080, 1),
                OutputOffer::new(2560, 1440, 2),
            ]);
        let session = Session::open(offer, WindowOptions::default().title("two outputs"));
        session.turn_until("the window's first frame", |session| {
            session.compositor.heard().frames > 0
        });
        session.compositor.play(Step::Enter(0));
        session.compositor.play(Step::Enter(1));
        session.turn_until("the window at scale 2, on both outputs", |session| {
            session.scale() == Some(2.0)
                && compositor_report(session.window.as_ref())
                    .is_some_and(|report| report.outputs.len() == 2)
        });
        session.compositor.play(Step::RemoveOutput(1));
        session.turn_until("the window back at scale 1", |session| {
            session.scale() == Some(1.0)
        });
        let report = compositor_report(session.window.as_ref()).expect("a Wayland window");
        assert!(
            !session.window.state().occluded,
            "the window is still on an output; it reported {report:?}"
        );
    }

    /// A burst of configures, each of another size, settles on the last: the
    /// window acknowledges its serial and takes its size, and acknowledges
    /// no serial out of order.
    fn a_configure_storm_settles_on_the_last_configure() {
        const STORM: i32 = 200;
        let session = Session::open(Offer::default(), WindowOptions::default().title("a storm"));
        session.turn_until("the window's first frame", |session| {
            session.compositor.heard().frames > 0
        });
        for step in 1..=STORM {
            session.compositor.play(Step::Configure(
                Configure::default()
                    .size(400 + step, 300 + step)
                    .state(ToplevelState::Activated),
            ));
        }
        let last = Size::new(f64::from(400 + STORM), f64::from(300 + STORM));
        session.turn_until("the last configure's size", |session| {
            session.window.state().content_size.get() == last
                && session.compositor.heard().configures.len()
                    > usize::try_from(STORM).expect("a small storm")
        });
        let heard = session.compositor.heard();
        let sent = heard.configures.last().copied();
        assert_eq!(
            heard.acknowledged.last().copied(),
            sent,
            "the last configure is acknowledged"
        );
        assert!(
            heard.acknowledged.is_sorted(),
            "serials acknowledged in order: {:?}",
            heard.acknowledged
        );
    }

    /// A compositor that closes the connection when the host asks for its
    /// window's toplevel, answering nothing, fails the open with an error,
    /// and the loop reports the lost connection: nothing waits on it.
    fn a_compositor_that_hangs_up_while_a_window_opens_fails_the_open() {
        let compositor = ScriptedCompositor::start(Offer::default().hang_up(HangUp::OnToplevel))
            .expect("the compositor starts");
        let platform = WaylandPlatform::try_new_at(compositor.socket())
            .expect("the host connects to the compositor");
        let started = Instant::now();
        let opened = platform.open_window(
            WindowOptions::default().title("hung up on"),
            RecordingWindowHandler::new() as _,
        );
        assert!(opened.is_err(), "the open failed");
        assert!(
            run_for(&platform, Duration::from_millis(10)).is_err(),
            "the loop reports the lost connection"
        );
        assert!(started.elapsed() < PATIENCE, "nothing waited on the dead peer");
    }

    /// A compositor that closes the connection on the window's first frame
    /// ends the loop with an error, whatever the window still waits for.
    fn a_compositor_that_hangs_up_on_a_frame_ends_the_loop_with_an_error() {
        let session = Session::open(
            Offer::default().hang_up(HangUp::OnFirstFrame),
            WindowOptions::default().title("hung up on at the frame"),
        );
        session.draw();
        let deadline = Instant::now() + PATIENCE;
        loop {
            if run_for(&session.platform, Duration::from_millis(10)).is_err() {
                break;
            }
            assert!(Instant::now() < deadline, "the loop never saw the hang-up");
        }
        assert_eq!(session.compositor.heard().clients, 0, "every connection closed");
    }
}
