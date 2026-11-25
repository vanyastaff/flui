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
    act::ActOutcome, act::ActParams, act::Actionability, act::ActionabilityCheck,
    act::ActionabilityParams, act::EffectKind, error::ErrorCode, error::ErrorObject,
    geometry::LogicalPoint, geometry::LogicalRect, input::Direction, input::DragParams,
    input::GestureOptions, input::InputKind, input::InputPath, input::InputPolicy, input::Journal,
    input::JournalParams, input::LongPressParams, input::PinchParams, input::PointerButton,
    input::PressKeysParams, input::ScrollIntoViewParams, input::ScrollUntilParams,
    input::SwipeParams, input::TapParams, input::TypeTextParams, locator::LocateParams,
    locator::Locator, locator::Pick, locator::Target, locator::TextMatch, snapshot::Snapshot,
    tree::ElementRef, wait::ClearedBy, wire::ActionName,
};
use flui_runtime::agent::Limits;
use flui_testing::host::{HeadlessDevAgent, HeadlessHost, HeadlessWindow};
use flui_view::Signal;
use flui_view::dev_agent::{AgentAnswer, AgentWindow, DevAgentHook, InputLease};
use flui_view::prelude::*;
use flui_widgets::{
    AnimatedPadding, Column, ListView, Listener, Positioned, RawButton, ScrollView, Semantics,
    SingleChildScrollView, SizedBox, Stack, Text,
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
/// right once "Move" is pressed; "Covered", under a cover that takes its hit; a list "Items" of
/// 100 rows 20 px tall, "Item 0" to "Item 99", in a viewport 100 px tall; a row "Columns" of 100
/// columns 50 px wide, "Col 0" to "Col 99", in a viewport 200 px wide; and two texts "Twin".
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
            Text::new("Twin").into_view().boxed(),
            Text::new("Twin").into_view().boxed(),
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
    let rows: [(&str, Ask, InputPath, &[InputKind]); 12] = [
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
            "element.press_keys of the plus key",
            |lease| {
                let params: PressKeysParams = serde_json::from_value(json!({
                    "target": { "locator": { "name": { "match": "exact", "text": "Go" } } },
                    "keys": ["Control++"],
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

/// "Busy", whose press marks the view for a rebuild, the count it shows, and "Covered", under a
/// cover that takes its hit.
#[derive(Clone, StatefulView)]
struct Busy;

struct BusyState {
    presses: Signal<u32>,
}

impl StatefulView for Busy {
    type State = BusyState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        BusyState {
            presses: ctx.signal(0),
        }
    }
}

impl ViewState<Busy> for BusyState {
    fn build(&self, _view: &Busy, ctx: &dyn BuildContext) -> impl IntoView {
        let presses = self.presses;
        Column::new(vec![
            RawButton::new(Text::new("Busy"))
                .on_pressed(move |ctx| presses.update(ctx, |count| *count += 1))
                .into_view()
                .boxed(),
            Text::new(format!("Pressed {}", presses.get(ctx)))
                .into_view()
                .boxed(),
            SizedBox::new(120.0, 30.0)
                .child(Stack::new(vec![
                    RawButton::new(Text::new("Covered"))
                        .on_pressed(|_| {})
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
        ])
    }
}

/// The last attempt of a held act may run on a turn without a frame, at the tree revision of the
/// frame before, while the app has work pending. The element held still over the frames before,
/// so the answer names the check that failed, `receives` for a covered button, never `stable`.
#[test]
fn a_last_attempt_on_a_turn_without_a_frame_names_the_check_that_failed() {
    let mut rig = crate::agent_rig::Rig::showing(&Busy);
    let mut tap = TapParams::new(named("Covered"));
    tap.options = timeout(tap.options, 200);
    let mut tap = rig
        .lease
        .call::<method::ElementTap>(tap)
        .expect("the inbox has room");
    for _ in 0..8 {
        rig.frame();
    }
    assert!(tap.try_take().is_none(), "held while covered");
    // The turn past the deadline drains a press that leaves a rebuild pending, then attempts the
    // act once more, before any frame.
    let mut busy = rig
        .window
        .call::<method::SemanticsAct>(ActParams::new(named("Busy"), ActionName::Invoke))
        .expect("the inbox has room");
    rig.clock.advance(Duration::from_millis(200));
    rig.runtime.pump_background();
    busy.try_take()
        .expect("pressed at the drain")
        .expect("pressed");
    let refused = tap
        .try_take()
        .expect("answered on the turn past its deadline")
        .expect_err("covered to the end");
    assert_eq!(refused.code, ErrorCode::NotActionable, "{refused:?}");
    assert_eq!(
        refused.failed,
        Some(ActionabilityCheck::Receives),
        "{refused:?}"
    );
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

/// A presentation holds at most 64 acts. While it holds 64, one more is refused `busy` before
/// anything of it runs (a scroll sends no step) and the 64 still hold; acts whose askers let
/// their answers go hold no place, and the next act is held again.
#[test]
fn a_window_holds_a_bounded_number_of_acts() {
    let (mut host, _agent, window, lease, _presses) = shown();
    let gated = || {
        let mut tap = TapParams::new(named("Gated"));
        tap.options = timeout(tap.options, 60_000);
        tap
    };
    let mut held: Vec<AgentAnswer<ActOutcome>> = (0..64)
        .map(|_| ask::<method::ElementTap>(&lease, gated()))
        .collect();
    let journaled = journal(&mut host, &window).entries.len();
    let mut scroll = ScrollUntilParams::new(named("Items"), Locator::named("Item 60"));
    scroll.options = timeout(scroll.options, 60_000);
    let mut one_more = ask::<method::ElementScrollUntil>(&lease, scroll);
    let refused = answered(&mut host, &mut one_more, 2)
        .0
        .expect_err("past the bound");
    assert_eq!(refused.code, ErrorCode::Busy, "{refused:?}");
    for _ in 0..5 {
        let _ = host.pump(Duration::from_millis(16));
    }
    assert_eq!(
        journal(&mut host, &window).entries.len(),
        journaled,
        "the refused scroll sent no step"
    );
    assert!(
        held.iter_mut().all(|act| act.try_take().is_none()),
        "the 64 still hold"
    );
    drop(held);
    let mut after = ask::<method::ElementTap>(&lease, gated());
    for _ in 0..3 {
        let _ = host.pump(Duration::from_millis(16));
    }
    assert!(
        after.try_take().is_none(),
        "held once the askers of the 64 let their answers go"
    );
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

fn scroll_item_12_into_view(via: InputPolicy) -> ScrollIntoViewParams {
    let mut params: ScrollIntoViewParams = serde_json::from_value(json!({
        "target": { "locator": { "name": { "match": "exact", "text": "Item 12" } } },
    }))
    .expect("scroll into view reads");
    params.options.via = Some(via);
    params
}

/// A gesture asked on the `os` path is the driver's: the agent refuses it, naming the capability.
/// `scroll_into_view`, which has only a semantics action, is held to the same path rule: the
/// `os` path is refused, and a call that requires synthetic input is refused naming it.
#[test]
fn the_agent_refuses_a_gesture_on_the_os_path() {
    let rows: [(&str, Ask, &str); 4] = [
        (
            "a tap",
            |lease| {
                let mut tap = TapParams::new(named("Go"));
                tap.options.via = Some(InputPolicy::prefer(InputPath::Os));
                ask::<method::ElementTap>(lease, tap)
            },
            "os_input",
        ),
        (
            "scroll_into_view preferring the os path",
            |lease| {
                ask::<method::ElementScrollIntoView>(
                    lease,
                    scroll_item_12_into_view(InputPolicy::prefer(InputPath::Os)),
                )
            },
            "os_input",
        ),
        (
            "scroll_into_view requiring the os path",
            |lease| {
                ask::<method::ElementScrollIntoView>(
                    lease,
                    scroll_item_12_into_view(InputPolicy::require(InputPath::Os)),
                )
            },
            "os_input",
        ),
        (
            "scroll_into_view requiring synthetic input",
            |lease| {
                ask::<method::ElementScrollIntoView>(
                    lease,
                    scroll_item_12_into_view(InputPolicy::require(InputPath::Synthetic)),
                )
            },
            "synthetic_input",
        ),
    ];
    for (case, call, capability) in rows {
        let (mut host, _agent, window, lease, presses) = shown();
        let item = element(&mut host, &window, "Item 12");
        let before = bounds(&mut host, &window, "Item 12");
        let mut asked = call(&lease);
        let refused = answered(&mut host, &mut asked, 2).0.expect_err("refused");
        assert_eq!(refused.code, ErrorCode::NotSupported, "{case}: {refused:?}");
        assert_eq!(refused.capability.as_deref(), Some(capability), "{case}");
        assert_eq!(presses.go.load(Ordering::SeqCst), 0, "{case}");
        let _ = host.pump(Duration::from_millis(16));
        assert_eq!(
            bounds(&mut host, &window, "Item 12"),
            before,
            "{case}: nothing scrolled {item:?}"
        );
    }
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

/// A window that draws nothing still answers an act at its deadline, which wakes the host's loop,
/// and nothing of it goes out after the answer, the window seen again included: a tap whose target
/// never passes; a tap whose target passes once the deadline's turn finds the window idle, whose
/// input no frame starts (`timeout`); keys queued at once, which no frame starts either; and a
/// scroll whose step was queued as the window stopped drawing, which is withdrawn. Meanwhile the
/// runtime lists no cause a frame would serve, since none runs.
#[test]
fn a_window_that_draws_nothing_answers_a_held_act_at_its_deadline() {
    let rows: [(&str, Ask, ErrorCode); 4] = [
        (
            "a tap on a disabled target",
            |lease| {
                let mut tap = TapParams::new(named("Gated"));
                tap.options = timeout(tap.options, 200);
                ask::<method::ElementTap>(lease, tap)
            },
            ErrorCode::NotActionable,
        ),
        (
            "a tap on a target that passes",
            |lease| {
                let mut tap = TapParams::new(named("Go"));
                tap.options = timeout(tap.options, 200);
                ask::<method::ElementTap>(lease, tap)
            },
            ErrorCode::Timeout,
        ),
        (
            "keys queued at once",
            |lease| {
                let mut keys: PressKeysParams = serde_json::from_value(json!({
                    "window": lease.window(),
                    "keys": ["a"],
                }))
                .expect("press keys reads");
                keys.options = timeout(keys.options, 200);
                ask::<method::ElementPressKeys>(lease, keys)
            },
            ErrorCode::Timeout,
        ),
        (
            "a scroll with its step queued",
            |lease| {
                let mut until = ScrollUntilParams::new(named("Items"), Locator::named("Item 99"));
                until.options = timeout(until.options, 200);
                ask::<method::ElementScrollUntil>(lease, until)
            },
            ErrorCode::NotFound,
        ),
    ];
    for (case, call, code) in rows {
        let (mut host, _agent, window, lease, presses) = shown();
        let journaled = journal(&mut host, &window).entries.len();
        host.set_window_visible(false);
        let _ = host.pump(Duration::from_millis(16));
        let mut asked = call(&lease);
        let _ = host.pump(Duration::ZERO);
        host.request_frame();
        let unsettled = host.unsettled();
        assert!(
            unsettled
                .causes()
                .all(|cause| cause.cleared_by != ClearedBy::NextFrame),
            "{case}: no frame runs, so none is owed: {unsettled:?}"
        );
        assert!(
            unsettled.wake().is_some(),
            "{case}: the act's deadline wakes the loop"
        );
        let Some(answer) = host.run_until_answered(
            &mut asked,
            Duration::from_millis(16),
            Duration::from_secs(5),
        ) else {
            panic!("{case}: answered at its deadline");
        };
        let refused = answer.expect_err("nothing came of it");
        assert_eq!(refused.code, code, "{case}: {refused:?}");
        host.set_window_visible(true);
        for _ in 0..5 {
            let _ = host.pump(Duration::from_millis(16));
        }
        assert_eq!(
            journal(&mut host, &window).entries.len(),
            journaled,
            "{case}: nothing went out after the answer"
        );
        assert_eq!(
            presses.go.load(Ordering::SeqCst),
            0,
            "{case}: nothing pressed"
        );
    }
}

/// Input under way when its window stops drawing is answered within the act's timeout and the
/// time the input takes to play past it, the play bound the reply counts: a long press whose
/// press went out before the window was hidden answers `timeout` with the ticks that went out in
/// full, and nothing more of it goes out when the window is seen again.
#[test]
fn input_whose_window_stops_drawing_is_answered_within_its_play_time() {
    let (mut host, _agent, window, lease, _presses) = shown();
    let journaled = journal(&mut host, &window).entries.len();
    let mut press: LongPressParams = serde_json::from_value(json!({
        "target": { "locator": { "name": { "match": "exact", "text": "Go" } } },
        "duration_ms": 1_000,
    }))
    .expect("a long press reads");
    press.options = timeout(press.options, 300);
    let mut press = ask::<method::ElementLongPress>(&lease, press);
    let mut pressed = false;
    for _ in 0..30 {
        let _ = host.pump(Duration::from_millis(16));
        let mut read = window
            .call::<method::InputJournal>(JournalParams::new(window.id()))
            .expect("the inbox has room");
        let _ = host.pump(Duration::ZERO);
        let entries = read
            .try_take()
            .expect("read at the drain")
            .expect("the journal is read")
            .entries;
        if entries[journaled..]
            .iter()
            .any(|entry| entry.kind == InputKind::PointerDown)
        {
            pressed = true;
            break;
        }
    }
    assert!(pressed, "the press went out while the window drew");
    host.set_window_visible(false);
    let asked_by = host.clock().elapsed();
    let Some(answer) = host.run_until_answered(
        &mut press,
        Duration::from_millis(16),
        Duration::from_secs(5),
    ) else {
        panic!("answered while the window draws nothing");
    };
    let refused = answer.expect_err("its input did not play out");
    assert_eq!(refused.code, ErrorCode::Timeout, "{refused:?}");
    let effect = refused.effect.expect("a partial effect");
    assert!(
        effect.sent.is_some_and(|sent| sent >= 1),
        "the press went out in full: {effect:?}"
    );
    assert!(
        host.clock().elapsed().saturating_sub(asked_by) < Duration::from_millis(1_500),
        "answered within its play time past the timeout"
    );
    let cut = journal(&mut host, &window).entries.len();
    host.set_window_visible(true);
    for _ in 0..80 {
        let _ = host.pump(Duration::from_millis(16));
    }
    assert_eq!(
        journal(&mut host, &window).entries.len(),
        cut,
        "nothing of it went out after the answer"
    );
}

/// "Row 0" to "Row 149", each a line of text.
#[derive(Clone, StatelessView)]
struct Rows;

impl StatelessView for Rows {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Column::new(
            (0..150)
                .map(|row| Text::new(format!("Row {row}")).into_view().boxed())
                .collect::<Vec<BoxedView>>(),
        )
    }
}

/// A window serving its agent, showing `view`, its agent window, after its first frames.
fn serving(
    view: &(impl View + Clone + 'static),
    height: u32,
) -> (HeadlessHost, HeadlessDevAgent, AgentWindow) {
    let windows = Arc::new(Mutex::new(Vec::new()));
    let mut agent = HeadlessDevAgent::attach(Windows(Arc::clone(&windows)));
    let mut host = HeadlessHost::new(HeadlessWindow::new(200, height)).with_dev_agent(&mut agent);
    host.attach(view).expect("mount");
    let _ = host.pump(Duration::from_millis(16));
    let window = windows
        .lock()
        .pop()
        .expect("the host handed the agent its window");
    let _ = host.pump(Duration::from_millis(16));
    (host, agent, window)
}

/// The stability check remembers every element read between two frames, as many as the acts a
/// window holds read (two each) and elements a driver polls beside them: 150 elements read, and
/// read again after a frame, are each stable.
#[test]
fn the_stability_check_remembers_every_element_read_between_two_frames() {
    let (mut host, _agent, window) = serving(&Rows, 4000);
    // A strict locate lists every match.
    let mut locate = LocateParams::new(Locator::default().name_matching(TextMatch::prefix("Row ")));
    locate.max_matches = Some(200);
    let mut found = window
        .call::<method::SemanticsLocate>(locate)
        .expect("the inbox has room");
    let rows: Vec<ElementRef> = answered(&mut host, &mut found, 2)
        .0
        .expect("located")
        .matches
        .iter()
        .map(|found| found.element)
        .collect::<Vec<ElementRef>>();
    assert_eq!(rows.len(), 150, "every row is published");
    let read = |host: &mut HeadlessHost| -> Vec<bool> {
        let mut stable = Vec::with_capacity(rows.len());
        for batch in rows.chunks(50) {
            let mut asked: Vec<AgentAnswer<Actionability>> = batch
                .iter()
                .map(|row| {
                    window
                        .call::<method::ElementActionability>(ActionabilityParams::new(*row))
                        .expect("the inbox has room")
                })
                .collect::<Vec<AgentAnswer<Actionability>>>();
            let _ = host.pump(Duration::ZERO);
            for answer in &mut asked {
                stable.push(
                    answer
                        .try_take()
                        .expect("answered at the drain")
                        .expect("its facts")
                        .stable,
                );
            }
        }
        stable
    };
    let _ = read(&mut host);
    let _ = host.pump(Duration::from_millis(16));
    let unstable = read(&mut host).iter().filter(|stable| !**stable).count();
    assert_eq!(
        unstable, 0,
        "{unstable} of 150 rows read twice are not stable"
    );
}

/// What a gesture answers when it cannot run: a sequence the call's parameters make none of, a
/// chord that does not read, a name two elements share (with every candidate), a name nothing
/// has by the timeout, a handle whose element left the tree; and a point the call names, which
/// is ready at once and names no element.
#[test]
fn a_gesture_answers_why_it_did_not_run() {
    type Setup = fn(&mut HeadlessHost, &AgentWindow, &InputLease) -> AgentAnswer<ActOutcome>;
    let rows: [(&str, Setup, Result<(), ErrorCode>); 6] = [
        (
            "a pinch to no scale",
            |_, _, lease| {
                let params: PinchParams = serde_json::from_value(json!({
                    "target": { "locator": { "name": { "match": "exact", "text": "Go" } } },
                    "scale": 0.0,
                }))
                .expect("a pinch reads");
                ask::<method::ElementPinch>(lease, params)
            },
            Err(ErrorCode::InvalidArgument),
        ),
        (
            "a chord ending in a separator",
            |_, _, lease| {
                let params: PressKeysParams = serde_json::from_value(json!({
                    "target": { "locator": { "name": { "match": "exact", "text": "Go" } } },
                    "keys": ["Control+"],
                }))
                .expect("press keys reads");
                ask::<method::ElementPressKeys>(lease, params)
            },
            Err(ErrorCode::InvalidArgument),
        ),
        (
            "a name two elements share",
            |_, _, lease| ask::<method::ElementTap>(lease, TapParams::new(named("Twin"))),
            Err(ErrorCode::Ambiguous),
        ),
        (
            "a name nothing has",
            |_, _, lease| {
                let mut tap = TapParams::new(named("Nowhere"));
                tap.options = timeout(tap.options, 80);
                ask::<method::ElementTap>(lease, tap)
            },
            Err(ErrorCode::NotFound),
        ),
        (
            "a handle whose element left",
            |host, window, lease| {
                let item = element(host, window, "Item 0");
                let mut scrolled = ask::<method::ElementScrollUntil>(
                    lease,
                    until("Items", Locator::named("Item 60")),
                );
                answered(host, &mut scrolled, 200).0.expect("scrolled");
                for _ in 0..4 {
                    let _ = host.pump(Duration::from_millis(16));
                }
                ask::<method::ElementTap>(lease, TapParams::new(Target::Element(item)))
            },
            Err(ErrorCode::Gone),
        ),
        (
            "a point",
            |host, window, lease| {
                let at = bounds(host, window, "Go").center();
                ask::<method::ElementTap>(
                    lease,
                    TapParams::new(Target::Point {
                        window: window.id(),
                        at,
                    }),
                )
            },
            Ok(()),
        ),
    ];
    for (case, setup, expected) in rows {
        let (mut host, _agent, window, lease, presses) = shown();
        let mut asked = setup(&mut host, &window, &lease);
        let (answer, frames) = answered(&mut host, &mut asked, 30);
        if case == "a name nothing has" {
            // Held for one to appear until its 80 ms timeout, five frames of 16 ms.
            assert!(frames >= 5, "{case}: answered at frame {frames}");
        }
        match (answer, expected) {
            (Ok(outcome), Ok(())) => {
                assert_eq!(outcome.element, None, "{case}: a point names no element");
                assert_eq!(presses.go.load(Ordering::SeqCst), 1, "{case}");
            }
            (Err(refused), Err(code)) => {
                assert_eq!(refused.code, code, "{case}: {refused:?}");
                if code == ErrorCode::Ambiguous {
                    assert_eq!(refused.candidates.len(), 2, "{case}: {refused:?}");
                }
                assert_eq!(presses.go.load(Ordering::SeqCst), 0, "{case}");
            }
            (answer, expected) => panic!("{case}: {answer:?}, not {expected:?}"),
        }
    }
}

fn until(scrollable: &str, sought: Locator) -> ScrollUntilParams {
    ScrollUntilParams::new(named(scrollable), sought)
}

/// The elements whose name starts with "Item 1", picked by `pick`.
fn item_1(pick: Pick) -> Locator {
    Locator::default()
        .name_matching(TextMatch::prefix("Item 1"))
        .pick(pick)
}

/// `scroll_until`: past its deadline it answers `not_found` while the content still moves; a
/// direction the call names is never turned from; a pick other than strict finds its match where
/// it shows (the second, the last), a strict one that matches several under the scrollable is
/// `ambiguous`; a point names no scrollable; and a found element carries the reading asked for.
#[test]
fn scroll_until_answers_by_its_deadline_direction_and_pick() {
    type Make = fn(&AgentWindow) -> ScrollUntilParams;
    let rows: [(&str, Make, Result<&str, ErrorCode>); 7] = [
        (
            "past its deadline, the content still moving",
            |_| {
                let mut params = until("Items", Locator::named("Item 999"));
                params.options = timeout(params.options, 100);
                params
            },
            Err(ErrorCode::NotFound),
        ),
        (
            "down named on a row that does not scroll down",
            |_| {
                let mut params = until("Columns", Locator::named("Col 30"));
                params.direction = Some(Direction::Down);
                params
            },
            Err(ErrorCode::NotFound),
        ),
        (
            "the second match",
            |_| until("Items", item_1(Pick::Nth(1))),
            Ok("Item 10"),
        ),
        (
            "the last match",
            |_| until("Items", item_1(Pick::Last)),
            Ok("Item 19"),
        ),
        (
            "several under a strict pick",
            |_| until("Items", item_1(Pick::Strict)),
            Err(ErrorCode::Ambiguous),
        ),
        (
            "a point",
            |window| {
                ScrollUntilParams::new(
                    Target::Point {
                        window: window.id(),
                        at: LogicalPoint::new(10.0, 10.0),
                    },
                    Locator::named("Item 60"),
                )
            },
            Err(ErrorCode::InvalidArgument),
        ),
        (
            "a reading asked for",
            |_| {
                let mut params = until("Items", Locator::named("Item 30"));
                params.options.snapshot = Snapshot::Full;
                params
            },
            Ok("Item 30"),
        ),
    ];
    for (case, make, expected) in rows {
        let (mut host, _agent, window, lease, _presses) = shown();
        let params = make(&window);
        let reads = params.options.snapshot != Snapshot::None;
        let mut asked = ask::<method::ElementScrollUntil>(&lease, params);
        let (answer, frames) = answered(&mut host, &mut asked, 400);
        match (answer, expected) {
            (Ok(outcome), Ok(sought)) => {
                let found = outcome.element.expect("the element found");
                assert_eq!(found, element(&mut host, &window, sought), "{case}");
                assert_eq!(outcome.snapshot.is_some(), reads, "{case}: {outcome:?}");
            }
            (Err(refused), Err(code)) => {
                assert_eq!(refused.code, code, "{case}: {refused:?}");
            }
            (answer, expected) => panic!("{case}: {answer:?}, not {expected:?}"),
        }
        if case.starts_with("past its deadline") {
            // 100 rows in a viewport of five take 20 steps, two frames each at least, to end.
            assert!(
                frames < 20,
                "{case}: answered at the deadline, frame {frames}"
            );
        }
    }
}

/// `twins` texts "Twin" below a list "Tall" of `rows` rows, "Row 0" onward, each given upfront.
#[derive(Clone, StatelessView)]
struct Crowd {
    twins: usize,
    rows: usize,
}

impl StatelessView for Crowd {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let rows = Column::new(
            (0..self.rows)
                .map(|row| Text::new(format!("Row {row}")).into_view().boxed())
                .collect::<Vec<BoxedView>>(),
        );
        let tall = Semantics::new()
            .label("Tall")
            .container(true)
            .child(SizedBox::new(200.0, 100.0).child(SingleChildScrollView::new().child(rows)));
        let mut children = vec![tall.into_view().boxed()];
        children.extend((0..self.twins).map(|_| Text::new("Twin").into_view().boxed()));
        Column::new(children)
    }
}

/// `scroll_until` reads at most 4096 of the scrollable's descendants and looks among at most 4096
/// of the window's matches; a read that leaves some out is refused, never taken for the whole: a
/// scrollable with more descendants is `not_supported`, a locator with more matches
/// `invalid_argument`.
#[test]
fn scroll_until_refuses_a_read_that_leaves_some_out() {
    let rows: [(&str, Crowd, &str, ErrorCode); 2] = [
        (
            "a scrollable of 4200 rows",
            Crowd {
                twins: 0,
                rows: 4200,
            },
            "Row 4199",
            ErrorCode::NotSupported,
        ),
        (
            "4200 matches in the window",
            Crowd {
                twins: 4200,
                rows: 3,
            },
            "Twin",
            ErrorCode::InvalidArgument,
        ),
    ];
    for (case, crowd, sought, code) in rows {
        let (mut host, _agent, window) = serving(&crowd, 400);
        let lease = window
            .input_lease()
            .expect("an open window leases its input");
        let mut asked = ask::<method::ElementScrollUntil>(
            &lease,
            ScrollUntilParams::new(named("Tall"), Locator::named(sought)),
        );
        let refused = answered(&mut host, &mut asked, 400).0.expect_err("refused");
        assert_eq!(refused.code, code, "{case}: {refused:?}");
    }
}

/// A strict locator that matches 25 elements is refused naming all 25 and the first ten of them,
/// whether a gesture resolves it or `scroll_until` looks for it under its scrollable; a locator
/// that picks the first of them acts on it.
#[test]
fn a_strict_locator_counts_every_match_it_refuses() {
    type Make = fn(&InputLease) -> AgentAnswer<ActOutcome>;
    let rows: [(&str, Make, Option<usize>); 3] = [
        (
            "a tap on 25 twins",
            |lease| ask::<method::ElementTap>(lease, TapParams::new(named("Twin"))),
            Some(25),
        ),
        (
            "scroll_until 25 rows under the scrollable",
            |lease| {
                let rows = Locator::default().name_matching(TextMatch::prefix("Row "));
                ask::<method::ElementScrollUntil>(
                    lease,
                    ScrollUntilParams::new(named("Tall"), rows),
                )
            },
            Some(25),
        ),
        (
            "a tap on the first of 25 twins",
            |lease| {
                let first = Locator::named("Twin").pick(Pick::First);
                ask::<method::ElementTap>(lease, TapParams::new(Target::Locator(first)))
            },
            None,
        ),
    ];
    for (case, make, matched) in rows {
        let (mut host, _agent, window) = serving(
            &Crowd {
                twins: 25,
                rows: 25,
            },
            2_000,
        );
        let lease = window
            .input_lease()
            .expect("an open window leases its input");
        let mut asked = make(&lease);
        let answer = answered(&mut host, &mut asked, 60).0;
        match (answer, matched) {
            (Err(refused), Some(matched)) => {
                assert_eq!(refused.code, ErrorCode::Ambiguous, "{case}: {refused:?}");
                assert_eq!(
                    refused.message,
                    format!("the locator matches {matched} elements"),
                    "{case}"
                );
                assert_eq!(refused.candidates.len(), 10, "{case}: {refused:?}");
                assert!(
                    refused
                        .candidates
                        .iter()
                        .all(|line| !line.is_empty() && !line.contains('\n')),
                    "{case}: one outline line each: {refused:?}"
                );
            }
            (Ok(outcome), None) => {
                assert!(outcome.element.is_some(), "{case}: {outcome:?}");
            }
            (answer, matched) => panic!("{case}: {answer:?}, expected {matched:?} matched"),
        }
    }
}

/// Every gesture, and a perf span's start and stop, crosses the wire through the connection's
/// lease on its window: asked over the protocol, each is answered by the window, none refused as
/// asked without a lease. A gesture the method table gains has a row here or fails the coverage
/// check; which methods the dispatcher sends through the lease is held by its own table test.
#[test]
fn every_gesture_crosses_the_wire_through_the_connections_lease() {
    let go = json!({ "locator": { "name": { "match": "exact", "text": "Go" } } });
    let mut rows: Vec<(&str, serde_json::Value)> = vec![
        (method::ElementTap::NAME, json!({ "target": go })),
        (method::ElementDoubleTap::NAME, json!({ "target": go })),
        (
            method::ElementLongPress::NAME,
            json!({ "target": go, "duration_ms": 100 }),
        ),
        (
            method::ElementDragTo::NAME,
            json!({
                "target": go,
                "to": { "locator": { "name": { "match": "prefix", "text": "Count" } } },
            }),
        ),
        (
            method::ElementSwipe::NAME,
            json!({ "target": go, "direction": "up" }),
        ),
        (
            method::ElementFling::NAME,
            json!({ "target": go, "direction": "up" }),
        ),
        (
            method::ElementPinch::NAME,
            json!({ "target": go, "scale": 2.0 }),
        ),
        (
            method::ElementPressKeys::NAME,
            json!({ "target": go, "keys": ["a"] }),
        ),
        (
            method::ElementTypeText::NAME,
            json!({ "target": go, "text": "hi" }),
        ),
        (method::ElementScrollIntoView::NAME, json!({ "target": go })),
        (
            method::ElementScrollUntil::NAME,
            json!({ "target": go, "until": { "name": { "match": "exact", "text": "Go" } } }),
        ),
    ];
    for gesture in method::METHODS
        .iter()
        .filter(|row| row.name.starts_with("element.") && row.effect.changes_app())
    {
        assert!(
            rows.iter().any(|(name, _)| *name == gesture.name),
            "{} has a row",
            gesture.name
        );
    }
    let service = crate::agent_wire::service(Limits::default(), None);
    let (mut host, _agent, _presses) = crate::agent_wire::shown(&service);
    let waker = Waker::from(CountingWaker::new().shared());
    let now = Instant::now();
    let mut session = crate::agent_wire::greeted(&service, now, &waker);
    // A perf span is the connection's too: it starts and stops through the same lease.
    session.receive(
        &crate::agent_wire::line(
            &json!({ "id": "windows", "method": "windows.list", "params": {} }),
        ),
        now,
    );
    session.poll(now, &waker);
    let _ = host.pump(Duration::ZERO);
    session.poll(now, &waker);
    let listed = crate::agent_wire::transmitted(&mut session);
    let Some(answer) = listed.iter().find(|line| line["id"] == json!("windows")) else {
        panic!("windows.list is answered: {listed:?}");
    };
    let handle = answer["result"]["windows"][0].clone();
    rows.push((method::PerfStart::NAME, json!({ "window": handle })));
    rows.push((method::PerfStop::NAME, json!({ "window": handle })));
    for (id, (name, params)) in rows.iter().enumerate() {
        session.receive(
            &crate::agent_wire::line(&json!({ "id": id, "method": name, "params": params })),
            now,
        );
        session.poll(now, &waker);
        let mut answer = None;
        for _ in 0..200 {
            let _ = host.pump(Duration::from_millis(16));
            session.poll(now, &waker);
            if let Some(line) = crate::agent_wire::transmitted(&mut session)
                .into_iter()
                .find(|line| line["id"] == json!(id))
            {
                answer = Some(line);
                break;
            }
        }
        let answer = answer.unwrap_or_else(|| panic!("{name} is answered"));
        assert_ne!(
            answer["error"]["code"],
            json!("invalid_argument"),
            "{name} went through the lease: {answer}"
        );
    }
}

/// Sends `params` to `name` over `session` as request `id`, and the line that answers it, pumping
/// `host` until it comes.
fn ask_over_wire(
    session: &mut flui_runtime::agent::Session,
    host: &mut HeadlessHost,
    id: &str,
    name: &str,
    params: serde_json::Value,
) -> serde_json::Value {
    let waker = Waker::from(CountingWaker::new().shared());
    let now = Instant::now();
    session.receive(
        &crate::agent_wire::line(&json!({ "id": id, "method": name, "params": params })),
        now,
    );
    session.poll(now, &waker);
    for _ in 0..200 {
        let _ = host.pump(Duration::from_millis(16));
        session.poll(now, &waker);
        if let Some(line) = crate::agent_wire::transmitted(session)
            .into_iter()
            .find(|line| line["id"] == json!(id))
        {
            return line;
        }
    }
    panic!("{name} is answered");
}

/// A perf span asked over the protocol is its connection's, through the connection's lease: when
/// the connection that started it ends, the span ends with its lease, and the next driver's
/// `perf.stop` finds none to end. Asked without the lease, the span would belong to the window's
/// handles every connection shares, and outlive its connection.
#[test]
fn a_perf_span_asked_over_the_wire_ends_with_its_connection() {
    let service = crate::agent_wire::service(Limits::default(), None);
    let (mut host, _agent, _presses) = crate::agent_wire::shown(&service);
    let waker = Waker::from(CountingWaker::new().shared());
    let mut first = crate::agent_wire::greeted(&service, Instant::now(), &waker);
    let listed = ask_over_wire(&mut first, &mut host, "windows", "windows.list", json!({}));
    let handle = listed["result"]["windows"][0].clone();
    let started = ask_over_wire(
        &mut first,
        &mut host,
        "start",
        method::PerfStart::NAME,
        json!({ "window": handle }),
    );
    assert!(started.get("error").is_none(), "started: {started}");
    drop(first);
    for _ in 0..4 {
        let _ = host.pump(Duration::from_millis(16));
    }
    let mut next = crate::agent_wire::greeted(&service, Instant::now(), &waker);
    let listed = ask_over_wire(&mut next, &mut host, "windows", "windows.list", json!({}));
    let handle = listed["result"]["windows"][0].clone();
    let stopped = ask_over_wire(
        &mut next,
        &mut host,
        "stop",
        method::PerfStop::NAME,
        json!({ "window": handle }),
    );
    assert_eq!(
        stopped["error"]["code"],
        json!("invalid_argument"),
        "the span ended with its connection: {stopped}"
    );
    assert!(
        stopped["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("no perf span runs")),
        "{stopped}"
    );
}
