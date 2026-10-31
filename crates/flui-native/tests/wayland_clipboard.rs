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
#[path = "support/wayland_clients.rs"]
mod wayland_clients;

#[cfg(target_os = "linux")]
mod cases {
    use std::sync::mpsc;
    use std::time::Instant;

    use flui_native::Platform as _;
    use flui_native::testing::clipboard::a_concealed_write_stays_out_of_history;
    use flui_native::testing::wayland::selection_offers_concealed_hint;
    use flui_platform::ClipboardWrite;
    use flui_testing::window_commands::AdmitAndApply as _;

    use super::wayland_clients::{Client, STEP};
    use super::weston_x11::Session;

    /// The output's size, which the kiosk shell gives every window.
    const OUTPUT: (u16, u16) = (320, 240);

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

    fn write(client: &Client, text: &str) {
        client.platform.clipboard().write_text(text.to_owned());
    }

    fn read(client: &Client) -> Option<std::sync::Arc<str>> {
        client.read(|answer| client.platform.clipboard().read_text(answer))
    }

    /// One client writes twice under one input, reading each back, then
    /// concealed under it; a second, whose window takes the
    /// keyboard, reads the first's selection through the compositor and
    /// writes its own; once the second's window closes and the keyboard is
    /// back, the first reads the second's text, then writes concealed under
    /// that new input and reads it back.
    fn the_selection_crosses_between_clients(session: &Session) {
        // Weston's X11 backend gives its seat's keyboard to a client while its
        // output window has the X focus, which the harness's X connection
        // gives it; the case holds that connection to its end.
        let screen = session.screen();
        let _focus = (screen.connection, screen.root, screen.origin);
        let first = Client::open("first");
        write(&first, "written by the first");
        assert_eq!(
            read(&first).as_deref(),
            Some("written by the first"),
            "a client reads back what it owns"
        );
        // A second write with no input between is under the same serial:
        // Weston up to 14 takes no second selection under it, so the source
        // already set answers the new text.
        write(&first, "written again by the first");
        assert_eq!(
            read(&first).as_deref(),
            Some("written again by the first"),
            "a client reads back its latest write"
        );
        // A concealed write under that serial offers another set of types,
        // which takes a new source. Weston up to 14 keeps the source it
        // holds, which offers no hint and so never answers the concealed
        // text: that write does not reach the clipboard. The first client
        // must follow which source the compositor kept: the one it dropped
        // is never cancelled, and owning it would keep the first reading its
        // own text after the second client copies.
        first
            .platform
            .clipboard()
            .write(ClipboardWrite::text("concealed by the first").concealed());
        first.turn();

        let (said, heard) = mpsc::channel();
        let (stop, stopped) = mpsc::channel::<()>();
        let second = std::thread::spawn(move || {
            let second = Client::open("second");
            let first_text = read(&second);
            let marked = selection_offers_concealed_hint(&second.platform);
            write(&second, "written by the second");
            // A turn sends the selection before the window closes.
            second.turn();
            second.window.close_for_cleanup();
            said.send((first_text, marked)).expect("the case listens");
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
        let (report, marked) = report;
        // Weston main and other compositors take the concealed write's
        // source; Weston up to 14 keeps the ordinary one. Either way the
        // concealed text goes out only through an offer marked concealed.
        let concealed_taken = marked == Some(true);
        assert_eq!(
            report.as_deref(),
            Some(if concealed_taken {
                "concealed by the first"
            } else {
                "written again by the first"
            }),
            "the second client reads the first's text through the compositor, \
             concealed only under the hint (marked: {marked:?})"
        );
        first.until("the keyboard to come back", || first.window.state().focused);
        assert_eq!(
            read(&first).as_deref(),
            Some("written by the second"),
            "the first client no longer owns the selection and reads the second's"
        );
        // The scenario every host runs: a concealed write pastes back and
        // stays out of the history (ADR-0152 §6). It runs under the input
        // the keyboard's return brought: Weston drops a selection set under
        // the serial of the one it replaces.
        let clipboard = first.platform.clipboard();
        assert_eq!(
            a_concealed_write_stays_out_of_history(&*clipboard, |clipboard| {
                first.read(|answer| clipboard.read_text(answer))
            }),
            Ok(())
        );
        stop.send(()).expect("the second client listens");
        second.join().expect("the second client ends");
        first.window.close_for_cleanup();
    }
}
