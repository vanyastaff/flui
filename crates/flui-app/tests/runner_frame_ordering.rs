//! Every runner path drives the ONE frame transaction, `UiRealm::pump`.
//!
//! # Why a source scan, and what it is *not* evidence of
//!
//! The four frame sites in the `app/runner/` module (`desktop.rs`,
//! `android.rs`, `ios.rs`, `web.rs`) are `cfg`-gated: the desktop site
//! compiles on this host, the `wasm32` site under
//! `cargo check -p flui-app --target wasm32-unknown-unknown`, and the android
//! and iOS sites only under `cargo xtask cross-typecheck`. The pump's own
//! phase order is proven by `flui-runtime`'s `pump_transaction` tests; the
//! sites themselves only type-check off the host.
//!
//! This scan therefore proves exactly one thing, and it is a real thing: **no
//! frame site assembles a frame out of scheduler phases itself.** A site that
//! reintroduced `handle_draw_frame()` would drain post-frame callbacks before
//! its pipeline, and one that called `drive_frame` directly would bypass the
//! pump's command drain and frame clock; either turns this test red. It is a
//! regression guard, not a proof of the mobile bodies' runtime behavior.

/// Every source file of the `app/runner/` module, so the scans below cover
/// the whole runner regardless of which file a frame site lives in — a
/// fifth frame site added anywhere in the module is counted, not just one
/// appearing next to the existing four.
const RUNNER_SOURCES: &[&str] = &[
    include_str!("../src/app/runner/mod.rs"),
    include_str!("../src/app/runner/android.rs"),
    include_str!("../src/app/runner/desktop.rs"),
    include_str!("../src/app/runner/ios.rs"),
    include_str!("../src/app/runner/main_window.rs"),
    include_str!("../src/app/runner/device_recovery.rs"),
    include_str!("../src/app/runner/frame_pacing.rs"),
    include_str!("../src/app/runner/host.rs"),
    include_str!("../src/app/runner/realm_dispatch.rs"),
    include_str!("../src/app/runner/secondary_window.rs"),
    include_str!("../src/app/runner/web.rs"),
];

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

/// Lines of one `app/runner/` source file, excluding comments and excluding
/// whole `#[cfg(test)]`-gated modules.
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

/// The `app/runner/` module reaches a frame only through `UiRealm::pump`.
///
/// Red-check: change any site back to `scheduler.drive_frame_with_lane(...)`
/// around `render_frame`, or to `handle_begin_frame` + `handle_draw_frame`.
#[test]
fn every_runner_frame_site_drives_the_realm_pump() {
    let code_lines: Vec<&str> = RUNNER_SOURCES
        .iter()
        .flat_map(|source| production_lines(source))
        .collect();

    for banned in [
        "handle_begin_frame",
        "handle_draw_frame",
        "end_frame(",
        "drive_frame(",
        "drive_frame_with_lane(",
        "finish_async_pump(",
        "drive_async_tasks(",
    ] {
        assert!(
            !code_lines.iter().any(|l| l.contains(banned)),
            "the app/runner/ module calls `{banned}` directly in production code; every frame \
             site must go through `UiRealm::pump` (apply commands → begin → persistent → \
             pipeline → post-frame → idle) and every background wake through \
             `UiRealm::pump_background`"
        );
    }

    // The native sites (desktop, Android, iOS) pump through the device-
    // recovery wrapper; the wrapper and the web site call the pump with the
    // wake's sampled clock. The wrapper's definition is spelled
    // `pump_with_device_recovery<B>(`, so it is not counted as a call.
    let recovery_sites = code_lines
        .iter()
        .filter(|l| l.contains("pump_with_device_recovery("))
        .count();
    assert_eq!(
        recovery_sites, 3,
        "expected exactly three PRODUCTION `pump_with_device_recovery(` call sites (desktop, \
         Android, iOS); found {recovery_sites}"
    );
    let direct_pump_sites = code_lines
        .iter()
        .filter(|l| l.contains(".pump(&mut SampledClock"))
        .count();
    assert_eq!(
        direct_pump_sites, 2,
        "expected exactly two PRODUCTION `.pump(&mut SampledClock` sites (the device-recovery \
         wrapper and the web runner); found {direct_pump_sites}"
    );
}

/// Every background wake — each `WakeAction::PumpAsync` arm (desktop,
/// Android, iOS, web) and iOS's owner turn — must pump the async driver
/// through `UiRealm::pump_background`, which clears the `frame_scheduled`
/// latch before polling. Its order is pinned by `flui-runtime`'s
/// `pump_background_clears_the_frame_latch_before_polling`; this pins that
/// every arm reaches it. An arm that skips it silently stops a spawned future
/// from advancing while the app is backgrounded.
///
/// Red-check: delete the `realm.pump_background();` call from the desktop
/// `PumpAsync` arm and this fails (found 4, not 5).
#[test]
fn every_background_wake_calls_pump_background() {
    let code_lines: Vec<&str> = RUNNER_SOURCES
        .iter()
        .flat_map(|source| production_lines(source))
        .collect();

    let background_sites = code_lines
        .iter()
        .filter(|l| l.contains("pump_background("))
        .count();
    assert_eq!(
        background_sites, 5,
        "expected exactly five PRODUCTION `pump_background()` call sites (the `PumpAsync` arms \
         of desktop, Android, iOS and web, and iOS's owner turn); found {background_sites}"
    );
}

/// The native (desktop/Android/iOS) production frame paths must drive the
/// raster mailbox — every frame crossing to the backend as an owned,
/// stamped `SceneSnapshot` through `RasterLane` (ADR-0045's inline lane) —
/// never the pre-mailbox direct sink. Only the web runner still pumps
/// through a `DirectSink`, for a stated reason (its renderer arrives
/// asynchronously and recovers across an `.await`, a shape the lane does
/// not yet accommodate — see `DirectSink`'s doc in `raster_lane.rs`).
///
/// Red-check: revert any native bootstrap to `Arc<Mutex<Renderer>>` +
/// `DirectSink` and the corresponding count here breaks.
#[test]
fn native_frame_sites_drive_the_raster_mailbox_not_the_direct_backend() {
    // `main` split the flat `runner.rs` into the `runner/` module, so this
    // pin scans every file in it (the shared `RUNNER_SOURCES` list) rather
    // than one path — otherwise a native site that moved between submodules
    // would drop out of the count silently.
    let code_lines: Vec<&str> = RUNNER_SOURCES
        .iter()
        .flat_map(|source| production_lines(source))
        .collect();

    let lane_constructions = code_lines
        .iter()
        .filter(|l| l.contains("RasterLane::new("))
        .count();
    assert_eq!(
        lane_constructions, 3,
        "expected exactly three PRODUCTION `RasterLane::new(` sites (the desktop, Android and \
         iOS bootstraps each wrap their renderer in the raster mailbox); found \
         {lane_constructions}"
    );

    let direct_sink_sites = code_lines
        .iter()
        .filter(|l| l.contains("DirectSink::new("))
        .count();
    assert_eq!(
        direct_sink_sites, 1,
        "expected exactly one PRODUCTION `DirectSink::new(` site (the web runner's \
         direct-backend path, the only one with a stated reason to bypass the mailbox); \
         found {direct_sink_sites} — a second site means a native path regressed to the \
         pre-mailbox direct call"
    );
}
