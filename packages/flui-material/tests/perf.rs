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
//! A target of its own, not a module of `tests/main.rs`: it pins the
//! process-global font system and installs the kit's counting global
//! allocator.

use flui_material::{Checkbox, ElevatedButton, InkWell, Radio, Switch, Theme, ThemeData};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::Text;
use flui_testing::control_cost::{
    ControlCost, control_alone_under, control_over_text, control_over_text_under,
};
use flui_testing::heap::CountingAllocator;

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
