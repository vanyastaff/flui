//! Every runner path drives the ONE shared frame ordering.
//!
//! # Why a source scan, and what it is *not* evidence of
//!
//! The three frame sites in `app/runner.rs` are `cfg`-gated: the desktop site
//! compiles on this host, the `wasm32` site compiles under
//! `cargo check -p flui-app --target wasm32-unknown-unknown` (run for this
//! change), and the **android site needs the NDK and was not compiled**. The
//! desktop site's runtime behavior is proven by
//! `production_post_frame_callback_observes_this_frames_committed_layout` in
//! `flui-app`'s unit tests; the wasm site only type-checks; the android site
//! neither.
//!
//! This scan therefore proves exactly one thing, and it is a real thing: **no
//! frame site hand-rolls the begin/draw/end sequence.** A site that reintroduced
//! `handle_draw_frame()` would drain post-frame callbacks before its pipeline —
//! the bug this test exists to catch — and this test would go red. It is a
//! regression guard, not a proof of the android body's runtime behavior. Stated
//! rather than implied.

/// Lines of `runner.rs`, excluding comments and excluding whole
/// `#[cfg(test)]`-gated modules.
///
/// ADR-0035 PR2 added a unit test that drives a throwaway `Scheduler`
/// directly (`scheduler.drive_frame(...)`) to prove a `Resumed` transition
/// produces exactly one frame — a legitimate test assertion, not a
/// production "frame site" hand-rolling anything. Excluding `#[cfg(test)]`
/// regions keeps the scan below scoped to what its own doc claims:
/// production runner paths, not test code that happens to call the same
/// method it verifies.
///
/// Heuristic, not a parser: tracks brace depth per line and treats a
/// `#[cfg(...)]` attribute run (single- or multi-line, closing on a line
/// ending `]`) whose text contains `test` as marking the *next* `mod` item
/// as a test region, excluded until its closing brace returns to the
/// enclosing depth.
fn production_lines(source: &str) -> Vec<&str> {
    let mut depth: i32 = 0;
    let mut test_region_base_depth: Option<i32> = None;
    let mut in_attr = false;
    let mut attr_mentions_test = false;
    let mut pending_test_cfg = false;
    let mut lines = Vec::new();

    for raw_line in source.lines() {
        let line = raw_line.trim_start();
        if line.starts_with("//") {
            continue;
        }

        if !in_attr && line.starts_with("#[") {
            in_attr = true;
            attr_mentions_test = false;
        }
        if in_attr {
            attr_mentions_test |= line.contains("test");
            if line.ends_with(']') {
                in_attr = false;
                pending_test_cfg = attr_mentions_test;
            }
            continue;
        }

        if pending_test_cfg && line.starts_with("mod ") {
            test_region_base_depth = Some(depth);
        }
        pending_test_cfg = false;

        if test_region_base_depth.is_none() {
            lines.push(line);
        }

        depth += line.matches('{').count() as i32 - line.matches('}').count() as i32;

        if let Some(base) = test_region_base_depth
            && depth <= base
        {
            test_region_base_depth = None;
        }
    }

    lines
}

/// `runner.rs` must reach the scheduler only through `drive_frame`.
///
/// Red-check: change any site back to `handle_begin_frame` + `handle_draw_frame`.
#[test]
fn every_runner_frame_site_uses_the_shared_drive_frame_helper() {
    const RUNNER: &str = include_str!("../src/app/runner.rs");
    let code_lines = production_lines(RUNNER);

    for banned in ["handle_begin_frame", "handle_draw_frame", "end_frame("] {
        assert!(
            !code_lines.iter().any(|l| l.contains(banned)),
            "runner.rs calls `{banned}` directly in production code; every frame site must go \
             through `Scheduler::drive_frame`, which orders begin → persistent → pipeline → \
             post-frame → idle"
        );
    }

    let sites = code_lines
        .iter()
        .filter(|l| l.contains("drive_frame("))
        .count();
    assert_eq!(
        sites, 3,
        "expected exactly three PRODUCTION frame sites (desktop, android, wasm); found {sites} \
         — a unit test driving a throwaway `Scheduler` directly is excluded from this count"
    );
}
