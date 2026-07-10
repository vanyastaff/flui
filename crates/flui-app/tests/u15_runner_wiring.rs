//! ADR-0021 U1.5: every runner path drives the ONE shared frame ordering.
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
//! the bug U1.5 exists to remove — and this test would go red. It is a
//! regression guard, not a proof of the android body's runtime behavior. Stated
//! rather than implied.

/// `runner.rs` must reach the scheduler only through `drive_frame`.
///
/// Red-check: change any site back to `handle_begin_frame` + `handle_draw_frame`.
#[test]
fn every_runner_frame_site_uses_the_shared_drive_frame_helper() {
    const RUNNER: &str = include_str!("../src/app/runner.rs");

    let code_lines = || {
        RUNNER
            .lines()
            .map(str::trim_start)
            .filter(|l| !l.starts_with("//"))
    };

    for banned in ["handle_begin_frame", "handle_draw_frame", "end_frame("] {
        assert!(
            !code_lines().any(|l| l.contains(banned)),
            "runner.rs calls `{banned}` directly; every frame site must go through \
             `Scheduler::drive_frame`, which orders begin → persistent → pipeline \
             → post-frame → idle"
        );
    }

    let sites = code_lines().filter(|l| l.contains("drive_frame(")).count();
    assert_eq!(
        sites, 3,
        "expected exactly three frame sites (desktop, android, wasm); found {sites}"
    );
}
