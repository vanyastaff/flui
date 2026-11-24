//! What a Cupertino control costs to mount, counted rather than timed.
//!
//! The control-cost scenarios of `crates/flui-widgets/tests/perf.rs` and
//! `packages/flui-material/tests/perf.rs`, for the controls of this catalog,
//! measured by `flui_testing::control_cost`: each one mounts `MOUNT_ROWS`
//! controls over a `Text` and records, per control, what it adds to the
//! `Text` it holds. ADR-0322 sets the threshold a control on the catalogs'
//! interactive node stays within: 2 elements and 1 render node for its
//! behaviours, beside the elements of its own decoration.
//!
//! The counters are held by `perf/baseline/<scenario>.toml`, which only
//! falls. `FLUI_PERF_OUT=<dir>` writes each record to `<dir>/<scenario>.toml`
//! before asserting, and a lower record is copied into the baseline by hand.
//!
//! A target of its own, not a module of `tests/main.rs`: it pins the
//! process-global font system and installs the kit's counting global
//! allocator.

use flui_cupertino::{CupertinoButton, CupertinoTabBar, CupertinoTabBarItem};
use flui_sdk::view::environment::{MediaQuery, MediaQueryData};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{Icon, IconData, Text};
use flui_testing::control_cost::{ControlCost, control_alone_under, control_over_text};
use flui_testing::heap::CountingAllocator;
use flui_testing::perf::{Baselines, Held};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

const TAB_BAR_ELEMENTS: u64 = 35;
const TAB_BAR_RENDER_NODES: u64 = 19;

/// Every scenario that holds a file, kept sorted.
const SCENARIOS: &[&str] = &["cupertino_button_over_text", "cupertino_tab_bar_alone"];

fn baselines() -> Baselines {
    Baselines::new(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("perf/baseline"),
        SCENARIOS,
    )
}

/// `cost` held to this catalog's baseline file as `scenario`; the hold is
/// kept to the end of the test, so a bless waits for its assertions.
fn held_to_baseline(scenario: &str, cost: ControlCost) -> (ControlCost, Held) {
    let held = baselines().hold(scenario, &cost.counts());
    (cost, held)
}

/// The baseline directory holds a file for each scenario and no other.
#[test]
fn every_baseline_file_is_a_scenario() {
    baselines().assert_files_are_scenarios();
}

/// What a `CupertinoButton` adds to the `Text` it holds: its behaviours and
/// its semantics on one interactive node, and its decoration (size
/// constraints, the press fade, the fill, padding, alignment and text style).
#[test]
fn perf_cupertino_button_over_text() {
    let (cost, _baseline) = held_to_baseline(
        "cupertino_button_over_text",
        control_over_text(|row| {
            CupertinoButton::new(Text::new(format!("Row {row}")))
                .on_pressed(|_ctx| {})
                .boxed()
        }),
    );
    // The button and its node, and the decoration's elements; no
    // `Semantics` or `GestureDetector` of its own (13 elements and 8 render
    // nodes when it had them).
    assert!(
        cost.elements <= 9,
        "a button adds {} elements",
        cost.elements
    );
}

/// What a `CupertinoTabBar` of two items costs in place of an empty box,
/// under a `MediaQuery`: the bar's backdrop filter, decoration and row, and per item its
/// icon and label on one interactive node that carries the item's
/// selection and tap.
#[test]
fn perf_cupertino_tab_bar_alone() {
    let (cost, _baseline) = held_to_baseline(
        "cupertino_tab_bar_alone",
        control_alone_under(
            |column| MediaQuery::new(MediaQueryData::default(), column).boxed(),
            |_row| {
                CupertinoTabBar::new(vec![
                    CupertinoTabBarItem::new(Icon::new(IconData::new(0xF3A1))).label("Home"),
                    CupertinoTabBarItem::new(Icon::new(IconData::new(0xF3A2))).label("Settings"),
                ])
                .on_tap(|_ctx, _index| {})
                .boxed()
            },
        ),
    );
    assert!(
        cost.elements <= TAB_BAR_ELEMENTS,
        "a tab bar adds {} elements",
        cost.elements
    );
    assert!(
        cost.render_nodes <= TAB_BAR_RENDER_NODES,
        "a tab bar adds {} render nodes",
        cost.render_nodes
    );
}
