//! The native Wayland host against a compositor the test scripts
//! (`flui_native::testing::wayland_compositor`): the wire protocol over a
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
    use std::sync::mpsc::{RecvTimeoutError, Sender};
    use std::time::{Duration, Instant};

    use flui_foundation::geometry::Size;
    use flui_native::testing::wayland::{
        MAX_BUFFER_SCALE, PortalParentRequest, compositor_report, cursor_report, present, run_for,
    };
    use flui_native::testing::wayland_compositor::{
        Configure, HangUp, Heard, KEY_A, Offer, OutputOffer, ScriptedCompositor, Step,
        ToplevelState,
    };
    use flui_native::{HostWindow, Platform as _, WaylandPlatform};
    use flui_platform::capability::PortalParent;
    use flui_platform::keyboard::{KeyPress, KeyState};
    use flui_platform::pointer::{PointerEvent, TouchpadSwipePhase};
    use flui_platform::{Availability, PlatformInput, WindowOptions};
    use flui_testing::window_commands::AdmitAndApply as _;
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
                "a_moving_wake_deadline_polls_no_window_the_compositor_never_draws",
                a_moving_wake_deadline_polls_no_window_the_compositor_never_draws,
            ),
            (
                "a_window_the_compositor_never_configures_holds_nothing",
                a_window_the_compositor_never_configures_holds_nothing,
            ),
            (
                "a_fractional_scale_no_screen_has_leaves_the_window_at_its_scale",
                a_fractional_scale_no_screen_has_leaves_the_window_at_its_scale,
            ),
            (
                "an_output_removed_under_a_window_leaves_it_on_the_others",
                an_output_removed_under_a_window_leaves_it_on_the_others,
            ),
            (
                "two_outputs_of_different_scales_are_each_their_own_logical_pixels",
                two_outputs_of_different_scales_are_each_their_own_logical_pixels,
            ),
            (
                "a_configure_storm_settles_on_the_last_configure",
                a_configure_storm_settles_on_the_last_configure,
            ),
            (
                "a_mode_the_compositor_reaches_in_two_configures_is_published_once",
                a_mode_the_compositor_reaches_in_two_configures_is_published_once,
            ),
            (
                "a_mode_the_compositor_does_not_carry_out_is_published_when_the_wait_ends",
                a_mode_the_compositor_does_not_carry_out_is_published_when_the_wait_ends,
            ),
            (
                "a_compositor_that_hangs_up_while_a_window_opens_fails_the_open",
                a_compositor_that_hangs_up_while_a_window_opens_fails_the_open,
            ),
            (
                "a_compositor_that_hangs_up_on_a_frame_ends_the_loop_with_an_error",
                a_compositor_that_hangs_up_on_a_frame_ends_the_loop_with_an_error,
            ),
            (
                "the_compositor_refuses_each_request_a_strict_compositor_refuses",
                the_compositor_refuses_each_request_a_strict_compositor_refuses,
            ),
            (
                "a_touchpad_swipe_and_hold_reach_the_window_through_the_wire",
                a_touchpad_swipe_and_hold_reach_the_window_through_the_wire,
            ),
            (
                "a_gestures_global_without_holds_gives_the_window_none",
                a_gestures_global_without_holds_gives_the_window_none,
            ),
            (
                "each_portal_request_exports_the_window_and_releases_the_export_as_it_ends",
                each_portal_request_exports_the_window_and_releases_the_export_as_it_ends,
            ),
            (
                "a_compositor_without_an_exporter_names_no_portal_parent",
                a_compositor_without_an_exporter_names_no_portal_parent,
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
            for value in [
                IMAGE_HEADER,
                IMAGE,
                nominal,
                1,
                side,
                side,
                side / 2,
                side / 2,
                0,
            ] {
                word(&mut file, value);
            }
            for _ in 0..side * side {
                word(&mut file, 0xff00_0000);
            }
        }
        file
    }

    /// Ends the process with a message naming a step that did not return
    /// within [`PATIENCE`]: a call that blocks the owner thread cannot be
    /// bounded from that thread.
    struct Watchdog(#[expect(dead_code, reason = "dropping it disarms the watchdog")] Sender<()>);

    impl Watchdog {
        fn arm(step: &'static str) -> Self {
            let (disarm, armed) = std::sync::mpsc::channel::<()>();
            std::thread::spawn(move || {
                if armed.recv_timeout(PATIENCE) == Err(RecvTimeoutError::Timeout) {
                    eprintln!("{step} did not return within {PATIENCE:?}");
                    std::process::exit(101);
                }
            });
            Self(disarm)
        }
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
            let platform = {
                let _bound = Watchdog::arm("the host connecting to the compositor");
                WaylandPlatform::try_new_at(compositor.socket())
                    .expect("the host connects to the compositor")
            };
            let handler = RecordingWindowHandler::new();
            let window = {
                let _bound = Watchdog::arm("the window opening");
                platform
                    .open_window(options, Rc::clone(&handler) as _)
                    .expect("a window opens")
            };
            Self {
                window,
                handler,
                platform,
                compositor,
            }
        }

        /// Turn the loop until what the compositor heard satisfies `done`,
        /// bounded at [`PATIENCE`]: the compositor records a request when its
        /// thread reads it, which may be after the host's turn returns.
        fn heard_until(&self, what: &str, done: impl Fn(&Heard) -> bool) -> Heard {
            let deadline = Instant::now() + PATIENCE;
            loop {
                let heard = self.compositor.heard();
                if done(&heard) {
                    return heard;
                }
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for the compositor to hear {what}; it heard {heard:?}"
                );
                // The loop may have ended (a hang-up); the compositor's thread
                // still records.
                let _ = run_for(&self.platform, Duration::from_millis(5));
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

        /// Play `configure` and turn the loop until the window acknowledged
        /// it: the window carries a configure out, and publishes what it
        /// says, in the turn that acknowledges it.
        fn configure(&self, configure: Configure) {
            let sent = self.compositor.heard().configures.len();
            self.compositor.play(Step::Configure(configure));
            let heard =
                self.heard_until("the configure sent", |heard| heard.configures.len() > sent);
            let serial = heard.configures[sent];
            self.turn_until("the configure acknowledged", |session| {
                session.compositor.heard().acknowledged.contains(&serial)
            });
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

    /// The portal parent the session's window answers one request, once the
    /// compositor named the export.
    fn portal_parent(session: &Session) -> PortalParent {
        let mut request = PortalParentRequest::ask(session.window.as_ref());
        let deadline = Instant::now() + PATIENCE;
        loop {
            if let Some(parent) = request.answer() {
                return parent;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for the export's handle; the compositor heard {:?}",
                session.compositor.heard()
            );
            run_for(&session.platform, Duration::from_millis(5)).expect("the loop turns");
        }
    }

    /// Every portal request exports the window anew (`xdg-foreign`) and is
    /// answered with the compositor's handle; the export lives while the
    /// request holds its parent and is destroyed when the request lets it go,
    /// so no handle names the window between requests. The window's close
    /// destroys the export a request still holds, and a closed window is
    /// exported for nobody.
    fn each_portal_request_exports_the_window_and_releases_the_export_as_it_ends() {
        let session = Session::open(
            Offer::default().exporter(),
            WindowOptions::default().title("exported"),
        );
        session.turn_until("the window's first frame", |session| {
            session.compositor.heard().frames > 0
        });
        assert_eq!(
            session.compositor.heard().exports,
            0,
            "the window is not exported before a request asks"
        );

        let first = portal_parent(&session);
        assert_eq!(first.as_str(), "wayland:flui-export-1");
        let second = portal_parent(&session);
        assert_eq!(second.as_str(), "wayland:flui-export-2");
        assert_eq!(
            session.compositor.heard().exported,
            ["flui-export-1", "flui-export-2"],
            "each request holds an export of its own"
        );

        drop(first);
        session.heard_until("the first export's release", |heard| {
            heard.exported == ["flui-export-2"]
        });
        drop(second);
        session.heard_until("the second export's release", |heard| {
            heard.exported.is_empty()
        });

        // A request still holding its parent when the window closes loses
        // the export with the window.
        let held = portal_parent(&session);
        assert_eq!(held.as_str(), "wayland:flui-export-3");
        session.window.close_for_cleanup();
        session.heard_until("the closed window's export released", |heard| {
            heard.exported.is_empty()
        });
        drop(held);
        assert!(
            portal_parent(&session).is_none(),
            "a closed window names no parent"
        );
        assert_eq!(session.compositor.heard().exports, 3, "and is not exported");
    }

    /// Where the compositor offers no exporter, a request is answered at once
    /// with no window, and the portal shows its dialog unattached.
    fn a_compositor_without_an_exporter_names_no_portal_parent() {
        let session = Session::open(Offer::default(), WindowOptions::default().title("plain"));
        session.turn_until("the window's first frame", |session| {
            session.compositor.heard().frames > 0
        });
        assert!(portal_parent(&session).is_none());
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
            WindowOptions::default().title("bounded").size(
                flui_platform::WindowExtent::try_new(Size::new(1600.0, 1000.0)).expect("an extent"),
            ),
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
        let cursor = session
            .heard_until("a cursor buffer at scale 3", |heard| {
                heard.cursor.iter().any(|image| image.scale == 3)
            })
            .cursor;
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
        let heard = session.heard_until("the cursor buffer for scale 2", |heard| {
            heard.cursor.iter().any(|image| image.destination.is_some())
        });
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

    impl Session {
        /// The touchpad holds and swipes the window heard, in order.
        fn touchpad(&self) -> Vec<String> {
            self.handler
                .calls()
                .into_iter()
                .filter_map(|call| match call {
                    HandlerCall::Input(PlatformInput::Pointer(PointerEvent::TouchpadHold(
                        hold,
                    ))) => Some(format!("hold {:?}", hold.phase)),
                    HandlerCall::Input(PlatformInput::Pointer(PointerEvent::TouchpadSwipe(
                        swipe,
                    ))) => Some(match swipe.phase {
                        TouchpadSwipePhase::Update(travel) => format!(
                            "swipe {} travel ({}, {})",
                            swipe.fingers,
                            travel.get().dx,
                            travel.get().dy
                        ),
                        phase => format!("swipe {} {phase:?}", swipe.fingers),
                    }),
                    _ => None,
                })
                .collect()
        }

        /// The last touchpad hold and swipe availability the window heard.
        fn touchpad_availability(&self) -> Option<(Availability, Availability)> {
            self.handler
                .calls()
                .into_iter()
                .rev()
                .find_map(|call| match call {
                    HandlerCall::Input(PlatformInput::Capabilities(capabilities)) => {
                        Some((capabilities.touchpad_hold(), capabilities.touchpad_swipe()))
                    }
                    _ => None,
                })
        }
    }

    /// A touchpad hold and swipe the compositor sends through
    /// `zwp_pointer_gestures_v1` version 3 reach the window as one hold and
    /// one swipe, its steps summed into its travel where the cursor rests,
    /// and the window is told both reach it.
    fn a_touchpad_swipe_and_hold_reach_the_window_through_the_wire() {
        let session = Session::open(
            Offer::default().seat(7).pointer_gestures(3),
            WindowOptions::default().title("gestures"),
        );
        session.heard_until("a swipe and a hold object", |heard| {
            heard.swipes > 0 && heard.holds > 0
        });
        session.turn_until("the window's first frame", |session| {
            session.compositor.heard().frames > 0
        });
        for step in [
            Step::PointerEnter { x: 40.0, y: 30.0 },
            Step::HoldBegin { fingers: 2 },
            Step::HoldEnd { cancelled: true },
            Step::SwipeBegin { fingers: 3 },
            Step::SwipeUpdate { dx: -20.0, dy: 0.0 },
            Step::SwipeUpdate { dx: -10.0, dy: 4.0 },
            Step::SwipeEnd { cancelled: false },
        ] {
            session.compositor.play(step);
        }
        session.turn_until("the swipe's end", |session| {
            session.touchpad().last().map(String::as_str) == Some("swipe 3 End")
        });
        assert_eq!(
            session.touchpad(),
            [
                "hold Begin",
                "hold Cancelled",
                "swipe 3 Start",
                "swipe 3 travel (-20, 0)",
                "swipe 3 travel (-30, 4)",
                "swipe 3 End",
            ]
        );
        assert_eq!(
            session.touchpad_availability(),
            Some((Availability::Native, Availability::Native))
        );
    }

    /// A gestures global older than version 3 has no hold: the host binds no
    /// hold object, the window is told no hold reaches it, and a swipe still
    /// does.
    fn a_gestures_global_without_holds_gives_the_window_none() {
        let session = Session::open(
            Offer::default().seat(7).pointer_gestures(2),
            WindowOptions::default().title("no holds"),
        );
        let heard = session.heard_until("a swipe object", |heard| heard.swipes > 0);
        assert_eq!((heard.gestures, heard.holds), (Some(2), 0));
        session.turn_until("the window's first frame", |session| {
            session.compositor.heard().frames > 0
        });
        for step in [
            Step::PointerEnter { x: 40.0, y: 30.0 },
            Step::HoldBegin { fingers: 2 },
            Step::SwipeBegin { fingers: 4 },
            Step::SwipeEnd { cancelled: true },
        ] {
            session.compositor.play(step);
        }
        session.turn_until("the swipe's cancel", |session| {
            session.touchpad().last().map(String::as_str) == Some("swipe 4 Cancelled")
        });
        assert_eq!(session.touchpad(), ["swipe 4 Start", "swipe 4 Cancelled"]);
        assert_eq!(
            session.touchpad_availability(),
            Some((Availability::Unavailable, Availability::Native))
        );
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
        session.heard_until("callbacks held back", |heard| heard.withheld > 0);
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

    /// A wake deadline that moves on every answer, as the runtime's in-flight
    /// floor does while a raster thread holds a frame of the window, polls no
    /// window the compositor never draws: the loop asks the window for its
    /// frame, which waits for the frame callback's grace, so it wakes for
    /// the deadline only once that grace runs out, as it would with no
    /// deadline, and delivers it then.
    fn a_moving_wake_deadline_polls_no_window_the_compositor_never_draws() {
        use flui_native::testing::app::{AppCall, AppLog};

        let compositor = ScriptedCompositor::start(Offer::default().withhold_frame_callbacks())
            .expect("the compositor starts");
        let platform = {
            let _bound = Watchdog::arm("the host connecting to the compositor");
            WaylandPlatform::try_new_at(compositor.socket())
                .expect("the host connects to the compositor")
        };
        let log = AppLog::new();
        // A deadline 50 ms ahead of every answer: each one is new, so the
        // loop arms it whether or not a frame ran since the last.
        log.answer_deadline_with(|| Some(Instant::now() + Duration::from_millis(50)));
        let frames = Rc::new(std::cell::Cell::new(0_usize));
        let drawn = Rc::clone(&frames);
        log.bootstrap(move |owner| {
            let handler = RecordingWindowHandler::new();
            let shown: Rc<std::cell::RefCell<Option<Rc<dyn HostWindow>>>> = Rc::default();
            let target = Rc::clone(&shown);
            // Each frame is presented as a renderer presents it, which arms
            // the frame callback the compositor never answers.
            handler.react_to_frame(move |_| {
                if let Some(window) = target.borrow().as_ref() {
                    window
                        .surface_target()
                        .pre_present_notify(None, flui_platform::SwapChainStatistics::None);
                    let _ = present(window.as_ref());
                    drawn.set(drawn.get() + 1);
                }
            });
            let window = owner
                .open_window(
                    WindowOptions::default().title("polled by no deadline"),
                    Rc::clone(&handler) as _,
                )
                .expect("a window opens")
                .try_ready()
                .expect("at once");
            shown.replace(Some(window));
            let proxy = owner.proxy();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(3_000));
                let _ = proxy.request_quit();
            });
            Ok(())
        });
        Box::new(platform)
            .run(Rc::clone(&log) as _)
            .expect("the run ends");
        let asked = log.count(AppCall::WakeDeadline);
        // The first frame, then one at the first callback's grace: the
        // deadline is delivered at the grace, not lost.
        assert!(
            frames.get() >= 2,
            "{} frames in three seconds: the deadline was not delivered at the grace",
            frames.get()
        );
        // Polled at the deadline's pace, the loop would turn about sixty
        // times in three seconds; at the graces it turns a few times.
        assert!(
            asked < 20,
            "the loop turned {asked} times in three seconds for a window whose frame \
             callback is withheld"
        );
    }

    /// A window the compositor never configures opens and holds nothing: the
    /// open returns, each turn returns in its time, and the window is asked
    /// for no frame, which it could not draw; the configure that comes late
    /// gets it its first frame.
    fn a_window_the_compositor_never_configures_holds_nothing() {
        let started = Instant::now();
        let session = Session::open(
            Offer::default().first_configure(None),
            WindowOptions::default().title("never configured"),
        );
        let frames = |session: &Session| {
            session
                .handler
                .calls()
                .iter()
                .filter(|call| matches!(call, HandlerCall::FrameRequested(_)))
                .count()
        };
        for _ in 0..50 {
            session.window.request_redraw();
            let turn = Instant::now();
            run_for(&session.platform, Duration::from_millis(10)).expect("the loop turns");
            assert!(
                turn.elapsed() < Duration::from_millis(500),
                "a turn took {:?}",
                turn.elapsed()
            );
        }
        assert!(
            started.elapsed() < PATIENCE,
            "the open waited for a configure"
        );
        assert_eq!(
            frames(&session),
            0,
            "a frame was asked of an unconfigured window"
        );
        assert_eq!(
            session.compositor.heard().frames,
            0,
            "nothing was committed"
        );
        session.compositor.play(Step::Configure(
            Configure::default().state(ToplevelState::Activated),
        ));
        session.turn_until("the first frame after the late configure", |session| {
            frames(session) > 0 && session.compositor.heard().frames > 0
        });
    }

    /// A fractional scale no screen has (zero, or past `DevicePixelRatio`'s
    /// range) is refused and leaves the window at the scale it had; one a
    /// screen can have but past the largest a buffer is drawn at draws at
    /// that largest.
    fn a_fractional_scale_no_screen_has_leaves_the_window_at_its_scale() {
        let session = Session::open(
            Offer::default().fractional_scale(),
            WindowOptions::default().title("fractional"),
        );
        let told = |numerator: u32, expected: f64, what: &str| {
            session
                .compositor
                .play(Step::PreferFractionalScale(numerator));
            session.turn_until(what, |session| session.scale() == Some(expected));
            // A refusal changes nothing: give it turns to show if it did.
            for _ in 0..20 {
                run_for(&session.platform, Duration::from_millis(5)).expect("the loop turns");
            }
            assert_eq!(session.scale(), Some(expected), "{what}");
        };
        told(180, 1.5, "the window at 1.5");
        told(0, 1.5, "a zero scale refused");
        told(
            120 * 20,
            MAX_BUFFER_SCALE,
            "twenty drawn at the largest buffer scale",
        );
        told(120 * 1_000, MAX_BUFFER_SCALE, "a thousand refused");
    }

    /// An output the compositor removes while the window's surface is on it,
    /// with no `leave`, no longer sets the window's scale or holds it: the
    /// window takes the scale of the output it is still on.
    fn an_output_removed_under_a_window_leaves_it_on_the_others() {
        let offer = Offer::default().compositor(5).outputs([
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

    /// Two outputs of different scales are each measured in their own
    /// logical pixels: an output's extent is its mode over its own scale, its
    /// whole area usable (Wayland reports no work area), its origin the
    /// compositor's arrangement. Wayland tells no window where it is, so the
    /// window publishes no position on either output, nor once the one under
    /// it goes.
    fn two_outputs_of_different_scales_are_each_their_own_logical_pixels() {
        use flui_foundation::geometry::{Rect, Size};

        let offer = Offer::default().compositor(5).outputs([
            OutputOffer::new(1920, 1080, 1),
            OutputOffer::new(2560, 1440, 2),
        ]);
        let session = Session::open(offer, WindowOptions::default().title("two scales"));
        let displays = |session: &Session| {
            session
                .platform
                .display_snapshot()
                .map_or_else(|_| Vec::new(), |snapshot| snapshot.displays().to_vec())
        };
        session.turn_until("both outputs in the snapshot", |session| {
            displays(session).len() == 2
        });
        let mut extents: Vec<(Size<f64>, f64, Rect<f64>)> = displays(&session)
            .iter()
            .map(|display| {
                (
                    display.bounds.extent(),
                    display.scale_factor.get(),
                    display.bounds.usable(),
                )
            })
            .collect();
        extents.sort_by(|a, b| a.1.total_cmp(&b.1));
        assert_eq!(
            extents,
            [
                (
                    Size::new(1920.0, 1080.0),
                    1.0,
                    Rect::from_ltrb(0.0, 0.0, 1920.0, 1080.0)
                ),
                (
                    Size::new(1280.0, 720.0),
                    2.0,
                    Rect::from_ltrb(0.0, 0.0, 1280.0, 720.0)
                ),
            ],
            "each output in its own logical pixels, all of it usable"
        );
        session.compositor.play(Step::Enter(0));
        session.compositor.play(Step::Enter(1));
        session.turn_until("the window on both outputs", |session| {
            compositor_report(session.window.as_ref())
                .is_some_and(|report| report.outputs.len() == 2)
        });
        assert_eq!(
            session.window.state().outer.position(),
            None,
            "a Wayland window publishes no position"
        );

        // The output under the window goes: the window is on the other, and
        // still publishes no position.
        session.compositor.play(Step::RemoveOutput(1));
        session.turn_until("the window back at scale 1", |session| {
            session.scale() == Some(1.0)
        });
        assert_eq!(session.window.state().outer.position(), None);
    }

    /// A `SetMode` out of fullscreen the compositor answers in two configures
    /// changes the published mode once, from fullscreen to the mode asked:
    /// the configure between, normal on the way to maximized or the
    /// maximized mode a compositor restores on the way to normal, is
    /// acknowledged and drawn at its size, and no snapshot names its mode.
    /// Back in normal mode the window has the size it had before. A newer
    /// request replaces the wait of an older one the compositor is still
    /// answering, also one for the mode the window is in. A `SetSize` out of
    /// fullscreen asks for normal mode as `SetMode` does: the maximized mode
    /// a compositor restores on the way is not published, nor is the one a
    /// `SetMode` before it in the same turn asked for, and the window ends
    /// normal at the size asked.
    fn a_mode_the_compositor_reaches_in_two_configures_is_published_once() {
        use flui_platform::{WindowCommand, WindowExtent, WindowMode};
        let normal = || Configure::default().state(ToplevelState::Activated);
        let maximized = || normal().size(1920, 1040).state(ToplevelState::Maximized);
        let fullscreen = || normal().size(1920, 1080).state(ToplevelState::Fullscreen);
        let mode = WindowCommand::SetMode;
        let sized = WindowExtent::try_new(Size::new(500.0, 400.0)).expect("an extent");
        // (the configures that take the window to fullscreen, the commands
        // out of it with no turn between, the configures that answer, the mode
        // between, the mode the window ends in)
        let table: [(&[_], &[_], [_; 2], _, _); 5] = [
            (
                &[fullscreen()],
                &[mode(WindowMode::Maximized)],
                [normal(), maximized()],
                WindowMode::Normal,
                WindowMode::Maximized,
            ),
            (
                &[maximized(), fullscreen()],
                &[mode(WindowMode::Normal)],
                [maximized(), normal()],
                WindowMode::Maximized,
                WindowMode::Normal,
            ),
            (
                &[fullscreen()],
                &[mode(WindowMode::Maximized), mode(WindowMode::Fullscreen)],
                [maximized(), fullscreen()],
                WindowMode::Maximized,
                WindowMode::Fullscreen,
            ),
            (
                &[fullscreen()],
                &[mode(WindowMode::Maximized), WindowCommand::SetSize(sized)],
                [maximized(), normal()],
                WindowMode::Maximized,
                WindowMode::Normal,
            ),
            (
                &[maximized(), fullscreen()],
                &[WindowCommand::SetSize(sized)],
                [maximized(), normal()],
                WindowMode::Maximized,
                WindowMode::Normal,
            ),
        ];
        for (into, commands, answers, between, asked) in table {
            let session = Session::open(Offer::default(), WindowOptions::default().title("modes"));
            session.turn_until("the window's first frame", |session| {
                session.compositor.heard().frames > 0
            });
            let opened = session.window.state().content_size;
            for configure in into {
                session.configure(configure.clone());
            }
            assert_eq!(session.window.state().mode, WindowMode::Fullscreen);
            let seen = Rc::new(std::cell::RefCell::new(Vec::new()));
            let log = Rc::clone(&seen);
            let window = Rc::downgrade(&session.window);
            session.handler.react_to_state_change(move || {
                if let Some(window) = window.upgrade() {
                    log.borrow_mut().push(window.state().mode);
                }
            });
            for command in commands {
                session
                    .window
                    .admit_and_apply(command.clone())
                    .expect("the compositor takes every mode and size");
            }
            for configure in answers {
                session.configure(configure);
            }
            let state = session.window.state();
            let seen = seen.borrow().clone();
            assert!(
                !seen.contains(&between),
                "{commands:?} read {between:?} on the way: {seen:?}"
            );
            assert_eq!(state.mode, asked, "{commands:?}: {seen:?}");
            if asked == WindowMode::Normal {
                let size = if commands.contains(&WindowCommand::SetSize(sized)) {
                    sized
                } else {
                    opened
                };
                assert_eq!(state.content_size, size, "{commands:?}");
            }
        }
    }

    /// A mode change the compositor does not carry out is published once the
    /// window stops waiting for it, with nothing from the compositor to wake
    /// the loop then: one blocking stretch of the loop, longer than the
    /// wait, ends with the mode the compositor configured.
    fn a_mode_the_compositor_does_not_carry_out_is_published_when_the_wait_ends() {
        use flui_platform::{WindowCommand, WindowMode};
        let session = Session::open(Offer::default(), WindowOptions::default().title("refused"));
        session.turn_until("the window's first frame", |session| {
            session.compositor.heard().frames > 0
        });
        let normal = Configure::default().state(ToplevelState::Activated);
        session.configure(
            normal
                .clone()
                .size(1920, 1080)
                .state(ToplevelState::Fullscreen),
        );
        session
            .window
            .admit_and_apply(WindowCommand::SetMode(WindowMode::Maximized))
            .expect("the compositor takes every mode");
        // Out of fullscreen, but never maximized.
        session.configure(normal);
        assert_eq!(session.window.state().mode, WindowMode::Fullscreen);
        run_for(&session.platform, Duration::from_millis(2_500)).expect("the loop turns");
        assert_eq!(session.window.state().mode, WindowMode::Normal);
    }

    /// A burst of configures, each of another size, sent before the loop
    /// turns, settles on the last: the window acknowledges only the newest
    /// configure it has read each time it carries them out, so the 200 cost
    /// a few acknowledgements (one per read of the socket, at most four
    /// here), in order, the last of them the burst's last, whose size the
    /// window takes.
    fn a_configure_storm_settles_on_the_last_configure() {
        const STORM: usize = 200;
        let session = Session::open(Offer::default(), WindowOptions::default().title("a storm"));
        session.turn_until("the window's first frame", |session| {
            session.compositor.heard().frames > 0
        });
        let before = session.compositor.heard();
        let storm = i32::try_from(STORM).expect("a small storm");
        for step in 1..=storm {
            session.compositor.play(Step::Configure(
                Configure::default()
                    .size(400 + step, 300 + step)
                    .state(ToplevelState::Activated),
            ));
        }
        // The whole burst is on its way before the loop turns again.
        let deadline = Instant::now() + PATIENCE;
        while session.compositor.heard().configures.len() < before.configures.len() + STORM {
            assert!(
                Instant::now() < deadline,
                "the compositor never sent the storm"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let last = Size::new(f64::from(400 + storm), f64::from(300 + storm));
        session.turn_until("the last configure's size", |session| {
            session.window.state().content_size.get() == last
        });
        let sent = session.compositor.heard().configures;
        let last_serial = *sent.last().expect("configures were sent");
        let heard = session.heard_until("the last configure acknowledged", |heard| {
            heard.acknowledged.last() == Some(&last_serial)
        });
        let storm_acks = &heard.acknowledged[before.acknowledged.len()..];
        assert!(
            storm_acks.is_sorted() && storm_acks.iter().all(|serial| sent.contains(serial)),
            "serials acknowledged in order, each one sent: {storm_acks:?}"
        );
        assert!(
            storm_acks.len() <= 4,
            "{} acknowledgements of a burst of {STORM}: {storm_acks:?}",
            storm_acks.len()
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
        assert!(
            started.elapsed() < PATIENCE,
            "nothing waited on the dead peer"
        );
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
        session.heard_until("every connection closed", |heard| heard.clients == 0);
    }

    /// Each rule the compositor enforces ends the connection of a client that
    /// breaks it with that rule's protocol error: what makes a host that
    /// passes the cases above one that keeps the rules.
    fn the_compositor_refuses_each_request_a_strict_compositor_refuses() {
        use wayland_protocols::xdg::shell::client::xdg_surface;
        use wrong_client::Wrong;

        let xdg_surface = |error: xdg_surface::Error| ("xdg_surface", error as u32);
        for (wrong, expected) in [
            (
                Wrong::BufferBeforeAcknowledgement,
                xdg_surface(xdg_surface::Error::UnconfiguredBuffer),
            ),
            (
                Wrong::SerialNeverSent,
                xdg_surface(xdg_surface::Error::InvalidSerial),
            ),
            (
                Wrong::SerialOlderThanAcknowledged,
                xdg_surface(xdg_surface::Error::InvalidSerial),
            ),
            (
                Wrong::BufferScaleBelowOne,
                (
                    "wl_surface",
                    wayland_client::protocol::wl_surface::Error::InvalidScale as u32,
                ),
            ),
            (
                Wrong::CursorOnToplevel,
                (
                    "wl_pointer",
                    wayland_client::protocol::wl_pointer::Error::Role as u32,
                ),
            ),
            (
                Wrong::BufferItsScaleDoesNotDivide,
                (
                    "wl_surface",
                    wayland_client::protocol::wl_surface::Error::InvalidSize as u32,
                ),
            ),
        ] {
            let compositor =
                ScriptedCompositor::start(Offer::default().seat(7)).expect("the compositor starts");
            let refused = wrong_client::play(&compositor, wrong);
            assert!(
                refused,
                "{wrong:?}: the client's connection outlived its request"
            );
            let deadline = Instant::now() + PATIENCE;
            loop {
                let errors = compositor.heard().errors;
                if errors
                    .iter()
                    .any(|error| (error.interface, error.code) == expected)
                {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "{wrong:?}: no {expected:?} was posted; posted {errors:?}"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    /// A Wayland client written to break one rule of the protocol, on
    /// `wayland-client` alone.
    mod wrong_client {
        use std::os::fd::AsFd as _;
        use std::os::unix::net::UnixStream;
        use std::time::Instant;

        use flui_native::testing::wayland_compositor::{ScriptedCompositor, Step};
        use wayland_client::protocol::wl_buffer::WlBuffer;
        use wayland_client::protocol::wl_compositor::WlCompositor;
        use wayland_client::protocol::wl_pointer::WlPointer;
        use wayland_client::protocol::wl_registry::{self, WlRegistry};
        use wayland_client::protocol::wl_seat::WlSeat;
        use wayland_client::protocol::wl_shm::{self, WlShm};
        use wayland_client::protocol::wl_shm_pool::WlShmPool;
        use wayland_client::protocol::wl_surface::WlSurface;
        use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
        use wayland_protocols::xdg::shell::client::xdg_surface::{self, XdgSurface};
        use wayland_protocols::xdg::shell::client::xdg_toplevel::XdgToplevel;
        use wayland_protocols::xdg::shell::client::xdg_wm_base::{self, XdgWmBase};

        use super::{Configure, PATIENCE};

        /// The rule a client breaks.
        #[derive(Debug, Clone, Copy)]
        pub(super) enum Wrong {
            /// Commits its toplevel's buffer before it acknowledges a configure.
            BufferBeforeAcknowledgement,
            /// Acknowledges a serial the compositor never sent.
            SerialNeverSent,
            /// Acknowledges a serial older than one it acknowledged.
            SerialOlderThanAcknowledged,
            /// Sets a buffer scale of zero.
            BufferScaleBelowOne,
            /// Sets its toplevel's surface as its cursor.
            CursorOnToplevel,
            /// Commits a 63-pixel buffer at buffer scale 2.
            BufferItsScaleDoesNotDivide,
        }

        /// The globals the client bound as they were announced, and the
        /// serials it was configured with.
        #[derive(Default)]
        struct Heard {
            compositor: Option<WlCompositor>,
            base: Option<XdgWmBase>,
            shm: Option<WlShm>,
            seat: Option<WlSeat>,
            serials: Vec<u32>,
        }

        impl Dispatch<WlRegistry, ()> for Heard {
            fn event(
                heard: &mut Self,
                registry: &WlRegistry,
                event: wl_registry::Event,
                (): &(),
                _: &Connection,
                queue: &QueueHandle<Self>,
            ) {
                let wl_registry::Event::Global {
                    name,
                    interface,
                    version,
                } = event
                else {
                    return;
                };
                match interface.as_str() {
                    "wl_compositor" => {
                        heard.compositor = Some(registry.bind(name, version.min(6), queue, ()));
                    }
                    "xdg_wm_base" => {
                        heard.base = Some(registry.bind(name, version.min(6), queue, ()));
                    }
                    "wl_shm" => heard.shm = Some(registry.bind(name, 1, queue, ())),
                    "wl_seat" => heard.seat = Some(registry.bind(name, version.min(7), queue, ())),
                    _ => {}
                }
            }
        }

        impl Dispatch<XdgSurface, ()> for Heard {
            fn event(
                heard: &mut Self,
                _: &XdgSurface,
                event: xdg_surface::Event,
                (): &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
                if let xdg_surface::Event::Configure { serial } = event {
                    heard.serials.push(serial);
                }
            }
        }

        impl Dispatch<XdgWmBase, ()> for Heard {
            fn event(
                _: &mut Self,
                base: &XdgWmBase,
                event: xdg_wm_base::Event,
                (): &(),
                _: &Connection,
                _: &QueueHandle<Self>,
            ) {
                if let xdg_wm_base::Event::Ping { serial } = event {
                    base.pong(serial);
                }
            }
        }

        delegate_noop!(Heard: ignore WlCompositor);
        delegate_noop!(Heard: ignore WlSurface);
        delegate_noop!(Heard: ignore WlShm);
        delegate_noop!(Heard: ignore WlShmPool);
        delegate_noop!(Heard: ignore WlBuffer);
        delegate_noop!(Heard: ignore WlSeat);
        delegate_noop!(Heard: ignore WlPointer);
        delegate_noop!(Heard: ignore XdgToplevel);

        /// Open a toplevel on `compositor`, break `wrong`'s rule, and say
        /// whether the compositor ended the connection for it.
        pub(super) fn play(compositor: &ScriptedCompositor, wrong: Wrong) -> bool {
            let stream = UnixStream::connect(compositor.socket()).expect("the socket answers");
            let connection = Connection::from_socket(stream).expect("a connection");
            let mut queue = connection.new_event_queue::<Heard>();
            let handle = queue.handle();
            let mut heard = Heard::default();
            let _registry = connection.display().get_registry(&handle, ());
            queue.roundtrip(&mut heard).expect("the registry answers");
            let wl_compositor = heard.compositor.clone().expect("wl_compositor is offered");
            let base = heard.base.clone().expect("xdg_wm_base is offered");
            let shm = heard.shm.clone().expect("wl_shm is offered");
            let seat = heard.seat.clone().expect("a seat is offered");
            let surface = wl_compositor.create_surface(&handle, ());
            let xdg = base.get_xdg_surface(&surface, &handle, ());
            let _toplevel = xdg.get_toplevel(&handle, ());
            surface.commit();
            queue.roundtrip(&mut heard).expect("the first configure");
            let first = *heard
                .serials
                .first()
                .expect("the compositor configured the toplevel");
            let buffer = |side: i32| {
                let file = tempfile::tempfile().expect("a buffer's memory");
                let bytes = side * side * 4;
                file.set_len(u64::try_from(bytes).expect("a small buffer"))
                    .expect("the memory is sized");
                let pool = shm.create_pool(file.as_fd(), bytes, &handle, ());
                pool.create_buffer(
                    0,
                    side,
                    side,
                    side * 4,
                    wl_shm::Format::Argb8888,
                    &handle,
                    (),
                )
            };
            match wrong {
                Wrong::BufferBeforeAcknowledgement => {
                    surface.attach(Some(&buffer(64)), 0, 0);
                    surface.commit();
                }
                Wrong::SerialNeverSent => xdg.ack_configure(first + 1_000),
                Wrong::SerialOlderThanAcknowledged => {
                    compositor.play(Step::Configure(Configure::default()));
                    let deadline = Instant::now() + PATIENCE;
                    while heard.serials.len() < 2 {
                        assert!(Instant::now() < deadline, "no second configure came");
                        queue.roundtrip(&mut heard).expect("the second configure");
                    }
                    xdg.ack_configure(heard.serials[1]);
                    xdg.ack_configure(first);
                }
                Wrong::BufferScaleBelowOne => surface.set_buffer_scale(0),
                Wrong::CursorOnToplevel => {
                    xdg.ack_configure(first);
                    let pointer = seat.get_pointer(&handle, ());
                    pointer.set_cursor(0, Some(&surface), 0, 0);
                }
                Wrong::BufferItsScaleDoesNotDivide => {
                    xdg.ack_configure(first);
                    surface.set_buffer_scale(2);
                    surface.attach(Some(&buffer(63)), 0, 0);
                    surface.commit();
                }
            }
            queue.roundtrip(&mut heard).is_err()
        }
    }
}
