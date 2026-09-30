//! `RawButton`: a press opens an `EventCx` through the button's
//! `WriterSource` and writes a signal (ADR-0086). Pointer and assistive
//! technology both reach it; without a callback it is disabled.

use std::cell::RefCell;
use std::rc::Rc;

use flui_testing::{Action, ActionRequest, NodeId, TreeId};
use flui_view::prelude::*;
use flui_view::{Reactive, SignalError};
use flui_widgets::{Column, RawButton, Text, column};

use crate::common::{LaidOut, lay_out, loose};

/// What a press does, chosen by the test.
#[derive(Clone, Copy)]
enum Press {
    /// `count += step`.
    Add(u32),
    /// Write a released signal, then nothing else.
    Released,
}

/// The count signal and its graph, handed out by `init_state` so the test can
/// read the value without a frame.
type Seen = Rc<RefCell<Option<(Signal<u32>, Reactive)>>>;

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
        // A handle whose slot is gone, as after its owning element unmounted.
        let graph = ctx.reactive();
        self.released = graph.signal(0);
        graph.release(self.released.slot());
    }

    fn build(&self, view: &Counter, ctx: &dyn BuildContext) -> impl IntoView {
        view.seen
            .borrow_mut()
            .get_or_insert_with(|| (self.count, ctx.reactive()));
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
    let (sig, graph) = seen.as_ref().expect("the counter built");
    sig.peek(graph, |v| *v)
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
        Ok(0),
        "the request is recorded, not performed inline"
    );

    app.pump();
    assert_eq!(
        value(&seen),
        Ok(1),
        "the press ran after the frame, outside any build, so the write was accepted"
    );
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
