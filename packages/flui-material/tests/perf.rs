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

use std::fmt::Write as _;

use flui_material::InkWell;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::Text;
use flui_testing::control_cost::{ControlCost, control_over_text};
use flui_testing::heap::CountingAllocator;

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

/// What `control` adds to the `Text` it holds, per control, recorded as
/// `scenario` and held to its baseline file.
fn held_to_baseline(scenario: &str, control: fn(usize) -> BoxedView) -> ControlCost {
    let cost = control_over_text(control);
    let counts = cost.counts();
    record_counts(scenario, &counts);
    let baseline = baseline(scenario);
    for (name, value) in counts {
        let held = baseline
            .iter()
            .find(|(counter, _)| *counter == name)
            .map_or_else(
                || panic!("{scenario}.toml holds no `{name}`"),
                |(_, held)| *held,
            );
        assert!(
            value <= held,
            "{scenario}: {name} rose to {value} over the baseline's {held}"
        );
    }
    cost
}

/// The counters of `perf/baseline/<scenario>.toml`.
fn baseline(scenario: &str) -> Vec<(String, u64)> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("perf/baseline")
        .join(format!("{scenario}.toml"));
    let body = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    body.lines()
        .filter_map(|line| {
            let (name, value) = line.split_once('=')?;
            let value = value.trim().parse::<u64>().ok()?;
            Some((name.trim().to_owned(), value))
        })
        .collect::<Vec<(String, u64)>>()
}

/// Writes `counts` as `<FLUI_PERF_OUT>/<scenario>.toml` when that variable is set.
fn record_counts(scenario: &str, counts: &[(&str, u64)]) {
    let Some(dir) = std::env::var_os("FLUI_PERF_OUT") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("create FLUI_PERF_OUT");
    let mut body = format!("[{scenario}]\n");
    for (name, value) in counts {
        writeln!(body, "{name} = {value}").expect("writing to a String cannot fail");
    }
    std::fs::write(dir.join(format!("{scenario}.toml")), body).expect("write perf record");
}

/// What an `InkWell` adds to the `Text` it holds: its behaviours on the
/// interactive node, and its ink, an idle `CustomPaint`.
#[test]
fn perf_ink_well_over_text() {
    let cost = held_to_baseline("ink_well_over_text", |row| {
        InkWell::new()
            .child(Text::new(format!("Row {row}")))
            .on_tap(|_ctx| {})
            .boxed()
    });
    // The well's element, its node's element and the ink's; the node's
    // render object and the ink's.
    assert!(cost.elements <= 3, "a well adds {} elements", cost.elements);
    assert!(
        cost.render_nodes <= 2,
        "a well adds {} render nodes",
        cost.render_nodes
    );
}
