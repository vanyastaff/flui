//! The native Wayland host's own window frame where the compositor draws
//! none: Weston 13, which offers no `xdg-decoration`, with its desktop shell
//! on an X server of the harness's own. The frame is read back from what
//! Weston composited on the X server, and driven through XTest as a user's
//! mouse drives it: a press on the title bar moves the window, a press on a
//! margin resizes it, the buttons maximize and close it, each through the
//! compositor's own grab, which it starts only for the serial of the press.
//!
//! Its own harness: it starts Xvfb and Weston before anything connects and
//! names that Weston for the process (`support/weston_x11.rs`). A run without
//! Xvfb or Weston is skipped, and fails where `FLUI_REQUIRE_WAYLAND` is set.

fn main() {
    #[cfg(target_os = "linux")]
    cases::main();
}

#[cfg(target_os = "linux")]
#[path = "support/weston_x11.rs"]
mod weston_x11;

#[cfg(target_os = "linux")]
mod cases {
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use flui_foundation::geometry::Size;
    use flui_native::testing::wayland::{
        client_frame_report, compositor_report, last_input_serial, present, run_for,
    };
    use flui_native::{HostWindow, Platform as _, WaylandPlatform};
    use flui_platform::{WindowCommand, WindowMode, WindowOptions};
    use flui_testing::window_commands::AdmitAndApply as _;
    use flui_testing::window_handler::{HandlerCall, RecordingWindowHandler};
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::{
        BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT, ConnectionExt as _, ImageFormat,
        MOTION_NOTIFY_EVENT,
    };
    use x11rb::protocol::xtest::ConnectionExt as _;

    use super::weston_x11::Session;
    use super::weston_x11::x11_session::Screen;

    /// Weston's output.
    const OUTPUT: (u16, u16) = (1024, 768);
    /// The content of the windows the cases open.
    const CONTENT: (f64, f64) = (400.0, 300.0);
    /// The title bar's height and its buttons' place, as the frame lays them
    /// out: 24-pixel discs 8 pixels apart from the bar's end, centred on its
    /// height.
    const HEADER: i32 = 37;
    const BUTTON: i32 = 24;
    const GAP: i32 = 8;

    pub(super) fn main() {
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::INFO)
            .with_writer(std::io::stderr)
            .try_init();
        let mut arguments = libtest_mimic::Arguments::from_args();
        // One seat: the cases drive it in turn.
        arguments.test_threads = Some(1);
        let session = (!arguments.list)
            .then(|| Session::start("desktop-shell.so", OUTPUT, "flui-wayland-frame"))
            .flatten()
            .map(std::sync::Arc::new);
        type Case = fn(&Session);
        let cases: [(&str, Case); 3] = [
            (
                "a_window_draws_its_own_frame_where_the_compositor_draws_none",
                a_window_draws_its_own_frame_where_the_compositor_draws_none,
            ),
            (
                "a_press_on_the_frame_moves_and_resizes_with_its_serial",
                a_press_on_the_frame_moves_and_resizes_with_its_serial,
            ),
            (
                "the_frame_buttons_maximize_and_close_the_window",
                the_frame_buttons_maximize_and_close_the_window,
            ),
        ];
        let trials = cases
            .into_iter()
            .map(|(name, case)| {
                let session = session.clone();
                libtest_mimic::Trial::test(name, move || {
                    match session.as_deref() {
                        Some(session) => case(session),
                        None => eprintln!("skipped: no Xvfb or Weston to run against"),
                    }
                    Ok(())
                })
            })
            .collect();
        libtest_mimic::run(&arguments, trials).exit();
    }

    /// A window, its platform and what its handler heard.
    struct Opened {
        platform: WaylandPlatform,
        window: Rc<dyn HostWindow>,
        handler: Rc<RecordingWindowHandler>,
    }

    impl Opened {
        fn new(title: &str) -> Self {
            let platform = WaylandPlatform::try_new().expect("the harness's Weston answers");
            let handler = RecordingWindowHandler::new();
            let window = platform
                .open_window(
                    WindowOptions::default()
                        .with_title(title)
                        .with_size(Size::new(CONTENT.0, CONTENT.1)),
                    Rc::clone(&handler) as _,
                )
                .expect("a Wayland window opens");
            let opened = Self {
                platform,
                window,
                handler,
            };
            opened.turn_until("the window on the output", |opened| {
                opened.window.state().visible
            });
            opened
        }

        /// Turn the loop for `span`, drawing a frame each turn as a renderer
        /// would.
        fn turn(&self, span: Duration) {
            let end = Instant::now() + span;
            while Instant::now() < end {
                run_for(&self.platform, Duration::from_millis(10)).expect("the loop turns");
                self.window
                    .surface_target()
                    .pre_present_notify(flui_platform::SwapChainStatistics::None);
                let _ = present(self.window.as_ref());
            }
        }

        /// Turn until `done` holds, bounded at 10 s.
        fn turn_until(&self, what: &str, done: impl Fn(&Self) -> bool) {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !done(self) {
                assert!(Instant::now() < deadline, "timed out waiting for {what}");
                self.turn(Duration::from_millis(20));
            }
        }
    }

    /// What Weston shows on its output, read off the X server.
    struct Shot {
        width: i32,
        height: i32,
        /// Four bytes a pixel: blue, green, red, unused.
        data: Vec<u8>,
    }

    impl Shot {
        fn take(screen: &Screen) -> Self {
            let image = screen
                .connection
                .get_image(
                    ImageFormat::Z_PIXMAP,
                    screen.root,
                    screen.origin.0,
                    screen.origin.1,
                    OUTPUT.0,
                    OUTPUT.1,
                    !0,
                )
                .expect("request")
                .reply()
                .expect("reply");
            Self {
                width: i32::from(OUTPUT.0),
                height: i32::from(OUTPUT.1),
                data: image.data,
            }
        }

        /// The pixel at (`x`, `y`) as `[r, g, b]`; black off the output.
        fn rgb(&self, x: i32, y: i32) -> [u8; 3] {
            if x < 0 || y < 0 || x >= self.width || y >= self.height {
                return [0, 0, 0];
            }
            let at = usize::try_from((y * self.width + x) * 4).expect("on the output");
            [self.data[at + 2], self.data[at + 1], self.data[at]]
        }

        /// Whether the pixel at (`x`, `y`) is a title bar's background,
        /// active or not.
        fn is_bar(&self, x: i32, y: i32) -> bool {
            matches!(self.rgb(x, y), [235, 235, 235] | [250, 250, 250])
        }

        /// The longest run of title bar background on row `y`: its start and
        /// length.
        fn bar_run(&self, y: i32) -> (i32, i32) {
            let (mut best, mut start, mut length) = ((0, 0), 0, 0);
            for x in 0..self.width {
                if self.is_bar(x, y) {
                    if length == 0 {
                        start = x;
                    }
                    length += 1;
                    if length > best.1 {
                        best = (start, length);
                    }
                } else {
                    length = 0;
                }
            }
            best
        }

        /// The top left corner of a title bar at least `width` wide: the
        /// topmost row of bar background nearly that wide (its corners are
        /// rounded), and the left end of the run three rows below, above the
        /// buttons and a cursor pressed lower on the bar, where the rounding
        /// leaves two pixels.
        fn title_bar_of(&self, width: i32) -> Option<(i32, i32)> {
            let top = (0..self.height).find(|y| self.bar_run(*y).1 >= width - 20)?;
            let (start, _) = self.bar_run(top + 3);
            Some((start - 2, top))
        }

        /// The top left corner of the cases' windows' title bar.
        fn title_bar(&self) -> Option<(i32, i32)> {
            self.title_bar_of(CONTENT.0 as i32)
        }
    }

    /// Drive Weston's seat through XTest.
    struct Mouse<'a> {
        screen: &'a Screen,
    }

    impl Mouse<'_> {
        fn fake(&self, kind: u8, detail: u8, at: (i32, i32)) {
            let x = i16::try_from(at.0 + i32::from(self.screen.origin.0)).expect("on screen");
            let y = i16::try_from(at.1 + i32::from(self.screen.origin.1)).expect("on screen");
            self.screen
                .connection
                .xtest_fake_input(kind, detail, x11rb::CURRENT_TIME, self.screen.root, x, y, 0)
                .expect("request");
            self.screen.connection.flush().expect("flush");
        }

        fn move_to(&self, at: (i32, i32)) {
            self.fake(MOTION_NOTIFY_EVENT, 0, at);
        }

        fn press(&self, button: u8) {
            self.fake(BUTTON_PRESS_EVENT, button, (0, 0));
        }

        fn release(&self, button: u8) {
            self.fake(BUTTON_RELEASE_EVENT, button, (0, 0));
        }
    }

    /// Turn the loop, reading where Weston shows a title bar, until `done`
    /// holds of it or 10 s pass; where it shows one then, or none.
    fn watch_bar(
        opened: &Opened,
        screen: &Screen,
        done: impl Fn(Option<(i32, i32)>) -> bool,
    ) -> Option<(i32, i32)> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            opened.turn(Duration::from_millis(30));
            let found = Shot::take(screen).title_bar();
            if done(found) || Instant::now() > deadline {
                return found;
            }
        }
    }

    /// Weston draws no frame: the window draws its own above its content, a
    /// title bar of its content's width with its title and three buttons,
    /// which Weston composites; turned off, it is gone, and back on, it
    /// returns.
    fn a_window_draws_its_own_frame_where_the_compositor_draws_none(session: &Session) {
        let opened = Opened::new("client frame");
        let screen = session.screen();
        let state = opened.window.state();
        assert!(
            state.decorated,
            "a decorated window on a compositor that draws no frame"
        );
        assert_eq!(state.outer.size.height, CONTENT.1 + f64::from(HEADER));
        let report = client_frame_report(opened.window.as_ref()).expect("a Wayland window");
        assert!(report.has_header(), "{report:?}");
        assert_eq!(
            report.geometry,
            Some((
                0.0,
                -f64::from(HEADER),
                CONTENT.0,
                CONTENT.1 + f64::from(HEADER)
            ))
        );

        let deadline = Instant::now() + Duration::from_secs(10);
        let (shot, (left, top)) = loop {
            opened.turn(Duration::from_millis(30));
            let shot = Shot::take(&screen);
            if let Some(bar) = shot.title_bar() {
                break (shot, bar);
            }
            assert!(Instant::now() < deadline, "Weston shows no title bar");
        };
        eprintln!("the title bar is at ({left}, {top})");
        let width = CONTENT.0 as i32;
        // As wide as the content, and as tall as the bar: the bar's
        // background in a column clear of the title and the buttons, down to
        // the line along its foot.
        let (_, run) = shot.bar_run(top + 3);
        assert_eq!(run, width - 4, "the bar's width, less its rounding");
        assert!(
            shot.is_bar(left + width - 3, top + 3),
            "the bar reaches the content's width"
        );
        assert!(!shot.is_bar(left + width + 3, top + 3), "and no further");
        for y in top..top + HEADER - 1 {
            assert!(shot.is_bar(left + 20, y), "the bar at row {}", y - top);
        }
        assert!(
            !shot.is_bar(left + 20, top + HEADER),
            "the content below the bar"
        );
        // The title, dark on the bar, centred.
        let inked = (top + 8..top + 30)
            .flat_map(|y| (left + 120..left + width - 120).map(move |x| (x, y)))
            .filter(|(x, y)| shot.rgb(*x, *y)[0] < 128)
            .count();
        assert!(inked > 40, "the title's ink: {inked} pixels");
        // Each button's disc, beside its icon, is darker than the bar.
        for index in 0..3 {
            let centre = (
                left + width - (GAP + BUTTON) * (index + 1) + BUTTON / 2,
                top + HEADER / 2,
            );
            let disc = shot.rgb(centre.0 - 9, centre.1);
            assert!(
                disc[0] < 235 && disc[0] > 160,
                "button {index}'s disc at {centre:?}: {disc:?}"
            );
        }

        opened
            .window
            .admit_and_apply(WindowCommand::SetDecorated(false))
            .expect("the frame is turned off");
        assert_eq!(
            watch_bar(&opened, &screen, |found| found.is_none()),
            None,
            "the frame is gone"
        );
        assert!(!opened.window.state().decorated);
        assert_eq!(opened.window.state().outer.size.height, CONTENT.1);
        opened
            .window
            .admit_and_apply(WindowCommand::SetDecorated(true))
            .expect("the frame is turned on");
        assert!(
            watch_bar(&opened, &screen, |found| found.is_some()).is_some(),
            "the frame is back"
        );
        opened.window.close_for_cleanup();
        opened.turn(Duration::from_millis(50));
    }

    /// A press on the title bar moves the window and a press on its corner
    /// margin resizes it, each through the compositor's grab, which Weston
    /// starts only for the serial of the press it names: the window then
    /// shows where the pointer took it, at the size the pointer gave it.
    fn a_press_on_the_frame_moves_and_resizes_with_its_serial(session: &Session) {
        let opened = Opened::new("moved and resized");
        let screen = session.screen();
        let mouse = Mouse { screen: &screen };
        let (left, top) =
            watch_bar(&opened, &screen, |found| found.is_some()).expect("a title bar");

        // A drag on the title bar, away from the title and the buttons.
        let grab = (left + 30, top + 10);
        mouse.move_to(grab);
        opened.turn(Duration::from_millis(100));
        mouse.press(1);
        opened.turn(Duration::from_millis(100));
        let pressed = last_input_serial(&opened.platform).expect("the press is input");
        for step in 1..=10 {
            mouse.move_to((grab.0 + 8 * step, grab.1 + 5 * step));
            opened.turn(Duration::from_millis(30));
        }
        mouse.release(1);
        let moved = watch_bar(&opened, &screen, |found| {
            found == Some((left + 80, top + 50))
        });
        assert_eq!(
            moved,
            Some((left + 80, top + 50)),
            "the window follows the drag of its title bar"
        );
        let report = client_frame_report(opened.window.as_ref()).expect("a Wayland window");
        assert!(
            report.requests.contains(&("move", pressed)),
            "a move with the press's serial {pressed}: {:?}",
            report.requests
        );

        // A drag on the margin past the content's lower right corner.
        let (left, top) = (left + 80, top + 50);
        let corner = (
            left + CONTENT.0 as i32 + 4,
            top + HEADER + CONTENT.1 as i32 + 4,
        );
        mouse.move_to(corner);
        opened.turn(Duration::from_millis(100));
        mouse.press(1);
        opened.turn(Duration::from_millis(100));
        let pressed = last_input_serial(&opened.platform).expect("the press is input");
        for step in 1..=10 {
            mouse.move_to((corner.0 + 6 * step, corner.1 + 4 * step));
            opened.turn(Duration::from_millis(30));
        }
        mouse.release(1);
        opened.turn_until("the window at its new size", |opened| {
            let size = opened.window.state().content_size;
            (size.width - (CONTENT.0 + 60.0)).abs() <= 1.0
                && (size.height - (CONTENT.1 + 40.0)).abs() <= 1.0
        });
        let report = client_frame_report(opened.window.as_ref()).expect("a Wayland window");
        assert!(
            report.requests.contains(&("resize", pressed)),
            "a resize with the press's serial {pressed}: {:?}",
            report.requests
        );
        opened.window.close_for_cleanup();
        opened.turn(Duration::from_millis(50));
    }

    /// The maximize button maximizes the window, whose frame keeps its title
    /// bar and loses its margins, and the close button asks the handler,
    /// which closes it.
    fn the_frame_buttons_maximize_and_close_the_window(session: &Session) {
        let opened = Opened::new("buttons");
        let screen = session.screen();
        let mouse = Mouse { screen: &screen };
        let (left, top) =
            watch_bar(&opened, &screen, |found| found.is_some()).expect("a title bar");
        let width = CONTENT.0 as i32;
        let button = |left: i32, top: i32, index: i32| {
            (
                left + width - (GAP + BUTTON) * (index + 1) + BUTTON / 2,
                top + HEADER / 2,
            )
        };
        let click = |at: (i32, i32)| {
            mouse.move_to(at);
            opened.turn(Duration::from_millis(100));
            mouse.press(1);
            opened.turn(Duration::from_millis(50));
            mouse.release(1);
        };
        click(button(left, top, 1));
        opened.turn_until("the window maximized", |opened| {
            opened.window.state().mode == WindowMode::Maximized
        });
        let report = client_frame_report(opened.window.as_ref()).expect("a Wayland window");
        assert_eq!(
            report
                .pieces
                .iter()
                .map(|(piece, _)| *piece)
                .collect::<Vec<_>>(),
            ["header"],
            "a maximized window keeps its title bar and loses its margins"
        );
        let content = opened.window.state().content_size;
        assert_eq!(
            content.width,
            f64::from(OUTPUT.0),
            "maximized to the output's width"
        );
        // The compositor sizes the window geometry: the title bar and the
        // content below it.
        let configured = compositor_report(opened.window.as_ref())
            .expect("a Wayland window")
            .size;
        assert_eq!(
            f64::from(configured.1),
            content.height + f64::from(HEADER),
            "the configured height {configured:?} holds the title bar and the content {content:?}"
        );

        let maximized_width = content.width as i32;
        let deadline = Instant::now() + Duration::from_secs(10);
        let top = loop {
            opened.turn(Duration::from_millis(30));
            let shot = Shot::take(&screen);
            if let Some(top) =
                (0..shot.height).find(|y| shot.bar_run(*y).1 >= maximized_width - 120)
            {
                break top;
            }
            assert!(Instant::now() < deadline, "the maximized title bar");
        };
        let close = (
            maximized_width - (GAP + BUTTON) + BUTTON / 2,
            top + HEADER / 2,
        );
        click(close);
        opened.turn_until("the handler asked and the window closed", |opened| {
            let calls = opened.handler.calls();
            calls.contains(&HandlerCall::CloseRequested) && calls.contains(&HandlerCall::Closed)
        });
    }
}
