//! `RawButton`: a press opens an `EventCx` through the button's
//! `WriterSource` and writes a signal (ADR-0086). Pointer and assistive
//! technology both reach it; without a callback it is disabled.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_testing::{Action, ActionRequest, NodeId, TreeId, invoke_semantics_action};
use flui_view::prelude::*;
use flui_view::{Reactive, SignalError};
use flui_widgets::{Column, RawButton, SizedBox, Text, column};

use crate::common::{LaidOut, lay_out, loose};

/// What a press does, chosen by the test.
#[derive(Clone, Copy)]
enum Press {
    /// `count += step`.
    Add(u32),
    /// `count = 7`, through a closure bound with `let` and fixed by
    /// `callback`.
    LetBound,
    /// Write a released signal, then nothing else.
    Released,
    /// No callback: the button is disabled.
    Disabled,
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
            Press::LetBound => {
                let press = callback(move |cx| count.set(cx, 7));
                button.on_press(press)
            }
            Press::Released => button.on_press(move |cx| released.set(cx, 1)),
            Press::Disabled => button,
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
    let x = offset.dx.get() + size.width.get() / 2.0;
    let y = offset.dy.get() + size.height.get() / 2.0;
    app.dispatch_pointer_down(x, y);
    app.dispatch_pointer_up(x, y);
    app.tick();
}

fn value(seen: &Seen) -> Result<u32, SignalError> {
    let seen = seen.borrow();
    let (sig, graph) = seen.as_ref().expect("the counter built");
    sig.peek(graph, |v| *v)
}

#[test]
fn raw_button_press_writes_a_signal_and_rebuilds_its_reader() {
    let (root, seen) = counter(Press::Add(1));
    let mut app = lay_out(root, loose(400.0));
    assert!(app.find_text("0").is_some());

    press(&mut app);

    assert_eq!(value(&seen), Ok(1));
    assert!(
        app.find_text("1").is_some(),
        "the reader rebuilt and laid out the new count"
    );
    assert!(app.find_text("0").is_none(), "the old count is gone");
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

#[test]
fn raw_button_press_is_reachable_through_a_platform_click() {
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

    invoke_semantics_action(
        &app.pipeline_owner(),
        ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node: id,
            data: None,
        },
    )
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

#[test]
fn raw_button_without_on_press_is_disabled_and_advertises_no_click() {
    let (root, seen) = counter(Press::Disabled);
    let mut app = lay_out(root, loose(400.0));
    let (tree, _) = button_node(&mut app);
    let node = tree.find_by_label("Press").expect("located a moment ago");
    assert!(node.is_disabled(), "{}", tree.describe());
    assert!(
        !node.supports_action(Action::Click),
        "a disabled button offers no click. Tree was:\n{}",
        tree.describe()
    );

    press(&mut app);
    assert_eq!(value(&seen), Ok(0), "a tap writes nothing");
}

#[test]
fn raw_button_honours_a_new_callback_after_rebuild() {
    let (root, seen) = counter(Press::Add(1));
    let mut app = lay_out(root, loose(400.0));
    press(&mut app);
    assert_eq!(value(&seen), Ok(1));

    let (root, _) = counter(Press::Add(10));
    app.pump_widget(Counter {
        seen: Rc::clone(&seen),
        ..root
    });
    press(&mut app);

    assert_eq!(value(&seen), Ok(11), "the rebuilt closure ran");
    assert!(app.find_text("11").is_some());
}

#[test]
fn a_let_bound_press_closure_compiles_through_callback() {
    let (root, seen) = counter(Press::LetBound);
    let mut app = lay_out(root, loose(400.0));

    press(&mut app);

    assert_eq!(value(&seen), Ok(7));
    assert!(app.find_text("7").is_some());
}

#[test]
fn a_refused_write_in_a_press_is_reported_not_panicked() {
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

#[derive(Clone, StatelessView)]
struct CountedSignalReader {
    signal: Signal<u32>,
    builds: Rc<Cell<u32>>,
    observed: Rc<Cell<u32>>,
}

impl StatelessView for CountedSignalReader {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        self.builds.set(self.builds.get() + 1);
        self.observed.set(self.signal.get(ctx));
        SizedBox::new(10.0, 10.0)
    }
}

#[derive(Clone, StatefulView)]
struct CountedEventApp {
    writes: u32,
    root_builds: Rc<Cell<u32>>,
    reader_builds: [Rc<Cell<u32>>; 3],
    observed: [Rc<Cell<u32>>; 3],
}

#[derive(Default)]
struct CountedEventState {
    changed: Signal<u32>,
    unrelated: Signal<u32>,
}

impl StatefulView for CountedEventApp {
    type State = CountedEventState;

    fn create_state(&self) -> Self::State {
        CountedEventState::default()
    }
}

impl ViewState<CountedEventApp> for CountedEventState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.changed = ctx.signal(0);
        self.unrelated = ctx.signal(7);
    }

    fn build(&self, view: &CountedEventApp, _ctx: &dyn BuildContext) -> impl IntoView {
        view.root_builds.set(view.root_builds.get() + 1);
        let reader = |index: usize, signal| CountedSignalReader {
            signal,
            builds: view.reader_builds[index].clone(),
            observed: view.observed[index].clone(),
        };
        let changed = self.changed;
        let writes = view.writes;
        Column::new(column![
            reader(0, self.changed),
            reader(1, self.changed),
            reader(2, self.unrelated),
            RawButton::new(Text::new("Press")).on_press(move |cx| {
                for _ in 0..writes {
                    changed.update(cx, |value| *value += 1)?;
                }
                Ok::<(), SignalError>(())
            }),
        ])
    }
}

/// Counts actual build invocations after pointer-delivered EventCx writes.
/// Increasing the write burst must not increase rebuild fan-out or rebuild an
/// unrelated signal reader. This is a work-count contract, not a timing claim.
#[test]
fn event_write_bursts_rebuild_each_subscriber_once_without_unrelated_builds() {
    for writes in [1, 100, 10_000] {
        let root_builds = Rc::new(Cell::new(0));
        let reader_builds = std::array::from_fn(|_| Rc::new(Cell::new(0)));
        let observed = std::array::from_fn(|_| Rc::new(Cell::new(0)));
        let mut app = lay_out(
            CountedEventApp {
                writes,
                root_builds: root_builds.clone(),
                reader_builds: reader_builds.clone(),
                observed: observed.clone(),
            },
            loose(400.0),
        );
        let before = reader_builds.each_ref().map(|count| count.get());
        let root_before = root_builds.get();
        assert_eq!(observed.each_ref().map(|value| value.get()), [0, 0, 7]);
        assert!(before.into_iter().all(|count| count > 0));

        // Real pointer down/up followed by a normal frame; no harness root
        // dirtying, reassemble, or replacement can inflate the reader counts.
        press(&mut app);
        let after = reader_builds.each_ref().map(|count| count.get());
        assert_eq!(after, [before[0] + 1, before[1] + 1, before[2]]);
        assert_eq!(root_builds.get(), root_before, "burst of {writes}");
        assert_eq!(
            observed.each_ref().map(|value| value.get()),
            [writes, writes, 7]
        );

        app.tick();
        assert_eq!(reader_builds.each_ref().map(|count| count.get()), after);
        assert_eq!(
            root_builds.get(),
            root_before,
            "idle after burst of {writes}"
        );
    }
}

/// Assistive requests coalesce into a pending flag until the next build drains
/// them. Closing the owner first releases the mounted callback's capture; this
/// does not exercise retention by queued post-frame closures.
#[test]
fn pending_assistive_requests_release_mounted_capture_when_owner_is_dropped() {
    struct CapturedModel {
        drops: Rc<Cell<u32>>,
        calls: Rc<Cell<u32>>,
    }

    impl CapturedModel {
        fn called(&self) {
            self.calls.set(self.calls.get() + 1);
        }
    }

    impl Drop for CapturedModel {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }

    let drops = Rc::new(Cell::new(0));
    let calls = Rc::new(Cell::new(0));
    let model = CapturedModel {
        drops: drops.clone(),
        calls: calls.clone(),
    };
    let mut app = lay_out(
        RawButton::new(Text::new("Press")).on_press(move |_cx| model.called()),
        loose(400.0),
    );
    let (tree, id) = button_node(&mut app);
    for _ in 0..100 {
        invoke_semantics_action(
            &app.pipeline_owner(),
            ActionRequest {
                action: Action::Click,
                target_tree: TreeId::ROOT,
                target_node: id,
                data: None,
            },
        )
        .expect("the mounted button accepts the activation request");
    }
    assert_eq!(calls.get(), 0, "requests must not run inline");
    assert_eq!(
        drops.get(),
        0,
        "the mounted callback still owns its capture"
    );

    // No pump: the coalesced request has not reached drain_semantics_requests,
    // so teardown drops the mounted callback, not a post-frame delivery batch.
    drop(tree);
    drop(app);
    assert_eq!(
        calls.get(),
        0,
        "closed owner must not deliver pending requests"
    );
    assert_eq!(drops.get(), 1, "the mounted callback releases its model");
}
