//! Drag and drop between the native Wayland host's windows and another client of a real
//! compositor: a sway (wlroots) on an X server of the harness's own, whose
//! seat's pointer the harness drives through sway's own `seat cursor`
//! commands, so the compositor runs a real drag from the other client's
//! surface to the window.
//!
//! sway tiles the window the case opens first on the left half of its
//! output and the other client opened after it on the right, its surface
//! drawn at the size of that half.
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
#[path = "support/wayland_drag_peer.rs"]
mod wayland_drag_peer;

#[cfg(target_os = "linux")]
mod cases {
    use std::process::{Command, Stdio};
    use std::sync::Arc;
    use std::task::{Context, Poll, Waker};
    use std::time::{Duration, Instant};

    use std::cell::Cell;
    use std::num::NonZeroU64;
    use std::rc::Rc;

    use flui_foundation::{DataTransferId, DragSessionId, UiRuntimeId};
    use flui_native::Platform as _;
    use flui_platform::data_transfer::{
        DropFeedback, RepresentationDescriptor, RepresentationIndex, TransferActions,
        TransferError, TransferFormat, TransferLimits, TransferPayload, TransferRequest,
    };
    use flui_platform::drag::{DragSessionKey, OsDragEnd, OsDragRequest, ProcessNonce};
    use flui_platform::{DragDropEvent, PlatformInput, PointerEvent};
    use flui_testing::window_commands::AdmitAndApply as _;
    use wayland_client::protocol::wl_data_device_manager::DndAction;

    use super::sway_x11::Session;
    use super::wayland_clients::{Client, STEP};
    use super::wayland_drag_peer::{Heard, Offered, Peer, Role, Wanted};

    /// The size of the output window sway's X11 backend opens.
    const OUTPUT: (u16, u16) = (1024, 768);
    /// A point on the peer's surface, which fills the right half sway
    /// tiles it in.
    const ON_PEER: (i16, i16) = (768, 384);
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
            trial(
                "a_session_from_the_window_drops_on_another_client",
                a_session_from_the_window_drops_on_another_client,
            ),
            trial(
                "a_refused_session_ends_cancelled",
                a_refused_session_ends_cancelled,
            ),
            trial(
                "a_session_after_the_release_ends_cancelled",
                a_session_after_the_release_ends_cancelled,
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

        /// sway's tree as `get_tree` prints it, for a failure to show where
        /// sway put the windows.
        fn layout(&self) -> String {
            Command::new("swaymsg")
                .arg("-s")
                .arg(&self.socket)
                .args(["-r", "-t", "get_tree"])
                .output()
                .map_or_else(
                    |error| format!("nothing: {error}"),
                    |output| String::from_utf8_lossy(&output.stdout).into_owned(),
                )
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

    /// Open the peer beside the window, and turn the window until
    /// sway shows both: sway applies a new layout only once every window in
    /// it drew at its new size, and puts no pointer on the peer before.
    fn open_peer(target: &Client, role: Role) -> Peer {
        let peer = Peer::open(role);
        target.until("sway to tile the peer beside the window", || {
            target.window.state().content_size.width < f64::from(OUTPUT.0) / 2.0
        });
        // The frame drawn at the new size reaches sway on the next turns.
        for _ in 0..10 {
            target.turn();
        }
        peer
    }

    /// Start a drag from the source and bring it over the window, which
    /// hears `Entered`.
    fn drag_over(client: &Client, pointer: &Pointer, source: &Peer) -> DataTransferId {
        // sway puts the pointer on the source once the layout that shows it
        // applied, which a loaded host may take a while for: move until the
        // source hears the pointer.
        let deadline = Instant::now() + STEP;
        let mut nudge = 0;
        while !source.heard().contains(&Heard::Pointed) {
            nudge = 1 - nudge;
            pointer.to((ON_PEER.0 + nudge, ON_PEER.1));
            client.turn();
            assert!(
                Instant::now() < deadline,
                "the pointer never reached the source; it heard {:?}; sway laid out {}",
                source.heard(),
                pointer.layout()
            );
        }
        pointer.press();
        until_heard(client, source, &Heard::Started);
        pointer.glide(client, ON_PEER, ON_WINDOW);
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
    fn until_heard(client: &Client, source: &Peer, what: &Heard) {
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
        let source = open_peer(
            &target,
            Role::Source(Offered {
                types: vec![(TEXT.to_owned(), Some(b"dragged text".to_vec()))],
                actions: DndAction::Copy | DndAction::Move,
            }),
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
        let source = open_peer(
            &target,
            Role::Source(Offered {
                types: vec![(TEXT.to_owned(), Some(b"refused".to_vec()))],
                actions: DndAction::Copy,
            }),
        );
        let first = drag_over(&target, &pointer, &source);
        pointer.glide(&target, ON_WINDOW, ON_PEER);
        target.until("the drag to leave", || {
            drags(&target)
                .iter()
                .any(|event| matches!(event, DragDropEvent::Exited { id } if *id == first))
        });
        pointer.glide(&target, ON_PEER, ON_WINDOW);
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
        let source = open_peer(
            &target,
            Role::Source(Offered {
                types: vec![
                    (TEXT.to_owned(), Some(b"longer than ten bytes".to_vec())),
                    ("application/x-flui-silent".to_owned(), None),
                ],
                actions: DndAction::Copy,
            }),
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
            matches!(silent, Err(TransferError::TimedOut)),
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

    /// The text a window's sessions offer.
    const DRAGGED: &[u8] = b"dragged from the window";

    /// A session offering text and its identity, allowing a copy or a move,
    /// counting the provider's answers in `served` and telling `ended` how
    /// it ended.
    fn session(served: &Rc<Cell<u32>>, ended: &Rc<Cell<Option<OsDragEnd>>>) -> OsDragRequest {
        let key = DragSessionKey::new(
            ProcessNonce::from_raw(NonZeroU64::new(0xF1A1).expect("non-zero")),
            UiRuntimeId::new_gen(
                3,
                flui_foundation::generation::Generation::FIRST
                    .next()
                    .expect("generation two"),
            ),
            DragSessionId::from_wire(NonZeroU64::new(7).expect("non-zero")),
        );
        let (served, ended) = (Rc::clone(served), Rc::clone(ended));
        OsDragRequest {
            key,
            pointer: None,
            representations: vec![
                RepresentationDescriptor {
                    format: TransferFormat::Text,
                    declared_len: None,
                },
                key.representation(),
            ],
            allowed: TransferActions::COPY | TransferActions::MOVE,
            provider: Box::new(move |RepresentationIndex(index)| {
                served.set(served.get() + 1);
                match index {
                    0 => Ok(TransferPayload::Text(
                        String::from_utf8(DRAGGED.to_vec()).expect("UTF-8"),
                    )),
                    _ => Err(TransferError::Cancelled),
                }
            }),
            on_end: Box::new(move |end| ended.set(Some(end))),
        }
    }

    /// Press on the window and start a session from it under that press,
    /// once the press reached the window.
    fn start_session(
        client: &Client,
        pointer: &Pointer,
        served: &Rc<Cell<u32>>,
        ended: &Rc<Cell<Option<OsDragEnd>>>,
    ) {
        let drag = client
            .window
            .drag_source()
            .expect("a Wayland window starts sessions");
        assert!(
            drag.start(session(served, ended)).is_err(),
            "no session starts with no button held"
        );
        pointer.to(ON_WINDOW);
        client.turn();
        pointer.press();
        client.until("a session to start under the press", || {
            drag.start(session(served, ended)).is_ok()
        });
    }

    /// Turn `client` until `peer` heard something `what` matches.
    fn until_peer(client: &Client, peer: &Peer, what: &str, matches: impl Fn(&Heard) -> bool) {
        let deadline = Instant::now() + STEP;
        loop {
            let heard = peer.heard();
            if heard.iter().any(&matches) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the peer never heard {what}; heard {heard:?}"
            );
            client.turn();
        }
    }

    /// A session started from a press on the window carries its text to
    /// another client: the client sees the types it offers, takes a copy at
    /// the drop, reads the text the provider serves, and the session ends
    /// dropped with the copy.
    fn a_session_from_the_window_drops_on_another_client(_: &Session) {
        let client = Client::open("drag source");
        let pointer = Pointer::new();
        let peer = open_peer(
            &client,
            Role::Target(Wanted {
                mime: Some(TEXT.to_owned()),
                action: DndAction::Copy,
            }),
        );
        let (served, ended) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(None)));
        start_session(&client, &pointer, &served, &ended);
        pointer.glide(&client, ON_WINDOW, ON_PEER);
        until_peer(
            &client,
            &peer,
            "the session enter",
            |heard| matches!(heard, Heard::Entered(types) if types.iter().any(|offered| offered == TEXT)),
        );
        pointer.release();
        until_peer(&client, &peer, "the text", |heard| {
            *heard == Heard::Received(DRAGGED.to_vec())
        });
        client.until("the session to end", || ended.get().is_some());
        assert_eq!(
            ended.get(),
            Some(OsDragEnd::Dropped(TransferActions::COPY)),
            "dropped with the copy the client took"
        );
        assert_eq!(served.get(), 1, "the provider served the one read");
        client.window.close_for_cleanup();
    }

    /// A session released over a client that takes none of its types ends
    /// cancelled, its provider never asked.
    fn a_refused_session_ends_cancelled(_: &Session) {
        let client = Client::open("drag source");
        let pointer = Pointer::new();
        let peer = open_peer(
            &client,
            Role::Target(Wanted {
                mime: None,
                action: DndAction::Copy,
            }),
        );
        let (served, ended) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(None)));
        start_session(&client, &pointer, &served, &ended);
        pointer.glide(&client, ON_WINDOW, ON_PEER);
        until_peer(&client, &peer, "the session enter", |heard| {
            matches!(heard, Heard::Entered(_))
        });
        pointer.release();
        client.until("the session to end", || ended.get().is_some());
        assert_eq!(ended.get(), Some(OsDragEnd::Cancelled));
        assert_eq!(served.get(), 0, "nothing was read");
        client.window.close_for_cleanup();
    }

    /// A session started under a press the user already let go of, which
    /// the window had not yet heard, ends cancelled: sway cancels the source
    /// of a drag it refuses, and a compositor that says nothing has the
    /// session end as the release reaches the window.
    fn a_session_after_the_release_ends_cancelled(_: &Session) {
        let client = Client::open("drag source");
        let pointer = Pointer::new();
        let drag = client
            .window
            .drag_source()
            .expect("a Wayland window starts sessions");
        pointer.to(ON_WINDOW);
        client.turn();
        pointer.press();
        client.until("the press to reach the window", || {
            client.handler.inputs().iter().any(|input| match input {
                PlatformInput::Pointer(PointerEvent::Down(_)) => true,
                PlatformInput::PointerFrame(frame) => frame
                    .events()
                    .iter()
                    .any(|event| matches!(event, PointerEvent::Down(_))),
                _ => false,
            })
        });
        // sway has the release before the window reads it.
        pointer.release();
        let (served, ended) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(None)));
        drag.start(session(&served, &ended))
            .expect("the window has heard no release yet");
        client.until("the session to end", || ended.get().is_some());
        assert_eq!(ended.get(), Some(OsDragEnd::Cancelled));
        client.window.close_for_cleanup();
    }
}
