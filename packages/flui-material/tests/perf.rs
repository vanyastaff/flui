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

use flui_material::{
    Checkbox, Chip, ElevatedButton, FilterChip, InkWell, ListTile, NavigationBar,
    NavigationDestination, Radio, Switch, Tab, TabBar, TabController, Theme, ThemeData,
};
use flui_sdk::view::environment::{MediaQuery, MediaQueryData};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::icon::IconData;
use flui_sdk::widgets::{Icon, Text};
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

/// What a `ListTile` adds to the `Text` it titles, under a theme and a media
/// query: its constraints, surface, safe area, icon theme and row, and its
/// well, whose node carries the tile's description and switch, with no
/// `Semantics` element of the tile's own.
#[test]
fn perf_list_tile_over_text() {
    let cost = held_to_baseline(
        "list_tile_over_text",
        control_over_text_under(
            |column| {
                Theme::new(
                    ThemeData::light(),
                    MediaQuery::new(MediaQueryData::default(), column),
                )
                .boxed()
            },
            |row| {
                ListTile::new()
                    .title(Text::new(format!("Row {row}")))
                    .on_tap(|_ctx| {})
                    .boxed()
            },
        ),
    );
    // 18 elements and 9 render nodes while the tile wrapped its content in a
    // `Semantics` of its own.
    assert!(
        cost.elements <= 17,
        "a list tile adds {} elements",
        cost.elements
    );
    assert!(
        cost.render_nodes <= 8,
        "a list tile adds {} render nodes",
        cost.render_nodes
    );
}
