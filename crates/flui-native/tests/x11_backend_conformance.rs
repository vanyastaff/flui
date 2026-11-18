//! The native X11 host's window against the backend conformance table of
//! `flui-testing` (ADR-0215 §7): each step the server can produce is
//! injected through XTest into a real window under the harness's window
//! manager, and what the window's handler hears is the row's input. The
//! decoder the host reads XInput 2 through runs the whole table on every host
//! as `native x11` (`backend_conformance`); this case shows the server, the
//! host's event selection and its translation in front of that decoder
//! deliver what the table asks.
//!
//! A row the server cannot take part in is named in [`UNEXERCISED`] with what
//! it reports and why; every other row must hold on input the window
//! delivered.

use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Duration;

use flui_native::testing::x11::run_for;
use flui_platform::WindowCommand;
use flui_platform::pointer::PointerKind;
use flui_platform::text_store::TextStore;
use flui_testing::backend_conformance::{
    BackendUnderTest, ClockProfile, ConformanceReport, ContactAction, ContactStep, FailureKind,
    HostInput, RowOutcome, Step, StepKey, TextInputPath, run_backend_conformance,
};
use flui_testing::window_commands::AdmitAndApply as _;
use flui_testing::window_conformance::WindowFixture as _;
use flui_testing::window_handler::{HandlerCall, RecordingWindowHandler};
use x11rb::connection::Connection as _;
use x11rb::protocol::xinput::{
    self, ChangeMode, ConnectionExt as _, HierarchyChange, HierarchyChangeData,
    HierarchyChangeDataAddMaster, HierarchyChangeDataRemoveMaster,
};
use x11rb::protocol::xproto::{
    BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT, ConnectionExt as _, KEY_PRESS_EVENT,
    KEY_RELEASE_EVENT, MOTION_NOTIFY_EVENT,
};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

use crate::cases::X11Fixture;
use crate::x11_input::{keycode, park_pointer};

pub(crate) const CASES: &[(&str, fn())] = &[(
    "the_x11_window_input_conforms",
    the_x11_window_input_conforms,
)];

/// Why the IME rows find no input method: none runs on the harness's server.
const NO_INPUT_METHOD: &str = "no input method runs on the harness's X server (no IBus or Fcitx5 \
     daemon, no XIM server); the X11 host's contexts run against stand-ins in \
     `text::linux::x11`, and the text-input events they become run through the table as `native x11`";

/// What a row the real window does not exercise reports.
#[derive(Debug, Clone, Copy)]
enum Unexercised {
    /// The row does not apply, in the row's own words.
    NotApplicable(&'static str),
    /// The row fails, for this kind of reason.
    Fails(FailureKind),
    /// The row holds, but no input of its reached the window as the row
    /// meant it: what it checks is not shown by this window.
    HoldsWithoutInput,
}

/// The rows the real window does not exercise, each with what it reports and
/// the reason. A row that reports otherwise fails the case: the entry is then
/// stale and goes. A `HoldsWithoutInput` entry is the exception: a row that
/// starts to take input from the window still holds, so that entry cannot be
/// found stale, and goes when the row is made to take the window's input.
/// Every other row holds on input the window delivered.
const UNEXERCISED: &[(&str, Unexercised, &str)] = &[
    (
        "a_reused_touch_label_is_a_new_pointer",
        Unexercised::NotApplicable("the backend has no touch"),
        "Xvfb lists no device with a touch class and XTest sends no touch; XInput 2 touches are \
         held by the decoder (`native x11`)",
    ),
    (
        "a_second_touch_is_additional",
        Unexercised::NotApplicable("the backend has no touch"),
        "Xvfb lists no device with a touch class and XTest sends no touch",
    ),
    (
        "event_times_follow_the_reading_clock",
        Unexercised::Fails(FailureKind::Interval),
        "XTest carries no stamp, only a delay: the server stamps an event when it handles the \
         request, and the window's loop turns between the steps, so the times are the server's \
         and not the step's; the decoder holds the row with the step's stamps",
    ),
    (
        "event_times_never_go_back",
        Unexercised::HoldsWithoutInput,
        "the step's earlier stamps never reach the window: the server stamps each event as it \
         handles the request, in order, so the times hold without a stamp that went back; the \
         decoder holds the floor with the step's stamps",
    ),
    (
        "the_coarse_clock_wraps_forward",
        Unexercised::Fails(FailureKind::Interval),
        "the server's 32-bit millisecond time wraps after 49 days of uptime, not inside a run; \
         the decoder holds the wrap with the step's stamps",
    ),
    (
        "precise_readings_are_stamped_from_the_counter",
        Unexercised::NotApplicable("the backend has no high-resolution counter"),
        "an X server stamps its events in milliseconds only",
    ),
    (
        "a_sequence_keeps_the_clock_it_started_on",
        Unexercised::NotApplicable("the backend has no high-resolution counter"),
        "an X server stamps its events in milliseconds only",
    ),
    (
        "a_device_whose_reading_lacked_the_counter_stays_coarse",
        Unexercised::NotApplicable("the backend has no high-resolution counter"),
        "an X server stamps its events in milliseconds only",
    ),
    (
        "two_finger_pan_is_phased_scroll_on_every_backend",
        Unexercised::NotApplicable("the backend claims no precise scrolling"),
        "XInput 2's smooth scrolling has no phases, so an X11 window claims none",
    ),
    (
        "pan_zoom_carries_only_scale_and_rotation",
        Unexercised::Fails(FailureKind::NoReading),
        "XInput 2.4's pinch events come only from a touchpad's input driver, which Xvfb does not \
         load, and XTest sends none; the decoder holds the row",
    ),
    (
        "an_undecodable_reading_cancels_its_contact_as_invalid_input",
        Unexercised::Fails(FailureKind::NoReading),
        "the server sends positions in fixed point, always finite; the decoder's refusal of a \
         position that is not is held as `native x11`",
    ),
    (
        "pen_is_delivered_as_claimed",
        Unexercised::Fails(FailureKind::NoReading),
        "XTest's devices report only x and y, and Xvfb loads no tablet driver: no pen reaches \
         the window, while an X11 window claims XInput 2's pens; the decoder holds the row",
    ),
    (
        "precise_scroll_is_delivered_as_claimed",
        Unexercised::HoldsWithoutInput,
        "the window claims no precise scrolling and has no reading for a pan, which is what the \
         row asks of it; no scroll reaches the window",
    ),
    (
        "trackpad_gestures_are_delivered_as_claimed",
        Unexercised::Fails(FailureKind::NoReading),
        "Xvfb announces XInput 2.4, so the window claims touchpad gestures, but only a touchpad's \
         input driver sends them; the decoder holds the row",
    ),
    (
        "outgoing_drag_is_delivered_as_claimed",
        Unexercised::HoldsWithoutInput,
        "the row compares the window's capability snapshot with its drag source; no drag runs",
    ),
    (
        "a_drag_source_starts_when_the_backend_declares",
        Unexercised::HoldsWithoutInput,
        "the row compares the window's drag source's timing with the one declared; no drag runs \
         (`x11_input`'s XTest drags run XDND sessions)",
    ),
    (
        "a_leaving_backend_escalates_when_the_pointer_leaves_the_window",
        Unexercised::HoldsWithoutInput,
        "the row plans a session from the drag source's timing; no pointer leaves the window \
         (`an_xtest_drag_from_the_window_drops_on_another_client` does)",
    ),
    (
        "a_backend_never_queues",
        Unexercised::HoldsWithoutInput,
        "the row asks the window's actuator, which no input reaches; an X11 window has none",
    ),
    (
        "semantic_feedback_reports_a_tier_one_outcome",
        Unexercised::HoldsWithoutInput,
        "the row asks the window's actuator, which no input reaches; an X11 window has none",
    ),
    (
        "a_stale_source_plays_on_the_window_actuator",
        Unexercised::HoldsWithoutInput,
        "the row asks the window's actuator, which no input reaches; an X11 window has none",
    ),
    (
        "capabilities_match_what_plays",
        Unexercised::HoldsWithoutInput,
        "the row asks the window's actuator, which no input reaches; an X11 window has none",
    ),
    (
        "an_at_start_backend_never_escalates_mid_drag",
        Unexercised::NotApplicable(
            "the backend starts its session when the pointer leaves the window",
        ),
        "an X11 window starts its XDND session as the pointer leaves it",
    ),
    (
        "start_decision_includes_sibling_presentation_with_identity_only_offer",
        Unexercised::NotApplicable(
            "the backend starts its session when the pointer leaves the window",
        ),
        "an X11 window starts its XDND session as the pointer leaves it",
    ),
    (
        "a_key_during_composition_is_the_input_methods",
        Unexercised::NotApplicable("the backend has no input-method key path the table drives"),
        NO_INPUT_METHOD,
    ),
    (
        "a_composition_reaches_the_field_in_order",
        Unexercised::NotApplicable(NO_INPUT_METHOD),
        NO_INPUT_METHOD,
    ),
    (
        "a_cancelled_composition_leaves_the_field_as_it_was",
        Unexercised::NotApplicable(NO_INPUT_METHOD),
        NO_INPUT_METHOD,
    ),
    (
        "each_composition_step_is_one_edit_of_the_field",
        Unexercised::NotApplicable(NO_INPUT_METHOD),
        NO_INPUT_METHOD,
    ),
];

/// The real X11 window against the table: every row holds on input the
/// window delivered, except those [`UNEXERCISED`] names, each as it says.
fn the_x11_window_input_conforms() {
    park_pointer();
    let mut window = X11Window::open();
    let report = run_backend_conformance(&mut window);
    drop(window);
    let wrong = unexpected(&report);
    assert!(
        wrong.is_empty(),
        "the X11 window's rows:\n{}",
        wrong.join("\n")
    );
    let exercised: Vec<&str> = report
        .rows
        .iter()
        .map(|(row, _, _)| *row)
        .filter(|row| !UNEXERCISED.iter().any(|(name, _, _)| name == row))
        .collect();
    eprintln!("rows the real window exercised: {}", exercised.join(", "));
}

/// Each row of `report` that is not as [`UNEXERCISED`] says, and each entry
/// of it that names no row.
fn unexpected(report: &ConformanceReport) -> Vec<String> {
    let mut wrong: Vec<String> = UNEXERCISED
        .iter()
        .filter(|(row, _, _)| report.outcome(row).is_none())
        .map(|(row, _, _)| format!("  - {row}: listed, but the table has no such row"))
        .collect();
    for (row, _, outcome) in &report.rows {
        let entry = UNEXERCISED.iter().find(|(name, _, _)| name == row);
        let as_expected = match (entry.map(|(_, said, _)| *said), outcome) {
            (None | Some(Unexercised::HoldsWithoutInput), RowOutcome::Held) => true,
            (Some(Unexercised::NotApplicable(said)), RowOutcome::NotApplicable(why)) => {
                said == *why
            }
            (Some(Unexercised::Fails(kind)), RowOutcome::Failed(failure)) => failure.kind == kind,
            _ => false,
        };
        if !as_expected {
            let expected = entry.map_or_else(
                || "Held".to_owned(),
                |(_, said, because)| format!("{said:?}, because {because}"),
            );
            wrong.push(format!("  - {row}: {outcome:?}; expected {expected}"));
        }
    }
    wrong
}

/// What the server is asked to do for one step, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Request {
    /// XTest's next pointer requests move the master pointer of the step's
    /// device, the server's own for none. A device not seen yet is a master
    /// pointer the server adds, and announces.
    Pointer(Option<u32>),
    /// The pointer goes to where the step presses, since XTest presses where
    /// the pointer is: the hover it delivers is not the step's.
    Approach(i16, i16),
    /// The pointer moves to this point of the window's content.
    Motion(i16, i16),
    /// The primary button goes down (`true`) or up.
    Button(bool),
    /// Shift is held (`true`) or let go around a key: the state the step
    /// names, whose own key events are not the step's.
    Shift(bool),
    /// A key, pressed and released.
    Key(u8),
    /// Another window of the test's own client takes the focus, as a click
    /// on it would: the window's contacts end.
    FocusAway,
}

impl Request {
    /// Whether what the window delivers for this request is the step's.
    const fn is_the_steps(self) -> bool {
        !matches!(self, Self::Approach(..) | Self::Shift(_))
    }
}

/// The native X11 window as the table drives it: XTest on a connection of
/// its own, whose client pointer picks the master pointer XTest moves.
struct X11Window {
    fixture: X11Fixture,
    handler: Rc<RecordingWindowHandler>,
    xtest: RustConnection,
    root: u32,
    /// The window's content origin on the root window.
    origin: (i16, i16),
    capabilities: flui_platform::input_capabilities::InputCapabilities,
    /// The server's own master pointer.
    core_pointer: u16,
    /// The master pointer XTest moves, once a request picked it.
    client_pointer: Option<u16>,
    /// The master pointer added for each device a step named.
    masters: BTreeMap<u32, u16>,
    /// Whether the primary button is down.
    pressed: bool,
    /// Whether another window took the focus.
    focus_away: bool,
}

impl X11Window {
    fn open() -> Self {
        let handler = RecordingWindowHandler::new();
        let mut fixture = X11Fixture::open_with("backend conformance", Rc::clone(&handler));
        fixture
            .window
            .admit_and_apply(WindowCommand::Activate)
            .expect("the window activates");
        fixture.settle();
        assert!(
            fixture.window.state().focus.is_focused(),
            "the window has the focus"
        );
        let capabilities = handler
            .inputs()
            .into_iter()
            .find_map(|input| match input {
                flui_platform::PlatformInput::Capabilities(capabilities) => Some(capabilities),
                _ => None,
            })
            .expect("the snapshot is the handler's first input");
        let (xtest, screen) = RustConnection::connect(None).expect("the X server answers");
        let root = xtest.setup().roots[screen].root;
        // A client says which XInput 2 it speaks before its first request.
        xtest
            .xinput_xi_query_version(2, 2)
            .expect("request")
            .reply()
            .expect("XInput 2");
        // The hierarchy events name each master pointer the server adds.
        xtest
            .xinput_xi_select_events(
                root,
                &[xinput::EventMask {
                    deviceid: u16::from(xinput::Device::ALL),
                    mask: vec![xinput::XIEventMask::HIERARCHY],
                }],
            )
            .expect("request")
            .check()
            .expect("the server selects hierarchy events");
        let origin = xtest
            .translate_coordinates(fixture.xid, root, 0, 0)
            .expect("request")
            .reply()
            .expect("the content's place");
        let core_pointer = master_named(&xtest, b"Virtual core pointer");
        handler.take_calls();
        Self {
            fixture,
            handler,
            xtest,
            root,
            origin: (origin.dst_x, origin.dst_y),
            capabilities,
            core_pointer,
            client_pointer: None,
            masters: BTreeMap::new(),
            pressed: false,
            focus_away: false,
        }
    }

    /// A point of the window's content on the root window.
    fn on_root(&self, (x, y): (i16, i16)) -> (i16, i16) {
        (self.origin.0 + x, self.origin.1 + y)
    }

    /// Add a master pointer for the step's `device`, and its id, kept in
    /// [`Self::masters`] as soon as the server names it, so the restart or
    /// the drop removes it whatever fails after.
    fn add_master(&mut self, device: u32) -> u16 {
        let name = format!("device {device}").into_bytes();
        // The server names a master pointer after the name it was added by.
        let pointer_name = [name.as_slice(), b" pointer"].concat();
        // The lengths are in 4-byte units: the header and the fields, then
        // the name, padded.
        let len = 2 + u16::try_from(name.len().div_ceil(4)).expect("a short name");
        self.xtest
            .xinput_xi_change_hierarchy(&[HierarchyChange {
                len,
                data: HierarchyChangeData::AddMaster(HierarchyChangeDataAddMaster {
                    send_core: true,
                    enable: true,
                    name,
                }),
            }])
            .expect("request")
            .check()
            .expect("the server adds a master pointer");
        // The check's round trip came back after the change's event, so the
        // event is queued. Another client may have added a master pointer in
        // the meantime: the one added here is the one of this name.
        let mut added = Vec::new();
        while let Some(event) = self.xtest.poll_for_event().expect("the connection") {
            if let x11rb::protocol::Event::XinputHierarchy(change) = event {
                added.extend(
                    change
                        .infos
                        .iter()
                        .filter(|info| {
                            info.flags.contains(xinput::HierarchyMask::MASTER_ADDED)
                                && info.type_ == xinput::DeviceType::MASTER_POINTER
                        })
                        .map(|info| info.deviceid),
                );
            }
        }
        let master = added
            .into_iter()
            .find(|id| {
                self.xtest
                    .xinput_xi_query_device(*id)
                    .ok()
                    .and_then(|cookie| cookie.reply().ok())
                    .is_some_and(|reply| reply.infos.iter().any(|info| info.name == pointer_name))
            })
            .expect("the server names the master pointer it added");
        self.masters.insert(device, master);
        master
    }

    /// One XTest request: an event of `kind` with `detail` at `at` of the
    /// root window.
    fn fake(&self, kind: u8, detail: u8, at: (i16, i16)) {
        self.xtest
            .xtest_fake_input(kind, detail, 0, self.root, at.0, at.1, 0)
            .expect("request");
    }

    /// Ask the server for `request`, and turn the window's loop until it has
    /// handled what the server sent for it.
    fn send(&mut self, request: Request) {
        match request {
            Request::Pointer(device) => {
                let master = match device {
                    None => self.core_pointer,
                    Some(device) => self
                        .masters
                        .get(&device)
                        .copied()
                        .unwrap_or_else(|| self.add_master(device)),
                };
                if self.client_pointer == Some(master) {
                    return;
                }
                self.xtest
                    .xinput_xi_set_client_pointer(x11rb::NONE, master)
                    .expect("request");
                self.client_pointer = Some(master);
            }
            Request::Approach(x, y) | Request::Motion(x, y) => {
                self.fake(MOTION_NOTIFY_EVENT, 0, self.on_root((x, y)));
            }
            Request::Button(down) => {
                let kind = if down {
                    BUTTON_PRESS_EVENT
                } else {
                    BUTTON_RELEASE_EVENT
                };
                self.fake(kind, 1, (0, 0));
                self.pressed = down;
            }
            Request::Shift(down) => {
                let kind = if down {
                    KEY_PRESS_EVENT
                } else {
                    KEY_RELEASE_EVENT
                };
                self.fake(kind, keycode::SHIFT_L, (0, 0));
            }
            Request::Key(key) => {
                self.fake(KEY_PRESS_EVENT, key, (0, 0));
                self.fake(KEY_RELEASE_EVENT, key, (0, 0));
            }
            Request::FocusAway => {
                assert!(self.fixture.take_focus_away(), "another window takes it");
                self.focus_away = true;
            }
        }
        // A round trip: the server has handled the requests and sent the
        // window what they made.
        self.xtest
            .get_input_focus()
            .expect("request")
            .reply()
            .expect("the server answers");
        run_for(&self.fixture.platform, Duration::from_millis(40)).expect("the loop turns");
    }

    /// The input the window's handler heard since it was last asked.
    fn heard(&self) -> impl Iterator<Item = HostInput> + use<> {
        self.handler
            .take_calls()
            .into_iter()
            .filter_map(|call| match call {
                HandlerCall::Input(input) => Some(HostInput::Platform(input)),
                _ => None,
            })
    }

    /// The requests that perform a contact step of the server's pointer.
    fn contact(step: ContactStep) -> Option<Vec<Request>> {
        if step.kind != PointerKind::Mouse || !(step.x.is_finite() && step.y.is_finite()) {
            return None;
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the table's points are small whole numbers inside the window"
        )]
        let at = (step.x as i16, step.y as i16);
        let device = Request::Pointer(step.device);
        let requests = match step.action {
            ContactAction::Press => {
                vec![device, Request::Approach(at.0, at.1), Request::Button(true)]
            }
            ContactAction::Move => vec![device, Request::Motion(at.0, at.1)],
            ContactAction::Release => vec![device, Request::Button(false)],
            ContactAction::Cancel => vec![device, Request::FocusAway],
            _ => return None,
        };
        Some(requests)
    }
}

impl BackendUnderTest for X11Window {
    type Reading = Vec<Request>;

    fn capabilities(&self) -> flui_platform::input_capabilities::InputCapabilities {
        self.capabilities
    }

    /// The server's time: 32-bit milliseconds.
    fn clock(&self) -> ClockProfile {
        ClockProfile::coarse(Duration::from_millis(1))
            .wrapping_after(Duration::from_millis(1 << 32))
    }

    fn restart(&mut self) {
        if self.pressed {
            self.send(Request::Button(false));
        }
        if !self.masters.is_empty() {
            for (_, master) in std::mem::take(&mut self.masters) {
                remove_master(&self.xtest, master);
            }
            // Picked again below, which turns the loop over the removals.
            self.client_pointer = None;
        }
        self.send(Request::Pointer(None));
        if self.focus_away {
            self.fixture
                .window
                .admit_and_apply(WindowCommand::Activate)
                .expect("the window activates");
            self.fixture.settle();
            assert!(
                self.fixture.window.state().focus.is_focused(),
                "the window has the focus back"
            );
            self.focus_away = false;
        }
        self.handler.take_calls();
    }

    fn reading(&mut self, step: Step) -> Option<Vec<Request>> {
        match step {
            Step::Contact(step) => Self::contact(step),
            Step::Key(step) => {
                let key = match step.key {
                    StepKey::Menu => keycode::MENU,
                    StepKey::F10 => keycode::F10,
                    StepKey::Escape => keycode::ESCAPE,
                    _ => return None,
                };
                Some(if step.shift {
                    vec![
                        Request::Shift(true),
                        Request::Key(key),
                        Request::Shift(false),
                    ]
                } else {
                    vec![Request::Key(key)]
                })
            }
            // Touch, pen and pinch have no injector on Xvfb, a position the
            // server sends is always finite, and no input method runs.
            _ => None,
        }
    }

    fn decode(&mut self, requests: Vec<Request>) -> impl Iterator<Item = HostInput> + use<'_> {
        let mut heard = Vec::new();
        for request in requests {
            self.send(request);
            let delivered = self.heard();
            if request.is_the_steps() {
                heard.extend(delivered);
            }
        }
        heard.into_iter()
    }

    fn haptics(&mut self) -> Option<std::sync::Arc<dyn flui_platform::haptics::PlatformHaptics>> {
        self.fixture.window.haptics()
    }

    fn drag_source(
        &mut self,
    ) -> Option<std::sync::Arc<dyn flui_platform::drag::PlatformDragSource>> {
        self.fixture.window.drag_source()
    }

    fn drag_start(&self) -> flui_platform::drag::OsDragStart {
        flui_native::input::availability::DESKTOP_DRAG_START
    }

    fn focus_text_store(&mut self, _store: Rc<dyn TextStore>) -> TextInputPath {
        TextInputPath::None(NO_INPUT_METHOD)
    }
}

impl Drop for X11Window {
    /// What a row left behind goes before the window closes, whatever the
    /// row found: no button stays down and no master pointer stays for the
    /// cases after this one.
    fn drop(&mut self) {
        if self.pressed {
            let _ = self
                .xtest
                .xtest_fake_input(BUTTON_RELEASE_EVENT, 1, 0, self.root, 0, 0, 0);
        }
        for master in self.masters.values() {
            let _ = self.xtest.xinput_xi_change_hierarchy(&[remove(*master)]);
        }
        let _ = self.xtest.flush();
    }
}

/// The id of the master device the server names `name`.
fn master_named(server: &RustConnection, name: &[u8]) -> u16 {
    server
        .xinput_xi_query_device(xinput::Device::ALL_MASTER)
        .expect("request")
        .reply()
        .expect("the masters")
        .infos
        .into_iter()
        .find(|info| info.name == name)
        .unwrap_or_else(|| panic!("the server has a master named {name:?}"))
        .deviceid
}

/// The change that removes `master`, its slaves left floating.
fn remove(master: u16) -> HierarchyChange {
    HierarchyChange {
        len: 3,
        data: HierarchyChangeData::RemoveMaster(HierarchyChangeDataRemoveMaster {
            deviceid: master,
            return_mode: ChangeMode::FLOAT,
            return_pointer: 0,
            return_keyboard: 0,
        }),
    }
}

fn remove_master(server: &RustConnection, master: u16) {
    server
        .xinput_xi_change_hierarchy(&[remove(master)])
        .expect("request")
        .check()
        .expect("the server removes the master pointer");
}
