//! The native Wayland host's text input against a real compositor and a real
//! input method: a sway (wlroots) on an X server of the harness's own, which
//! routes `zwp_text_input_v3` to an input method speaking
//! `zwp_input_method_v2`, played here by a client of the test's own on a
//! second connection. Weston has no input-method-v2.
//!
//! What the input method composes, commits and deletes reaches the field's
//! store through the window's `zwp_text_input_v3`, and what the field holds
//! reaches the input method as its surrounding text.
//!
//! Its own harness, as `wayland_primary_selection`: it starts Xvfb and a sway
//! before anything connects and names that sway for the process. A run
//! without Xvfb or sway is skipped, and fails where `FLUI_REQUIRE_WAYLAND` is
//! set.

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
mod cases {
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::Instant;

    use flui_platform::text_store::utf16::{Utf16Offset, Utf16Range};
    use flui_platform::text_store::{Composition, Selection, TextStore};
    use flui_testing::text_store_kit::InMemoryTextStore;
    use wayland_client::protocol::wl_registry::{self, WlRegistry};
    use wayland_client::protocol::wl_seat::WlSeat;
    use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle};
    use wayland_protocols_misc::zwp_input_method_v2::client::zwp_input_method_manager_v2::ZwpInputMethodManagerV2;
    use wayland_protocols_misc::zwp_input_method_v2::client::zwp_input_method_v2::{
        self, ZwpInputMethodV2,
    };

    use super::sway_x11::Session;
    use super::wayland_clients::{Client, STEP};

    /// The size of the output window sway's X11 backend opens.
    const OUTPUT: (u16, u16) = (1024, 768);

    pub(super) fn main() {
        let mut arguments = libtest_mimic::Arguments::from_args();
        // The trials share one sway, its seat and its one input method.
        arguments.test_threads = Some(1);
        let session = Arc::new(
            (!arguments.list)
                .then(|| Session::start_sway(OUTPUT, "flui-wayland-text-input"))
                .flatten(),
        );
        let trial = |name: &'static str, case: fn()| {
            let session = Arc::clone(&session);
            libtest_mimic::Trial::test(name, move || {
                match session.as_ref() {
                    Some(_) => case(),
                    None => eprintln!("skipped: no Xvfb or sway to run against"),
                }
                Ok(())
            })
        };
        let trials = vec![
            trial(
                "an_input_method_composes_and_commits_into_the_field",
                an_input_method_composes_and_commits_into_the_field,
            ),
            trial(
                "an_input_method_deletes_whole_characters_around_the_caret",
                an_input_method_deletes_whole_characters_around_the_caret,
            ),
            trial(
                "the_input_method_hears_what_the_application_types",
                the_input_method_hears_what_the_application_types,
            ),
        ];
        libtest_mimic::run(&arguments, trials).exit();
    }

    // ------------------------------------------------------------------
    // The input method
    // ------------------------------------------------------------------

    /// What the input method was last told, as of its last `done`.
    #[derive(Debug, Default, Clone, PartialEq, Eq)]
    struct Told {
        active: bool,
        surrounding: Option<(String, u32, u32)>,
        purpose: Option<u32>,
    }

    #[derive(Default)]
    struct ImeState {
        manager: Option<ZwpInputMethodManagerV2>,
        seat: Option<WlSeat>,
        /// Pending since the last `done`.
        pending: Told,
        /// As of the last `done`.
        told: Told,
        /// How many `done`s came: the serial each commit names.
        dones: u32,
    }

    impl Dispatch<WlRegistry, ()> for ImeState {
        fn event(
            state: &mut Self,
            registry: &WlRegistry,
            event: wl_registry::Event,
            (): &(),
            _: &Connection,
            queue: &QueueHandle<Self>,
        ) {
            if let wl_registry::Event::Global {
                name, interface, ..
            } = event
            {
                if interface == "wl_seat" && state.seat.is_none() {
                    state.seat = Some(registry.bind(name, 1, queue, ()));
                } else if interface == "zwp_input_method_manager_v2" {
                    state.manager = Some(registry.bind(name, 1, queue, ()));
                }
            }
        }
    }

    impl Dispatch<WlSeat, ()> for ImeState {
        fn event(
            _: &mut Self,
            _: &WlSeat,
            _: wayland_client::protocol::wl_seat::Event,
            (): &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<ZwpInputMethodManagerV2, ()> for ImeState {
        fn event(
            _: &mut Self,
            _: &ZwpInputMethodManagerV2,
            _: wayland_protocols_misc::zwp_input_method_v2::client::zwp_input_method_manager_v2::Event,
            (): &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }

    impl Dispatch<ZwpInputMethodV2, ()> for ImeState {
        fn event(
            state: &mut Self,
            _: &ZwpInputMethodV2,
            event: zwp_input_method_v2::Event,
            (): &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            match event {
                zwp_input_method_v2::Event::Activate => {
                    // The protocol resets the state an activation brings.
                    state.pending = Told {
                        active: true,
                        ..Told::default()
                    };
                }
                zwp_input_method_v2::Event::Deactivate => {
                    state.pending.active = false;
                }
                zwp_input_method_v2::Event::SurroundingText {
                    text,
                    cursor,
                    anchor,
                } => state.pending.surrounding = Some((text, cursor, anchor)),
                zwp_input_method_v2::Event::ContentType { purpose, .. } => {
                    state.pending.purpose = Some(match purpose {
                        wayland_client::WEnum::Value(purpose) => purpose as u32,
                        wayland_client::WEnum::Unknown(purpose) => purpose,
                    });
                }
                zwp_input_method_v2::Event::Done => {
                    state.dones = state.dones.wrapping_add(1);
                    state.told = state.pending.clone();
                }
                zwp_input_method_v2::Event::Unavailable => {
                    panic!("another input method holds sway's seat");
                }
                _ => {}
            }
        }
    }

    /// An input method on a connection of its own.
    struct InputMethod {
        _connection: Connection,
        queue: EventQueue<ImeState>,
        state: ImeState,
        method: ZwpInputMethodV2,
    }

    impl InputMethod {
        fn connect() -> Self {
            let connection = Connection::connect_to_env().expect("sway answers");
            let mut queue = connection.new_event_queue();
            let handle = queue.handle();
            let _registry = connection.display().get_registry(&handle, ());
            let mut state = ImeState::default();
            queue.roundtrip(&mut state).expect("the globals");
            let manager = state.manager.clone().expect("sway offers input-method-v2");
            let seat = state.seat.clone().expect("sway has a seat");
            let method = manager.get_input_method(&seat, &handle, ());
            queue.roundtrip(&mut state).expect("the input method");
            Self {
                _connection: connection,
                queue,
                state,
                method,
            }
        }

        /// Read what came, without waiting.
        fn roundtrip(&mut self) {
            self.queue.roundtrip(&mut self.state).expect("sway answers");
        }

        /// Make what was sent current, naming the `done`s seen.
        fn commit(&mut self) {
            self.method.commit(self.state.dones);
            self.roundtrip();
        }
    }

    impl Drop for InputMethod {
        fn drop(&mut self) {
            self.method.destroy();
            let _ = self.queue.roundtrip(&mut self.state);
        }
    }

    // ------------------------------------------------------------------
    // Helpers
    // ------------------------------------------------------------------

    fn span(start: usize, end: usize) -> Utf16Range {
        Utf16Range::try_new(Utf16Offset::new(start), Utf16Offset::new(end)).expect("ordered")
    }

    /// Turn both sides until `done` holds, bounded at [`STEP`].
    fn until(client: &Client, ime: &mut InputMethod, what: &str, done: impl Fn(&Told) -> bool) {
        let deadline = Instant::now() + STEP;
        loop {
            client.turn();
            ime.roundtrip();
            if done(&ime.state.told) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
        }
    }

    /// Turn the client until `done` holds of the store.
    fn until_store(client: &Client, ime: &mut InputMethod, what: &str, done: impl Fn() -> bool) {
        let deadline = Instant::now() + STEP;
        loop {
            client.turn();
            ime.roundtrip();
            if done() {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
        }
    }

    /// A window whose field `store` has focus, and the input method it
    /// activated.
    fn field(title: &str, store: &Rc<InMemoryTextStore>) -> (Client, InputMethod) {
        let mut ime = InputMethod::connect();
        let client = Client::open(title);
        let host = flui_native::testing::wayland::text_store_host(client.window.as_ref())
            .expect("sway offers zwp_text_input_v3");
        host.focus_store(Some(Rc::clone(store) as Rc<dyn TextStore>));
        until(&client, &mut ime, "the input method to activate", |told| {
            told.active
        });
        (client, ime)
    }

    // ------------------------------------------------------------------
    // Cases
    // ------------------------------------------------------------------

    fn an_input_method_composes_and_commits_into_the_field() {
        let store = InMemoryTextStore::new("ab");
        let (client, mut ime) = field("compose", &store);
        assert_eq!(
            ime.state.told.surrounding,
            Some(("ab".to_owned(), 2, 2)),
            "the input method reads the field as it activates"
        );

        // "に" is three bytes: the caret after it.
        ime.method.set_preedit_string("に".to_owned(), 3, 3);
        ime.commit();
        until_store(&client, &mut ime, "the preedit", || {
            store.composition().is_some()
        });
        assert_eq!(store.text(), "abに");
        assert_eq!(
            store.composition(),
            Some(Composition {
                range: span(2, 3),
                hides_caret: false,
            })
        );

        ime.method.commit_string("日本".to_owned());
        ime.commit();
        until_store(&client, &mut ime, "the commit", || store.text() == "ab日本");
        assert_eq!(store.composition(), None, "the commit replaced the preedit");
        assert_eq!(store.selection(), Selection::collapsed(Utf16Offset::new(4)));
        // "ab日本" is eight bytes, and the input method hears the commit.
        until(
            &client,
            &mut ime,
            "the field's text after the commit",
            |told| told.surrounding == Some(("ab日本".to_owned(), 8, 8)),
        );
    }

    fn an_input_method_deletes_whole_characters_around_the_caret() {
        let store = InMemoryTextStore::new("aéb");
        store.app_select(Selection::collapsed(Utf16Offset::new(2)));
        let (client, mut ime) = field("delete", &store);
        assert_eq!(ime.state.told.surrounding, Some(("aéb".to_owned(), 3, 3)));

        // `é` is two bytes before the caret, `b` one after.
        ime.method.delete_surrounding_text(2, 1);
        ime.commit();
        until_store(&client, &mut ime, "the deletion", || store.text() == "a");
        assert_eq!(store.selection(), Selection::collapsed(Utf16Offset::new(1)));
    }

    fn the_input_method_hears_what_the_application_types() {
        let store = InMemoryTextStore::new("");
        let (client, mut ime) = field("typed", &store);
        store.app_replace(span(0, 0), "héllo");
        until(&client, &mut ime, "the application's text", |told| {
            told.surrounding == Some(("héllo".to_owned(), 6, 6))
        });
    }
}
