//! The native Wayland host's clipboard between two clients of a real
//! compositor: a Weston on an X server of the harness's own, with the kiosk
//! shell, which gives the keyboard to the window shown last and back to the
//! one before when it closes. A compositor tells a client its seat's
//! selection only as the keyboard enters one of its surfaces, so each
//! client reads what the other wrote once its own window has the keyboard.
//!
//! Its own harness, as `wayland_input`: it starts Xvfb and a Weston before
//! anything connects and names that Weston for the process. A run without
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
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use flui_native::testing::wayland::{present, run_for};
    use flui_native::{HostWindow, Platform as _, WaylandPlatform};
    use flui_platform::WindowOptions;
    use flui_platform::capability::{
        CompletionSource, OwnerJob, OwnerPoster, OwnerRetired, OwnerScope, PendingCompletions,
    };
    use flui_testing::window_commands::AdmitAndApply as _;
    use flui_testing::window_handler::RecordingWindowHandler;

    use super::weston_x11::Session;

    /// The output's size, which the kiosk shell gives every window.
    const OUTPUT: (u16, u16) = (320, 240);
    /// How long a step may take before the case fails.
    const STEP: Duration = Duration::from_secs(10);

    pub(super) fn main() {
        let arguments = libtest_mimic::Arguments::from_args();
        let session = (!arguments.list)
            .then(|| Session::start("kiosk-shell.so", OUTPUT, "flui-wayland-clipboard"))
            .flatten();
        let trials = vec![libtest_mimic::Trial::test(
            "the_selection_crosses_between_clients",
            move || {
                match &session {
                    Some(session) => the_selection_crosses_between_clients(session),
                    None => eprintln!("skipped: no Xvfb or Weston to run against"),
                }
                Ok(())
            },
        )];
        libtest_mimic::run(&arguments, trials).exit();
    }

    /// The completions a clipboard read answers through, settled by hand on
    /// the thread that made them, as an application's runtime settles them.
    struct Answers {
        queue: Arc<Mutex<Vec<OwnerJob>>>,
        pending: PendingCompletions,
        source: CompletionSource,
    }

    impl Answers {
        fn new() -> Self {
            let queue = Arc::new(Mutex::new(Vec::new()));
            let sink = Arc::clone(&queue);
            let poster = OwnerPoster::new(move |job| {
                sink.lock().map_err(|_| OwnerRetired)?.push(job);
                Ok(())
            });
            let pending = PendingCompletions::new();
            Self {
                queue,
                source: CompletionSource::new(poster, pending.clone()),
                pending,
            }
        }

        /// Settle what the resolvers answered since the last call.
        fn settle(&self) {
            let jobs = std::mem::take(&mut *self.queue.lock().expect("the answers"));
            let scope = OwnerScope::new(&self.pending);
            for job in jobs {
                job.run(&scope);
            }
        }
    }

    /// A client: a platform of its own on the calling thread, with one
    /// window.
    struct Client {
        platform: WaylandPlatform,
        window: Rc<dyn HostWindow>,
        answers: Answers,
    }

    impl Client {
        /// Connect and open a window, which the kiosk shell shows over every
        /// other and gives the keyboard.
        fn open(title: &str) -> Self {
            let platform = WaylandPlatform::try_new().expect("the harness's Weston answers");
            let window = platform
                .open_window(
                    WindowOptions::default().with_title(title),
                    RecordingWindowHandler::new() as _,
                )
                .expect("a Wayland window opens");
            let client = Self {
                platform,
                window,
                answers: Answers::new(),
            };
            client.until("its window to have the keyboard", || {
                let state = client.window.state();
                state.visible && state.focused
            });
            client
        }

        /// One turn of the loop, drawing a frame the window asks for.
        fn turn(&self) {
            run_for(&self.platform, Duration::from_millis(10)).expect("the loop turns");
            self.window
                .surface_target()
                .pre_present_notify(flui_platform::SwapChainStatistics::None);
            let _ = present(self.window.as_ref());
            self.answers.settle();
        }

        /// Turn until `done` holds, bounded at [`STEP`].
        fn until(&self, what: &str, done: impl Fn() -> bool) {
            let deadline = Instant::now() + STEP;
            loop {
                self.turn();
                if done() {
                    return;
                }
                assert!(Instant::now() < deadline, "timed out waiting for {what}");
            }
        }

        fn write(&self, text: &str) {
            self.platform.clipboard().write_text(text.to_owned());
        }

        /// Read the clipboard, turning the loop until the read answers.
        fn read(&self) -> Option<String> {
            let (mut completion, resolver) = self.answers.source.pending::<Option<String>>();
            self.platform.clipboard().read_text(resolver);
            let deadline = Instant::now() + STEP;
            loop {
                self.turn();
                if let Some(outcome) = completion.poll() {
                    return outcome.expect("the read is answered, not abandoned");
                }
                assert!(Instant::now() < deadline, "timed out waiting for the read");
            }
        }
    }

    /// One client writes and reads its own text back; a second, whose
    /// window takes the keyboard, reads that text from the first through
    /// the compositor and writes its own; once the second's window closes
    /// and the keyboard is back, the first reads the second's text.
    fn the_selection_crosses_between_clients(session: &Session) {
        // Weston's X11 backend gives its seat's keyboard to a client while its
        // output window has the X focus, which the harness's X connection
        // gives it; the case holds that connection to its end.
        let screen = session.screen();
        let _focus = (screen.connection, screen.root, screen.origin);
        let first = Client::open("first");
        first.write("written by the first");
        assert_eq!(
            first.read().as_deref(),
            Some("written by the first"),
            "a client reads back what it owns"
        );

        let (said, heard) = mpsc::channel();
        let (stop, stopped) = mpsc::channel::<()>();
        let second = std::thread::spawn(move || {
            let second = Client::open("second");
            let read = second.read();
            second.write("written by the second");
            // A turn sends the selection before the window closes.
            second.turn();
            second.window.close_for_cleanup();
            said.send(read).expect("the case listens");
            // The second client answers the first's read until told to stop.
            while stopped.try_recv().is_err() {
                second.turn();
            }
        });
        // The first client keeps turning: the second's read asks it for the
        // text it owns.
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
            report,
            Some("written by the first".to_owned()),
            "the second client reads the first's text through the compositor"
        );
        first.until("the keyboard to come back", || first.window.state().focused);
        assert_eq!(
            first.read().as_deref(),
            Some("written by the second"),
            "the first client no longer owns the selection and reads the second's"
        );
        stop.send(()).expect("the second client listens");
        second.join().expect("the second client ends");
        first.window.close_for_cleanup();
    }
}
