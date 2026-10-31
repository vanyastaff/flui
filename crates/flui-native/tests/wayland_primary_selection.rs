//! The native Wayland host's primary selection between two clients of a
//! real compositor: a sway (wlroots) on an X server of the harness's own, as
//! Weston has no primary selection. sway gives the keyboard to the window
//! shown last and back to the one before when it closes, and tells a client
//! its seat's primary selection as the keyboard enters one of its surfaces.
//!
//! Its own harness, as `wayland_clipboard`: it starts Xvfb and a sway before
//! anything connects and names that sway for the process. A run without
//! Xvfb or sway is skipped, and fails where `FLUI_REQUIRE_WAYLAND` is set.

fn main() {
    #[cfg(target_os = "linux")]
    cases::main();
}

#[cfg(target_os = "linux")]
#[path = "support/sway_x11.rs"]
mod sway_x11;

#[cfg(target_os = "linux")]
#[path = "support/wayland_clients.rs"]
mod wayland_clients;

#[cfg(target_os = "linux")]
mod cases {
    use std::cell::Cell;
    use std::rc::{Rc, Weak};
    use std::sync::{Arc, mpsc};
    use std::time::Instant;

    use flui_native::{NativePrimarySelection, Platform as _};
    use flui_testing::window_commands::AdmitAndApply as _;

    use super::sway_x11::Session;
    use super::wayland_clients::{Client, STEP};

    /// The size of the output window sway's X11 backend opens.
    const OUTPUT: (u16, u16) = (1024, 768);

    pub(super) fn main() {
        let mut arguments = libtest_mimic::Arguments::from_args();
        // The trials share one sway and its keyboard.
        arguments.test_threads = Some(1);
        let session = Arc::new(
            (!arguments.list)
                .then(|| Session::start_sway(OUTPUT, "flui-wayland-primary"))
                .flatten(),
        );
        let trial = |name: &'static str, case: fn(&Session)| {
            let session = Arc::clone(&session);
            libtest_mimic::Trial::test(name, move || {
                match session.as_ref() {
                    Some(session) => case(session),
                    None => eprintln!("skipped: no Xvfb or sway to run against"),
                }
                Ok(())
            })
        };
        let trials = vec![
            trial(
                "the_primary_selection_is_asked_at_the_turn",
                the_primary_selection_is_asked_at_the_turn,
            ),
            trial(
                "a_selection_another_client_takes_is_dropped_at_the_turn",
                a_selection_another_client_takes_is_dropped_at_the_turn,
            ),
        ];
        libtest_mimic::run(&arguments, trials).exit();
    }

    fn primary(client: &Client) -> Rc<dyn NativePrimarySelection> {
        client
            .platform
            .primary_selection()
            .expect("sway has a primary selection")
    }

    fn read(client: &Client) -> Option<std::sync::Arc<str>> {
        let primary = primary(client);
        client.read(|answer| primary.read_text(answer))
    }

    /// The first client offers its selection, which is asked for its text
    /// only when read, and re-enters the selection while asked; a second,
    /// whose window takes the keyboard, reads it through the compositor and
    /// offers its own, which the first reads once the keyboard is back.
    fn the_primary_selection_is_asked_at_the_turn(session: &Session) {
        // sway's X11 backend gives its seat's keyboard to a client while its
        // output window has the X focus; the case holds that connection.
        let screen = session.screen();
        let _focus = (screen.connection, screen.root, screen.origin);
        let first = Client::open("first");
        let selection = primary(&first);
        let asked = Rc::new(Cell::new(0_u32));
        let reentered: Weak<dyn NativePrimarySelection> = Rc::downgrade(&selection);
        selection.offer(Rc::new({
            let asked = Rc::clone(&asked);
            move || {
                asked.set(asked.get() + 1);
                // A widget's selection reads the state it shares: here the
                // selection itself, which no lock of its own holds.
                let selection = reentered.upgrade().expect("the selection lives");
                assert!(selection.owns(), "asked while it holds the selection");
                Some("selected in the first".to_owned())
            }
        }));
        first.until("the selection to be set", || selection.owns());
        assert_eq!(asked.get(), 0, "an offer copies nothing");
        assert_eq!(
            read(&first).as_deref(),
            Some("selected in the first"),
            "a client reads back what it offers"
        );
        assert_eq!(asked.get(), 1);

        let (said, heard) = mpsc::channel();
        let (stop, stopped) = mpsc::channel::<()>();
        let second = std::thread::spawn(move || {
            let second = Client::open("second");
            let first_text = read(&second);
            primary(&second).offer(Rc::new(|| Some("selected in the second".to_owned())));
            // A turn sets the selection before the window closes.
            second.turn();
            second.window.close_for_cleanup();
            said.send(first_text).expect("the case listens");
            while stopped.try_recv().is_err() {
                second.turn();
            }
        });
        let deadline = Instant::now() + STEP * 2;
        let report = loop {
            first.turn();
            if let Ok(report) = heard.try_recv() {
                break report;
            }
            assert!(
                Instant::now() < deadline && !second.is_finished(),
                "the second client did not report"
            );
        };
        assert_eq!(
            report.as_deref(),
            Some("selected in the first"),
            "the second client reads the first's selection through the compositor"
        );
        assert_eq!(
            asked.get(),
            2,
            "the compositor's request asked the selection once, at a turn"
        );
        first.until("the keyboard to come back", || first.window.state().focused);
        assert!(
            !selection.owns(),
            "the second client's selection replaced the first's"
        );
        assert_eq!(
            read(&first).as_deref(),
            Some("selected in the second"),
            "the first client reads the second's selection"
        );
        stop.send(()).expect("the second client listens");
        second.join().expect("the second client ends");
        first.window.close_for_cleanup();
    }

    /// A capture of the offered selection that opens a window when it is
    /// dropped: opening one waits on the compositor, which reads the loop's
    /// events, so a drop inside the dispatch would find the loop busy.
    struct OpensAWindow {
        platform: Weak<flui_native::WaylandPlatform>,
        opened: Rc<Cell<bool>>,
    }

    impl Drop for OpensAWindow {
        fn drop(&mut self) {
            let platform = self.platform.upgrade().expect("the client lives");
            let window = platform
                .open_window(
                    flui_platform::WindowOptions::default().with_title("opened by a drop"),
                    flui_testing::window_handler::RecordingWindowHandler::new() as _,
                )
                .expect("a window opens from a dropped capture");
            window.close_for_cleanup();
            self.opened.set(true);
        }
    }

    /// Another client taking the selection retires the application's
    /// closure, whose captures are application code: they are dropped at the
    /// loop's turn, where they may call into the platform, never inside the
    /// dispatch that heard of it.
    fn a_selection_another_client_takes_is_dropped_at_the_turn(session: &Session) {
        let screen = session.screen();
        let _focus = (screen.connection, screen.root, screen.origin);
        let first = Client::open("first");
        let selection = primary(&first);
        let opened = Rc::new(Cell::new(false));
        let capture = OpensAWindow {
            platform: Rc::downgrade(&first.platform),
            opened: Rc::clone(&opened),
        };
        selection.offer(Rc::new(move || {
            let _held = &capture;
            Some("selected in the first".to_owned())
        }));
        first.until("the selection to be set", || selection.owns());

        let (stop, stopped) = mpsc::channel::<()>();
        let second = std::thread::spawn(move || {
            let second = Client::open("second");
            primary(&second).offer(Rc::new(|| Some("selected in the second".to_owned())));
            while stopped.try_recv().is_err() {
                second.turn();
            }
            second.window.close_for_cleanup();
        });
        first.until("the second client to take the selection", || opened.get());
        assert!(
            !selection.owns(),
            "the second client's selection replaced it"
        );
        stop.send(()).expect("the second client listens");
        second.join().expect("the second client ends");
        first.window.close_for_cleanup();
    }
}
