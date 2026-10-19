//! The alternatives to a swipe that a `TabBarView` offers (ADR-0216): its
//! commands step the selection, and the single-pointer path of them is the
//! `TabBar` on the same controller, which registers on the controller's
//! companion link while it is mounted.

use crate::common::{LaidOut, lay_out, tight};

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use flui_material::{Tab, TabBar, TabBarView, TabController, Theme, ThemeData};
use flui_sdk::geometry::Offset;
use flui_sdk::platform::TargetPlatform;
use flui_sdk::view::BoxedView;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{
    Alternative, Column, CompanionGap, CompanionSpec, Expanded, GestureCommand, SizedBox,
};
use flui_testing::a11y::{Action, ActionData, ActionRequest, Role, TreeId};
use flui_testing::log_capture::capture;
use flui_testing::replay::MouseScript;
use flui_testing::{AlternativesSubject, GestureAlternativesCheck, assert_gesture_alternatives};

const SIZE: f64 = 400.0;

fn three_tabs() -> Vec<Tab> {
    ["One", "Two", "Three"]
        .into_iter()
        .map(|label| Tab::new().text(label))
        .collect()
}

/// The pager over three pages, sharing `controller`.
fn pager(controller: &TabController) -> BoxedView {
    let pages = (0..3)
        .map(|_| SizedBox::expand().into_view().boxed())
        .collect();
    Expanded::new(TabBarView::new(pages).controller(controller.clone())).boxed()
}

/// The bar on `controller`, when `with_bar`, over the pager.
fn tabs(controller: &TabController, with_bar: bool) -> impl View {
    let mut column = Vec::new();
    if with_bar {
        column.push(
            TabBar::secondary(three_tabs())
                .controller(controller.clone())
                .boxed(),
        );
    }
    column.push(pager(controller));
    Theme::new(ThemeData::light(), Column::new(column))
}

/// What a step changes: the selected tab, and where the pager rests, in
/// thousandths of a page.
fn selection(controller: &TabController) -> (usize, i64) {
    (
        controller.index(),
        (controller.position().value() * 1000.0).round() as i64,
    )
}

/// A swipe toward the reading start, slow enough to rest on the page it
/// reaches: it performs "Previous tab".
fn swipe_back() -> flui_testing::replay::PointerScript {
    let mut swipe = MouseScript::new("swipe back").press(Offset::new(60.0, 250.0));
    for step in 1..=10 {
        swipe = swipe
            .wait(Duration::from_millis(16))
            .move_to(Offset::new(60.0 + 30.0 * f64::from(step), 250.0));
    }
    swipe
        .wait(Duration::from_millis(300))
        .release()
        .build()
        .expect("one whole press")
}

fn check(with_bar: bool) -> GestureAlternativesCheck<(usize, i64)> {
    GestureAlternativesCheck::new(
        if with_bar {
            "TabBarView / with its TabBar"
        } else {
            "TabBarView / without a TabBar"
        },
        tight(SIZE, SIZE),
        swipe_back(),
        move || {
            // On the middle tab, so both commands step.
            let controller = TabController::new(3, 1);
            let alternatives = controller.step_alternatives();
            let model = controller.clone();
            AlternativesSubject::new(tabs(&controller, with_bar), &alternatives, move || {
                selection(&model)
            })
        },
    )
}

/// The a11y warning a mounted subject leaves.
fn companion_warnings(with_bar: bool) -> usize {
    let controller = TabController::new(3, 1);
    let ((), log) = capture(|| {
        let mut laid = lay_out(tabs(&controller, with_bar), tight(SIZE, SIZE));
        laid.pump();
    });
    log.records()
        .iter()
        .filter(|record| {
            record.target == "flui::a11y" && record.message.contains("none is registered")
        })
        .count()
}

/// Matrix row: a `TabBarView` beside its `TabBar`. A swipe toward the reading
/// start selects the previous tab, as "Previous tab" does; each command is
/// reached by assistive technology, by its key and by the adjacent tab of the
/// bar with each pointer, to the same tab and page.
pub(crate) fn tab_bar_view_with_its_tab_bar() {
    assert_gesture_alternatives(&check(true));
    let controller = TabController::new(3, 1);
    for command in controller.step_alternatives().commands() {
        assert_eq!(
            command.keys(TargetPlatform::current()).count(),
            1,
            "{:?} is bound to its key",
            command.label()
        );
    }
    let mut laid = lay_out(tabs(&controller, true), tight(SIZE, SIZE));
    laid.enable_semantics();
    laid.pump();
    let tree = laid.a11y_tree().expect("semantics enabled");
    assert_eq!(
        tree.find_all(Role::TabList).len(),
        1,
        "the bar is a tab list"
    );
    let selected: Vec<bool> = tree
        .find_all(Role::Tab)
        .iter()
        .map(|tab| tab.raw().is_selected() == Some(true))
        .collect();
    assert_eq!(
        selected,
        [false, true, false],
        "each tab is a node, the second selected"
    );
    assert_eq!(
        companion_warnings(true),
        0,
        "a bar mounted in the same frame fulfils the alternatives"
    );
}

/// Matrix row: a `TabBarView` with no `TabBar` on its controller claims a
/// single-pointer path that is not there. The check rejects it, naming the
/// commands, and mounting it warns on `flui::a11y`.
pub(crate) fn tab_bar_view_without_a_tab_bar() {
    let payload = catch_unwind(AssertUnwindSafe(|| {
        assert_gesture_alternatives(&check(false));
    }))
    .expect_err("a pager whose companion is missing fails the check");
    let message = payload
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_default();
    for label in ["Previous tab", "Next tab"] {
        assert!(
            message.contains(&format!(
                "\"{label}\": the alternatives declare a companion, but no companion is registered on the link"
            )),
            "{label}: the missing companion is named:\n{message}"
        );
    }
    assert_eq!(
        message.matches("by assistive technology: ").count(),
        0,
        "the commands that are there still conform:\n{message}"
    );
    assert_eq!(
        companion_warnings(false),
        1,
        "mounting the view without a bar warns once"
    );
}

/// The companion spec of `controller`'s step alternatives and its command
/// labelled `label`.
fn step(controller: &TabController, label: &str) -> (CompanionSpec, GestureCommand) {
    let alternatives = controller.step_alternatives();
    let Alternative::Companion(spec) = alternatives.alternative().clone() else {
        panic!("the step alternatives rely on a companion");
    };
    let command = alternatives
        .commands()
        .find(|command| command.label() == label)
        .expect("the step command")
        .clone();
    (spec, command)
}

/// Invoke `label` as assistive technology's custom action on the pager's row.
fn invoke_custom_action(laid: &mut LaidOut, label: &str) {
    let tree = laid.a11y_tree().expect("semantics enabled");
    let (target, id) = tree
        .nodes()
        .find_map(|node| {
            node.raw()
                .custom_actions()
                .iter()
                .find(|action| action.description == label)
                .map(|action| (node.id(), action.id))
        })
        .expect("the pager's row offers the command");
    laid.invoke_semantics_action(ActionRequest {
        action: Action::CustomAction,
        target_tree: TreeId::ROOT,
        target_node: target,
        data: Some(ActionData::CustomAction(id)),
    })
    .expect("the row takes its custom action");
    for _ in 0..60 {
        laid.pump_for(Duration::from_millis(16));
    }
}

/// At the first tab "Previous tab" and at the last "Next tab" have no tab to
/// select: the bar publishes no node for them and the intent is disabled, so
/// invoking them leaves the selection and the pager alone, while the command
/// that does step still moves both.
pub(crate) fn the_end_tabs_have_no_step_beyond_them() {
    for (index, blocked, open) in [
        (0, "Previous tab", "Next tab"),
        (2, "Next tab", "Previous tab"),
    ] {
        let controller = TabController::new(3, index);
        let mut laid = lay_out(tabs(&controller, true), tight(SIZE, SIZE));
        laid.enable_semantics();
        laid.pump();
        let at_rest = selection(&controller);

        let (spec, command) = step(&controller, blocked);
        assert_eq!(
            spec.node_for(&command),
            Err(CompanionGap::Unavailable),
            "tab {index}: the bar has no node for {blocked:?}"
        );
        let (spec, command) = step(&controller, open);
        assert!(
            spec.node_for(&command).is_ok(),
            "tab {index}: the bar publishes the adjacent tab for {open:?}"
        );

        invoke_custom_action(&mut laid, blocked);
        assert_eq!(
            selection(&controller),
            at_rest,
            "tab {index}: {blocked:?} is disabled"
        );
        invoke_custom_action(&mut laid, open);
        assert_ne!(
            selection(&controller),
            at_rest,
            "tab {index}: {open:?} steps"
        );
    }
}

/// The bar registers while it is mounted: unmounting it unregisters it, and
/// mounting another registers that one.
pub(crate) fn unmounting_the_tab_bar_unregisters_it() {
    let controller = TabController::new(3, 1);
    let (spec, command) = step(&controller, "Next tab");
    // The pager stays first, so only the bar after it comes and goes.
    let root = |with_bar: bool| {
        let mut column = vec![pager(&controller)];
        if with_bar {
            column.push(
                TabBar::secondary(three_tabs())
                    .controller(controller.clone())
                    .boxed(),
            );
        }
        Theme::new(ThemeData::light(), Column::new(column))
    };
    let mut laid = lay_out(root(true), tight(SIZE, SIZE));
    laid.enable_semantics();
    laid.pump();
    assert!(spec.node_for(&command).is_ok(), "the mounted bar is found");

    laid.pump_widget(root(false));
    laid.pump();
    assert_eq!(
        spec.node_for(&command),
        Err(CompanionGap::Unregistered),
        "an unmounted bar is no companion"
    );

    laid.pump_widget(root(true));
    laid.pump();
    assert!(spec.node_for(&command).is_ok(), "a new bar registers");
}

/// Replacing the controller under a mounted pager and bar moves both to the
/// new one: its steps work, the old one's are no longer fulfilled, and the old
/// controller keeps the tab it had.
pub(crate) fn replacing_the_controller_moves_the_pager_and_bar_to_the_new_one() {
    let old = TabController::new(3, 0);
    let new = TabController::new(3, 2);
    let mut laid = lay_out(tabs(&old, true), tight(SIZE, SIZE));
    laid.enable_semantics();
    laid.pump();
    invoke_custom_action(&mut laid, "Next tab");
    assert_eq!(old.index(), 1, "the old controller steps while mounted");

    laid.pump_widget(tabs(&new, true));
    laid.pump();
    let (old_spec, old_command) = step(&old, "Next tab");
    assert_eq!(
        old_spec.node_for(&old_command),
        Err(CompanionGap::Unregistered),
        "the replaced controller has no bar"
    );
    let (new_spec, new_command) = step(&new, "Previous tab");
    assert!(
        new_spec.node_for(&new_command).is_ok(),
        "the new controller has the bar"
    );

    invoke_custom_action(&mut laid, "Previous tab");
    assert_eq!(new.index(), 1, "the new controller steps");
    assert_eq!(old.index(), 1, "the old controller is left where it was");
}
