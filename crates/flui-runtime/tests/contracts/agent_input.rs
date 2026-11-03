//! A driver injects input into a window through an input lease (ADR-0288): the input enters
//! the window's own input path one tick per frame boundary, what it leaves held is the lease's
//! until it is released or the lease is dropped, and the answer comes once a frame has passed
//! since its last tick.

use std::sync::Arc;
use std::time::Duration;

use flui_foundation::geometry::Offset;
use flui_interaction::events::{
    Code, InputValueError, PointerEvent, PointerKind, make_down_event, make_move_event,
    make_up_event,
};
use flui_interaction::testing::input::KeyEventBuilder;
use flui_platform::PlatformInput;
use flui_protocol::method::{self, InputInject, InputRelease};
use flui_protocol::{
    ActOutcome, ActionSequence, DeviceClass, EffectKind, EffectUnit, ErrorCode, ErrorObject, Event,
    EventFilter, EventKind, InputKind, InputParams, InputPath, InputPolicy, InputSource, Journal,
    JournalEntry, JournalParams, JournalSpan, KeyAction, LogicalPoint, PointerAction,
    PointerButton, PointerDevice, Released, TrackpadAction, WindowParams,
};
use flui_rendering::hit_testing::HitTestBehavior;
use flui_testing::{HeadlessDevAgent, HeadlessHost, HeadlessWindow};
use flui_view::dev_agent::{AgentAnswer, AgentWindow, DevAgentHook, InputLease};
use flui_widgets::{Listener, SizedBox};
use parking_lot::Mutex;

/// What the window's listener saw, in order.
type Seen = Arc<Mutex<Vec<&'static str>>>;

/// Keeps every window the host hands it, as a tool serving agents does.
struct Windows(Arc<Mutex<Vec<AgentWindow>>>);

impl DevAgentHook for Windows {
    fn attach(&mut self) -> bool {
        true
    }

    fn window_opened(&mut self, window: AgentWindow) {
        self.0.lock().push(window);
    }
}

/// A window filled by a listener that records the pointer events it is handed, with its first
/// frame shown and its agent window handed out.
fn shown() -> (HeadlessHost, HeadlessDevAgent, AgentWindow, Seen) {
    let windows = Arc::new(Mutex::new(Vec::new()));
    let mut agent = HeadlessDevAgent::attach(Windows(Arc::clone(&windows)));
    let mut host = HeadlessHost::new(HeadlessWindow::new(200, 200)).with_dev_agent(&mut agent);
    let seen: Seen = Arc::default();
    let log = |name: &'static str| {
        let seen = Arc::clone(&seen);
        move |_: &mut flui_view::EventCx<'_>, _: flui_interaction::PointerDispatch<'_>| {
            seen.lock().push(name);
        }
    };
    host.attach(
        &Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .on_pointer_down(log("down"))
            .on_pointer_move(log("move"))
            .on_pointer_up(log("up"))
            .on_pointer_cancel(log("cancel"))
            .child(SizedBox::new(200.0, 200.0)),
    )
    .expect("mount");
    let _ = host.pump(Duration::from_millis(16));
    let window = windows
        .lock()
        .pop()
        .expect("the host handed the agent its window");
    (host, agent, window, seen)
}

fn pointer(actions: Vec<PointerAction>) -> InputSource {
    InputSource::Pointer {
        id: "finger".to_owned(),
        device: PointerDevice::Touch,
        actions,
    }
}

fn to(x: f64, y: f64) -> PointerAction {
    PointerAction::Move {
        to: LogicalPoint::new(x, y),
        duration_ms: 0,
        pressure: None,
    }
}

fn down() -> PointerAction {
    PointerAction::Down {
        button: PointerButton::Primary,
        pressure: None,
    }
}

fn up() -> PointerAction {
    PointerAction::Up {
        button: PointerButton::Primary,
    }
}

fn pause(duration_ms: u64) -> PointerAction {
    PointerAction::Pause { duration_ms }
}

fn inject(lease: &InputLease, sources: Vec<InputSource>) -> AgentAnswer<ActOutcome> {
    lease
        .call::<InputInject>(InputParams::new(
            lease.window(),
            ActionSequence::new(sources),
        ))
        .expect("the inbox has room")
}

fn release(lease: &InputLease) -> AgentAnswer<Released> {
    lease
        .call::<InputRelease>(WindowParams::new(lease.window()))
        .expect("the inbox has room")
}

/// Pumps 16 ms frames until `answer` is answered, at most `frames` of them; the frames it took.
fn answered<T>(
    host: &mut HeadlessHost,
    answer: &mut AgentAnswer<T>,
    frames: u32,
) -> (Result<T, ErrorObject>, u32) {
    for frame in 1..=frames {
        let _ = host.pump(Duration::from_millis(16));
        if let Some(answer) = answer.try_take() {
            return (answer, frame);
        }
    }
    panic!("no answer within {frames} frames");
}

/// A tap goes in a tick per frame boundary, through the window's own input path, and is
/// answered `ran`, by the synthetic path, once a frame has passed since its last tick.
#[test]
fn an_injected_tap_reaches_the_app_one_tick_per_frame_and_answers_after_a_frame() {
    let (mut host, _agent, window, seen) = shown();
    let lease = window
        .input_lease()
        .expect("an open window leases its input");
    let mut tap = inject(&lease, vec![pointer(vec![to(50.0, 50.0), down(), up()])]);

    let (outcome, frames) = answered(&mut host, &mut tap, 10);
    let outcome = outcome.expect("the tap plays");
    assert_eq!(outcome.via, InputPath::Synthetic);
    assert_eq!(outcome.effect.kind, EffectKind::Ran);
    assert_eq!(
        *seen.lock(),
        ["down", "up"],
        "a contact moved, pressed and lifted"
    );
    assert!(
        frames >= 4,
        "three ticks at three boundaries, then a frame before the answer: {frames}"
    );
}

/// A pause holds its tick for its duration, and the answer waits for it.
#[test]
fn a_pause_holds_the_injection_for_its_duration() {
    let (mut host, _agent, window, seen) = shown();
    let lease = window
        .input_lease()
        .expect("an open window leases its input");
    let mut press = inject(
        &lease,
        vec![pointer(vec![to(50.0, 50.0), down(), pause(160), up()])],
    );

    let (outcome, frames) = answered(&mut host, &mut press, 40);
    outcome.expect("the press plays");
    assert_eq!(*seen.lock(), ["down", "up"]);
    assert!(
        frames >= 10,
        "the press is held 160 ms, ten 16 ms frames: {frames}"
    );
}

/// What a lease leaves held stays down until it is released: `input.release` lets it go, says
/// what it let go, and a second release lets go nothing.
#[test]
fn held_input_stays_down_until_the_lease_releases_it() {
    let (mut host, _agent, window, seen) = shown();
    let lease = window
        .input_lease()
        .expect("an open window leases its input");
    let mut press = inject(
        &lease,
        vec![
            pointer(vec![to(50.0, 50.0), down()]),
            InputSource::Keys {
                id: "keyboard".to_owned(),
                actions: vec![KeyAction::Down {
                    key: "Shift".to_owned(),
                }],
            },
        ],
    );
    answered(&mut host, &mut press, 10)
        .0
        .expect("the press plays");
    for _ in 0..5 {
        let _ = host.pump(Duration::from_millis(16));
    }
    assert_eq!(
        *seen.lock(),
        ["down"],
        "the contact stays down between calls"
    );

    let mut released = release(&lease);
    let released = answered(&mut host, &mut released, 4)
        .0
        .expect("the release runs");
    assert_eq!((released.pointers, released.keys), (1, 1), "{released:?}");
    assert_eq!(
        *seen.lock(),
        ["down", "cancel"],
        "a held contact is cancelled, as a platform takes a contact away"
    );

    let mut again = release(&lease);
    let again = answered(&mut host, &mut again, 4)
        .0
        .expect("a release with nothing held");
    assert_eq!((again.pointers, again.keys), (0, 0), "{again:?}");
}

/// Dropping the lease lets go what it holds, and answers the work still queued through it that
/// the lease was released: a holder that goes leaves nothing pressed.
#[test]
fn dropping_the_lease_lets_go_what_it_holds_and_answers_its_queued_work() {
    let (mut host, _agent, window, seen) = shown();
    let lease = window
        .input_lease()
        .expect("an open window leases its input");
    let mut press = inject(&lease, vec![pointer(vec![to(50.0, 50.0), down()])]);
    answered(&mut host, &mut press, 10)
        .0
        .expect("the press plays");

    let mut queued = inject(&lease, vec![pointer(vec![pause(1_000), up()])]);
    drop(lease);
    let (dropped, _) = answered(&mut host, &mut queued, 4);
    let dropped = dropped.expect_err("the lease went before its work ran");
    assert_eq!(dropped.code, ErrorCode::Gone, "{dropped:?}");
    assert_eq!(*seen.lock(), ["down", "cancel"], "the contact is let go");

    // A new lease starts with nothing held, and its pointer is a new one.
    let lease = window.input_lease().expect("leased again");
    let mut tap = inject(&lease, vec![pointer(vec![to(60.0, 60.0), down(), up()])]);
    answered(&mut host, &mut tap, 10).0.expect("the tap plays");
    assert_eq!(*seen.lock(), ["down", "cancel", "down", "up"]);
}

/// Two leases hold their own input: one's release does not let go the other's.
#[test]
fn a_lease_releases_only_its_own_input() {
    let (mut host, _agent, window, seen) = shown();
    let first = window.input_lease().expect("leased");
    let second = window.input_lease().expect("leased");
    let mut press = inject(&first, vec![pointer(vec![to(50.0, 50.0), down()])]);
    answered(&mut host, &mut press, 10)
        .0
        .expect("the press plays");

    let mut released = release(&second);
    let released = answered(&mut host, &mut released, 4)
        .0
        .expect("the release runs");
    assert_eq!((released.pointers, released.keys), (0, 0), "{released:?}");
    drop(second);
    for _ in 0..3 {
        let _ = host.pump(Duration::from_millis(16));
    }
    assert_eq!(
        *seen.lock(),
        ["down"],
        "the first lease's contact stays down"
    );
}

/// Input asked without a lease, or that asks for another path, is refused before anything
/// plays; a sequence the window cannot play as asked is refused whole.
#[test]
fn input_the_window_cannot_play_as_asked_is_refused_whole() {
    let (mut host, _agent, window, seen) = shown();
    let tap = || ActionSequence::new(vec![pointer(vec![to(50.0, 50.0), down(), up()])]);

    let mut unleased = window
        .call::<InputInject>(InputParams::new(window.id(), tap()))
        .expect("the inbox has room");
    let refused = answered(&mut host, &mut unleased, 2)
        .0
        .expect_err("asked without a lease");
    assert_eq!(refused.code, ErrorCode::InvalidArgument, "{refused:?}");
    let mut unleased = window
        .call::<InputRelease>(WindowParams::new(window.id()))
        .expect("the inbox has room");
    let refused = answered(&mut host, &mut unleased, 2)
        .0
        .expect_err("asked without a lease");
    assert_eq!(refused.code, ErrorCode::InvalidArgument, "{refused:?}");

    let lease = window.input_lease().expect("leased");
    let mut native = InputParams::new(window.id(), tap());
    native.via = Some(InputPolicy::require(InputPath::Os));
    let mut native = lease
        .call::<InputInject>(native)
        .expect("the inbox has room");
    let refused = answered(&mut host, &mut native, 2)
        .0
        .expect_err("injected input is synthetic");
    assert_eq!(refused.code, ErrorCode::InvalidArgument, "{refused:?}");

    let cases: [(&str, Vec<InputSource>, ErrorCode); 6] = [
        (
            "two sources of one name",
            vec![pointer(vec![to(1.0, 1.0)]), pointer(vec![to(2.0, 2.0)])],
            ErrorCode::InvalidArgument,
        ),
        (
            "a key that is neither a key name nor the text of one",
            vec![InputSource::Keys {
                id: "keyboard".to_owned(),
                actions: vec![KeyAction::Down {
                    key: "NoSuchKey".to_owned(),
                }],
            }],
            ErrorCode::InvalidArgument,
        ),
        (
            "a non-finite point",
            vec![pointer(vec![to(f64::NAN, 1.0)])],
            ErrorCode::InvalidArgument,
        ),
        (
            "a point past any window, where a move between two would overflow",
            vec![pointer(vec![
                to(-f64::MAX, 1.0),
                PointerAction::Move {
                    to: LogicalPoint::new(f64::MAX, 1.0),
                    duration_ms: 32,
                    pressure: None,
                },
            ])],
            ErrorCode::InvalidArgument,
        ),
        (
            "longer than ten minutes",
            vec![pointer(vec![pause(600_000), pause(1)])],
            ErrorCode::InvalidArgument,
        ),
        (
            "a trackpad gesture",
            vec![InputSource::Trackpad {
                id: "trackpad".to_owned(),
                actions: vec![TrackpadAction::Begin {
                    at: LogicalPoint::new(1.0, 1.0),
                }],
            }],
            ErrorCode::NotSupported,
        ),
    ];
    for (case, sources, code) in cases {
        let mut refused = inject(&lease, sources);
        let refused = answered(&mut host, &mut refused, 2).0.expect_err(case);
        assert_eq!(refused.code, code, "{case}: {refused:?}");
        assert_eq!(refused.effect, None, "{case} is refused whole: {refused:?}");
    }
    assert!(seen.lock().is_empty(), "nothing played: {:?}", seen.lock());
}

/// A sequence that fails part of the way through stops there: the error says how many of its
/// ticks went out.
#[test]
fn an_injection_that_fails_part_of_the_way_says_how_far_it_got() {
    let (mut host, _agent, window, _seen) = shown();
    let lease = window.input_lease().expect("leased");
    // The contact presses before it has moved anywhere: the second tick cannot play.
    let mut press = inject(&lease, vec![pointer(vec![pause(0), down(), up()])]);

    let failed = answered(&mut host, &mut press, 10)
        .0
        .expect_err("a contact goes down where it is");
    assert_eq!(failed.code, ErrorCode::InvalidArgument, "{failed:?}");
    let effect = failed.effect.clone().expect("a partial effect");
    assert_eq!(effect.kind, EffectKind::Partial, "{failed:?}");
    assert_eq!(
        (effect.sent, effect.total, effect.unit),
        (Some(1), Some(3), Some(EffectUnit::Tick)),
        "{failed:?}"
    );
}

/// A window that closes answers the input queued on it `gone`.
#[test]
fn a_closing_window_answers_its_queued_input_gone() {
    let (mut host, _agent, window, _seen) = shown();
    let lease = window.input_lease().expect("leased");
    let mut press = inject(
        &lease,
        vec![pointer(vec![to(50.0, 50.0), down(), pause(5_000)])],
    );
    let _ = host.pump(Duration::from_millis(16));
    drop(host);
    let gone = press
        .try_take()
        .expect("the closing window answered")
        .expect_err("it closed first");
    assert_eq!(gone.code, ErrorCode::Gone, "{gone:?}");
}

fn read_journal(
    host: &mut HeadlessHost,
    window: &AgentWindow,
    since: Option<u64>,
    max_entries: Option<u32>,
) -> Journal {
    let mut params = JournalParams::new(window.id());
    params.since = since;
    params.max_entries = max_entries;
    let mut read = window
        .call::<method::InputJournal>(params)
        .expect("the inbox has room");
    answered(host, &mut read, 2).0.expect("the journal is read")
}

fn native(event: Result<PointerEvent, InputValueError>) -> PlatformInput {
    PlatformInput::Pointer(event.expect("a valid pointer event"))
}

/// What one entry says, without its sequence number and time.
fn step(entry: &JournalEntry) -> (DeviceClass, InputKind, Option<(f64, f64)>, bool) {
    (
        entry.device,
        entry.kind,
        entry.at.map(|at| (at.x, at.y)),
        entry.injected,
    )
}

/// The journal lists the input the window was delivered, the platform's and the agent's alike,
/// in order, each with the next sequence number; the agent's are marked injected, and an
/// injection's answer names the entries it made.
#[test]
fn the_journal_lists_what_the_window_was_delivered_and_an_injection_names_its_entries() {
    let (mut host, _agent, window, _seen) = shown();
    let at = Offset::new(20.0, 30.0);
    host.dispatch(native(make_down_event(at, PointerKind::Mouse)));
    host.dispatch(native(make_up_event(at, PointerKind::Mouse)));
    host.dispatch(PlatformInput::Keyboard(
        KeyEventBuilder::new(Code::KeyA).build(),
    ));
    let lease = window
        .input_lease()
        .expect("an open window leases its input");
    let mut tap = inject(&lease, vec![pointer(vec![to(50.0, 50.0), down(), up()])]);
    let outcome = answered(&mut host, &mut tap, 10).0.expect("the tap plays");

    let journal = read_journal(&mut host, &window, None, None);
    let mouse = Some((20.0, 30.0));
    let finger = Some((50.0, 50.0));
    assert_eq!(
        journal.entries.iter().map(step).collect::<Vec<_>>(),
        [
            (DeviceClass::Mouse, InputKind::PointerDown, mouse, false),
            (DeviceClass::Mouse, InputKind::PointerUp, mouse, false),
            (DeviceClass::Keyboard, InputKind::KeyDown, None, false),
            (DeviceClass::Touch, InputKind::PointerDown, finger, true),
            (DeviceClass::Touch, InputKind::PointerUp, finger, true),
        ],
        "{journal:?}"
    );
    assert_eq!(
        journal
            .entries
            .iter()
            .map(|entry| entry.seq)
            .collect::<Vec<u64>>(),
        [1, 2, 3, 4, 5]
    );
    assert_eq!(journal.span, Some(JournalSpan { from: 1, to: 5 }));
    assert_eq!(journal.dropped, 0);
    assert_eq!(
        outcome.observed,
        Some(JournalSpan { from: 4, to: 5 }),
        "the injection names the entries its ticks made"
    );
}

/// Each entry goes out to a subscriber as an `input` event as the journal takes it.
#[test]
fn each_journal_entry_goes_out_as_an_input_event() {
    let (mut host, _agent, window, _seen) = shown();
    let mut subscribed = window
        .subscribe(vec![EventFilter::new(EventKind::Input)])
        .expect("the inbox has room");
    let mut events = answered(&mut host, &mut subscribed, 2)
        .0
        .expect("subscribed");
    let lease = window
        .input_lease()
        .expect("an open window leases its input");
    let mut tap = inject(&lease, vec![pointer(vec![to(50.0, 50.0), down(), up()])]);
    answered(&mut host, &mut tap, 10).0.expect("the tap plays");
    host.dispatch(PlatformInput::Keyboard(
        KeyEventBuilder::new(Code::KeyA).build(),
    ));

    let streamed: Vec<JournalEntry> = std::iter::from_fn(|| events.try_next())
        .map(|event| match event {
            Event::Input(input) => {
                assert_eq!(input.window, window.id());
                input.entry
            }
            other => panic!("only input was asked for: {other:?}"),
        })
        .collect();
    let journal = read_journal(&mut host, &window, None, None);
    assert_eq!(streamed, journal.entries);
    assert_eq!(streamed.len(), 3, "{streamed:?}");
}

/// The journal holds its latest 4096 entries; a read after `since` counts the ones after it the
/// journal let go, and takes at most `max_entries`, oldest first.
#[test]
fn the_journal_keeps_its_latest_entries_and_counts_the_ones_it_let_go() {
    let (mut host, _agent, window, _seen) = shown();
    for step in 0..4099_u32 {
        let at = Offset::new(f64::from(step % 100), 10.0);
        host.dispatch(native(make_move_event(at, PointerKind::Mouse)));
    }

    // (since, max_entries) → the entries' span, how many, dropped
    let span = |from, to| Some(JournalSpan { from, to });
    let cases = [
        ((None, None), (span(4, 4099), 4096, 3)),
        ((Some(0), Some(2)), (span(4, 5), 2, 3)),
        ((Some(2), None), (span(4, 4099), 4096, 1)),
        ((Some(3), Some(1)), (span(4, 4), 1, 0)),
        ((Some(4000), Some(10)), (span(4001, 4010), 10, 0)),
        ((Some(4099), None), (None, 0, 0)),
    ];
    for ((since, max_entries), expected) in cases {
        let journal = read_journal(&mut host, &window, since, max_entries);
        assert_eq!(
            (journal.span, journal.entries.len(), journal.dropped),
            expected,
            "since {since:?}, at most {max_entries:?}"
        );
    }
}
