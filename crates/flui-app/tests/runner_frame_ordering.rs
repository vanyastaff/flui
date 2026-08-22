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

/// Whether a (whitespace-concatenated) `#[cfg(...)]` attribute's text names
/// `test` as one of its predicates — anchored on the `(test,`/`(test)`/
/// `,test,`/`,test)` tokens a real `cfg` predicate list produces, not a bare
/// substring match. A bare `.contains("test")` would also fire on an
/// unrelated `feature = "latest"` — none of the anchored patterns below can
/// match inside a quoted string like that, since a quoted value never sits
/// directly against `(`/`,`/`)` the way an actual `cfg` predicate item does.
fn attr_names_test_predicate(attr_text: &str) -> bool {
    ["(test,", "(test)", ",test,", ",test)"]
        .iter()
        .any(|needle| attr_text.contains(needle))
}

/// Lines of `runner.rs`, excluding comments and excluding whole
/// `#[cfg(test)]`-gated modules.
///
/// A unit test may legitimately drive a throwaway `UpdateScheduler` directly
/// (`scheduler.drive_frame(...)`, `scheduler.drive_async_tasks()`) to prove
/// a lifecycle transition's effect — that is a test assertion, not a
/// production "frame site" hand-rolling anything. Excluding `#[cfg(test)]`
/// regions keeps the scans below scoped to what their own docs claim:
/// production runner paths, not test code that happens to call the same
/// methods it verifies.
///
/// Heuristic, not a parser: tracks brace depth per line and treats a
/// `#[cfg(...)]` attribute run (single- or multi-line, closing on a line
/// ending `]`) whose text names a `test` predicate (see
/// [`attr_names_test_predicate`]) as marking the *next* `mod` item as a test
/// region, excluded until its closing brace returns to the enclosing depth.
fn production_lines(source: &str) -> Vec<&str> {
    let mut depth: i32 = 0;
    let mut test_region_base_depth: Option<i32> = None;
    let mut in_attr = false;
    let mut attr_buf = String::new();
    let mut pending_test_cfg = false;
    let mut lines = Vec::new();

    for raw_line in source.lines() {
        let line = raw_line.trim_start();
        if line.starts_with("//") {
            continue;
        }

        if !in_attr && line.starts_with("#[") {
            in_attr = true;
            attr_buf.clear();
        }
        if in_attr {
            attr_buf.push_str(line);
            if line.ends_with(']') {
                in_attr = false;
                pending_test_cfg = attr_names_test_predicate(&attr_buf);
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
             through `UpdateScheduler::drive_frame`, which orders begin → persistent → pipeline → \
             post-frame → idle"
        );
    }

    // `drive_frame_with_lane` is the same shared helper, additionally
    // draining the realm's owner-local post-frame lane in the same total
    // order as the shared queue (drain-by-parameter, not an ambient lookup)
    // — every production frame site names it, not the bare `drive_frame`,
    // because every production site owns a `UiRealm` and its lane. Asserted
    // as two counts, not one OR'd count: an OR'd `== 3` stays green if a
    // production site regresses from `drive_frame_with_lane(` back to the
    // bare `drive_frame(` spelling (silently losing the lane's drain), since
    // the total site count would not change. `"drive_frame_with_lane(".
    // contains("drive_frame(")` is false (the literal text between them is
    // `_with_lane`, not `(`), so these two filters are already disjoint —
    // no site is double-counted.
    let with_lane_sites = code_lines
        .iter()
        .filter(|l| l.contains("drive_frame_with_lane("))
        .count();
    let bare_sites = code_lines
        .iter()
        .filter(|l| l.contains("drive_frame("))
        .count();
    assert_eq!(
        with_lane_sites, 3,
        "expected exactly three PRODUCTION frame sites (desktop, android, wasm) naming \
         drive_frame_with_lane(; found {with_lane_sites} — a unit test driving a throwaway \
         `UpdateScheduler` directly is excluded from this count"
    );
    assert_eq!(
        bare_sites, 0,
        "found {bare_sites} production site(s) still calling the bare drive_frame( instead of \
         drive_frame_with_lane( — that silently stops draining the realm's owner-local post-frame \
         lane"
    );
}

/// Every `WakeAction::PumpAsync` arm (desktop, Android, web) must actually
/// pump the async driver, and must consume the `frame_scheduled` latch
/// FIRST — see `UpdateScheduler::finish_async_pump`'s doc for why a `PumpAsync`
/// wake that skips either call silently starves a future indefinitely
/// (`drive_async_tasks` never runs = the future never advances;
/// `finish_async_pump` never runs = a LATER, independent wake finds the
/// latch already set and never re-fires `on_frame_scheduled`).
///
/// A per-arm assertion (not just "these two calls appear somewhere in the
/// file") would need a real parser; the site-count pin below is the same
/// mechanism `every_runner_frame_site_uses_the_shared_drive_frame_helper`
/// already uses for `drive_frame` sites, and is proven load-bearing the
/// same way: deleting either call from any ONE arm drops its count below 3.
///
/// Red-check: delete the `scheduler.drive_async_tasks();` call from the
/// desktop `PumpAsync` arm and this fails (found 2, not 3) — deleting that
/// one call from `run_desktop` alone otherwise passes every other test in
/// the suite, since nothing else exercises a real backgrounded frame loop.
#[test]
fn every_pump_async_arm_calls_finish_then_drive_async_tasks() {
    const RUNNER: &str = include_str!("../src/app/runner.rs");
    let code_lines = production_lines(RUNNER);

    let finish_sites = code_lines
        .iter()
        .filter(|l| l.contains("finish_async_pump("))
        .count();
    assert_eq!(
        finish_sites, 3,
        "expected exactly three PRODUCTION `finish_async_pump()` call sites (one per \
         `PumpAsync` arm: desktop, Android, web); found {finish_sites}"
    );

    let drive_sites = code_lines
        .iter()
        .filter(|l| l.contains("drive_async_tasks("))
        .count();
    assert_eq!(
        drive_sites, 3,
        "expected exactly three PRODUCTION `drive_async_tasks()` call sites in a `PumpAsync` \
         arm (desktop, Android, web); found {drive_sites} — a `PumpAsync` wake that never \
         drives the async tasks silently stops any spawned future from ever advancing while \
         the app is backgrounded"
    );
}

/// The native (desktop/Android) production frame paths must drive the
/// raster mailbox — every frame crossing to the backend as an owned,
/// stamped `SceneSnapshot` through `RasterLane` (ADR-0045's inline lane) —
/// never the pre-mailbox direct backend call. Only the web runner still
/// takes the direct entry point, for a stated reason (its renderer arrives
/// asynchronously and recovers across an `.await`, a shape the lane does
/// not yet accommodate — see `FrameSink`'s own doc in `raster_lane.rs`).
///
/// Red-check: revert either native bootstrap to `Arc<Mutex<Renderer>>` +
/// `render_frame_entered` and the corresponding count here breaks.
#[test]
fn native_frame_sites_drive_the_raster_mailbox_not_the_direct_backend() {
    const RUNNER: &str = include_str!("../src/app/runner.rs");
    let code_lines = production_lines(RUNNER);

    let lane_constructions = code_lines
        .iter()
        .filter(|l| l.contains("RasterLane::new("))
        .count();
    assert_eq!(
        lane_constructions, 2,
        "expected exactly two PRODUCTION `RasterLane::new(` sites (the desktop and Android \
         bootstraps each wrap their renderer in the raster mailbox); found \
         {lane_constructions}"
    );

    let direct_entry_sites = code_lines
        .iter()
        .filter(|l| l.contains(".render_frame_entered("))
        .count();
    assert_eq!(
        direct_entry_sites, 1,
        "expected exactly one PRODUCTION `.render_frame_entered(` site (the web runner's \
         direct-backend path, the only one with a stated reason to bypass the mailbox); \
         found {direct_entry_sites} — a second site means a native path regressed to the \
         pre-mailbox direct call"
    );

    let recovery_sites = code_lines
        .iter()
        .filter(|l| l.contains("render_frame_with_device_recovery("))
        .count();
    assert_eq!(
        recovery_sites, 2,
        "expected exactly two PRODUCTION `render_frame_with_device_recovery(` call sites \
         (desktop and Android, each handing their raster lane to the recovery wrapper); \
         found {recovery_sites}"
    );
}
