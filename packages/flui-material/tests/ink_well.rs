//! `InkWell` widget-level state-transition coverage.
//!
//! The harness routes input through the same presentation-local
//! `GestureBinding` and `MouseTracker` as production. These tests therefore
//! cover structural enter/exit, tap, disabled behavior, focus, press timing,
//! and overlay resolution without a second headless-only input protocol.

use crate::common;

use std::rc::Rc;
use std::time::Duration;

use common::{LaidOut, lay_out, tight};
use flui_interaction::KeyEventResult;
use flui_interaction::events::{Code, Key, KeyEvent, NamedKey};
use flui_interaction::testing::input::KeyEventBuilder;
use flui_material::{Checkbox, InkWell, Theme, ThemeData};
use flui_painting::display_list::DrawOp;
use flui_painting::styling::Color;
use flui_sdk::geometry::Offset;
use flui_sdk::interaction::FocusNode;
use flui_sdk::view::SignalWriteExt;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{
    FocusRingStyle, SizedBox, WidgetState, WidgetStates, WidgetStatesController,
};
use flui_testing::replay::MouseScript;

pub fn press_highlight_lasts_its_delay_under_reduced_motion() {
    use flui_sdk::animation::MotionOverride;
    use std::time::Duration;
    let states = WidgetStatesController::default();
    let mut laid = lay_out(
        InkWell::new()
            .child(SizedBox::new(80.0, 40.0))
            .states_controller(states.clone())
            .on_tap(|_| {}),
        tight(80.0, 40.0),
    );
    laid.set_motion_override(MotionOverride::Reduce);
    laid.pump_for(Duration::from_millis(16));
    laid.dispatch_pointer_down(40.0, 20.0);
    laid.dispatch_pointer_up(40.0, 20.0);
    assert!(states.value().contains_state(WidgetState::Pressed));
    laid.pump_for(Duration::from_millis(16));
    laid.pump_for(Duration::from_millis(50));
    assert!(
        states.value().contains_state(WidgetState::Pressed),
        "Reduce must not consume the authored press delay"
    );
    laid.pump_for(Duration::from_millis(64));
    assert!(
        !states.value().contains_state(WidgetState::Pressed),
        "the delay still expires under Reduce"
    );
}

pub fn pointer_and_keyboard_activation_write_the_owning_signal() {
    let node = FocusNode::with_debug_label("writer-activation");
    let child_node = Rc::clone(&node);
    let probe = common::SignalProbe::new(move |signals| {
        InkWell::new()
            .child(SizedBox::new(80.0, 40.0))
            .focus_node(Rc::clone(&child_node))
            .on_tap(move |ctx| signals.count.update(ctx, |count| *count += 1))
    });
    let mut laid = lay_out(probe.view(), tight(80.0, 40.0));
    laid.dispatch_pointer_down(40.0, 20.0);
    laid.dispatch_pointer_up(40.0, 20.0);
    assert_eq!(probe.value(), Ok(1));
    let _ = node.request_focus();
    assert_eq!(
        laid.focus_manager().dispatch_key_event(&enter()),
        KeyEventResult::Handled
    );
    assert_eq!(probe.value(), Ok(2));
    laid.pump();
    assert_eq!(probe.reads(), [0, 2]);
}

pub fn disabled_ink_well_does_not_fire_a_tap_callback() {
    // Mutation-honest companion to the enabled case above: this test only
    // proves something if a *would-be* tap on a disabled InkWell is
    // observably inert. It mounts with no `on_tap` at all (there is nothing
    // to fire), which is the only way to construct a disabled `InkWell` —
    // `is_interactive()` is derived from `on_tap.is_some()`, not a separate
    // flag (see `ink_well.rs`'s module doc).
    let states = WidgetStatesController::default();
    let laid = lay_out(
        InkWell::new()
            .child(SizedBox::new(60.0, 40.0))
            .states_controller(states.clone()),
        tight(60.0, 40.0),
    );

    laid.dispatch_pointer_down(30.0, 20.0);
    laid.dispatch_pointer_up(30.0, 20.0);

    assert!(
        !states.value().contains_state(WidgetState::Pressed),
        "a disabled InkWell mounts no tap, so no tap can ever be recognized",
    );
}

// A `build()`-vs-`init_state`/`did_update_view` regression test for the
// Disabled-sync relocation was attempted and DELIBERATELY dropped, not
// silently skipped: two different observability angles were tried —
// (1) a `StatelessView` child counting its own `build()` invocations, and
// (2) `BuildOwner::pending_external_builds()` (the out-of-frame rebuild
// inbox `RebuildHandle::schedule()` enqueues into) read immediately after
// `lay_out` returns. Both were run against the reintroduced original bug
// (`self.states.update(WidgetState::Disabled, !enabled)` back at the top of
// `build()`) via `cargo test`, and BOTH passed unchanged — `lay_out`'s
// single bootstrap frame is a fixpoint (ADR-0017) that drains any rebuild
// scheduled during its own pass before returning, regardless of which
// lifecycle hook triggered it, so neither approach can observe "extra
// rebuild" as a distinguishable side effect through this harness. The
// architectural fix (never mutate a possibly-caller-shared controller from
// `build`, i.e. no rebuild scheduled during build —
// `ink_well.rs`'s `init_state`/`did_update_view`) is applied and is correct
// independent of whether this harness can prove
// the "spurious rebuild" symptom specifically; the existing `Disabled`-state
// assertions (`disabled_ink_well_does_not_fire_a_tap_callback`) already prove the sync
// itself still happens correctly at the new call sites.

fn enter() -> KeyEvent {
    KeyEventBuilder::new(Code::Enter)
        .key(Key::Named(NamedKey::Enter))
        .build()
}

fn tab() -> KeyEvent {
    KeyEventBuilder::new(Code::Tab)
        .key(Key::Named(NamedKey::Tab))
        .build()
}

/// Enter on a focused `ElevatedButton` reaches its `InkWell`'s activation
/// action, which runs inside the key event's own dispatch and hands its `ctx`
/// to `on_pressed` — no separate writer bridge. The write lands at dispatch
/// and the signal's reader rebuilds on the next frame (a tick, which dirties
/// nothing itself).
pub fn enter_on_a_focused_elevated_button_writes_a_signal_and_rebuilds_its_reader() {
    use flui_material::{ElevatedButton, Theme, ThemeData};
    use flui_sdk::widgets::Text;

    let probe = common::SignalProbe::new(|signals| {
        Theme::new(
            ThemeData::light(),
            ElevatedButton::new(Text::new("Save"))
                .on_pressed(move |ctx| signals.count.update(ctx, |count| *count += 1)),
        )
    });
    let mut laid = lay_out(probe.view(), tight(120.0, 48.0));

    assert_eq!(
        laid.focus_manager().dispatch_key_event(&tab()),
        KeyEventResult::Handled,
        "the first Tab focuses the button"
    );
    assert_eq!(
        laid.focus_manager().dispatch_key_event(&enter()),
        KeyEventResult::Handled,
        "Enter is consumed"
    );
    assert_eq!(probe.value(), Ok(1), "the activation wrote at dispatch");
    laid.tick();
    assert_eq!(probe.reads(), [0, 1], "and the reader rebuilt once");
}

/// A ring style no Material colour role produces, so its bands are found by
/// colour alone.
pub const RING: FocusRingStyle = FocusRingStyle {
    outer: Color::rgb(200, 0, 0),
    inner: Color::rgb(0, 200, 0),
    outer_width: 2.0,
    inner_width: 1.0,
};

/// `child` under the light theme with [`RING`] as its focus ring.
pub fn themed(child: impl IntoView) -> Theme {
    let mut data = ThemeData::light();
    data.focus_ring = RING;
    Theme::new(data, child)
}

/// Whether the last frame painted both bands of [`RING`].
pub fn ring_painted(laid: &LaidOut) -> bool {
    let colors: Vec<Color> = laid
        .draw_ops()
        .into_iter()
        .filter_map(|command| match command.op {
            DrawOp::Rect { paint, .. }
            | DrawOp::RRect { paint, .. }
            | DrawOp::DRRect { paint, .. }
            | DrawOp::Path { paint, .. }
            | DrawOp::Line { paint, .. } => Some(paint.color),
            _ => None,
        })
        .collect();
    let outer = colors.contains(&RING.outer);
    let inner = colors.contains(&RING.inner);
    assert_eq!(outer, inner, "the ring paints both of its bands or neither");
    outer
}

/// A mouse click at `(x, y)`, through the UI runtime's input path, so the
/// presentation sees pointer input.
fn click(laid: &mut LaidOut, x: f64, y: f64) {
    let click = MouseScript::new("click")
        .press(Offset::new(x, y))
        .wait(Duration::from_millis(16))
        .release()
        .build()
        .expect("the click releases its button");
    laid.replay(&click);
    laid.pump();
}

/// `Focused` is the focus the well holds and `FocusVisible` the focus a user
/// can see: focus taken after a click is held and not shown, neither as the
/// state nor as a ring, and the next key shows both, in the theme's ring style.
pub fn a_well_shows_focus_only_when_the_keyboard_is_in_use() {
    let node = FocusNode::with_debug_label("visible-focus");
    let states = WidgetStatesController::default();
    let mut laid = lay_out(
        themed(
            InkWell::new()
                .child(SizedBox::new(80.0, 40.0))
                .focus_node(Rc::clone(&node))
                .states_controller(states.clone())
                .on_tap(|_| {}),
        ),
        tight(80.0, 40.0),
    );
    click(&mut laid, 40.0, 20.0);
    let _ = laid.enter_owner_scope(|| node.request_focus());
    laid.pump_for(Duration::from_millis(200));
    assert!(node.has_primary_focus(), "the well took the focus");
    assert!(
        states.value().contains_state(WidgetState::Focused),
        "the well holds the focus, whatever gave it"
    );
    assert!(
        !states.value().contains_state(WidgetState::FocusVisible),
        "focus after a click is not shown"
    );
    assert!(!ring_painted(&laid), "and draws no ring");
    let _ = laid.enter_owner_scope(|| laid.focus_manager().dispatch_key_event(&tab()));
    laid.pump_for(Duration::from_millis(200));
    assert!(
        states.value().contains_state(WidgetState::FocusVisible),
        "a key shows the focus"
    );
    assert!(ring_painted(&laid), "as the theme's ring");
}

/// A checkbox shows its focus with the M3 halo, so it draws no ring.
pub fn a_checkbox_shows_focus_with_its_halo_not_a_ring() {
    let mut laid = lay_out(
        themed(Checkbox::new(false).on_changed(|_ctx, _value| {})),
        tight(48.0, 48.0),
    );
    let manager = laid.focus_manager();
    let _ = laid.enter_owner_scope(|| manager.dispatch_key_event(&tab()));
    laid.pump_for(Duration::from_millis(200));
    assert!(
        manager.primary_focus().is_some(),
        "Tab focused the checkbox"
    );
    assert!(!ring_painted(&laid), "the halo is its indicator");
}

/// How a test's well is made.
#[derive(Clone, Copy, Debug)]
enum Made {
    /// [`InkWell`].
    Well,
    /// The public wrappers a well was composed from before it mounted one
    /// node for its behaviours (ADR-0322): what it must stay
    /// indistinguishable from to assistive technology and to the gesture
    /// arena.
    Wrappers,
}

type OnTap = Rc<dyn Fn(&mut flui_sdk::view::EventContext<'_>)>;

/// The pre-node `InkWell` without its ink: a `GestureDetector` with the tap
/// family over a contact `Listener`, a `MouseRegion`, `Actions`, a `Focus`
/// that takes focus while enabled, and a `FocusRing`.
#[derive(Clone, StatelessView)]
struct WrapperWell {
    on_tap: Option<OnTap>,
    child: BoxedView,
}

impl StatelessView for WrapperWell {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        use flui_sdk::widgets::{
            Actions, ButtonActivateIntent, CallbackAction, Focus, FocusRing, GestureDetector,
            Listener, MouseRegion,
        };
        let enabled = self.on_tap.is_some();
        let mut actions =
            Actions::new(Focus::new(FocusRing::new(self.child.clone())).can_request_focus(enabled));
        let mut detector = GestureDetector::new();
        if let Some(on_tap) = &self.on_tap {
            let (activate, button_activate, tap) =
                (Rc::clone(on_tap), Rc::clone(on_tap), Rc::clone(on_tap));
            actions = actions
                .action(CallbackAction::new(
                    move |ctx, _: &flui_sdk::widgets::ActivateIntent| activate(ctx),
                ))
                .action(CallbackAction::new(move |ctx, _: &ButtonActivateIntent| {
                    button_activate(ctx);
                }));
            detector = detector
                .on_tap_down(|_ctx, _details| {})
                .on_tap_up(|_ctx, _details| {})
                .on_tap(move |ctx| tap(ctx))
                .on_tap_cancel(|_ctx, _details| {});
        }
        let surface = MouseRegion::new().child(actions);
        if enabled {
            detector
                .child(
                    Listener::new()
                        .on_pointer_up(|_ctx, _dispatch| {})
                        .child(surface),
                )
                .boxed()
        } else {
            detector.child(surface).boxed()
        }
    }
}

fn well(made: Made, on_tap: Option<OnTap>, child: impl IntoView) -> BoxedView {
    match made {
        Made::Well => {
            let mut well = InkWell::new().child(child);
            if let Some(on_tap) = on_tap {
                well = well.on_tap(move |ctx| on_tap(ctx));
            }
            well.boxed()
        }
        Made::Wrappers => WrapperWell {
            on_tap,
            child: child.into_view().boxed(),
        }
        .boxed(),
    }
}

type TapLog = Rc<std::cell::RefCell<Vec<&'static str>>>;

fn logging(log: &TapLog, entry: &'static str) -> OnTap {
    let log = Rc::clone(log);
    Rc::new(move |_ctx| log.borrow_mut().push(entry))
}

/// A well's accessibility tree is the wrappers' tree, enabled and disabled,
/// focused and not.
///
/// Red-check: drop the node's focus annotation (the focused well loses
/// `focusable`/`focused`) or its click (the enabled well offers none).
pub fn ink_well_a11y_tree_matches_wrappers() {
    use flui_sdk::widgets::{Column, Text};
    use flui_testing::Action;

    for enabled in [true, false] {
        for focused in [false, true] {
            let tree = |made: Made| {
                let log = TapLog::default();
                let on_tap = enabled.then(|| logging(&log, "tap"));
                let mut laid = lay_out(
                    Column::new((Text::new("Above"), well(made, on_tap, Text::new("Press")))),
                    common::loose(400.0),
                );
                if focused {
                    let manager = laid.focus_manager();
                    let _ = laid.enter_owner_scope(|| manager.dispatch_key_event(&tab()));
                    laid.pump();
                    assert_eq!(
                        manager.primary_focus().is_some(),
                        enabled,
                        "{made:?}: Tab reaches an enabled well only"
                    );
                }
                laid.enable_semantics();
                laid.pump();
                let tree = laid
                    .a11y_tree()
                    .expect("semantics enabled before the frame");
                let clicks = tree
                    .find_by_label("Press")
                    .expect("the well's label is one node")
                    .supports_action(Action::Click);
                assert_eq!(
                    clicks, enabled,
                    "{made:?}: only an enabled well offers a click"
                );
                canonical_ids(&format!("{:#?}", tree.raw()))
            };
            assert_eq!(
                tree(Made::Well),
                tree(Made::Wrappers),
                "enabled={enabled} focused={focused}: the well's tree is the wrappers' tree"
            );
        }
    }
}

/// One contact with a scene.
#[derive(Clone, Copy, Debug)]
enum Contact {
    Tap,
    DragPastSlop,
    LongPress,
    Cancel,
}

/// Makes `contact` starting at `(x, y)` and settles.
fn contact(laid: &mut LaidOut, contact: Contact, x: f64, y: f64) {
    laid.dispatch_pointer_down(x, y);
    match contact {
        Contact::Tap => laid.dispatch_pointer_up(x, y),
        Contact::DragPastSlop => {
            for step in 1..=6 {
                let moved = y - 10.0 * f64::from(step);
                laid.dispatch_pointer_move_after(x, moved, Duration::from_millis(16));
                laid.pump_for(Duration::from_millis(16));
            }
            laid.dispatch_pointer_up(x, y - 60.0);
        }
        Contact::LongPress => {
            laid.pump_for(Duration::from_millis(800));
            laid.dispatch_pointer_up(x, y);
        }
        Contact::Cancel => laid.dispatch_pointer_cancel(),
    }
    laid.pump_for(Duration::from_millis(400));
}

/// A well in a scrollable and a well inside a well enter the arena as the
/// wrappers do: a tap taps, a drag past the slop scrolls and taps nothing,
/// a long press and a cancel end as they do for the wrappers, and the inner
/// well wins a contact over it while the outer one wins a contact beside it.
///
/// Red-check: mount the node's tap without its arena admission (the drag
/// taps the well, or the scrollable never wins).
pub fn ink_well_contests_like_gesture_detector() {
    use flui_sdk::widgets::{Column, SingleChildScrollView, Text};

    for made_contact in [
        Contact::Tap,
        Contact::DragPastSlop,
        Contact::LongPress,
        Contact::Cancel,
    ] {
        let scrolled = |made: Made| {
            let log = TapLog::default();
            let scene = SizedBox::new(300.0, 200.0).child(SingleChildScrollView::new().child(
                Column::new((
                    well(
                        made,
                        Some(logging(&log, "tap")),
                        SizedBox::new(300.0, 60.0).child(Text::new("Press")),
                    ),
                    SizedBox::new(300.0, 2_000.0),
                )),
            ));
            let mut laid = lay_out(scene, common::loose(400.0));
            let label = laid.find_text("Press").expect("the well's label");
            let before = laid.absolute_offset(label);
            contact(&mut laid, made_contact, 150.0, 30.0);
            let label = laid.find_text("Press").expect("the well's label");
            let taps = log.borrow().clone();
            (taps, before != laid.absolute_offset(label))
        };
        let (on_node, wrapped) = (scrolled(Made::Well), scrolled(Made::Wrappers));
        assert_eq!(on_node, wrapped, "{made_contact:?} in a scrollable");
        match made_contact {
            Contact::Tap => assert_eq!(on_node, (vec!["tap"], false)),
            Contact::DragPastSlop => assert_eq!(on_node, (vec![], true), "the drag scrolls"),
            Contact::Cancel => assert_eq!(on_node, (vec![], false)),
            Contact::LongPress => {}
        }
        for (place, y) in [("inner", 60.0), ("outer", 20.0)] {
            let nested = |made: Made| {
                let log = TapLog::default();
                let inner = well(
                    made,
                    Some(logging(&log, "inner")),
                    SizedBox::new(200.0, 40.0),
                );
                let outer = well(
                    made,
                    Some(logging(&log, "outer")),
                    Column::new((SizedBox::new(200.0, 40.0), inner)),
                );
                let mut laid = lay_out(outer, common::loose(400.0));
                contact(&mut laid, made_contact, 100.0, y);
                log.borrow().clone()
            };
            let on_node = nested(Made::Well);
            assert_eq!(
                on_node,
                nested(Made::Wrappers),
                "{made_contact:?} over the {place} well"
            );
            if matches!(made_contact, Contact::Tap) {
                assert_eq!(on_node, [place], "a tap taps the well under it");
            }
        }
    }
}

/// A disabled well is disabled in every part (ADR-0322 §2): no tap, no
/// focus, `Disabled` and no `Hovered`, still hit where it is, and an Enter
/// that reaches it from a focused descendant stops at it instead of
/// activating an enclosing binding.
///
/// Red-check: give the node's bindings to a disabled well as they are (the
/// descendant's Enter taps a disabled well), or drop them (it reaches the
/// enclosing binding).
pub fn a_disabled_ink_well_is_disabled_in_every_part() {
    use flui_sdk::widgets::{
        Action as IntentAction, ActionOutcome, Actions, ActivateIntent, Focus, GestureDetector,
        Stack,
    };

    struct Outer(TapLog);
    impl IntentAction<ActivateIntent> for Outer {
        fn invoke(
            &self,
            _ctx: &mut flui_sdk::view::EventContext<'_>,
            _intent: &ActivateIntent,
        ) -> ActionOutcome {
            self.0.borrow_mut().push("outer");
            ActionOutcome::Performed
        }
    }

    let log = TapLog::default();
    let states = WidgetStatesController::default();
    let inner = FocusNode::with_debug_label("inside-a-disabled-well");
    let beneath = logging(&log, "beneath");
    let mut laid = lay_out(
        Actions::new(Stack::new((
            GestureDetector::new()
                .on_tap(move |ctx| beneath(ctx))
                .child(SizedBox::new(80.0, 40.0)),
            InkWell::new()
                .child(Focus::new(SizedBox::new(80.0, 40.0)).focus_node(Rc::clone(&inner)))
                .states_controller(states.clone()),
        )))
        .action::<ActivateIntent>(Outer(Rc::clone(&log))),
        tight(80.0, 40.0),
    );
    assert!(states.value().contains_state(WidgetState::Disabled));

    laid.dispatch_pointer_hover(40.0, 20.0);
    laid.pump();
    assert!(
        !states.value().contains_state(WidgetState::Hovered),
        "a disabled well tracks no hover"
    );
    laid.dispatch_pointer_down(40.0, 20.0);
    laid.dispatch_pointer_up(40.0, 20.0);
    laid.pump();
    assert!(
        log.borrow().is_empty(),
        "a disabled well covers the well beneath it and taps nothing: {:?}",
        log.borrow()
    );
    assert!(!states.value().contains_state(WidgetState::Pressed));

    let manager = laid.focus_manager();
    let _ = laid.enter_owner_scope(|| manager.dispatch_key_event(&tab()));
    laid.pump();
    assert!(
        inner.has_primary_focus(),
        "Tab skips the disabled well and reaches the focusable inside it"
    );
    let _ = laid.enter_owner_scope(|| manager.dispatch_key_event(&enter()));
    laid.pump();
    assert!(
        log.borrow().is_empty(),
        "the Enter stops at the disabled well: {:?}",
        log.borrow()
    );
}

/// `tree` with each node id replaced by the order it first appears in. An
/// id is the slot the semantics arena gave the node, which follows the order
/// the nodes were assembled in, not anything a platform reads of them: the
/// node and the wrappers assemble one tree in a different order.
fn canonical_ids(tree: &str) -> String {
    let mut seen: Vec<&str> = Vec::new();
    let mut out = String::with_capacity(tree.len());
    let mut rest = tree;
    while let Some(at) = rest.find('#') {
        out.push_str(&rest[..=at]);
        rest = &rest[at + 1..];
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let (id, tail) = rest.split_at(digits);
        let index = seen
            .iter()
            .position(|known| *known == id)
            .unwrap_or_else(|| {
                seen.push(id);
                seen.len() - 1
            });
        out.push_str(&index.to_string());
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// A well whose test turns it off and on, rebuilds it with a new tap closure
/// on every change of `generation`, and hands it the first or the second of
/// `states`.
#[derive(Clone, StatefulView)]
struct ToggledWell {
    enabled: flui_testing::widgets::SharedSignal<bool>,
    generation: flui_testing::widgets::SharedSignal<u32>,
    first: flui_testing::widgets::SharedSignal<bool>,
    states: [WidgetStatesController; 2],
}

struct ToggledWellState {
    enabled: flui_testing::widgets::SharedSignal<bool>,
    generation: flui_testing::widgets::SharedSignal<u32>,
    first: flui_testing::widgets::SharedSignal<bool>,
}

impl StatefulView for ToggledWell {
    type State = ToggledWellState;
    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        ToggledWellState {
            enabled: self.enabled.clone(),
            generation: self.generation.clone(),
            first: self.first.clone(),
        }
    }
}

impl ViewState<ToggledWell> for ToggledWellState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.enabled.bind(ctx);
        self.generation.bind(ctx);
        self.first.bind(ctx);
    }

    fn build(&self, view: &ToggledWell, ctx: &dyn BuildContext) -> impl IntoView {
        let generation = self.generation.get(ctx);
        let [first, second] = &view.states;
        let states = if self.first.get(ctx) { first } else { second };
        let well = InkWell::new()
            .child(SizedBox::new(80.0, 40.0))
            .states_controller(states.clone());
        if self.enabled.get(ctx) {
            well.on_tap(move |_ctx| {
                let _ = generation;
            })
        } else {
            well
        }
    }
}

/// A controller the well stops writing to keeps none of its states:
/// handed a second controller while disabled, the well leaves `Disabled` on
/// the second alone, and enabled again on the first, it leaves nothing on
/// the second.
///
/// Red-check: skip clearing the replaced controller in the node's
/// `did_update_view`, and the first keeps `Disabled`.
pub fn a_replaced_controller_keeps_no_disabled() {
    let enabled = flui_testing::widgets::SharedSignal::new(false);
    let generation = flui_testing::widgets::SharedSignal::new(0_u32);
    let first = flui_testing::widgets::SharedSignal::new(true);
    let [states, second] = [
        WidgetStatesController::default(),
        WidgetStatesController::default(),
    ];
    let mut laid = lay_out(
        ToggledWell {
            enabled: enabled.clone(),
            generation,
            first: first.clone(),
            states: [states.clone(), second.clone()],
        },
        tight(80.0, 40.0),
    );
    assert_eq!(states.value(), WidgetStates::DISABLED);
    first.set(false);
    laid.pump();
    assert_eq!(
        (states.value(), second.value()),
        (WidgetStates::NONE, WidgetStates::DISABLED),
        "the replaced controller is cleared, the new one disabled"
    );
    enabled.set(true);
    first.set(true);
    laid.pump();
    assert_eq!(
        (states.value(), second.value()),
        (WidgetStates::NONE, WidgetStates::NONE),
        "enabled on the first, nothing is left on the second"
    );
}

/// The well's activation holds `Pressed` for its delay past the contact
/// (ADR-0322 §3): through a rebuild of the well with a new view, and to the
/// controller handed to it under the hold, where it ends with its delay;
/// disabling the well under the hold ends the press at once, and the hold's
/// late end breaks nothing.
///
/// Red-check: leave the hold on the first controller in `did_update_view`
/// (the second stays pressed after the delay).
pub fn the_press_hold_survives_a_rebuild_and_a_disable_ends_it() {
    let enabled = flui_testing::widgets::SharedSignal::new(true);
    let generation = flui_testing::widgets::SharedSignal::new(0_u32);
    let first = flui_testing::widgets::SharedSignal::new(true);
    let [states, second] = [
        WidgetStatesController::default(),
        WidgetStatesController::default(),
    ];
    let mut laid = lay_out(
        ToggledWell {
            enabled: enabled.clone(),
            generation: generation.clone(),
            first: first.clone(),
            states: [states.clone(), second.clone()],
        },
        tight(80.0, 40.0),
    );
    let pressed =
        |states: &WidgetStatesController| states.value().contains_state(WidgetState::Pressed);

    laid.pump_for(Duration::from_millis(16));
    laid.dispatch_pointer_down(40.0, 20.0);
    laid.dispatch_pointer_up(40.0, 20.0);
    assert!(pressed(&states), "the tap's activation holds the press");
    generation.set(1);
    laid.pump_for(Duration::from_millis(40));
    assert!(pressed(&states), "a rebuild with a new view keeps the hold");
    laid.pump_for(Duration::from_millis(100));
    assert!(!pressed(&states), "the hold ends with its delay");

    laid.dispatch_pointer_down(40.0, 20.0);
    laid.dispatch_pointer_up(40.0, 20.0);
    laid.pump_for(Duration::from_millis(40));
    laid.dispatch_pointer_down(40.0, 20.0);
    laid.pump_for(Duration::from_millis(100));
    assert!(
        pressed(&states),
        "the first tap's delay ending leaves the next contact's press"
    );
    laid.dispatch_pointer_up(40.0, 20.0);
    laid.pump_for(Duration::from_millis(40));
    assert!(pressed(&states), "the second tap holds the press");
    laid.pump_for(Duration::from_millis(100));
    assert!(
        !pressed(&states),
        "the second tap's hold ends with its delay"
    );

    laid.dispatch_pointer_down(40.0, 20.0);
    laid.dispatch_pointer_up(40.0, 20.0);
    first.set(false);
    laid.pump_for(Duration::from_millis(40));
    assert!(
        pressed(&second) && !pressed(&states),
        "the hold follows the activation to the controller handed over under it"
    );
    laid.pump_for(Duration::from_millis(100));
    assert!(!pressed(&second), "and ends there with its delay");
    first.set(true);
    laid.pump();

    laid.dispatch_pointer_down(40.0, 20.0);
    laid.dispatch_pointer_up(40.0, 20.0);
    assert!(pressed(&states));
    enabled.set(false);
    laid.pump();
    assert!(
        !pressed(&states),
        "a well disabled under its hold is not pressed"
    );
    laid.pump_for(Duration::from_millis(200));
    assert_eq!(
        states.value(),
        WidgetStates::DISABLED,
        "and the hold's end changes nothing"
    );

    enabled.set(true);
    laid.pump();
    laid.dispatch_pointer_down(40.0, 20.0);
    laid.dispatch_pointer_up(40.0, 20.0);
    enabled.set(false);
    laid.pump();
    enabled.set(true);
    laid.pump();
    assert!(
        !pressed(&states),
        "a disable inside the delay ends the hold, so enabling again shows no press"
    );
}

/// The well makes its tap once and reads the handler at event time: a
/// rebuild with a new handler is what the next tap calls, and the handlers
/// it held are released when the well unmounts, so nothing the tap shares
/// with the well keeps the well alive.
///
/// Red-check: drop the handler refresh from the well's `build` (the second
/// tap calls the first handler), or let the state hold its tap through a
/// strong cycle (the guard outlives the unmount).
pub fn a_rebuilt_well_calls_its_new_handler_and_releases_it_when_unmounted() {
    let generation = flui_testing::widgets::SharedSignal::new(0_u32);
    let shown = flui_testing::widgets::SharedSignal::new(true);
    let calls = Rc::new(std::cell::RefCell::new(Vec::<u32>::new()));
    let guard = Rc::new(());
    let mut laid = lay_out(
        RebuiltWell {
            generation: generation.clone(),
            shown: shown.clone(),
            calls: Rc::clone(&calls),
            guard: Rc::clone(&guard),
        },
        tight(80.0, 40.0),
    );

    laid.dispatch_pointer_down(40.0, 20.0);
    laid.dispatch_pointer_up(40.0, 20.0);
    generation.set(1);
    laid.pump();
    laid.dispatch_pointer_down(40.0, 20.0);
    laid.dispatch_pointer_up(40.0, 20.0);
    assert_eq!(
        *calls.borrow(),
        [0, 1],
        "the tap after a rebuild calls the rebuilt handler"
    );

    shown.set(false);
    laid.pump_for(Duration::from_millis(200));
    assert_eq!(
        Rc::strong_count(&guard),
        2,
        "only the test and the view that holds the guard keep it once the well is gone"
    );
}

/// A well whose handler records `generation`, removed while `shown` is
/// `false`. Each handler holds a clone of `guard`.
#[derive(Clone, StatefulView)]
struct RebuiltWell {
    generation: flui_testing::widgets::SharedSignal<u32>,
    shown: flui_testing::widgets::SharedSignal<bool>,
    calls: Rc<std::cell::RefCell<Vec<u32>>>,
    guard: Rc<()>,
}

struct RebuiltWellState {
    generation: flui_testing::widgets::SharedSignal<u32>,
    shown: flui_testing::widgets::SharedSignal<bool>,
}

impl StatefulView for RebuiltWell {
    type State = RebuiltWellState;
    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        RebuiltWellState {
            generation: self.generation.clone(),
            shown: self.shown.clone(),
        }
    }
}

impl ViewState<RebuiltWell> for RebuiltWellState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.generation.bind(ctx);
        self.shown.bind(ctx);
    }

    fn build(&self, view: &RebuiltWell, ctx: &dyn BuildContext) -> impl IntoView {
        let generation = self.generation.get(ctx);
        if !self.shown.get(ctx) {
            return SizedBox::new(80.0, 40.0).into_view().boxed();
        }
        let calls = Rc::clone(&view.calls);
        let guard = Rc::clone(&view.guard);
        InkWell::new()
            .child(SizedBox::new(80.0, 40.0))
            .on_tap(move |_ctx| {
                let _ = &guard;
                calls.borrow_mut().push(generation);
            })
            .into_view()
            .boxed()
    }
}

/// Builds a control, enabled or not.
type SwitchedBuild = fn(bool) -> BoxedView;

/// A control a test turns off and on, rebuilt from `build`.
#[derive(Clone, StatefulView)]
struct SwitchedControl {
    enabled: flui_testing::widgets::SharedSignal<bool>,
    build: SwitchedBuild,
}

struct SwitchedControlState {
    enabled: flui_testing::widgets::SharedSignal<bool>,
}

impl StatefulView for SwitchedControl {
    type State = SwitchedControlState;
    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        SwitchedControlState {
            enabled: self.enabled.clone(),
        }
    }
}

impl ViewState<SwitchedControl> for SwitchedControlState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.enabled.bind(ctx);
    }

    fn build(&self, view: &SwitchedControl, ctx: &dyn BuildContext) -> impl IntoView {
        (view.build)(self.enabled.get(ctx))
    }
}

/// A control that resolves its colors from its well's controller above the
/// well seeds `Disabled` before it builds, from the switch it gives the well:
/// the frame that turns it off or on builds no element twice, so the control
/// is drawn in its new state from that frame's one build.
///
/// Red-check: drop the `Disabled` seed of a control's `did_update_view`, and
/// its frame runs one build more than the elements it builds.
pub fn a_control_turned_off_or_on_builds_each_element_once() {
    use flui_material::{ElevatedButton, FloatingActionButton, Radio, Switch};
    use flui_sdk::widgets::Text;

    let cases: [(&str, SwitchedBuild); 5] = [
        ("Checkbox", |on| {
            let control = Checkbox::new(false);
            if on {
                control.on_changed(|_ctx, _| {}).boxed()
            } else {
                control.boxed()
            }
        }),
        ("Radio", |on| {
            let control = Radio::new(1, Some(1));
            if on {
                control.on_changed(|_ctx, _| {}).boxed()
            } else {
                control.boxed()
            }
        }),
        ("Switch", |on| {
            let control = Switch::new(false);
            if on {
                control.on_changed(|_ctx, _| {}).boxed()
            } else {
                control.boxed()
            }
        }),
        ("ElevatedButton", |on| {
            let control = ElevatedButton::new(Text::new("Go"));
            if on {
                control.on_pressed(|_ctx| {}).boxed()
            } else {
                control.boxed()
            }
        }),
        ("FloatingActionButton", |on| {
            let control = FloatingActionButton::new();
            if on {
                control.on_pressed(|_ctx| {}).boxed()
            } else {
                control.boxed()
            }
        }),
    ];
    for (name, build) in cases {
        let enabled = flui_testing::widgets::SharedSignal::new(true);
        let mut laid = lay_out(
            Theme::new(
                ThemeData::light(),
                SwitchedControl {
                    enabled: enabled.clone(),
                    build,
                },
            ),
            tight(200.0, 100.0),
        );
        laid.pump();
        for on in [false, true] {
            enabled.set(on);
            let report = laid.pump_report(Duration::from_millis(16)).build;
            assert_eq!(
                report.builds_run, report.elements_built,
                "{name}, on: {on}: no element builds twice in the frame that turns it: {report:?}"
            );
        }
    }
}
