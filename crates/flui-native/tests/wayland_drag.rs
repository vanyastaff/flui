//! Drops on the native Wayland host's windows from another client of a real
//! compositor: a sway (wlroots) on an X server of the harness's own, whose
//! seat's pointer the harness drives through sway's own `seat cursor`
//! commands, so the compositor runs a real drag from the other client's
//! surface to the window.
//!
//! sway tiles the window the case opens first on the left half of its
//! output and the drag source opened after it on the right, its surface
//! a square centred in that half.
//!
//! Its own harness, as `wayland_primary_selection`: it starts Xvfb and a sway
//! before anything connects and names that sway for the process. A run
//! without Xvfb or sway is skipped, and fails where `FLUI_REQUIRE_WAYLAND`
//! is set.

fn main() {
    #[cfg(target_os = "linux")]
    cases::main();
}

#[cfg(target_os = "linux")]
#[path = "support/sway_x11.rs"]
#[expect(
    dead_code,
    reason = "the session's screen is the readback harnesses', which this one does not read"
)]
mod sway_x11;

#[cfg(target_os = "linux")]
#[path = "support/wayland_clients.rs"]
#[expect(
    dead_code,
    reason = "a clipboard read is the selection harnesses', which this one does not make"
)]
mod wayland_clients;

#[cfg(target_os = "linux")]
#[path = "support/wayland_drag_source.rs"]
mod wayland_drag_source;

#[cfg(target_os = "linux")]
mod cases {
    use std::process::{Command, Stdio};
    use std::sync::Arc;
    use std::task::{Context, Poll, Waker};
    use std::time::{Duration, Instant};

    use flui_foundation::DataTransferId;
    use flui_native::Platform as _;
    use flui_platform::data_transfer::{
        DropFeedback, RepresentationIndex, TransferActions, TransferError, TransferFormat,
        TransferLimits, TransferPayload, TransferRequest,
    };
    use flui_platform::{DragDropEvent, PlatformInput};
    use flui_testing::window_commands::AdmitAndApply as _;
    use wayland_client::protocol::wl_data_device_manager::DndAction;

    use super::sway_x11::Session;
    use super::wayland_clients::{Client, STEP};
    use super::wayland_drag_source::{DragSource, Heard, Offered};

    /// The size of the output window sway's X11 backend opens.
    const OUTPUT: (u16, u16) = (1024, 768);
    /// A point on the drag source's surface, which sway centres in the right
    /// half.
    const ON_SOURCE: (i16, i16) = (768, 384);
    /// A point on the window, in the left half.
    const ON_WINDOW: (i16, i16) = (256, 400);
    /// The plain text type the sources offer.
    const TEXT: &str = "text/plain;charset=utf-8";

    pub(super) fn main() {
        let mut arguments = libtest_mimic::Arguments::from_args();
        // The trials share one sway and its pointer.
        arguments.test_threads = Some(1);
        let session = Arc::new(
            (!arguments.list)
                .then(|| Session::start_sway(OUTPUT, "flui-wayland-drag"))
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
                "an_accepted_drop_is_fetched_and_finished",
                an_accepted_drop_is_fetched_and_finished,
            ),
            trial(
                "a_refused_drop_cancels_its_source",
                a_refused_drop_cancels_its_source,
            ),
            trial(
                "a_fetch_is_held_to_its_limit_and_its_deadline",
                a_fetch_is_held_to_its_limit_and_its_deadline,
            ),
        ];
        libtest_mimic::run(&arguments, trials).exit();
    }

    /// The pointer of sway's seat, driven through sway's own `seat cursor`
    /// commands, which move it and press its button where they say with no
    /// X pointer between.
    struct Pointer {
        socket: std::path::PathBuf,
    }

    impl Pointer {
        fn new() -> Self {
            let runtime = std::env::var_os("XDG_RUNTIME_DIR").expect("the harness names a runtime");
            let socket = std::fs::read_dir(runtime)
                .expect("the runtime directory")
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .find(|path| {
                    path.file_name()
                        .is_some_and(|name| name.to_string_lossy().starts_with("sway-ipc."))
                })
                .expect("sway's IPC socket");
            Self { socket }
        }

        fn cursor(&self, command: &[&str]) {
            let status = Command::new("swaymsg")
                .arg("-s")
                .arg(&self.socket)
                .args(["seat", "seat0", "cursor"])
                .args(command)
                .stdout(Stdio::null())
                .status()
                .expect("swaymsg runs");
            assert!(status.success(), "sway takes `cursor {command:?}`");
        }

        fn to(&self, at: (i16, i16)) {
            self.cursor(&["set", &at.0.to_string(), &at.1.to_string()]);
        }

        fn press(&self) {
            self.cursor(&["press", "button1"]);
        }

        fn release(&self) {
            self.cursor(&["release", "button1"]);
        }

        /// Move to `to` from `from` in steps, the window turning between
        /// them, as a hand moves.
        fn glide(&self, client: &Client, from: (i16, i16), to: (i16, i16)) {
            for step in 1..=8_i16 {
                self.to((
                    from.0 + (to.0 - from.0) * step / 8,
                    from.1 + (to.1 - from.1) * step / 8,
                ));
                client.turn();
            }
        }
    }

    /// The drag events the window heard.
    fn drags(client: &Client) -> Vec<DragDropEvent> {
        client
            .handler
            .inputs()
            .into_iter()
            .filter_map(|input| match input {
                PlatformInput::DragDrop(event) => Some(event),
                _ => None,
            })
            .collect()
    }

    /// The offer of the last `Entered` the window heard.
    fn entered(client: &Client) -> Option<DataTransferId> {
        drags(client).iter().rev().find_map(|event| match event {
            DragDropEvent::Entered { offer, .. } => Some(offer.id()),
            _ => None,
        })
    }

    /// Open the drag source beside the window, and turn the window until
    /// sway shows both: sway applies a new layout only once every window in
    /// it drew at its new size, and puts no pointer on the source before.
    fn open_source(target: &Client, offered: Offered) -> DragSource {
        let source = DragSource::open(offered);
        target.until("sway to tile the source beside the window", || {
            target.window.state().content_size.width < f64::from(OUTPUT.0) / 2.0
        });
        // The frame drawn at the new size reaches sway on the next turns.
        for _ in 0..10 {
            target.turn();
        }
        source
    }

    /// Start a drag from the source and bring it over the window, which
    /// hears `Entered`.
    fn drag_over(client: &Client, pointer: &Pointer, source: &DragSource) -> DataTransferId {
        // sway puts the pointer on the source once the layout that shows it
        // applied, which a loaded host may take a while for: move until the
        // source hears the pointer.
        let deadline = Instant::now() + STEP;
        let mut nudge = 0;
        while !source.heard().contains(&Heard::Pointed) {
            nudge = 1 - nudge;
            pointer.to((ON_SOURCE.0 + nudge, ON_SOURCE.1));
            client.turn();
            assert!(
                Instant::now() < deadline,
                "the pointer never reached the source; it heard {:?}",
                source.heard()
            );
        }
        pointer.press();
        until_heard(client, source, &Heard::Started);
        pointer.glide(client, ON_SOURCE, ON_WINDOW);
        client.until("the drag to enter the window", || entered(client).is_some());
        entered(client).expect("entered above")
    }

    /// Turn `client` until `request` answers.
    fn resolve(
        client: &Client,
        mut request: TransferRequest,
    ) -> Result<TransferPayload, TransferError> {
        let deadline = Instant::now() + STEP;
        let mut context = Context::from_waker(Waker::noop());
        loop {
            if let Poll::Ready(result) = std::pin::Pin::new(&mut request).poll(&mut context) {
                return result;
            }
            assert!(Instant::now() < deadline, "the request was never answered");
            client.turn();
        }
    }

    /// Turn `client` until `source` heard `what`.
    fn until_heard(client: &Client, source: &DragSource, what: &Heard) {
        let deadline = Instant::now() + STEP;

        loop {
            let heard = source.heard();
            if heard.contains(what) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the source never heard {what:?}; heard {heard:?}"
            );
            client.turn();
        }
    }

    /// A drag from another client enters the window, which hears where it
    /// moves; the source hears the type and action the window accepts once
    /// its target says so, the drop reaches the window with the action, its
    /// text is fetched, and concluding it finishes the source's drag.
    fn an_accepted_drop_is_fetched_and_finished(_: &Session) {
        let target = Client::open("target");
        let pointer = Pointer::new();
        let source = open_source(
            &target,
            Offered {
                types: vec![(TEXT.to_owned(), Some(b"dragged text".to_vec()))],
                actions: DndAction::Copy | DndAction::Move,
            },
        );
        let id = drag_over(&target, &pointer, &source);
        let Some(DragDropEvent::Entered {
            offer,
            allowed,
            position,
        }) = drags(&target)
            .into_iter()
            .find(|event| matches!(event, DragDropEvent::Entered { .. }))
        else {
            panic!("the window heard Entered");
        };
        assert_eq!(allowed, TransferActions::COPY | TransferActions::MOVE);
        assert_eq!(
            offer
                .representations()
                .first()
                .map(|described| &described.format),
            Some(&TransferFormat::Text),
            "the text is offered"
        );
        assert!(position.is_some(), "the drag enters at a place");
        let transfer = target.platform.data_transfer();
        transfer.update_drop_feedback(
            id,
            DropFeedback {
                accept: Some(TransferActions::COPY),
            },
        );
        until_heard(&target, &source, &Heard::Target(Some(TEXT.to_owned())));
        pointer.glide(&target, ON_WINDOW, (ON_WINDOW.0 + 40, ON_WINDOW.1 + 20));
        target.until("the drag to move", || {
            drags(&target).iter().any(
                |event| matches!(event, DragDropEvent::Moved { id: moved, .. } if *moved == id),
            )
        });
        pointer.release();
        target.until("the drop", || {
            drags(&target)
                .iter()
                .any(|event| matches!(event, DragDropEvent::Dropped { .. }))
        });
        let dropped = drags(&target)
            .into_iter()
            .find(|event| matches!(event, DragDropEvent::Dropped { .. }));
        assert!(
            matches!(
                dropped,
                Some(DragDropEvent::Dropped { id: dropped, action, .. })
                    if dropped == id && action == TransferActions::COPY
            ),
            "dropped with the action accepted: {dropped:?}"
        );
        until_heard(&target, &source, &Heard::DropPerformed);
        let payload = resolve(
            &target,
            transfer.request(id, RepresentationIndex(0), TransferLimits::default()),
        );
        assert!(
            matches!(&payload, Ok(TransferPayload::Text(text)) if text == "dragged text"),
            "the dropped text: {payload:?}"
        );
        transfer.conclude_drop(id);
        until_heard(&target, &source, &Heard::Finished);
        let stale = resolve(
            &target,
            transfer.request(id, RepresentationIndex(0), TransferLimits::default()),
        );
        assert!(
            matches!(stale, Err(TransferError::StaleOffer(stale)) if stale == id),
            "a concluded offer is stale: {stale:?}"
        );
        target.window.close_for_cleanup();
    }

    /// A drag that leaves the window ends its offer there; one dropped where
    /// the window's target refuses reaches it as `Exited`, and the
    /// compositor cancels the source's drag.
    fn a_refused_drop_cancels_its_source(_: &Session) {
        let target = Client::open("target");
        let pointer = Pointer::new();
        let source = open_source(
            &target,
            Offered {
                types: vec![(TEXT.to_owned(), Some(b"refused".to_vec()))],
                actions: DndAction::Copy,
            },
        );
        let first = drag_over(&target, &pointer, &source);
        pointer.glide(&target, ON_WINDOW, ON_SOURCE);
        target.until("the drag to leave", || {
            drags(&target)
                .iter()
                .any(|event| matches!(event, DragDropEvent::Exited { id } if *id == first))
        });
        pointer.glide(&target, ON_SOURCE, ON_WINDOW);
        target.until("the drag to enter again", || {
            entered(&target).is_some_and(|again| again != first)
        });
        let again = entered(&target).expect("entered above");
        pointer.release();
        target.until("the refused drop", || {
            drags(&target)
                .iter()
                .any(|event| matches!(event, DragDropEvent::Exited { id } if *id == again))
        });
        assert!(
            !drags(&target)
                .iter()
                .any(|event| matches!(event, DragDropEvent::Dropped { .. })),
            "a refused drop is no drop"
        );
        until_heard(&target, &source, &Heard::Cancelled);
        target.window.close_for_cleanup();
    }

    /// A fetch past its request's limit is refused as too large, and one
    /// whose source never writes gives up at its deadline.
    fn a_fetch_is_held_to_its_limit_and_its_deadline(_: &Session) {
        let target = Client::open("target");
        let pointer = Pointer::new();
        let source = open_source(
            &target,
            Offered {
                types: vec![
                    (TEXT.to_owned(), Some(b"longer than ten bytes".to_vec())),
                    ("application/x-flui-silent".to_owned(), None),
                ],
                actions: DndAction::Copy,
            },
        );
        let id = drag_over(&target, &pointer, &source);
        let transfer = target.platform.data_transfer();
        transfer.update_drop_feedback(
            id,
            DropFeedback {
                accept: Some(TransferActions::COPY),
            },
        );
        until_heard(&target, &source, &Heard::Target(Some(TEXT.to_owned())));
        let limited = resolve(
            &target,
            transfer.request(
                id,
                RepresentationIndex(0),
                TransferLimits::default().with_max_bytes(10),
            ),
        );
        assert!(
            matches!(limited, Err(TransferError::TooLarge { limit: 10, .. })),
            "past the limit: {limited:?}"
        );
        let started = Instant::now();
        let silent = resolve(
            &target,
            transfer.request(id, RepresentationIndex(1), TransferLimits::default()),
        );
        assert!(
            matches!(silent, Err(TransferError::SourceGone)),
            "a source that never writes: {silent:?}"
        );
        assert!(
            started.elapsed() >= Duration::from_secs(4),
            "given up at the deadline, not before"
        );
        pointer.release();
        target.until("the drop", || {
            drags(&target)
                .iter()
                .any(|event| matches!(event, DragDropEvent::Dropped { .. }))
        });
        transfer.conclude_drop(id);
        until_heard(&target, &source, &Heard::Finished);
        target.window.close_for_cleanup();
    }
}
