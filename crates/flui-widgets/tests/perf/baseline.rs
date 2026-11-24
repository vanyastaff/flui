//! The counts each perf scenario is held to: one file per scenario in
//! `crates/flui-widgets/perf/baseline/`, compared and blessed by
//! `flui_testing::perf::Baselines` as `FLUI_PERF_BASELINE` says.
//!
//! Unset, a scenario holds only its own budget assertions, which hold on
//! every host and with any feature set. CI's `test` job runs `check` on the
//! work and heap counters, on Linux and with the features and opt-level of
//! `flui_testing::perf::BLESS_COMMAND`, which is where they were blessed: the
//! heap counters move with the allocator's platform and with the features
//! that change what a frame builds. CI's `perf-gates` job checks and blesses
//! the `instructions` lines, in release under callgrind.

use flui_testing::perf::Baselines;

/// Every scenario that may hold a file, kept sorted.
const SCENARIOS: &[&str] = &[
    "caret_blink_in_10k_list",
    "filter_3000_keyed_rows",
    "filter_3000_memo_rows",
    "full_reassemble",
    "idle_frame_retained_scene",
    "label_change_kept",
    "label_change_large",
    "label_change_small",
    "list_10k_scroll_one_screen",
    "list_10k_scroll_warm",
    "not_shown_retry",
    "raw_button_over_text",
    "semantics_action_over_text",
    "slide_transition_tick",
    "texture_frame",
];

/// This target's baseline files.
pub(crate) fn baselines() -> Baselines {
    Baselines::new(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("perf/baseline"),
        SCENARIOS,
    )
}

/// The directory holds a file for each scenario of [`SCENARIOS`] and no other,
/// and each one parses.
#[test]
fn every_baseline_file_is_a_scenario() {
    baselines().assert_files_are_scenarios();
}
