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
    AnimatedPadding, Column, ListView, Listener, Padding, Positioned, RawButton, ScrollView,
    Semantics, SingleChildScrollView, SizedBox, Stack, Text,
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
            |lease| {
                let mut fling = swipe_up();
                fling.distance = Some(80.0);
                ask::<method::ElementFling>(lease, fling)
            },
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

/// What `element` shows in `rig`'s window, read at one drain.
fn facts_in(rig: &mut crate::agent_rig::Rig, element: ElementRef) -> Actionability {
    let mut facts = rig
        .window
        .call::<method::ElementActionability>(ActionabilityParams::new(element))
        .expect("the inbox has room");
    rig.runtime.pump_background();
    facts
        .try_take()
        .expect("answered at the drain")
        .expect("its facts")
}

/// "Jump", whose press moves "Target" 80 px right on the next frame.
#[derive(Clone, StatefulView)]
struct Jumping;

struct JumpingState {
    jumped: Signal<bool>,
}

impl StatefulView for Jumping {
    type State = JumpingState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        JumpingState {
            jumped: ctx.signal(false),
        }
    }
}

impl ViewState<Jumping> for JumpingState {
    fn build(&self, _view: &Jumping, ctx: &dyn BuildContext) -> impl IntoView {
        let jumped = self.jumped;
        let inset = if jumped.get(ctx) {
            EdgeInsets::only_left(80.0)
        } else {
            EdgeInsets::ZERO
        };
        Column::new(vec![
            RawButton::new(Text::new("Jump"))
                .on_pressed(move |ctx| jumped.update(ctx, |on| *on = true))
                .into_view()
                .boxed(),
            Padding::new(inset)
                .child(RawButton::new(Text::new("Target")).on_pressed(|_| {}))
                .into_view()
                .boxed(),
        ])
    }
}

/// The stability an idle window answered is not carried to a later read of the same frame with
/// work pending: a target read twice while nothing is pending, then moved by a handler in the
/// drain that asks for a tap on it, is held until it holds still, and the press lands where it
/// rests, not where it was when the tap was asked.
#[test]
fn stability_answered_while_idle_does_not_pass_an_act_once_work_is_pending() {
    let mut rig = crate::agent_rig::Rig::showing(&Jumping);
    let mut located = rig
        .window
        .call::<method::SemanticsLocate>(LocateParams::new(Locator::named("Target")))
        .expect("the inbox has room");
    rig.runtime.pump_background();
    let target = located
        .try_take()
        .expect("located at the drain")
        .expect("located")
        .matches[0]
        .element;
    let _ = facts_in(&mut rig, target);
    let asked = facts_in(&mut rig, target);
    assert!(asked.stable, "idle, it holds still: {asked:?}");
    let asked_at = asked.rect.expect("Target shows bounds").center();
    // One drain: the handler moves it, then the tap is attempted with that work pending and no
    // frame since the idle read.
    let mut moved = rig
        .window
        .call::<method::SemanticsAct>(ActParams::new(named("Jump"), ActionName::Invoke))
        .expect("the inbox has room");
    let mut tap = rig
        .lease
        .call::<method::ElementTap>(TapParams::new(named("Target")))
        .expect("the inbox has room");
    rig.runtime.pump_background();
    moved
        .try_take()
        .expect("pressed at the drain")
        .expect("pressed");
    let mut outcome = None;
    for _ in 0..90 {
        rig.frame();
        if let Some(answer) = tap.try_take() {
            outcome = Some(answer.expect("the target settles and is tapped"));
            break;
        }
    }
    let outcome = outcome.expect("the tap is answered within 90 frames");
    let rest = facts_in(&mut rig, target)
        .rect
        .expect("Target shows bounds")
        .center();
    assert!(rest.x - asked_at.x > 10.0, "{asked_at:?} to {rest:?}");
    let span = outcome.observed.expect("the tap made entries");
    let down = rig
        .journal()
        .entries
        .into_iter()
        .filter(|entry| (span.from..=span.to).contains(&entry.seq))
        .find(|entry| entry.kind == InputKind::PointerDown)
        .expect("the tap pressed");
    let at = down.at.expect("a press has a position");
    assert!(
        (at.x - rest.x).abs() < 0.5 && (at.y - rest.y).abs() < 0.5,
        "pressed at {at:?}, the target rests at {rest:?}, and was at {asked_at:?}"
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
/// frame before, while the app has work pending. Whether the element holds still is not known
/// then (the work pending may move it on the next frame), so a second reader of that frame is not
/// told it is stable, and the act is not performed; but the answer names the check the committed
/// tree fails outright, `receives` for a covered button, not `stable`.
#[test]
fn a_last_attempt_on_a_turn_without_a_frame_names_the_check_that_failed() {
    let mut rig = crate::agent_rig::Rig::showing(&Busy);
    let mut tap = TapParams::new(named("Covered"));
    tap.options = timeout(tap.options, 200);
    let mut tap = rig
        .lease
        .call::<method::ElementTap>(tap)
        .expect("the inbox has room");
    let mut located = rig
        .window
        .call::<method::SemanticsLocate>(LocateParams::new(Locator::named("Covered")))
        .expect("the inbox has room");
    for _ in 0..8 {
        rig.frame();
    }
    let covered = located
        .try_take()
        .expect("located")
        .expect("located")
        .matches[0]
        .element;
    assert!(tap.try_take().is_none(), "held while covered");
    // The turn past the deadline drains a press that leaves a rebuild pending and a second reader
    // of the element, then attempts the act once more, before any frame.
    let mut busy = rig
        .window
        .call::<method::SemanticsAct>(ActParams::new(named("Busy"), ActionName::Invoke))
        .expect("the inbox has room");
    let mut polled = rig
        .window
        .call::<method::ElementActionability>(ActionabilityParams::new(covered))
        .expect("the inbox has room");
    rig.clock.advance(Duration::from_millis(200));
    rig.runtime.pump_background();
    busy.try_take()
        .expect("pressed at the drain")
        .expect("pressed");
    let facts = polled
        .try_take()
        .expect("read at the drain")
        .expect("its facts");
    assert!(
        !facts.stable,
        "with work pending and no frame since, nothing says it holds still: {facts:?}"
    );
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
    let plays = gestures::double_tap(
        origin,
        PointerButton::Primary,
        &gestures::Defaults::WINDOWLESS,
    )
    .plays_within();
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
        if capability == "synthetic_input" {
            assert_eq!(
                refused.message, "this gesture has no synthetic input that does the same",
                "{case}"
            );
        }
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
/// never passes; a tap on a point, ready at once, a forced tap, ready once attached, and a tap
/// whose target passes once the deadline's turn finds the window idle, each of whose
/// input no frame starts (`timeout`); keys queued at once, which no frame starts either; and a
/// scroll whose step was queued as the window stopped drawing, which is withdrawn. Meanwhile the
/// runtime lists no cause a frame would serve, since none runs.
#[test]
fn a_window_that_draws_nothing_answers_a_held_act_at_its_deadline() {
    let rows: [(&str, Ask, ErrorCode); 6] = [
        (
            "a tap on a point, ready at once",
            |lease| {
                let mut tap = TapParams::new(Target::Point {
                    window: lease.window(),
                    at: LogicalPoint::new(10.0, 10.0),
                });
                tap.options = timeout(tap.options, 200);
                ask::<method::ElementTap>(lease, tap)
            },
            ErrorCode::Timeout,
        ),
        (
            "a forced tap, ready once attached",
            |lease| {
                let mut tap = TapParams::new(named("Go"));
                tap.options = timeout(tap.options, 200);
                tap.options.force = true;
                ask::<method::ElementTap>(lease, tap)
            },
            ErrorCode::Timeout,
        ),
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
        let served_at = host.clock().elapsed();
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
        assert_eq!(
            host.clock().elapsed().saturating_sub(served_at),
            Duration::from_millis(200),
            "{case}: answered at its deadline, input none of whose ticks started included"
        );
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
    assert_eq!(
        (effect.sent, effect.total),
        (Some(2), Some(4)),
        "the move and the press went out in full, of the move, press, hold and lift: {effect:?}"
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

/// Input under way when its window stops drawing is answered at the end of its act's promise and
/// not before: the deadline, then the play time the reply counts (what its ticks last, and a frame
/// for each), on an owner turn without a frame. A long press of 1000 ms whose press went out is
/// still held a millisecond before that instant, and answered at it with the two ticks played.
#[test]
fn input_under_way_is_answered_at_its_deadline_and_play_time() {
    let mut rig = crate::agent_rig::Rig::showing(&Board {
        presses: Presses::default(),
    });
    let mut press: LongPressParams = serde_json::from_value(json!({
        "target": { "locator": { "name": { "match": "exact", "text": "Go" } } },
        "duration_ms": 1_000,
    }))
    .expect("a long press reads");
    press.options = timeout(press.options, 300);
    let mut press = rig
        .lease
        .call::<method::ElementLongPress>(press)
        .expect("the inbox has room");
    rig.runtime.pump_background();
    let served = flui_foundation::MonotonicClock::now(&rig.clock);
    // The move and the press go out on the boundaries after it passes; the hold starts.
    for _ in 0..3 {
        rig.frame();
    }
    let plays = gestures::long_press(
        LogicalPoint::new(0.0, 0.0),
        Some(1_000),
        &gestures::Defaults::WINDOWLESS,
    )
    .plays_within();
    let ends = served + Duration::from_millis(300) + plays;
    let now = flui_foundation::MonotonicClock::now(&rig.clock);
    rig.clock
        .advance(ends.saturating_duration_since(now) - Duration::from_millis(1));
    rig.runtime.pump_background();
    assert!(
        press.try_take().is_none(),
        "held a millisecond before its promise ends"
    );
    rig.clock.advance(Duration::from_millis(1));
    rig.runtime.pump_background();
    let refused = press
        .try_take()
        .expect("answered as its promise ends")
        .expect_err("its input did not play out");
    assert_eq!(refused.code, ErrorCode::Timeout, "{refused:?}");
    let effect = refused.effect.expect("a partial effect");
    assert_eq!(
        (effect.sent, effect.total),
        (Some(2), Some(4)),
        "the move and the press went out in full: {effect:?}"
    );
}

/// How many of `entries` are of `kind`.
fn count_of(entries: &[flui_protocol::input::JournalEntry], kind: InputKind) -> usize {
    entries.iter().filter(|entry| entry.kind == kind).count()
}

/// Pumps 250 ms frames, four times slower than the 60 Hz an act's play time counts, until `act`
/// is answered, at most 20 of them: the failure it answers, as an act cut off by its promise does.
fn refused_slowly(host: &mut HeadlessHost, act: &mut AgentAnswer<ActOutcome>) -> ErrorObject {
    for _ in 0..20 {
        let _ = host.pump(Duration::from_millis(250));
        if let Some(answer) = act.try_take() {
            return answer.expect_err("its input did not play out by its promise");
        }
    }
    panic!("no answer within 20 slow frames");
}

/// An act whose input stops before its last tick, its promise run out in a window drawing slower
/// than the 60 Hz the play time counts, lets go of what it pressed as it is answered: a drag cut
/// after its press cancels its contact, so the same lease's next tap presses and the button sees
/// the tap; a chord cut after its first keys lifts them.
#[test]
fn an_act_stopped_mid_play_lets_go_of_what_it_pressed() {
    let (mut host, _agent, window, lease, presses) = shown();
    let journaled = journal(&mut host, &window).entries.len();
    let mut drag: DragParams = serde_json::from_value(json!({
        "target": { "locator": { "name": { "match": "exact", "text": "Go" } } },
        "to": { "locator": { "name": { "match": "exact", "text": "Enable" } } },
    }))
    .expect("a drag reads");
    drag.options = timeout(drag.options, 50);
    let mut drag = ask::<method::ElementDragTo>(&lease, drag);
    let cut = refused_slowly(&mut host, &mut drag);
    assert_eq!(cut.code, ErrorCode::Timeout, "{cut:?}");
    assert!(
        cut.effect
            .as_ref()
            .and_then(|effect| effect.sent)
            .is_some_and(|sent| sent >= 2),
        "it was cut after its press: {cut:?}"
    );
    let entries = journal(&mut host, &window).entries.split_off(journaled);
    assert_eq!(count_of(&entries, InputKind::PointerDown), 1, "{entries:?}");
    assert_eq!(
        count_of(&entries, InputKind::PointerUp) + count_of(&entries, InputKind::PointerCancel),
        1,
        "the press is let go: {entries:?}"
    );
    let mut tap = ask::<method::ElementTap>(&lease, TapParams::new(named("Go")));
    let outcome = answered(&mut host, &mut tap, 30).0.expect("the tap runs");
    assert_eq!(outcome.effect.kind, EffectKind::Ran);
    let made = kinds_made(&mut host, &window, &outcome);
    assert!(
        made.contains(&InputKind::PointerDown) && made.contains(&InputKind::PointerUp),
        "the next tap through the lease presses and lifts: {made:?}"
    );
    assert_eq!(presses.go.load(Ordering::SeqCst), 1, "Go saw the tap");

    let (mut host, _agent, window, lease, _presses) = shown();
    let journaled = journal(&mut host, &window).entries.len();
    let mut keys: PressKeysParams = serde_json::from_value(json!({
        "window": lease.window(),
        "keys": ["Shift+Control+a"],
    }))
    .expect("press keys reads");
    keys.options = timeout(keys.options, 50);
    let mut keys = ask::<method::ElementPressKeys>(&lease, keys);
    let cut = refused_slowly(&mut host, &mut keys);
    assert_eq!(cut.code, ErrorCode::Timeout, "{cut:?}");
    let entries = journal(&mut host, &window).entries.split_off(journaled);
    let down = count_of(&entries, InputKind::KeyDown);
    assert!(down >= 1, "a key went down before the cut: {entries:?}");
    assert_eq!(
        count_of(&entries, InputKind::KeyUp),
        down,
        "every key it pressed is up: {entries:?}"
    );
}

/// An act whose last tick has played is answered by its deadline even when its window stops
/// drawing before the frame its reading waits for: a tap whose release reached the button, with
/// the window hidden right after that boundary, is answered `ran` by the owner turn at the end of
/// its promise, which wakes the loop.
#[test]
fn an_act_whose_window_stops_drawing_after_its_input_played_is_answered_by_its_deadline() {
    let (mut host, _agent, _window, lease, presses) = shown();
    let mut tap = TapParams::new(named("Go"));
    tap.options = timeout(tap.options, 300);
    let mut tap = ask::<method::ElementTap>(&lease, tap);
    let mut played = false;
    for _ in 0..30 {
        let _ = host.pump(Duration::from_millis(16));
        if presses.go.load(Ordering::SeqCst) == 1 {
            played = true;
            break;
        }
    }
    assert!(played, "the tap's release reached Go while the window drew");
    assert!(
        tap.try_take().is_none(),
        "its answer waits for the frame after its last tick"
    );
    host.set_window_visible(false);
    let hidden_at = host.clock().elapsed();
    let Some(answer) =
        host.run_until_answered(&mut tap, Duration::from_millis(16), Duration::from_secs(5))
    else {
        panic!("answered by its deadline while the window draws nothing");
    };
    let outcome = answer.expect("its input played");
    assert_eq!(outcome.effect.kind, EffectKind::Ran, "{outcome:?}");
    assert!(
        !outcome.readback_failed,
        "no reading was asked for: {outcome:?}"
    );
    assert!(
        host.clock().elapsed().saturating_sub(hidden_at) < Duration::from_millis(1_000),
        "answered by its promise, not at the 5 s bound"
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
    let rows: [(&str, Setup, Result<(), ErrorCode>); 10] = [
        (
            "a pinch inside the scale slop",
            |_, _, lease| {
                let params: PinchParams = serde_json::from_value(json!({
                    "target": { "locator": { "name": { "match": "exact", "text": "Go" } } },
                    "scale": 1.01,
                }))
                .expect("a pinch reads");
                ask::<method::ElementPinch>(lease, params)
            },
            Err(ErrorCode::InvalidArgument),
        ),
        (
            "a default fling over too short a stroke",
            |_, _, lease| ask::<method::ElementFling>(lease, swipe_up()),
            Err(ErrorCode::InvalidArgument),
        ),
        (
            "a drag inside the drag slop",
            |_, _, lease| {
                let params: DragParams = serde_json::from_value(json!({
                    "target": { "locator": { "name": { "match": "exact", "text": "Go" } } },
                    "to": { "locator": { "name": { "match": "exact", "text": "Go" } } },
                }))
                .expect("a drag reads");
                ask::<method::ElementDragTo>(lease, params)
            },
            Err(ErrorCode::InvalidArgument),
        ),
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
            "a chord naming a mistyped key",
            |_, _, lease| {
                let params: PressKeysParams = serde_json::from_value(json!({
                    "target": { "locator": { "name": { "match": "exact", "text": "Go" } } },
                    "keys": ["Ctrl-a"],
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
                let says = match case {
                    "a pinch inside the scale slop" => "does not pass this window's scale slop",
                    "a default fling over too short a stroke" => {
                        "name a longer distance or a slower velocity"
                    }
                    "a drag inside the drag slop" => "does not pass this window's drag slop",
                    _ => "",
                };
                assert!(refused.message.contains(says), "{case}: {refused:?}");
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
    let rows: [(&str, Make, Result<&str, ErrorCode>); 9] = [
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
            "the first match",
            |_| until("Items", item_1(Pick::First)),
            Ok("Item 1"),
        ),
        (
            "a scrollable that never appears",
            |_| {
                let mut params = until("Nowhere", Locator::named("Item 60"));
                params.options = timeout(params.options, 100);
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
                if case == "a point" {
                    assert_eq!(refused.message, "a point names no scrollable", "{case}");
                }
            }
            (answer, expected) => panic!("{case}: {answer:?}, not {expected:?}"),
        }
        if case == "a scrollable that never appears" {
            // Held for one to appear until its 100 ms timeout, six frames of 16 ms.
            assert!(frames >= 6, "{case}: answered at frame {frames}");
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
        let says = match case {
            "a scrollable of 4200 rows" => {
                "the scrollable holds more descendants than a scroll reads (4096)"
            }
            _ => {
                "the locator matches more elements in the window than a scroll looks among (4096): narrow it"
            }
        };
        assert_eq!(refused.message, says, "{case}");
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
            json!({ "target": go, "direction": "up", "distance": 80 }),
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

/// `element.type_text` of "hi" into "Go", on `via`.
fn text_into_go(via: InputPolicy) -> TypeTextParams {
    let mut params: TypeTextParams = serde_json::from_value(json!({
        "target": { "locator": { "name": { "match": "exact", "text": "Go" } } },
        "text": "hi",
    }))
    .expect("type text reads");
    params.options.via = Some(via);
    params
}

/// A point at (10, 10) in `window`.
fn point_in(window: &AgentWindow) -> Target {
    Target::Point {
        window: window.id(),
        at: LogicalPoint::new(10.0, 10.0),
    }
}

/// What a gesture's target and path rules answer, each refusal naming why: a point and a path the
/// gesture has a form on are taken; a path it has no form on falls back unless required, then is
/// refused; a point to scroll into view and a call with no lease are
/// refused before anything is held; text typed on the semantic path focuses its target through
/// the semantics action and injects only the text.
#[test]
fn a_gesture_answers_by_its_target_and_path_rules() {
    type Make = fn(&AgentWindow, &InputLease) -> AgentAnswer<ActOutcome>;
    type Expected = Result<InputPath, (ErrorCode, &'static str)>;
    let rows: [(&str, Make, Expected); 10] = [
        (
            "a tap on a point preferring the semantic path falls back to synthetic input",
            |window, lease| {
                let mut tap = TapParams::new(point_in(window));
                tap.options.via = Some(InputPolicy::prefer(InputPath::Semantic));
                ask::<method::ElementTap>(lease, tap)
            },
            Ok(InputPath::Synthetic),
        ),
        (
            "a secondary tap requiring the semantic path",
            |_, lease| {
                let mut tap = TapParams::new(named("Go"));
                tap.button = PointerButton::Secondary;
                tap.options.via = Some(InputPolicy::require(InputPath::Semantic));
                ask::<method::ElementTap>(lease, tap)
            },
            Err((
                ErrorCode::NotSupported,
                "this gesture has no semantics action that does the same",
            )),
        ),
        (
            "scroll_into_view requiring the semantic path",
            |_, lease| {
                ask::<method::ElementScrollIntoView>(
                    lease,
                    scroll_item_12_into_view(InputPolicy::require(InputPath::Semantic)),
                )
            },
            Ok(InputPath::Semantic),
        ),
        (
            "scroll_into_view on a point",
            |window, lease| {
                let mut params = scroll_item_12_into_view(InputPolicy::default());
                params.target = point_in(window);
                ask::<method::ElementScrollIntoView>(lease, params)
            },
            Err((
                ErrorCode::InvalidArgument,
                "a point names no element to scroll into view",
            )),
        ),
        (
            "text requiring the semantic path",
            |_, lease| {
                ask::<method::ElementTypeText>(
                    lease,
                    text_into_go(InputPolicy::require(InputPath::Semantic)),
                )
            },
            Err((
                ErrorCode::NotSupported,
                "text and keys have no semantics action that does the same",
            )),
        ),
        (
            "text requiring synthetic input",
            |_, lease| {
                ask::<method::ElementTypeText>(
                    lease,
                    text_into_go(InputPolicy::require(InputPath::Synthetic)),
                )
            },
            Ok(InputPath::Synthetic),
        ),
        (
            "text preferring the semantic path",
            |_, lease| {
                ask::<method::ElementTypeText>(
                    lease,
                    text_into_go(InputPolicy::prefer(InputPath::Semantic)),
                )
            },
            Ok(InputPath::Synthetic),
        ),
        (
            "a swipe down on a point naming no distance",
            |window, lease| {
                let mut swipe = swipe_up();
                swipe.direction = Direction::Down;
                swipe.target = Target::Point {
                    window: window.id(),
                    at: LogicalPoint::new(100.0, 200.0),
                };
                ask::<method::ElementSwipe>(lease, swipe)
            },
            Ok(InputPath::Synthetic),
        ),
        (
            "a tap asked with no lease, on a target that never passes",
            |window, _| {
                window
                    .call::<method::ElementTap>(TapParams::new(named("Gated")))
                    .expect("the inbox has room")
            },
            Err((
                ErrorCode::InvalidArgument,
                "a gesture's synthetic input is asked through an input lease",
            )),
        ),
        (
            "a tap on the os path",
            |_, lease| {
                let mut tap = TapParams::new(named("Go"));
                tap.options.via = Some(InputPolicy::prefer(InputPath::Os));
                ask::<method::ElementTap>(lease, tap)
            },
            Err((
                ErrorCode::NotSupported,
                "the os path is composed by the driver with its lane, not in the app",
            )),
        ),
    ];
    for (case, make, expected) in rows {
        let (mut host, _agent, window, lease, _presses) = shown();
        let mut asked = make(&window, &lease);
        let (answer, frames) = answered(&mut host, &mut asked, 60);
        match (answer, expected) {
            (Ok(outcome), Ok(via)) => {
                assert_eq!(outcome.via, via, "{case}: {outcome:?}");
                if case.starts_with("text preferring") {
                    // The focus is the semantics action; only the text is injected.
                    assert_eq!(
                        kinds_made(&mut host, &window, &outcome),
                        [InputKind::Text],
                        "{case}"
                    );
                    assert!(outcome.element.is_some(), "{case}: {outcome:?}");
                    let go = element(&mut host, &window, "Go");
                    let mut read = window
                        .call::<method::SemanticsRead>(
                            flui_protocol::tree::ReadParams::new(window.id())
                                .query(flui_protocol::tree::ReadQuery::new().root(go.element)),
                        )
                        .expect("the inbox has room");
                    let tree = answered(&mut host, &mut read, 2).0.expect("read");
                    assert!(tree.roots[0].focused, "{case}: Go has focus: {tree:?}");
                }
            }
            (Err(refused), Err((code, says))) => {
                assert_eq!(refused.code, code, "{case}: {refused:?}");
                assert_eq!(refused.message, says, "{case}: {refused:?}");
                if code == ErrorCode::NotSupported {
                    let capability = if case.contains("os path") {
                        "os_input"
                    } else {
                        "semantic_input"
                    };
                    assert_eq!(refused.capability.as_deref(), Some(capability), "{case}");
                }
                assert_eq!(frames, 1, "{case}: refused before anything is held");
            }
            (answer, expected) => panic!("{case}: {answer:?}, not {expected:?}"),
        }
    }
}

/// Whether `host`'s loop runs a frame at once: what a window holding acts asks for, as a wake, not
/// as a cause of the application's (ADR-0400 §3), which the test checks apart.
fn asks_for_a_frame(host: &mut HeadlessHost) -> bool {
    host.time_to_next_frame() == Some(Duration::ZERO)
}

/// A gated tap, held until `timeout_ms` passes, through `lease`; on the semantic path when
/// `semantic`.
fn gated(lease: &InputLease, timeout_ms: u64, semantic: bool) -> AgentAnswer<ActOutcome> {
    let mut tap = TapParams::new(named("Gated"));
    tap.options = timeout(tap.options, timeout_ms);
    if semantic {
        tap.options.via = Some(InputPolicy::require(InputPath::Semantic));
    }
    ask::<method::ElementTap>(lease, tap)
}

/// The window's held acts are each their own: an act nobody awaits any more injects nothing and
/// holds back none behind it; a lease let go ends the acts that inject through it and no other,
/// and not an act on the semantic path, which injects through none; while an act is held the
/// window asks for frames, and once the last is answered it stops asking.
#[test]
fn each_held_act_ends_on_its_own() {
    let (mut host, _agent, window, lease, presses) = shown();
    // Given up on: injects nothing when its target turns ready, and holds back no act behind it.
    let mut before = ask::<method::ElementTap>(&lease, TapParams::new(named("Go")));
    let abandoned = gated(&lease, 60_000, false);
    let mut go = ask::<method::ElementTap>(&lease, TapParams::new(named("Move")));
    // All three are held at the drain that serves them, then the middle one is given up on.
    let _ = host.pump(Duration::ZERO);
    assert!(go.try_take().is_none(), "held for stability");
    drop(abandoned);
    answered(&mut host, &mut go, 30)
        .0
        .expect("the act behind the one given up on is performed");
    answered(&mut host, &mut before, 30)
        .0
        .expect("the act before the one given up on is performed");
    press(&mut host, &window, "Enable");
    for _ in 0..10 {
        let _ = host.pump(Duration::from_millis(16));
    }
    assert_eq!(
        presses.gated.load(Ordering::SeqCst),
        0,
        "an act nobody awaits injects nothing"
    );

    let (mut host, _agent, window, first, _presses) = shown();
    let second = window.input_lease().expect("leased again");
    let mut through_first = gated(&first, 60_000, false);
    let mut through_second = gated(&second, 60_000, false);
    let mut semantic = gated(&first, 300, true);
    for _ in 0..3 {
        let _ = host.pump(Duration::from_millis(16));
    }
    assert!(
        asks_for_a_frame(&mut host),
        "a window holding acts asks for frames"
    );
    assert!(
        host.unsettled()
            .causes()
            .all(|cause| cause.cleared_by != ClearedBy::NextFrame),
        "the frames a held act asks for are the agent's wake, not the application's cause"
    );
    drop(first);
    let gone = answered(&mut host, &mut through_first, 4)
        .0
        .expect_err("its lease went");
    assert_eq!(gone.code, ErrorCode::Gone, "{gone:?}");
    for _ in 0..3 {
        let _ = host.pump(Duration::from_millis(16));
    }
    assert!(
        through_second.try_take().is_none(),
        "the act through the other lease holds on"
    );
    let timed_out = answered(&mut host, &mut semantic, 40)
        .0
        .expect_err("the semantic act holds to its timeout, not to the lease");
    assert_eq!(timed_out.code, ErrorCode::NotActionable, "{timed_out:?}");
    drop(through_second);
    drop(second);
    for _ in 0..3 {
        let _ = host.pump(Duration::from_millis(16));
    }
    assert!(
        !asks_for_a_frame(&mut host),
        "a window holding no act asks for no frame"
    );
}

/// On a turn without a frame the acts whose deadline passed are answered and the others hold on,
/// in whatever order they were held, and an act nobody awaits holds back none behind it: in a
/// window that draws nothing, a tap held to 60 s and one given up on, before
/// one held to 200 ms leaves the last answered at its deadline and the first still held.
#[test]
fn a_turn_without_a_frame_answers_the_acts_due_and_keeps_the_rest() {
    let (mut host, _agent, _window, lease, _presses) = shown();
    host.set_window_visible(false);
    let _ = host.pump(Duration::from_millis(16));
    let mut long = gated(&lease, 60_000, false);
    let abandoned = gated(&lease, 60_000, false);
    let mut short = gated(&lease, 200, false);
    let _ = host.pump(Duration::ZERO);
    drop(abandoned);
    let asked_at = host.clock().elapsed();
    let Some(answer) = host.run_until_answered(
        &mut short,
        Duration::from_millis(16),
        Duration::from_secs(5),
    ) else {
        panic!("the act held to 200 ms is answered at its deadline");
    };
    let refused = answer.expect_err("its target never passed");
    assert_eq!(refused.code, ErrorCode::NotActionable, "{refused:?}");
    // The host loop sleeps until the owner's next wake: the earliest deadline, not a later one.
    assert!(
        host.clock().elapsed().saturating_sub(asked_at) < Duration::from_secs(1),
        "answered at its deadline, the earliest the window holds"
    );
    let _ = host.pump(Duration::ZERO);
    assert!(long.try_take().is_none(), "the act held to 60 s holds on");
}

/// Keys for "a" pressed in `lease`'s window, held to `timeout_ms`.
fn keys_held_to(lease: &InputLease, timeout_ms: u64) -> AgentAnswer<ActOutcome> {
    let mut keys: PressKeysParams = serde_json::from_value(json!({
        "window": lease.window(),
        "keys": ["a"],
    }))
    .expect("press keys reads");
    keys.options = timeout(keys.options, timeout_ms);
    ask::<method::ElementPressKeys>(lease, keys)
}

/// A window that draws nothing wakes the owner at the earliest deadline of any kind it holds: a
/// tap held to 60 s, keys queued to 60 s, then keys queued to 200 ms, leave the last answered at
/// its own deadline, not at a later one.
#[test]
fn a_window_wakes_at_the_earliest_deadline_of_its_acts_and_their_input() {
    let (mut host, _agent, _window, lease, _presses) = shown();
    host.set_window_visible(false);
    let _ = host.pump(Duration::from_millis(16));
    let _held = gated(&lease, 60_000, false);
    let _queued = keys_held_to(&lease, 60_000);
    let mut short = keys_held_to(&lease, 200);
    let _ = host.pump(Duration::ZERO);
    let asked_at = host.clock().elapsed();
    let Some(answer) = host.run_until_answered(
        &mut short,
        Duration::from_millis(16),
        Duration::from_secs(5),
    ) else {
        panic!("the keys queued to 200 ms are answered at their deadline");
    };
    let refused = answer.expect_err("no frame plays them");
    assert_eq!(refused.code, ErrorCode::Timeout, "{refused:?}");
    assert_eq!(
        host.clock().elapsed().saturating_sub(asked_at),
        Duration::from_millis(200),
        "the owner woke at the earliest deadline"
    );
}

/// `scroll_until` answered at a frame boundary past its deadline plays none of the step it queued
/// before: the step a search queued when it was served is withdrawn at the boundary that answers
/// the search, before the boundary's input would start it.
#[test]
fn scroll_until_answered_at_a_boundary_past_its_deadline_plays_its_queued_step_not() {
    let mut rig = crate::agent_rig::Rig::showing(&Board {
        presses: Presses::default(),
    });
    let journaled = rig.journal().entries.len();
    let mut until = ScrollUntilParams::new(named("Items"), Locator::named("Item 99"));
    until.options = timeout(until.options, 50);
    let mut until = rig
        .lease
        .call::<method::ElementScrollUntil>(until)
        .expect("the inbox has room");
    rig.runtime.pump_background();
    assert!(until.try_take().is_none(), "its first step is queued");
    rig.clock.advance(Duration::from_millis(100));
    rig.frame();
    let ended = until
        .try_take()
        .expect("answered at the boundary past its deadline")
        .expect_err("Item 99 never showed");
    assert_eq!(ended.code, ErrorCode::NotFound, "{ended:?}");
    let entries = rig.journal().entries.split_off(journaled);
    assert_eq!(
        count_of(&entries, InputKind::Scroll),
        0,
        "nothing of the search went out: {entries:?}"
    );
}

/// `wait.frames` holds out for its own timeout: a wait for 1000 frames asked to give up after
/// 200 ms is answered `unsatisfied` once 200 ms of frames have passed.
#[test]
fn a_frames_wait_gives_up_at_its_own_timeout() {
    let (mut host, _agent, window, _lease, _presses) = shown();
    let frames: flui_protocol::wait::FramesParams = serde_json::from_value(json!({
        "window": window.id(),
        "frames": 1_000,
        "timeout_ms": 200,
    }))
    .expect("frames params read");
    let mut waited = window
        .call::<method::WaitFrames>(frames)
        .expect("the inbox has room");
    let Some(answer) = host.run_until_answered(
        &mut waited,
        Duration::from_millis(16),
        Duration::from_secs(2),
    ) else {
        panic!("answered at its 200 ms timeout");
    };
    let outcome = answer.expect("a timeout is an answer");
    assert_eq!(
        outcome.end,
        flui_protocol::wait::WaitEnd::Unsatisfied,
        "{outcome:?}"
    );
}

/// `audit.a11y` cannot judge target sizes against a minimum that is no size: a window whose mouse
/// minimum target is 0 lists `target_size` as not checked, naming the pointer.
#[test]
fn target_size_with_no_minimum_is_not_checked() {
    use flui_interaction::gesture::{GestureProfiles, GestureSettings};
    use flui_platform::TargetPlatform;
    use flui_platform::input::pointer::PointerKind;
    let mut values = GestureSettings::preset(TargetPlatform::Linux, PointerKind::Mouse).values();
    values.min_target = 0.0;
    let mouse = GestureSettings::try_from_values(values).expect("a minimum of 0 is admitted");
    let windows = Arc::new(Mutex::new(Vec::new()));
    let mut agent = HeadlessDevAgent::attach(Windows(Arc::clone(&windows)));
    let mut host = HeadlessHost::new(
        HeadlessWindow::new(200, 400)
            .with_platform(TargetPlatform::Linux)
            .with_gesture_preset(
                GestureProfiles::preset(TargetPlatform::Linux).with_kind(PointerKind::Mouse, mouse),
            ),
    )
    .with_dev_agent(&mut agent);
    host.attach(&Board {
        presses: Presses::default(),
    })
    .expect("mount");
    let _ = host.pump(Duration::from_millis(16));
    let window = windows
        .lock()
        .pop()
        .expect("the host handed the agent its window");
    let _ = host.pump(Duration::from_millis(16));
    let mut params = flui_protocol::audit::AuditParams::new(window.id());
    params.rules = vec![flui_protocol::audit::AuditRule::TargetSize];
    let mut audited = window
        .call::<method::AuditA11y>(params)
        .expect("the inbox has room");
    let report = answered(&mut host, &mut audited, 2).0.expect("audited");
    let [entry] = report.not_checked.as_slice() else {
        panic!("target_size is not checked: {report:#?}");
    };
    assert_eq!(
        entry.reason,
        "the minimum target of a mouse, this window's primary pointer, is not a finite positive size"
    );
}

/// A wheel turned by lines scrolls by lines, not by pixels: one line down moves a list's rows
/// more than a pixel.
#[test]
fn an_injected_scroll_by_lines_scrolls_by_lines() {
    use flui_protocol::input::{ActionSequence, InputParams, InputSource, ScrollUnit, WheelAction};
    let (mut host, _agent, window, lease, _presses) = shown();
    let before = bounds(&mut host, &window, "Item 1");
    let at = bounds(&mut host, &window, "Items").center();
    let scroll = ActionSequence::new(vec![InputSource::Wheel {
        id: "wheel".to_owned(),
        actions: vec![WheelAction::Scroll {
            at,
            delta_x: 0.0,
            delta_y: 1.0,
            unit: ScrollUnit::Line,
        }],
    }]);
    let mut scrolled = lease
        .call::<method::InputInject>(InputParams::new(window.id(), scroll))
        .expect("the inbox has room");
    answered(&mut host, &mut scrolled, 20)
        .0
        .expect("the scroll plays");
    for _ in 0..10 {
        let _ = host.pump(Duration::from_millis(16));
    }
    let after = bounds(&mut host, &window, "Item 1");
    assert!(
        before.y - after.y > 5.0,
        "a line moves more than a pixel: {before:?} to {after:?}"
    );
}

/// At its deadline `scroll_until` withdraws its own step and nothing else queued: input queued
/// before the step, behind a long injection, and a release queued before it still run and answer.
#[test]
fn scroll_until_withdraws_only_its_own_step() {
    use flui_protocol::input::{
        ActionSequence, InputParams, InputSource, PointerAction, PointerDevice,
    };
    let pause = |ms: u64| {
        ActionSequence::new(vec![InputSource::Pointer {
            id: "pen".to_owned(),
            device: PointerDevice::Mouse,
            actions: vec![PointerAction::Pause { duration_ms: ms }],
        }])
    };
    for release_before in [false, true] {
        let (mut host, _agent, window, lease, _presses) = shown();
        let mut front = lease
            .call::<method::InputInject>(InputParams::new(window.id(), pause(600)))
            .expect("the inbox has room");
        let _ = host.pump(Duration::from_millis(16));
        let mut before = None;
        let mut released = None;
        if release_before {
            released = Some(
                lease
                    .call::<method::InputRelease>(flui_protocol::session::WindowParams::new(
                        window.id(),
                    ))
                    .expect("the inbox has room"),
            );
        } else {
            before = Some(
                lease
                    .call::<method::InputInject>(InputParams::new(window.id(), pause(16)))
                    .expect("the inbox has room"),
            );
        }
        let mut until = ScrollUntilParams::new(named("Items"), Locator::named("Item 99"));
        until.options = timeout(until.options, 100);
        let mut until = ask::<method::ElementScrollUntil>(&lease, until);
        let ended = answered(&mut host, &mut until, 30)
            .0
            .expect_err("the deadline passes with the step still queued");
        assert_eq!(ended.code, ErrorCode::NotFound, "{ended:?}");
        answered(&mut host, &mut front, 60)
            .0
            .expect("the long injection plays");
        if let Some(mut released) = released {
            answered(&mut host, &mut released, 10)
                .0
                .expect("the release queued before the step runs");
        } else if let Some(mut before) = before {
            answered(&mut host, &mut before, 10)
                .0
                .expect("the injection queued before the step plays");
        }
    }
}

/// A list of 100 rows 20 px tall in a viewport 100 px tall, named "Items" until "Hide" renames it
/// "Elsewhere", and "Items" again at the next press.
#[derive(Clone, StatefulView)]
struct Hideable;

struct HideableState {
    shown: Signal<bool>,
}

impl StatefulView for Hideable {
    type State = HideableState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        HideableState {
            shown: ctx.signal(true),
        }
    }
}

impl ViewState<Hideable> for HideableState {
    fn build(&self, _view: &Hideable, ctx: &dyn BuildContext) -> impl IntoView {
        let shown = self.shown;
        let label = if shown.get(ctx) { "Items" } else { "Elsewhere" };
        let list = ListView::builder(100, 20.0, |index| {
            Some(
                SizedBox::new(200.0, 20.0)
                    .child(Text::new(format!("Item {index}")))
                    .into_view()
                    .boxed(),
            )
        })
        .semantics_label(label);
        Column::new(vec![
            RawButton::new(Text::new("Hide"))
                .on_pressed(move |ctx| shown.update(ctx, |on| *on = !*on))
                .into_view()
                .boxed(),
            SizedBox::new(200.0, 100.0).child(list).into_view().boxed(),
        ])
    }
}

/// `scroll_until` holds on while its locator names no scrollable, the answer of a step under way
/// taken, and goes on scrolling once it names one again: it finds the element.
#[test]
fn scroll_until_goes_on_once_its_scrollable_is_back() {
    let (mut host, _agent, window) = serving(&Hideable, 400);
    let lease = window
        .input_lease()
        .expect("an open window leases its input");
    let mut until = ScrollUntilParams::new(named("Items"), Locator::named("Item 40"));
    until.options = timeout(until.options, 10_000);
    let mut until = ask::<method::ElementScrollUntil>(&lease, until);
    let journaled = journal(&mut host, &window).entries.len();
    let mut stepped = false;
    for _ in 0..30 {
        let _ = host.pump(Duration::from_millis(16));
        if journal(&mut host, &window).entries.len() >= journaled + 2 {
            stepped = true;
            break;
        }
    }
    assert!(stepped, "two steps went out");
    press(&mut host, &window, "Hide");
    for _ in 0..8 {
        let _ = host.pump(Duration::from_millis(16));
    }
    assert!(
        until.try_take().is_none(),
        "held while the scrollable is gone"
    );
    press(&mut host, &window, "Hide");
    let found = answered(&mut host, &mut until, 300)
        .0
        .expect("found once the scrollable is back");
    assert_eq!(found.element, Some(element(&mut host, &window, "Item 40")));
}

/// A search naming no direction that moved down to the end of a vertical list ends there,
/// `not_found`: it turns rightwards only when its first step down moved nothing. Each step is one
/// wheel input in the window's journal: 100 rows of 20 px in a 100 px viewport take 19 steps that
/// move and one that does not.
#[test]
fn scroll_until_turns_only_when_its_first_step_moves_nothing() {
    let (mut host, _agent, window, lease, _presses) = shown();
    let journaled = journal(&mut host, &window).entries.len();
    let mut until = ScrollUntilParams::new(named("Items"), Locator::named("Item 100"));
    until.options = timeout(until.options, 60_000);
    let mut until = ask::<method::ElementScrollUntil>(&lease, until);
    let ended = answered(&mut host, &mut until, 400)
        .0
        .expect_err("the list ends first");
    assert_eq!(ended.code, ErrorCode::NotFound, "{ended:?}");
    let scrolls = journal(&mut host, &window).entries[journaled..]
        .iter()
        .filter(|entry| entry.kind == InputKind::Scroll)
        .count();
    assert_eq!(scrolls, 20, "steps");
}

/// The element `scroll_until` found is reported as the agent's: once it scrolls away, a tap on
/// its handle answers `gone`, not `unknown_handle`.
#[test]
fn the_element_scroll_until_found_is_the_agents() {
    let (mut host, _agent, _window, lease, _presses) = shown();
    let mut down = ask::<method::ElementScrollUntil>(
        &lease,
        ScrollUntilParams::new(named("Items"), Locator::named("Item 60")),
    );
    let found = answered(&mut host, &mut down, 200)
        .0
        .expect("found")
        .element
        .expect("the element found");
    let mut up = ScrollUntilParams::new(named("Items"), Locator::named("Item 0"));
    up.direction = Some(Direction::Up);
    let mut up = ask::<method::ElementScrollUntil>(&lease, up);
    answered(&mut host, &mut up, 200)
        .0
        .expect("back at the top");
    for _ in 0..4 {
        let _ = host.pump(Duration::from_millis(16));
    }
    let mut tap = TapParams::new(Target::Element(found));
    tap.options = timeout(tap.options, 200);
    let mut tap = ask::<method::ElementTap>(&lease, tap);
    let gone = answered(&mut host, &mut tap, 30)
        .0
        .expect_err("it scrolled away");
    assert_eq!(gone.code, ErrorCode::Gone, "{gone:?}");
}

/// A gesture on a handle this agent was never given answers `unknown_handle`.
#[test]
fn a_gesture_on_a_handle_never_given_is_unknown() {
    let (mut host, _agent, window, lease, _presses) = shown();
    let made_up = ElementRef::new(
        window.id(),
        flui_protocol::tree::ElementId::from_u64(987_654_321).expect("an element id"),
    );
    let mut tap = TapParams::new(Target::Element(made_up));
    tap.options = timeout(tap.options, 200);
    let mut tap = ask::<method::ElementTap>(&lease, tap);
    let refused = answered(&mut host, &mut tap, 30)
        .0
        .expect_err("no such element");
    assert_eq!(refused.code, ErrorCode::UnknownHandle, "{refused:?}");
}

/// `scroll_until` reads up to 4096 of the scrollable's descendants and 4096 of the window's
/// matches, and refuses only a read that leaves some out: exactly at each bound it searches (4093
/// rows under the scrollable make its 4096 descendants with the nodes that hold them).
#[test]
fn scroll_until_reads_exactly_up_to_its_bounds() {
    type Expected = Result<(), ErrorCode>;
    let rows: [(&str, Crowd, &str, Expected); 4] = [
        (
            "4096 matches",
            Crowd {
                twins: 4096,
                rows: 3,
            },
            "Twin",
            Err(ErrorCode::NotFound),
        ),
        (
            "4097 matches",
            Crowd {
                twins: 4097,
                rows: 3,
            },
            "Twin",
            Err(ErrorCode::InvalidArgument),
        ),
        (
            "4093 rows",
            Crowd {
                twins: 0,
                rows: 4093,
            },
            "Row 0",
            Ok(()),
        ),
        (
            "4094 rows",
            Crowd {
                twins: 0,
                rows: 4094,
            },
            "Row 0",
            Err(ErrorCode::NotSupported),
        ),
    ];
    for (case, crowd, sought, expected) in rows {
        let (mut host, _agent, window) = serving(&crowd, 400);
        let lease = window
            .input_lease()
            .expect("an open window leases its input");
        let mut asked = ask::<method::ElementScrollUntil>(
            &lease,
            ScrollUntilParams::new(named("Tall"), Locator::named(sought)),
        );
        let answer = answered(&mut host, &mut asked, 400).0;
        match (answer, expected) {
            (Ok(_), Ok(())) => {}
            (Err(refused), Err(code)) => assert_eq!(refused.code, code, "{case}: {refused:?}"),
            (answer, expected) => panic!("{case}: {answer:?}, not {expected:?}"),
        }
    }
}

/// "Gated", a button with no handler, so disabled; "Busy", whose press marks the view for a
/// rebuild; and "Drop", whose press lets go of the lease it is handed.
#[derive(Clone, StatefulView)]
struct Holder {
    lease: std::rc::Rc<std::cell::RefCell<Option<InputLease>>>,
}

struct HolderState {
    presses: Signal<u32>,
}

impl StatefulView for Holder {
    type State = HolderState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        HolderState {
            presses: ctx.signal(0),
        }
    }
}

impl ViewState<Holder> for HolderState {
    fn build(&self, view: &Holder, ctx: &dyn BuildContext) -> impl IntoView {
        let presses = self.presses;
        let lease = std::rc::Rc::clone(&view.lease);
        Column::new(vec![
            RawButton::new(Text::new("Gated")).into_view().boxed(),
            RawButton::new(Text::new(format!("Busy {}", presses.get(ctx))))
                .on_pressed(move |ctx| presses.update(ctx, |count| *count += 1))
                .into_view()
                .boxed(),
            RawButton::new(Text::new("Drop"))
                .on_pressed(move |_| drop(lease.borrow_mut().take()))
                .into_view()
                .boxed(),
        ])
    }
}

/// [`Holder`] shown by a [`crate::agent_rig::Rig`], and the cell its "Drop" empties.
fn holder() -> (
    crate::agent_rig::Rig,
    std::rc::Rc<std::cell::RefCell<Option<InputLease>>>,
) {
    let cell = std::rc::Rc::new(std::cell::RefCell::new(None));
    let rig = crate::agent_rig::Rig::showing(&Holder {
        lease: std::rc::Rc::clone(&cell),
    });
    (rig, cell)
}

/// A gated tap held to `timeout_ms` through `lease`.
fn gated_through(lease: &InputLease, timeout_ms: u64) -> AgentAnswer<ActOutcome> {
    let mut tap = TapParams::new(named("Gated"));
    tap.options = timeout(tap.options, timeout_ms);
    lease
        .call::<method::ElementTap>(tap)
        .expect("the inbox has room")
}

/// A lease let go while a drain runs (by a handler that drain reaches) is released by the next
/// drain, behind the calls it already took: the act held through it holds on until then.
#[test]
fn a_lease_let_go_during_a_drain_is_released_by_the_next() {
    let (mut rig, cell) = holder();
    let lease = rig.window.input_lease().expect("leased");
    // A lease let go before the drain, which the drain counts, and one let go during it.
    let counted = rig.window.input_lease().expect("leased");
    let mut held = gated_through(&lease, 60_000);
    *cell.borrow_mut() = Some(lease);
    rig.runtime.pump_background();
    assert!(held.try_take().is_none(), "held while disabled");
    drop(counted);
    let mut dropped = rig
        .window
        .call::<method::SemanticsAct>(ActParams::new(named("Drop"), ActionName::Invoke))
        .expect("the inbox has room");
    rig.runtime.pump_background();
    dropped
        .try_take()
        .expect("pressed at the drain")
        .expect("pressed");
    assert!(cell.borrow().is_none(), "the handler let go of the lease");
    assert!(
        held.try_take().is_none(),
        "the lease the drain saw let go after it began waits for the next drain"
    );
    rig.runtime.pump_background();
    let gone = held
        .try_take()
        .expect("released by the next drain")
        .expect_err("its lease went");
    assert_eq!(gone.code, ErrorCode::Gone, "{gone:?}");
}

/// A window keeps the readings its actions owe for at most 256 of them: the 257th lets the
/// oldest go, answered at once that its tree could not be read back, before any frame.
#[test]
fn the_oldest_owed_reading_is_answered_when_a_newer_one_displaces_it() {
    let (mut rig, _cell) = holder();
    let act = || ActParams::new(named("Busy 0"), ActionName::Invoke).snapshot(Snapshot::Delta);
    let mut first = rig
        .window
        .call::<method::SemanticsAct>(act())
        .expect("the inbox has room");
    rig.runtime.pump_background();
    let mut rest: Vec<AgentAnswer<ActOutcome>> = Vec::new();
    for batch in 0..8 {
        for _ in 0..32 {
            rest.push(
                rig.window
                    .call::<method::SemanticsAct>(act())
                    .expect("the inbox has room"),
            );
        }
        rig.runtime.pump_background();
        assert!(
            batch == 7 || first.try_take().is_none(),
            "batch {batch}: the first reading is still owed"
        );
    }
    let outcome = first
        .try_take()
        .expect("the 257th reading displaced the first")
        .expect("the action ran");
    assert!(outcome.readback_failed, "{outcome:?}");
    assert!(
        rest.iter_mut().all(|reading| reading.try_take().is_none()),
        "the 256 newer readings are still owed"
    );
}

/// The acts of every window are attempted and their deadlines wake the owner, whichever window
/// holds them: with nothing held in the first window, an act in a second window is answered at
/// its deadline on a turn without a frame, and the earliest deadline across windows is the one
/// the owner wakes at.
#[test]
fn every_windows_acts_are_attempted_and_wake_the_owner() {
    let (mut rig, _cell) = holder();
    let second = rig.runtime.install_presentation(
        rig.runtime
            .assemble_presentation(crate::owner_publication::window()),
    );
    rig.runtime
        .attach_root_widget_to(second, &RawButton::new(Text::new("Gated")))
        .expect("mount the second window");
    let other = rig
        .runtime
        .dev_agent_window(second)
        .expect("the runtime hosts its second presentation");
    let other_lease = other.input_lease().expect("leased");
    for _ in 0..4 {
        rig.frame();
    }
    // Nothing held in the first window: the second window's act is answered at its deadline.
    let mut short = gated_through(&other_lease, 100);
    rig.runtime.pump_background();
    rig.clock.advance(Duration::from_millis(150));
    rig.runtime.pump_background();
    let refused = short
        .try_take()
        .expect("answered at its deadline")
        .expect_err("disabled to the end");
    assert_eq!(refused.code, ErrorCode::NotActionable, "{refused:?}");
    // An act in each window: the owner wakes by the earlier deadline, the second window's.
    let served_at = flui_foundation::MonotonicClock::now(&rig.clock);
    let _long = gated_through(&rig.lease, 60_000);
    let _short = gated_through(&other_lease, 100);
    rig.runtime.pump_background();
    let wake = rig
        .runtime
        .next_wake()
        .expect("the deadlines wake the owner");
    assert!(
        wake <= served_at + Duration::from_millis(100),
        "the owner wakes by the earlier deadline: {:?} past it",
        wake.saturating_duration_since(served_at)
    );
}

/// "Flip", which takes "Leaving" (a button with no handler, so disabled) away.
#[derive(Clone, StatefulView)]
struct Leaving;

struct LeavingState {
    shown: Signal<bool>,
}

impl StatefulView for Leaving {
    type State = LeavingState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        LeavingState {
            shown: ctx.signal(true),
        }
    }
}

impl ViewState<Leaving> for LeavingState {
    fn build(&self, _view: &Leaving, ctx: &dyn BuildContext) -> impl IntoView {
        let shown = self.shown;
        let mut children = vec![
            RawButton::new(Text::new("Flip"))
                .on_pressed(move |ctx| shown.update(ctx, |on| *on = false))
                .into_view()
                .boxed(),
        ];
        if shown.get(ctx) {
            children.push(RawButton::new(Text::new("Leaving")).into_view().boxed());
        }
        Column::new(children)
    }
}

/// A window stops asking for frames for its acts once the last is answered: a held tap whose
/// element leaves the tree is answered `gone` at the frame boundary that sees it, and asks for no
/// frame after it.
#[test]
fn a_window_asks_for_no_frame_once_its_last_act_is_answered() {
    let (mut host, _agent, window) = serving(&Leaving, 400);
    let lease = window
        .input_lease()
        .expect("an open window leases its input");
    let leaving = element(&mut host, &window, "Leaving");
    let mut tap = TapParams::new(Target::Element(leaving));
    tap.options = timeout(tap.options, 60_000);
    let mut tap = ask::<method::ElementTap>(&lease, tap);
    for _ in 0..3 {
        let _ = host.pump(Duration::from_millis(16));
    }
    assert!(tap.try_take().is_none(), "held while disabled");
    let _flipped = window
        .call::<method::SemanticsAct>(ActParams::new(named("Flip"), ActionName::Invoke))
        .expect("the inbox has room");
    // Read right after the frame that answered it, before another frame could clear a request.
    let gone = answered(&mut host, &mut tap, 10)
        .0
        .expect_err("its element left");
    assert_eq!(gone.code, ErrorCode::Gone, "{gone:?}");
    // The frames an act asks for are a wake of the loop, never a cause of the application's.
    assert!(
        !asks_for_a_frame(&mut host),
        "no act is held, so the loop is not woken for a frame"
    );
}

/// `scroll_until` that finds its element at once, with no reading asked for, answers in the turn
/// that serves it, with no frame.
#[test]
fn scroll_until_finding_its_element_at_once_answers_with_no_frame() {
    let mut rig = crate::agent_rig::Rig::showing(&Board {
        presses: Presses::default(),
    });
    let mut until = rig
        .lease
        .call::<method::ElementScrollUntil>(ScrollUntilParams::new(
            named("Items"),
            Locator::named("Item 1"),
        ))
        .expect("the inbox has room");
    rig.runtime.pump_background();
    let found = until
        .try_take()
        .expect("answered in the turn that served it")
        .expect("Item 1 shows");
    assert!(found.element.is_some(), "{found:?}");
}

/// "Busy", whose press marks the view for a rebuild, and "Row 0" to "Row 199".
#[derive(Clone, StatefulView)]
struct Many;

struct ManyState {
    presses: Signal<u32>,
}

impl StatefulView for Many {
    type State = ManyState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        ManyState {
            presses: ctx.signal(0),
        }
    }
}

impl ViewState<Many> for ManyState {
    fn build(&self, _view: &Many, ctx: &dyn BuildContext) -> impl IntoView {
        let presses = self.presses;
        let mut children = vec![
            RawButton::new(Text::new(format!("Busy {}", presses.get(ctx))))
                .on_pressed(move |ctx| presses.update(ctx, |count| *count += 1))
                .into_view()
                .boxed(),
        ];
        children.extend((0..200).map(|row| Text::new(format!("Row {row}")).into_view().boxed()));
        Column::new(children)
    }
}

/// The stability check forgets the element read longest ago, and a read again within the frame
/// counts as a read: an element read, then 191 others, then read again before one more, is still
/// remembered after the frame and found stable.
#[test]
fn the_stability_check_forgets_the_element_read_longest_ago() {
    let mut rig = crate::agent_rig::Rig::showing(&Many);
    let mut located =
        LocateParams::new(Locator::default().name_matching(TextMatch::prefix("Row ")));
    located.max_matches = Some(300);
    let mut located = rig
        .window
        .call::<method::SemanticsLocate>(located)
        .expect("the inbox has room");
    rig.runtime.pump_background();
    let rows: Vec<ElementRef> = located
        .try_take()
        .expect("located at the drain")
        .expect("located")
        .matches
        .iter()
        .map(|found| found.element)
        .collect::<Vec<ElementRef>>();
    assert_eq!(rows.len(), 200, "every row is published");
    let read = |rig: &mut crate::agent_rig::Rig, batch: &[ElementRef]| -> Vec<bool> {
        let mut asked: Vec<AgentAnswer<Actionability>> = batch
            .iter()
            .map(|row| {
                rig.window
                    .call::<method::ElementActionability>(ActionabilityParams::new(*row))
                    .expect("the inbox has room")
            })
            .collect::<Vec<AgentAnswer<Actionability>>>();
        rig.runtime.pump_background();
        asked
            .iter_mut()
            .map(|answer| {
                answer
                    .try_take()
                    .expect("answered at the drain")
                    .expect("its facts")
                    .stable
            })
            .collect::<Vec<bool>>()
    };
    // Work stays pending through the reads below, so none of them compares within the frame.
    let mut busy = rig
        .window
        .call::<method::SemanticsAct>(ActParams::new(named("Busy 0"), ActionName::Invoke))
        .expect("the inbox has room");
    let _ = read(&mut rig, &rows[..1]);
    busy.try_take()
        .expect("pressed at the drain")
        .expect("pressed");
    for batch in rows[1..192].chunks(32) {
        let _ = read(&mut rig, batch);
    }
    let _ = read(&mut rig, &[rows[0], rows[192]]);
    rig.frame();
    assert_eq!(
        read(&mut rig, &rows[..1]),
        [true],
        "the element read again is remembered across the frame"
    );
}
