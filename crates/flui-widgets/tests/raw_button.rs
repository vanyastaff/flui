//! `RawButton`: a press opens an `EventCx` through the button's
//! `WriterSource` and writes a signal (ADR-0086). Pointer and assistive
//! technology both reach it; without a callback it is disabled.

use std::cell::RefCell;
use std::rc::Rc;

use flui_rendering::hit_testing::HitTestBehavior;
use flui_testing::{Action, ActionRequest, NodeId, TreeId};
use flui_view::prelude::*;
use flui_view::{SignalError, WriterSource};
use flui_widgets::interaction::{
    Action as IntentAction, ActionOutcome, Actions, ActivateIntent, ActivationPressIntent,
    ButtonActivateIntent, Focus, FocusRing, GestureDetector,
};
use flui_widgets::{Column, RawButton, Semantics, SemanticsRole, Text, column};

use crate::common::{LaidOut, lay_out, loose};

/// What a press does, chosen by the test.
#[derive(Clone, Copy)]
enum Press {
    /// `count += step`.
    Add(u32),
    /// Write a released signal, then nothing else.
    Released,
}

/// The count signal and its writer source, handed out by `init_state` so the test can
/// read the value without a frame.
type Seen = Rc<RefCell<Option<(Signal<u32>, WriterSource)>>>;

/// A counter: the count as text, and a button labelled `Press` whose press
/// the test chooses.
#[derive(Clone, StatefulView)]
struct Counter {
    press: Press,
    seen: Seen,
}

#[derive(Default)]
struct CounterState {
    count: Signal<u32>,
    released: Signal<u32>,
    writer: Option<WriterSource>,
}

impl StatefulView for Counter {
    type State = CounterState;

    fn create_state(&self) -> Self::State {
        CounterState::default()
    }
}

impl ViewState<Counter> for CounterState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.count = ctx.signal(0);
        // A handle kept past its slot: released on a graph no presentation owns.
        let gone = flui_view::Reactive::new();
        self.released = gone.signal(0);
        gone.release(self.released.slot());
        self.writer = Some(ctx.writer_source());
    }

    fn build(&self, view: &Counter, ctx: &dyn BuildContext) -> impl IntoView {
        view.seen
            .borrow_mut()
            .get_or_insert_with(|| (self.count, self.writer.clone().expect("init_state ran")));
        let count = self.count;
        let released = self.released;
        let button = RawButton::new(Text::new("Press"));
        let button = match view.press {
            Press::Add(step) => button.on_press(move |cx| count.update(cx, |n| *n += step)),
            Press::Released => button.on_press(move |cx| released.set(cx, 1)),
        };
        Column::new(column![Text::new(count.get(ctx).to_string()), button])
    }
}

fn counter(press: Press) -> (Counter, Seen) {
    let seen: Seen = Rc::new(RefCell::new(None));
    (
        Counter {
            press,
            seen: Rc::clone(&seen),
        },
        seen,
    )
}

/// A primary-button tap at the centre of the button's label, then a frame.
fn press(app: &mut LaidOut) {
    let label = app.find_text("Press").expect("the button's label");
    let offset = app.absolute_offset(label);
    let size = app.size(label);
    let x = offset.dx + size.width / 2.0;
    let y = offset.dy + size.height / 2.0;
    app.dispatch_pointer_down(x, y);
    app.dispatch_pointer_up(x, y);
    app.tick();
}

fn value(seen: &Seen) -> Result<u32, SignalError> {
    let seen = seen.borrow();
    let (sig, writer) = seen.as_ref().expect("the counter built");
    writer.write(|cx| sig.peek(cx, |v| *v))
}

/// The node of the button: the one labelled with the button's text.
fn button_node(app: &mut LaidOut) -> (flui_testing::A11yTree, NodeId) {
    app.enable_semantics();
    app.pump();
    let tree = app.a11y_tree().expect("semantics enabled before the frame");
    let id = tree
        .find_by_label("Press")
        .unwrap_or_else(|error| panic!("one node labelled \"Press\": {error}\n{}", tree.describe()))
        .id();
    (tree, id)
}

pub(crate) fn raw_button_press_is_reachable_through_a_platform_click() {
    let (root, seen) = counter(Press::Add(1));
    let mut app = lay_out(root, loose(400.0));
    let (tree, id) = button_node(&mut app);
    let node = tree.find_by_label("Press").expect("located a moment ago");
    assert!(
        node.supports_action(Action::Click),
        "an enabled button advertises a click. Tree was:\n{}",
        tree.describe()
    );
    assert_eq!(
        format!("{:?}", node.role()),
        "Button",
        "{}",
        tree.describe()
    );
    assert!(!node.is_disabled());

    app.invoke_semantics_action(ActionRequest {
        action: Action::Click,
        target_tree: TreeId::ROOT,
        target_node: id,
        data: None,
    })
    .expect("a click on a node advertising one resolves");
    assert_eq!(
        value(&seen),
        Ok(1),
        "the press runs inside the platform request's dispatch, as Enter does, \
         and its signal write is accepted there"
    );

    app.pump();
    assert_eq!(value(&seen), Ok(1), "no later frame replays the press");
    assert!(app.find_text("1").is_some(), "the count's reader rebuilt");
}

pub(crate) fn a_refused_write_in_a_press_is_reported_not_panicked() {
    let (root, seen) = counter(Press::Released);
    let mut app = lay_out(root, loose(400.0));

    let ((), log) = flui_testing::log_capture::capture(|| press(&mut app));

    assert!(
        log.contains("an event callback's signal write was refused"),
        "the refusal is logged at the dispatch boundary: {log}"
    );
    assert_eq!(value(&seen), Ok(0), "other state is intact");
    assert!(app.find_text("0").is_some(), "and the tree still lays out");
}

/// The keyboard reaches an enabled button the way a tap does: Tab focuses it,
/// Enter and Space activate it, and so does a shortcut bound to
/// `ButtonActivateIntent`. A button without `on_press` takes no focus.
pub(crate) fn raw_button_activates_from_the_keyboard() {
    use std::cell::Cell;

    use flui_platform::keyboard::{Key, NamedKey};
    use flui_widgets::interaction::{ButtonActivateIntent, Shortcuts, SingleActivator};
    use flui_widgets::{Row, SizedBox};

    use crate::common::harness::mount;
    use crate::keyboard_focus::{press, tab};

    let presses = Rc::new(Cell::new(0));
    let counted = Rc::clone(&presses);
    let harness = mount(
        Shortcuts::new(Row::new((
            RawButton::new(SizedBox::new(40.0, 40.0)),
            RawButton::new(SizedBox::new(40.0, 40.0))
                .on_press(move |_cx| counted.set(counted.get() + 1)),
        )))
        .shortcut(SingleActivator::character("b"), ButtonActivateIntent),
    );
    let manager = harness.focus_manager();

    assert!(tab(&manager), "Tab finds the enabled button");
    let focused = manager.primary_focus().expect("a button has the focus");
    assert!(press(&manager, &Key::Named(NamedKey::Enter)));
    assert_eq!(presses.get(), 1, "Enter presses it");
    let _ = press(&manager, &Key::character(" "));
    assert_eq!(presses.get(), 2, "Space presses it");
    assert!(press(&manager, &Key::character("b")));
    assert_eq!(
        presses.get(),
        3,
        "a ButtonActivateIntent shortcut presses it"
    );

    let _ = tab(&manager);
    assert!(
        manager
            .primary_focus()
            .is_some_and(|node| Rc::ptr_eq(&node, &focused)),
        "Tab stays on the enabled button: the disabled one is not a Tab stop"
    );
}

/// The same button made two ways, so one test body compares them.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Made {
    /// [`RawButton`].
    Raw,
    /// The public wrappers a button is composed from by hand.
    Wrappers,
}

type OnPress = Rc<dyn Fn(&mut EventCx<'_>)>;

/// A button composed from the public wrappers: what `RawButton` must stay
/// indistinguishable from to assistive technology and to the gesture arena
/// (ADR-0322 §6).
#[derive(Clone, StatelessView)]
struct WrapperButton {
    on_press: Option<OnPress>,
    child: BoxedView,
}

#[derive(Clone)]
struct WrapperPress(Option<OnPress>);

impl WrapperPress {
    fn run(&self, cx: &mut EventCx<'_>) -> ActionOutcome {
        match &self.0 {
            Some(on_press) => {
                on_press(cx);
                ActionOutcome::Performed
            }
            None => ActionOutcome::NotPerformed,
        }
    }
}

impl IntentAction<ActivateIntent> for WrapperPress {
    fn is_enabled(&self, _intent: &ActivateIntent) -> bool {
        self.0.is_some()
    }

    fn invoke(&self, cx: &mut EventCx<'_>, _intent: &ActivateIntent) -> ActionOutcome {
        self.run(cx)
    }
}

impl IntentAction<ButtonActivateIntent> for WrapperPress {
    fn is_enabled(&self, _intent: &ButtonActivateIntent) -> bool {
        self.0.is_some()
    }

    fn invoke(&self, cx: &mut EventCx<'_>, _intent: &ButtonActivateIntent) -> ActionOutcome {
        self.run(cx)
    }
}

impl IntentAction<ActivationPressIntent> for WrapperPress {
    fn is_enabled(&self, _intent: &ActivationPressIntent) -> bool {
        self.0.is_some()
    }

    fn invoke(&self, _cx: &mut EventCx<'_>, _intent: &ActivationPressIntent) -> ActionOutcome {
        ActionOutcome::Performed
    }
}

impl StatelessView for WrapperButton {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let enabled = self.on_press.is_some();
        let press = WrapperPress(self.on_press.clone());
        let actions = Actions::new(
            Focus::new(FocusRing::new(self.child.clone()))
                .can_request_focus(enabled)
                .debug_label("RawButton"),
        )
        .action::<ActivateIntent>(press.clone())
        .action::<ButtonActivateIntent>(press.clone())
        .action::<ActivationPressIntent>(press);
        let mut detector = GestureDetector::new()
            .behavior(HitTestBehavior::Opaque)
            .child(actions);
        if let Some(handler) = self.on_press.clone() {
            detector = detector.on_tap(move |cx| handler(cx));
        }
        Semantics::new()
            .container(true)
            .role(SemanticsRole::Button)
            .enabled(enabled)
            .child(detector)
    }
}

/// A button over `child`, made as `made` says; `on_press` absent disables it.
fn button(made: Made, on_press: Option<OnPress>, child: impl IntoView) -> BoxedView {
    match made {
        Made::Raw => {
            let button = RawButton::new(child);
            match on_press {
                Some(on_press) => button.on_press(move |cx| on_press(cx)).boxed(),
                None => button.boxed(),
            }
        }
        Made::Wrappers => WrapperButton {
            on_press,
            child: child.into_view().boxed(),
        }
        .boxed(),
    }
}

/// What a scene's presses recorded, in order.
type PressLog = Rc<RefCell<Vec<&'static str>>>;

fn logging(log: &PressLog, name: &'static str) -> OnPress {
    let log = Rc::clone(log);
    Rc::new(move |_cx: &mut EventCx<'_>| {
        log.borrow_mut().push(name);
    })
}

/// The accessibility tree of a button is the tree of the wrappers it
/// replaces: every node, property, action and bound, enabled and disabled,
/// with and without the keyboard focus.
///
/// Red-check: drop the `on_click` the node takes from its tap slot (the
/// enabled rows lose the click action).
pub(crate) fn raw_button_a11y_tree_matches_wrappers() {
    for enabled in [true, false] {
        for focused in [false, true] {
            let tree = |made: Made| {
                let log = PressLog::default();
                let on_press = enabled.then(|| logging(&log, "press"));
                let mut app = lay_out(
                    Column::new(column![
                        Text::new("Above"),
                        button(made, on_press, Text::new("Press")),
                    ]),
                    loose(400.0),
                );
                if focused {
                    let manager = app.focus_manager();
                    let moved = app.enter_owner_scope(|| crate::keyboard_focus::tab(&manager));
                    assert_eq!(moved, enabled, "Tab reaches an enabled button only");
                    app.pump();
                }
                app.enable_semantics();
                app.pump();
                let tree = app.a11y_tree().expect("semantics enabled before the frame");
                let clicks = tree
                    .find_by_label("Press")
                    .expect("the button is one node named by its label")
                    .supports_action(Action::Click);
                assert_eq!(
                    clicks, enabled,
                    "{made:?}: only an enabled button offers a click"
                );
                format!("{:#?}", tree.raw())
            };
            let (raw, wrappers) = (tree(Made::Raw), tree(Made::Wrappers));
            assert!(
                raw.contains("Button"),
                "the tree describes a button:\n{raw}"
            );
            assert_eq!(
                raw, wrappers,
                "enabled={enabled} focused={focused}: the button's tree is the wrappers' tree"
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

const CONTACTS: [Contact; 4] = [
    Contact::Tap,
    Contact::DragPastSlop,
    Contact::LongPress,
    Contact::Cancel,
];

/// Makes `contact` starting at `(x, y)` and settles.
fn contact(app: &mut LaidOut, contact: Contact, x: f64, y: f64) {
    use std::time::Duration;

    app.dispatch_pointer_down(x, y);
    match contact {
        Contact::Tap => app.dispatch_pointer_up(x, y),
        Contact::DragPastSlop => {
            for step in 1..=6 {
                let moved = y - 10.0 * f64::from(step);
                app.dispatch_pointer_move_after(x, moved, Duration::from_millis(16));
                app.pump_for(Duration::from_millis(16));
            }
            app.dispatch_pointer_up(x, y - 60.0);
        }
        Contact::LongPress => {
            app.pump_for(Duration::from_millis(800));
            app.dispatch_pointer_up(x, y);
        }
        Contact::Cancel => app.dispatch_pointer_cancel(),
    }
    app.pump_for(Duration::from_millis(400));
}

/// A button at the top of a vertical scrollable, over content taller than
/// the viewport.
fn scrollable_scene(made: Made, log: &PressLog) -> flui_widgets::SizedBox {
    use flui_widgets::{SingleChildScrollView, SizedBox};

    SizedBox::new(300.0, 200.0).child(SingleChildScrollView::new().child(Column::new(column![
        button(
            made,
            Some(logging(log, "press")),
            SizedBox::new(300.0, 60.0).child(Text::new("Press"))
        ),
        SizedBox::new(300.0, 2_000.0),
    ])))
}

/// What a contact left behind: the presses, and where the label ended up.
fn outcome(app: &LaidOut, log: &PressLog) -> (Vec<&'static str>, String) {
    let label = app.find_text("Press").expect("the button's label");
    (
        log.borrow().clone(),
        format!("{:?}", app.absolute_offset(label)),
    )
}

/// A button in a scrollable enters the arena as a `GestureDetector` does: a
/// tap presses it, a drag past the slop scrolls and presses nothing, and a
/// long press and a cancel end as they do for the wrappers.
///
/// Red-check: give the node's recognizers no arena admission (the drag
/// presses the button or the scrollable never wins).
pub(crate) fn raw_button_in_scrollable_contests_like_gesture_detector() {
    for made_contact in CONTACTS {
        let run = |made: Made| {
            let log = PressLog::default();
            let mut app = lay_out(scrollable_scene(made, &log), loose(400.0));
            let before = outcome(&app, &log).1;
            contact(&mut app, made_contact, 150.0, 30.0);
            let (presses, after) = outcome(&app, &log);
            (presses, before != after)
        };
        let (raw, wrappers) = (run(Made::Raw), run(Made::Wrappers));
        assert_eq!(raw, wrappers, "{made_contact:?}");
        match made_contact {
            Contact::Tap => assert_eq!(raw, (vec!["press"], false)),
            Contact::DragPastSlop => assert_eq!(raw, (vec![], true), "the drag scrolls"),
            Contact::Cancel => assert_eq!(raw, (vec![], false)),
            Contact::LongPress => {}
        }
    }
}

/// A button inside a button: the inner one wins a contact over it, the outer
/// one a contact beside it, as nested `GestureDetector`s do.
///
/// Red-check: make the node's hit test skip its child (the outer button
/// takes the inner one's taps).
pub(crate) fn nested_raw_button_contests_like_gesture_detector() {
    use flui_widgets::SizedBox;

    for made_contact in CONTACTS {
        for (place, y) in [("inner", 60.0), ("outer", 20.0)] {
            let run = |made: Made| {
                let log = PressLog::default();
                let inner = button(
                    made,
                    Some(logging(&log, "inner")),
                    SizedBox::new(200.0, 40.0),
                );
                let outer = button(
                    made,
                    Some(logging(&log, "outer")),
                    Column::new(column![SizedBox::new(200.0, 40.0), inner]),
                );
                let mut app = lay_out(outer, loose(400.0));
                contact(&mut app, made_contact, 100.0, y);
                log.borrow().clone()
            };
            let (raw, wrappers) = (run(Made::Raw), run(Made::Wrappers));
            assert_eq!(raw, wrappers, "{made_contact:?} over the {place} button");
            if matches!(made_contact, Contact::Tap) {
                assert_eq!(raw, [place], "a tap presses the button under it");
            }
        }
    }
}
