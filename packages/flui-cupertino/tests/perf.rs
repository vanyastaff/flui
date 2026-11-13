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

use flui_cupertino::CupertinoButton;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::Text;
use flui_testing::control_cost::{ControlCost, control_over_text};
use flui_testing::heap::CountingAllocator;

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// `cost` recorded as `scenario` and held to this catalog's baseline file.
fn held_to_baseline(scenario: &str, cost: ControlCost) -> ControlCost {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("perf/baseline");
    flui_testing::control_cost::held_to_baseline(&dir, scenario, cost)
}

/// What a `CupertinoButton` adds to the `Text` it holds: its behaviours and
/// its semantics on one interactive node, and its decoration (size
/// constraints, the press fade, the fill, padding, alignment and text style).
#[test]
fn perf_cupertino_button_over_text() {
    let cost = held_to_baseline(
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
