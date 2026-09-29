//! Thin re-export of the canonical headless harness shared by every
//! widget-layer test suite.
//!
//! Historically this file was a copy of `flui-material`'s harness (itself a
//! copy of `flui-widgets`'), and the copies drifted: this crate's pointer
//! dispatch kept reusing `PointerId::PRIMARY` for every contact after the
//! canonical harness moved to a fresh pointer id per Down
//! (production-faithful — a platform never recycles an id into a
//! still-tracked gesture). The harness now lives once in
//! [`flui_testing::widgets`], so mount ordering, contact identity, and
//! virtual-clock policy are shared instead of forked. Note the dispatch
//! consequences: a contact Move/Up requires a preceding Down, and a
//! contactless hover is `dispatch_pointer_hover`, not
//! `dispatch_pointer_move`.

pub use flui_testing::widgets::*;

/// Runs every `(name, scenario)` row of a table-driven test. Every row runs,
/// and the failure report names each row that failed.
pub fn run_cases(cases: &[(&str, fn())]) {
    let mut failures = Vec::new();
    for (name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| {
                    payload
                        .downcast_ref::<&str>()
                        .map(|text| (*text).to_owned())
                })
                .unwrap_or_else(|| "non-string panic payload".to_owned());
            failures.push(format!("case `{name}` failed: {message}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
