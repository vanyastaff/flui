//! What a Material control costs to mount, counted rather than timed.
//!
//! The control-cost scenarios of `crates/flui-widgets/tests/perf.rs`
//! (`perf_raw_button_over_text`), for the controls of this catalog, measured
//! the same way by `flui_testing::control_cost::control_over_text`: each one
//! mounts `MOUNT_ROWS` controls over a `Text` and records, per control, what
//! it adds to the `Text` it holds. ADR-0322 sets the threshold a control on
//! the catalogs' interactive node stays within: 2 elements and 1 render node
//! for its behaviours, beside the elements of its own decoration.
//!
//! The allocations and bytes are held by `perf/baseline/<scenario>.toml`,
//! which only falls: a scenario fails when it costs more than its file says.
//! `FLUI_PERF_OUT=<dir>` writes each record to `<dir>/<scenario>.toml`
//! before asserting, and a lower record is copied into the baseline by hand.
//!
//! The plateau test at the end holds the heap level: a Material application navigated and
//! dropped again and again leaves no more bytes live each time
//! (`flui_testing::heap::assert_rounds_plateau`).
//!
//! A target of its own, not a module of `tests/main.rs`: it pins the
//! process-global font system and installs the kit's counting global
//! allocator.

use std::cell::RefCell;
use std::rc::Rc;

use flui_material::{
    AlertDialog, AppBar, Checkbox, Chip, ElevatedButton, FilterChip, InkWell, MaterialApp,
    NavigationBar, NavigationDestination, Radio, Scaffold, Switch, Tab, TabBar, TabController,
    TextButton, Theme, ThemeData, show_dialog,
};
use flui_sdk::view::environment::{MediaQuery, MediaQueryData};
use flui_sdk::view::prelude::*;
use flui_sdk::view::{AsyncDriver, Signal, TaskToken, WriterSource};
use flui_sdk::widgets::icon::IconData;
use flui_sdk::widgets::{Column, Icon, PageHandle, Routable, Router, Text};
use flui_testing::control_cost::{
    ControlCost, control_alone_under, control_over_text, control_over_text_under,
};
use flui_testing::heap::{CountingAllocator, assert_rounds_plateau};
use flui_testing::{Tester, find};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// `cost` recorded as `scenario` and held to this catalog's baseline file.
fn held_to_baseline(scenario: &str, cost: ControlCost) -> ControlCost {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("perf/baseline");
    flui_testing::control_cost::held_to_baseline(&dir, scenario, cost)
}

/// What an `InkWell` adds to the `Text` it holds: its behaviours on the
/// interactive node, and its ink, an idle `CustomPaint`.
#[test]
fn perf_ink_well_over_text() {
    let cost = held_to_baseline(
        "ink_well_over_text",
        control_over_text(|row| {
            InkWell::new()
                .child(Text::new(format!("Row {row}")))
                .on_tap(|_ctx| {})
                .boxed()
        }),
    );
    // The well's element, its node's element and the ink's; the node's
    // render object and the ink's.
    assert!(cost.elements <= 3, "a well adds {} elements", cost.elements);
    assert!(
        cost.render_nodes <= 2,
        "a well adds {} render nodes",
        cost.render_nodes
    );
}

/// What an `ElevatedButton` adds to the `Text` it holds under a theme: the
/// `ButtonStyleButton` composition (size constraints, `Material`, the well,
/// padding and text style), its button semantics on the well's node.
#[test]
fn perf_elevated_button_over_text() {
    let cost = held_to_baseline(
        "elevated_button_over_text",
        control_over_text_under(
            |column| Theme::new(ThemeData::light(), column).boxed(),
            |row| {
                ElevatedButton::new(Text::new(format!("Row {row}")))
                    .on_pressed(|_ctx| {})
                    .boxed()
            },
        ),
    );
    // The well's three and the composition's own (the button, its core,
    // its constraints, its `Material`, padding and text style); one
    // semantics node, the well's, and no `Semantics` element of its own.
    assert!(
        cost.elements <= 11,
        "a button adds {} elements",
        cost.elements
    );
    assert!(
        cost.render_nodes <= 5,
        "a button adds {} render nodes",
        cost.render_nodes
    );
}

/// What a toggle costs in place of an empty box, under a theme: its own
/// element, the well's three and its painter, less the box; its checked
/// state rides on the well's node, with no `Semantics` element of its own.
fn toggle_cost(scenario: &str, control: fn(usize) -> BoxedView) -> ControlCost {
    let cost = held_to_baseline(
        scenario,
        control_alone_under(
            |column| Theme::new(ThemeData::light(), column).boxed(),
            control,
        ),
    );
    assert!(
        cost.elements <= 4,
        "{scenario}: a toggle adds {} elements",
        cost.elements
    );
    assert!(
        cost.render_nodes <= 2,
        "{scenario}: a toggle adds {} render nodes",
        cost.render_nodes
    );
    cost
}

#[test]
fn perf_checkbox_alone() {
    toggle_cost("checkbox_alone", |row| {
        Checkbox::new(row % 2 == 0)
            .on_changed(|_ctx, _value| {})
            .boxed()
    });
}

#[test]
fn perf_switch_alone() {
    toggle_cost("switch_alone", |row| {
        Switch::new(row % 2 == 0)
            .on_changed(|_ctx, _value| {})
            .boxed()
    });
}

#[test]
fn perf_radio_alone() {
    toggle_cost("radio_alone", |row| {
        Radio::new(row % 3, Some(0))
            .on_changed(|_ctx, _value| {})
            .boxed()
    });
}

/// What a pressable `Chip` adds to the `Text` it labels, under a theme: its
/// composition (the chip, its border painter, `Material`, the well, padding,
/// constraints, opacity and the label's row), and the `Semantics` that keeps
/// the chip one node whether it has `on_pressed` or not.
#[test]
fn perf_chip_over_text() {
    let cost = held_to_baseline(
        "chip_over_text",
        control_over_text_under(
            |column| Theme::new(ThemeData::light(), column).boxed(),
            |row| {
                Chip::new(Text::new(format!("Row {row}")))
                    .on_pressed(|_ctx| {})
                    .boxed()
            },
        ),
    );
    // The well's three, the chip's own composition and its `Semantics`, which
    // stays so that gaining or losing the handler keeps the subtree.
    assert!(
        cost.elements <= 15,
        "a chip adds {} elements",
        cost.elements
    );
    assert!(
        cost.render_nodes <= 10,
        "a chip adds {} render nodes",
        cost.render_nodes
    );
}

/// What a `FilterChip` adds to the `Text` it labels, under a theme, its
/// selection on the well's node as the button's toggled state.
#[test]
fn perf_filter_chip_over_text() {
    let cost = held_to_baseline(
        "filter_chip_over_text",
        control_over_text_under(
            |column| Theme::new(ThemeData::light(), column).boxed(),
            |row| {
                FilterChip::new(Text::new(format!("Row {row}")))
                    .selected(row % 2 == 0)
                    .on_selected(|_ctx, _selected| {})
                    .boxed()
            },
        ),
    );
    // A chip's composition and the slot of its checkmark.
    assert!(
        cost.elements <= 15,
        "a filter chip adds {} elements",
        cost.elements
    );
    assert!(
        cost.render_nodes <= 10,
        "a filter chip adds {} render nodes",
        cost.render_nodes
    );
}

/// What a `TabBar` of two text tabs costs in place of an empty box, under a
/// theme: the bar's composition and indicator, and per tab its well, whose
/// node carries the tab's role, selection and merged label, with no
/// `Semantics` or `MergeSemantics` element of the tab's own.
#[test]
fn perf_tab_bar_alone() {
    let cost = held_to_baseline(
        "tab_bar_alone",
        control_alone_under(
            |column| Theme::new(ThemeData::light(), column).boxed(),
            |_row| {
                TabBar::secondary(vec![Tab::new().text("One"), Tab::new().text("Two")])
                    .controller(TabController::new(2, 0))
                    .boxed()
            },
        ),
    );
    // 45 elements and 28 render nodes while each tab had a `MergeSemantics`
    // and a `Semantics` of its own.
    assert!(
        cost.elements <= 41,
        "a tab bar adds {} elements",
        cost.elements
    );
    assert!(
        cost.render_nodes <= 24,
        "a tab bar adds {} render nodes",
        cost.render_nodes
    );
}

/// What a `NavigationBar` of two destinations costs in place of an empty
/// box, under a theme and a `MediaQuery`: the bar's composition and per
/// destination its well, whose node carries the destination's role,
/// selection and merged label.
#[test]
fn perf_navigation_bar_alone() {
    let cost = held_to_baseline(
        "navigation_bar_alone",
        control_alone_under(
            |column| {
                Theme::new(
                    ThemeData::light(),
                    MediaQuery::new(MediaQueryData::default(), column),
                )
                .boxed()
            },
            |_row| {
                NavigationBar::new(vec![
                    NavigationDestination::new(Icon::new(IconData::new(0xE88A)), "Home"),
                    NavigationDestination::new(Icon::new(IconData::new(0xE7FD)), "Profile"),
                ])
                .boxed()
            },
        ),
    );
    // 55 elements and 30 render nodes while each destination had a
    // `MergeSemantics` and a `Semantics` of its own.
    assert!(
        cost.elements <= 51,
        "a navigation bar adds {} elements",
        cost.elements
    );
    assert!(
        cost.render_nodes <= 26,
        "a navigation bar adds {} render nodes",
        cost.render_nodes
    );
}

/// The routes of the application a plateau round drives.
#[derive(Routable, Clone, Debug, PartialEq)]
enum Mail {
    #[route("/")]
    Inbox,
    #[route("/message/:id")]
    Message { id: u32 },
}

/// The inbox opens a message and shows what the message page answered.
#[derive(Clone, StatefulView)]
struct Inbox;

struct InboxState {
    answer: Signal<String>,
    writer: WriterSource,
    driver: Option<AsyncDriver>,
    /// The task awaiting the open message's result; dropping it cancels it.
    waiting: Rc<RefCell<Option<TaskToken>>>,
}

impl StatefulView for Inbox {
    type State = InboxState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        InboxState {
            answer: ctx.signal("Open the message".to_owned()),
            writer: ctx.writer_source(),
            driver: ctx.async_driver(),
            waiting: Rc::default(),
        }
    }
}

impl ViewState<Inbox> for InboxState {
    fn build(&self, _view: &Inbox, ctx: &dyn BuildContext) -> impl IntoView {
        let router = Router::<Mail>::of(ctx).expect("the inbox is a page of the Router");
        let (answer, writer, driver) = (self.answer, self.writer.clone(), self.driver.clone());
        let waiting = Rc::clone(&self.waiting);
        let open = move |_ctx: &mut EventContext<'_>| {
            let (Ok(result), Some(driver)) = (
                router.push_for_result::<bool>(Mail::Message { id: 1 }),
                &driver,
            ) else {
                return;
            };
            let writer = writer.clone();
            let task = driver.spawn_local(Box::pin(async move {
                let text = match result.await {
                    Ok(Some(true)) => "Archived",
                    _ => "Kept",
                };
                let _ = writer.write(|ctx| answer.set(ctx, text.to_owned()));
            }));
            *waiting.borrow_mut() = Some(task);
        };
        Scaffold::new()
            .app_bar(AppBar::new().title(Text::new("Inbox")))
            .body(Column::new((
                Text::new(answer.get(ctx)),
                TextButton::new(Text::new("Open message 1")).on_pressed(open),
                Checkbox::new(false),
                Switch::new(true),
            )))
    }
}

/// A message: archiving it asks first, in a dialog, then pops the page with `true`.
#[derive(Clone, StatelessView)]
struct Message {
    id: u32,
}

impl StatelessView for Message {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let page = PageHandle::of(ctx).expect("a message is a page of the Router");
        let ask = move |_ctx: &mut EventContext<'_>| {
            let message = page.clone();
            let _dialog = show_dialog::<(), _, _>(&page, move |ctx| {
                let dialog = PageHandle::of(ctx).expect("the dialog is a route");
                let archive = message.clone();
                AlertDialog::new()
                    .title(Text::new("Archive this message?"))
                    .actions(vec![
                        TextButton::new(Text::new("Archive it"))
                            .on_pressed(move |_ctx| {
                                let _ = dialog.pop();
                                let _ = archive.pop_with(true);
                            })
                            .boxed(),
                    ])
            });
        };
        Scaffold::new()
            .app_bar(AppBar::new().title(Text::new(format!("Message {}", self.id))))
            .body(ElevatedButton::new(Text::new("Archive")).on_pressed(ask))
    }
}

/// A Material application with a Router: a page pushed for a result, a dialog over it, both
/// popped, the awaiting task writing the answer back. Routes, overlays, transitions, ink and
/// the async driver each hold `Rc`s to the others; a cycle among them would leave a page behind
/// every round (`flui_testing::heap::assert_rounds_plateau`).
#[test]
fn a_material_app_navigated_and_dropped_again_and_again_holds_no_more_heap() {
    assert_rounds_plateau(|| {
        let router = Router::new(Mail::Inbox, |route: &Mail, _ctx| match route {
            Mail::Inbox => Inbox.boxed(),
            Mail::Message { id } => Message { id: *id }.boxed(),
        });
        let mut tester = Tester::mount(MaterialApp::router(router));
        tester.tap(&find::button("Open message 1"));
        tester.expect(find::text("Message 1")).to_exist();
        tester.tap(&find::button("Archive"));
        tester.expect(find::button("Archive it")).to_exist();
        tester.tap(&find::button("Archive it"));
        tester.expect(find::text("Archived")).to_exist();
    });
}
