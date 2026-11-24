//! The agent answers a gesture in the application (ADR-0400 §2): it holds the act at frame
//! boundaries until the target passes every actionability check or the act's timeout passes,
//! injects in the turn it passed, aimed at that frame's bounds, and drops an act it answered, so
//! nothing injects after its answer.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::task::Waker;
use std::time::Duration;

use flui_foundation::geometry::{Axis, EdgeInsets};
use flui_foundation::testing::CountingWaker;
use flui_interaction::pointer::HitTestBehavior;
use flui_protocol::input::gestures;
use flui_protocol::method::{self, Method};
use flui_protocol::{
    act::ActOutcome, act::ActParams, act::ActionabilityCheck, act::ActionabilityParams,
    act::EffectKind, error::ErrorCode, error::ErrorObject, geometry::LogicalPoint,
    geometry::LogicalRect, input::DragParams, input::GestureOptions, input::InputKind,
    input::InputPath, input::InputPolicy, input::Journal, input::JournalParams,
    input::LongPressParams, input::PinchParams, input::PointerButton, input::PressKeysParams,
    input::ScrollIntoViewParams, input::ScrollUntilParams, input::SwipeParams, input::TapParams,
    input::TypeTextParams, locator::LocateParams, locator::Locator, locator::Target,
    tree::ElementRef, wire::ActionName,
};
use flui_runtime::agent::Limits;
use flui_testing::host::{HeadlessDevAgent, HeadlessHost, HeadlessWindow};
use flui_view::Signal;
use flui_view::dev_agent::{AgentAnswer, AgentWindow, DevAgentHook, InputLease};
use flui_view::prelude::*;
use flui_widgets::{
    AnimatedPadding, Column, ListView, Listener, Positioned, RawButton, ScrollView, SizedBox,
    Stack, Text,
};
use parking_lot::Mutex;
use serde_json::json;
use web_time::Instant;

/// How many times each button of [`Board`] was pressed.
#[derive(Clone, Default)]
struct Presses {
    go: Arc<AtomicU32>,
    gated: Arc<AtomicU32>,
    covered: Arc<AtomicU32>,
}

/// A board of targets: "Go"; "Gated", disabled until "Enable" is pressed; "Target", which slides
/// "Items" of 100 rows 20 px tall, "Item 0" to "Item 99", in a viewport 100 px tall; and a row
/// "Columns" of 100 columns 50 px wide, "Col 0" to "Col 99", in a viewport 200 px wide.
/// "Items" of 100 rows 20 px tall, "Item 0" to "Item 99", in a viewport 100 px tall.
#[derive(Clone, StatefulView)]
struct Board {
    presses: Presses,
}

struct BoardState {
    enabled: Signal<bool>,
    moved: Signal<bool>,
}

impl StatefulView for Board {
    type State = BoardState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        BoardState {
            enabled: ctx.signal(false),
            moved: ctx.signal(false),
        }
    }
}

fn counting(count: &Arc<AtomicU32>) -> impl Fn(&mut EventContext<'_>) + 'static {
    let count = Arc::clone(count);
    move |_: &mut EventContext<'_>| {
        count.fetch_add(1, Ordering::SeqCst);
    }
}

impl ViewState<Board> for BoardState {
    fn build(&self, view: &Board, ctx: &dyn BuildContext) -> impl IntoView {
        let enabled = self.enabled;
        let moved = self.moved;
        let gated = if enabled.get(ctx) {
            RawButton::new(Text::new("Gated")).on_pressed(counting(&view.presses.gated))
        } else {
            RawButton::new(Text::new("Gated"))
        };
        let inset = if moved.get(ctx) {
            EdgeInsets::only_left(80.0)
        } else {
            EdgeInsets::ZERO
        };
        let rows = ListView::builder(100, 20.0, |index| {
            Some(
                SizedBox::new(200.0, 20.0)
                    .child(Text::new(format!("Item {index}")))
                    .into_view()
                    .boxed(),
            )
        })
        .semantics_label("Items");
        let columns = ListView::builder(100, 50.0, |index| {
            Some(
                SizedBox::new(50.0, 30.0)
                    .child(Text::new(format!("Col {index}")))
                    .into_view()
                    .boxed(),
            )
        })
        .scroll_direction(Axis::Horizontal)
        .semantics_label("Columns");
        Column::new(vec![
            RawButton::new(Text::new("Go"))
                .on_pressed(counting(&view.presses.go))
                .into_view()
                .boxed(),
            RawButton::new(Text::new("Enable"))
                .on_pressed(move |ctx| enabled.update(ctx, |on| *on = true))
                .into_view()
                .boxed(),
            gated.into_view().boxed(),
            RawButton::new(Text::new("Move"))
                .on_pressed(move |ctx| moved.update(ctx, |on| *on = true))
                .into_view()
                .boxed(),
            AnimatedPadding::new(inset)
                .child(RawButton::new(Text::new("Target")).on_pressed(|_| {}))
                .into_view()
                .boxed(),
            SizedBox::new(120.0, 30.0)
                .child(Stack::new(vec![
                    RawButton::new(Text::new("Covered"))
                        .on_pressed(counting(&view.presses.covered))
                        .into_view()
                        .boxed(),
                    Positioned::fill(
                        Listener::new()
                            .behavior(HitTestBehavior::Opaque)
                            .child(SizedBox::expand()),
                    )
                    .into_view()
                    .boxed(),
                ]))
                .into_view()
                .boxed(),
            SizedBox::new(200.0, 100.0).child(rows).into_view().boxed(),
            SizedBox::new(200.0, 30.0)
                .child(columns)
                .into_view()
                .boxed(),
        ])
    }
}

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

/// The board shown, its first frame presented, its agent window and a lease on its input.
fn shown() -> (
    HeadlessHost,
    HeadlessDevAgent,
    AgentWindow,
    InputLease,
    Presses,
) {
    let windows = Arc::new(Mutex::new(Vec::new()));
    let mut agent = HeadlessDevAgent::attach(Windows(Arc::clone(&windows)));
    let mut host = HeadlessHost::new(HeadlessWindow::new(200, 400)).with_dev_agent(&mut agent);
    let presses = Presses::default();
    host.attach(&Board {
        presses: presses.clone(),
    })
    .expect("mount");
    let _ = host.pump(Duration::from_millis(16));
    let window = windows
        .lock()
        .pop()
        .expect("the host handed the agent its window");
    let _ = host.pump(Duration::from_millis(16));
    let lease = window
        .input_lease()
        .expect("an open window leases its input");
    (host, agent, window, lease, presses)
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

fn named(name: &str) -> Target {
    Target::Locator(Locator::named(name))
}

fn timeout(mut options: GestureOptions, timeout_ms: u64) -> GestureOptions {
    options.timeout_ms = Some(timeout_ms);
    options
}

/// The one element `name` names now.
fn element(host: &mut HeadlessHost, window: &AgentWindow, name: &str) -> ElementRef {
    let mut found = window
        .call::<method::SemanticsLocate>(LocateParams::new(Locator::named(name)))
        .expect("the inbox has room");
    let matches = answered(host, &mut found, 2).0.expect("located");
    assert_eq!(matches.matches.len(), 1, "one {name}: {matches:?}");
    matches.matches[0].element
}

/// The bounds `name` has on the frame last committed.
fn bounds(host: &mut HeadlessHost, window: &AgentWindow, name: &str) -> LogicalRect {
    let element = element(host, window, name);
    let mut facts = window
        .call::<method::ElementActionability>(ActionabilityParams::new(element))
        .expect("the inbox has room");
    answered(host, &mut facts, 2)
        .0
        .expect("its facts")
        .rect
        .unwrap_or_else(|| panic!("{name} shows bounds"))
}

fn journal(host: &mut HeadlessHost, window: &AgentWindow) -> Journal {
    let mut read = window
        .call::<method::InputJournal>(JournalParams::new(window.id()))
        .expect("the inbox has room");
    answered(host, &mut read, 2).0.expect("the journal is read")
}

/// The kinds of the journal's entries the outcome says its act made.
fn kinds_made(
    host: &mut HeadlessHost,
    window: &AgentWindow,
    outcome: &ActOutcome,
) -> Vec<InputKind> {
    let Some(span) = outcome.observed else {
        return Vec::new();
    };
    journal(host, window)
        .entries
        .iter()
        .filter(|entry| (span.from..=span.to).contains(&entry.seq))
        .map(|entry| entry.kind)
        .collect()
}

fn press(host: &mut HeadlessHost, window: &AgentWindow, name: &str) {
    let mut pressed = window
        .call::<method::SemanticsAct>(ActParams::new(named(name), ActionName::Invoke))
        .expect("the inbox has room");
    answered(host, &mut pressed, 2).0.expect("pressed");
}

type Ask = fn(&InputLease) -> AgentAnswer<ActOutcome>;

fn ask<M: Method<Output = ActOutcome> + flui_protocol::method::WindowMethod>(
    lease: &InputLease,
    params: M::Params,
) -> AgentAnswer<ActOutcome> {
    lease.call::<M>(params).expect("the inbox has room")
}

/// Each gesture the agent answers, asked through an input lease, is answered by the agent with
/// what it ran: the input it injected (the kinds of the journal's entries it made), or the
/// semantics action that does the same. A gesture the agent refused `not_supported` fails here.
#[test]
fn the_agent_answers_every_gesture_with_what_it_ran() {
    use InputKind::{KeyDown, KeyUp, PointerDown, PointerMove, PointerUp, Scroll, Text};
    let rows: [(&str, Ask, InputPath, &[InputKind]); 11] = [
        (
            method::ElementTap::NAME,
            |lease| ask::<method::ElementTap>(lease, TapParams::new(named("Go"))),
            InputPath::Synthetic,
            &[PointerDown, PointerUp],
        ),
        (
            method::ElementDoubleTap::NAME,
            |lease| ask::<method::ElementDoubleTap>(lease, TapParams::new(named("Go"))),
            InputPath::Synthetic,
            &[PointerDown, PointerUp, PointerDown, PointerUp],
        ),
        (
            method::ElementLongPress::NAME,
            |lease| {
                let params: LongPressParams = serde_json::from_value(
                    json!({ "target": { "locator": { "name": { "match": "exact", "text": "Go" } } }, "duration_ms": 100 }),
                )
                .expect("a long press reads");
                ask::<method::ElementLongPress>(lease, params)
            },
            InputPath::Synthetic,
            &[PointerDown, PointerUp],
        ),
        (
            method::ElementDragTo::NAME,
            |lease| {
                let params: DragParams = serde_json::from_value(json!({
                    "target": { "locator": { "name": { "match": "exact", "text": "Go" } } },
                    "to": { "locator": { "name": { "match": "exact", "text": "Enable" } } },
                }))
                .expect("a drag reads");
                ask::<method::ElementDragTo>(lease, params)
            },
            InputPath::Synthetic,
            &[PointerDown, PointerMove, PointerUp],
        ),
        (
            method::ElementSwipe::NAME,
            |lease| ask::<method::ElementSwipe>(lease, swipe_up()),
            InputPath::Synthetic,
            &[PointerDown, PointerMove, PointerUp],
        ),
        (
            method::ElementFling::NAME,
            |lease| ask::<method::ElementFling>(lease, swipe_up()),
            InputPath::Synthetic,
            &[PointerDown, PointerMove, PointerUp],
        ),
        (
            method::ElementPinch::NAME,
            |lease| {
                let params: PinchParams = serde_json::from_value(json!({
                    "target": { "locator": { "name": { "match": "exact", "text": "Go" } } },
                    "scale": 2.0,
                }))
                .expect("a pinch reads");
                ask::<method::ElementPinch>(lease, params)
            },
            InputPath::Synthetic,
            &[PointerDown, PointerDown, PointerUp, PointerUp],
        ),
        (
            method::ElementPressKeys::NAME,
            |lease| {
                let params: PressKeysParams = serde_json::from_value(json!({
                    "target": { "locator": { "name": { "match": "exact", "text": "Go" } } },
                    "keys": ["Shift+a"],
                }))
                .expect("press keys reads");
                ask::<method::ElementPressKeys>(lease, params)
            },
            InputPath::Synthetic,
            &[PointerDown, PointerUp, KeyDown, KeyDown, KeyUp, KeyUp],
        ),
        (
            method::ElementTypeText::NAME,
            |lease| {
                let params: TypeTextParams = serde_json::from_value(json!({
                    "target": { "locator": { "name": { "match": "exact", "text": "Go" } } },
                    "text": "hi",
                }))
                .expect("type text reads");
                ask::<method::ElementTypeText>(lease, params)
            },
            InputPath::Synthetic,
            &[PointerDown, PointerUp, Text],
        ),
        (
            method::ElementScrollIntoView::NAME,
            |lease| {
                let params: ScrollIntoViewParams = serde_json::from_value(json!({
                    "target": { "locator": { "name": { "match": "exact", "text": "Item 12" } } },
                }))
                .expect("scroll into view reads");
                ask::<method::ElementScrollIntoView>(lease, params)
            },
            InputPath::Semantic,
            &[],
        ),
        (
            method::ElementScrollUntil::NAME,
            |lease| {
                ask::<method::ElementScrollUntil>(
                    lease,
                    ScrollUntilParams::new(named("Items"), Locator::named("Item 30")),
                )
            },
            InputPath::Synthetic,
            &[Scroll],
        ),
    ];
    for (name, call, via, made) in rows {
        let (mut host, _agent, window, lease, _presses) = shown();
        let mut asked = call(&lease);
        let outcome = answered(&mut host, &mut asked, 120)
            .0
            .unwrap_or_else(|error| panic!("{name}: {error:?}"));
        assert_eq!(outcome.via, via, "{name}: {outcome:?}");
        assert_eq!(outcome.effect.kind, EffectKind::Ran, "{name}: {outcome:?}");
        assert!(outcome.element.is_some(), "{name} names its element");
        let kinds = kinds_made(&mut host, &window, &outcome);
        let mut wanted = made.iter().peekable();
        for kind in &kinds {
            wanted.next_if(|want| *want == kind);
        }
        assert!(
            wanted.peek().is_none(),
            "{name} made {made:?} in order: {kinds:?}"
        );
    }
}

fn swipe_up() -> SwipeParams {
    serde_json::from_value(json!({
        "target": { "locator": { "name": { "match": "exact", "text": "Items" } } },
        "direction": "up",
    }))
    .expect("a swipe reads")
}

/// A tap on a target that is still moving is held until the target holds still over two frames,
/// and the press lands at the bounds of the frame the check passed on, not those it had when the
/// act was asked.
#[test]
fn an_act_held_for_stability_lands_at_the_bounds_of_the_frame_it_passed_on() {
    let (mut host, _agent, window, lease, _presses) = shown();
    let asked_at = bounds(&mut host, &window, "Target").center();
    press(&mut host, &window, "Move");
    // Under way: it has started to slide.
    for _ in 0..3 {
        let _ = host.pump(Duration::from_millis(16));
    }
    let sliding = bounds(&mut host, &window, "Target").center();
    assert!(sliding.x > asked_at.x, "{asked_at:?} to {sliding:?}");
    let mut tap = ask::<method::ElementTap>(&lease, TapParams::new(named("Target")));
    let (outcome, frames) = answered(&mut host, &mut tap, 60);
    let outcome = outcome.expect("the target settles and is tapped");
    assert!(frames > 5, "held while the target slid: {frames}");
    let rest = bounds(&mut host, &window, "Target").center();
    assert!(
        rest.x - sliding.x > 10.0,
        "the target slid on after the act was asked: {sliding:?} to {rest:?}"
    );
    let span = outcome.observed.expect("the tap made entries");
    let down = journal(&mut host, &window)
        .entries
        .into_iter()
        .filter(|entry| (span.from..=span.to).contains(&entry.seq))
        .find(|entry| entry.kind == InputKind::PointerDown)
        .expect("the tap pressed");
    let at = down.at.expect("a press has a position");
    assert!(
        (at.x - rest.x).abs() < 0.5 && (at.y - rest.y).abs() < 0.5,
        "pressed at {at:?}, the target rests at {rest:?}"
    );
}

/// A target that never passes a check before the act's timeout answers `not_actionable` naming
/// that check (and, for one covered, what covers it), and nothing was sent.
#[test]
fn an_act_whose_target_never_passes_names_the_check_and_sends_nothing() {
    let cases = [
        ("Gated", ActionabilityCheck::Enabled),
        ("Covered", ActionabilityCheck::Receives),
    ];
    for (name, check) in cases {
        let (mut host, _agent, window, lease, presses) = shown();
        let before = journal(&mut host, &window).entries.len();
        let mut tap = TapParams::new(named(name));
        tap.options = timeout(tap.options, 80);
        let mut tap = ask::<method::ElementTap>(&lease, tap);
        let refused = answered(&mut host, &mut tap, 20)
            .0
            .expect_err("the target never passes");
        assert_eq!(
            refused.code,
            ErrorCode::NotActionable,
            "{name}: {refused:?}"
        );
        assert_eq!(refused.failed, Some(check), "{name}: {refused:?}");
        if check == ActionabilityCheck::Receives {
            assert!(refused.covered_by.is_some(), "{name}: {refused:?}");
        }
        assert_eq!(
            journal(&mut host, &window).entries.len(),
            before,
            "{name}: nothing was sent"
        );
        assert_eq!(presses.covered.load(Ordering::SeqCst), 0, "{name}");
    }
}

/// An act answered at its timeout is dropped: the target turning actionable on the frame after
/// the answer gets no press.
#[test]
fn an_act_past_its_timeout_never_injects() {
    let (mut host, _agent, window, lease, presses) = shown();
    let mut tap = TapParams::new(named("Gated"));
    tap.options = timeout(tap.options, 80);
    let mut tap = ask::<method::ElementTap>(&lease, tap);
    let refused = answered(&mut host, &mut tap, 20)
        .0
        .expect_err("disabled until the timeout");
    assert_eq!(
        refused.failed,
        Some(ActionabilityCheck::Enabled),
        "{refused:?}"
    );
    let sent = journal(&mut host, &window).entries.len();
    press(&mut host, &window, "Enable");
    for _ in 0..10 {
        let _ = host.pump(Duration::from_millis(16));
    }
    assert_eq!(
        journal(&mut host, &window).entries.len(),
        sent,
        "nothing went out after the answer"
    );
    assert_eq!(presses.gated.load(Ordering::SeqCst), 0);
}

/// `force` skips every check but attached: a tap on a covered target goes out at its centre, which
/// the cover takes.
#[test]
fn force_acts_on_a_covered_target() {
    let (mut host, _agent, window, lease, presses) = shown();
    let centre = bounds(&mut host, &window, "Covered").center();
    let mut tap = TapParams::new(named("Covered"));
    tap.options.force = true;
    let mut tap = ask::<method::ElementTap>(&lease, tap);
    let outcome = answered(&mut host, &mut tap, 10)
        .0
        .expect("forced past the cover");
    let span = outcome.observed.expect("the tap went out");
    let down = journal(&mut host, &window)
        .entries
        .into_iter()
        .filter(|entry| (span.from..=span.to).contains(&entry.seq))
        .find(|entry| entry.kind == InputKind::PointerDown)
        .expect("the tap pressed");
    assert_eq!(
        down.at,
        Some(centre),
        "pressed at the covered target's centre"
    );
    assert_eq!(
        presses.covered.load(Ordering::SeqCst),
        0,
        "the cover took it"
    );
}

/// A presentation holds at most 64 acts: one more is refused `busy` while they hold.
#[test]
fn a_window_holds_a_bounded_number_of_acts() {
    let (mut host, _agent, _window, lease, _presses) = shown();
    let held: Vec<AgentAnswer<ActOutcome>> = (0..64)
        .map(|_| {
            let mut tap = TapParams::new(named("Gated"));
            tap.options = timeout(tap.options, 60_000);
            ask::<method::ElementTap>(&lease, tap)
        })
        .collect();
    let mut tap = TapParams::new(named("Gated"));
    tap.options = timeout(tap.options, 60_000);
    let mut one_more = ask::<method::ElementTap>(&lease, tap);
    let refused = answered(&mut host, &mut one_more, 2)
        .0
        .expect_err("past the bound");
    assert_eq!(refused.code, ErrorCode::Busy, "{refused:?}");
    drop(held);
}

/// Every way a held act ends without its target answers it once: the window closing, and the
/// lease it injects through let go, each `gone`.
#[test]
fn a_held_act_is_answered_when_its_window_closes_or_its_lease_goes() {
    let (mut host, _agent, _window, lease, _presses) = shown();
    let mut tap = TapParams::new(named("Gated"));
    tap.options = timeout(tap.options, 60_000);
    let mut held = ask::<method::ElementTap>(&lease, tap.clone());
    let _ = host.pump(Duration::from_millis(16));
    drop(host);
    let gone = held
        .try_take()
        .expect("the closing window answered")
        .expect_err("it closed first");
    assert_eq!(gone.code, ErrorCode::Gone, "{gone:?}");

    let (mut host, _agent, _window, lease, _presses) = shown();
    let mut held = ask::<method::ElementTap>(&lease, tap);
    let _ = host.pump(Duration::from_millis(16));
    drop(lease);
    let gone = answered(&mut host, &mut held, 4)
        .0
        .expect_err("its lease went first");
    assert_eq!(gone.code, ErrorCode::Gone, "{gone:?}");
}

/// `scroll_until` scrolls a viewport at a time until the element shows under the scrollable;
/// ends `not_found` once the content stops moving, at the scroll extent's end and on a scrollable
/// that does not scroll, well before its timeout.
#[test]
fn scroll_until_finds_a_far_element_and_ends_where_the_content_stops() {
    // A call naming no direction scrolls down, and a row that does not move down rightwards.
    let found_cases = [("Items", "Item 60"), ("Columns", "Col 30")];
    for (scrollable, sought) in found_cases {
        let (mut host, _agent, window, lease, _presses) = shown();
        let mut until = ask::<method::ElementScrollUntil>(
            &lease,
            ScrollUntilParams::new(named(scrollable), Locator::named(sought)),
        );
        let outcome = answered(&mut host, &mut until, 200)
            .0
            .unwrap_or_else(|error| panic!("{sought} shows after some steps: {error:?}"));
        let found = outcome.element.expect("the element found");
        assert_eq!(found, element(&mut host, &window, sought));
        let mut facts = window
            .call::<method::ElementActionability>(ActionabilityParams::new(found))
            .expect("the inbox has room");
        assert!(
            answered(&mut host, &mut facts, 2).0.expect("facts").visible,
            "{sought} shows"
        );
    }

    let cases = [("Items", "Item 100"), ("Go", "Item 3")];
    for (scrollable, sought) in cases {
        let (mut host, _agent, _window, lease, _presses) = shown();
        let mut until = ScrollUntilParams::new(named(scrollable), Locator::named(sought));
        until.options = timeout(until.options, 60_000);
        let mut until = ask::<method::ElementScrollUntil>(&lease, until);
        let (ended, frames) = answered(&mut host, &mut until, 400);
        let ended = ended.expect_err("the content stops before it shows");
        assert_eq!(ended.code, ErrorCode::NotFound, "{scrollable}: {ended:?}");
        assert!(
            frames < 400,
            "{scrollable}: ended by the content, not the timeout"
        );
    }
}

/// The conversation counts a gesture's own timeout and the time its input takes to play in the
/// reply's deadline, past the reply timeout, as the window may hold the act that long.
#[test]
fn a_gestures_reply_deadline_is_its_timeout_and_its_play_past_the_reply_timeout() {
    let limits = Limits::default();
    let service = crate::agent_wire::service(limits, None);
    let (_host, _agent, _presses) = crate::agent_wire::shown(&service);
    let waker = Waker::from(CountingWaker::new().shared());
    let now = Instant::now();
    let mut session = crate::agent_wire::greeted(&service, now, &waker);
    let tap = json!({
        "id": 1,
        "method": method::ElementTap::NAME,
        "params": { "target": { "locator": { "name": { "match": "exact", "text": "Go" } } }, "timeout_ms": 1000 },
    });
    session.receive(&crate::agent_wire::line(&tap), now);
    session.poll(now, &waker);
    let origin = LogicalPoint::new(0.0, 0.0);
    let plays = gestures::double_tap(origin, PointerButton::Primary).plays_within();
    assert!(plays > Duration::ZERO);
    assert_eq!(
        session.next_deadline(),
        Some(now + limits.reply_timeout + Duration::from_millis(1000) + plays),
        "the reply timeout, the act's own, and its play"
    );
}

/// A gesture asked on the `os` path is the driver's: the agent refuses it, naming the capability.
#[test]
fn the_agent_refuses_a_gesture_on_the_os_path() {
    let (mut host, _agent, _window, lease, presses) = shown();
    let mut tap = TapParams::new(named("Go"));
    tap.options.via = Some(InputPolicy::prefer(InputPath::Os));
    let mut tap = ask::<method::ElementTap>(&lease, tap);
    let refused = answered(&mut host, &mut tap, 2)
        .0
        .expect_err("the os path is the driver's");
    assert_eq!(refused.code, ErrorCode::NotSupported, "{refused:?}");
    assert_eq!(refused.capability.as_deref(), Some("os_input"));
    assert_eq!(presses.go.load(Ordering::SeqCst), 0);
}

/// On the semantic path a gesture with a semantics action that does the same performs it and
/// injects nothing; one without falls back to synthetic input, unless the call requires the
/// semantic path, which is refused naming the capability.
#[test]
fn the_semantic_path_performs_the_semantics_action_that_does_the_same() {
    let cases: [(&str, bool, InputPolicy, Result<InputPath, &str>); 3] = [
        (
            "a tap invokes",
            false,
            InputPolicy::require(InputPath::Semantic),
            Ok(InputPath::Semantic),
        ),
        (
            "a double tap has no action, and falls back",
            true,
            InputPolicy::prefer(InputPath::Semantic),
            Ok(InputPath::Synthetic),
        ),
        (
            "a double tap has no action, and the path is required",
            true,
            InputPolicy::require(InputPath::Semantic),
            Err("semantic_input"),
        ),
    ];
    for (case, double, policy, expected) in cases {
        let (mut host, _agent, window, lease, presses) = shown();
        let mut tap = TapParams::new(named("Go"));
        tap.options.via = Some(policy);
        let mut asked = if double {
            ask::<method::ElementDoubleTap>(&lease, tap)
        } else {
            ask::<method::ElementTap>(&lease, tap)
        };
        let answer = answered(&mut host, &mut asked, 20).0;
        match (answer, expected) {
            (Ok(outcome), Ok(via)) => {
                assert_eq!(outcome.via, via, "{case}: {outcome:?}");
                let made = kinds_made(&mut host, &window, &outcome);
                assert_eq!(
                    made.is_empty(),
                    via == InputPath::Semantic,
                    "{case}: {made:?}"
                );
                assert!(presses.go.load(Ordering::SeqCst) >= 1, "{case}");
            }
            (Err(refused), Err(capability)) => {
                assert_eq!(refused.code, ErrorCode::NotSupported, "{case}: {refused:?}");
                assert_eq!(refused.capability.as_deref(), Some(capability), "{case}");
                assert_eq!(presses.go.load(Ordering::SeqCst), 0, "{case}");
            }
            (answer, expected) => panic!("{case}: {answer:?}, not {expected:?}"),
        }
    }
}
