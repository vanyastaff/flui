//! The composition an app actually ships: FLUI's log subscriber (an `INFO`
//! filter by default) with the profiler layer beside it.
//!
//! The frame spans are `DEBUG`. Without a filter of its own the profiler
//! would either be starved by the log filter, or — as an unfiltered layer —
//! declare interest in every callsite and drag every `DEBUG`/`TRACE` event in
//! the process into the registry the moment profiling starts. Both halves are
//! asserted here: the profiler sees the frame, and an unrelated `DEBUG` event
//! stays disabled.
#![cfg(feature = "profiling")]

use std::sync::Arc;

use flui_devtools::profiler::FramePhase;
use flui_devtools::{FrameTimingLayer, Profiler};
use flui_log::{FilterConfig, LogConfig};
use tracing::{Dispatch, Level};
use tracing_subscriber::layer::SubscriberExt;

#[test]
fn the_profiler_sees_debug_frame_spans_through_an_info_log_filter() {
    // Deliberately NOT `flui_testing::log_capture::disarm_interest_cache()`.
    // That sentinel forces every callsite's interest to `sometimes`, and on
    // that path a registry with per-layer filters answers `enabled` with
    // `true` even when every filter rejected the event, which would turn the
    // interest assertion below into a false failure. This binary holds one
    // test, so no sibling can poison the cache it is checking.

    let profiler = Arc::new(Profiler::new());
    // `without_env_var`: a developer's `FLUI_LOG=debug` must not decide
    // this test's outcome.
    let config = LogConfig::builder()
        .filter(FilterConfig::new("info").without_env_var())
        .build();
    let subscriber = config
        .subscriber()
        .expect("the literal `info` directive parses")
        .with(FrameTimingLayer::new(Arc::clone(&profiler)));

    tracing::dispatcher::with_default(&Dispatch::new(subscriber), || {
        assert!(
            !tracing::event_enabled!(Level::DEBUG),
            "an unrelated DEBUG event must stay disabled: the profiler admits frame spans \
             only, so attaching it must not switch on debug logging process-wide"
        );
        {
            let _frame = tracing::debug_span!("frame").entered();
            let _build = tracing::debug_span!("build").entered();
        }
    });

    let stats = profiler
        .frame_stats()
        .expect("a DEBUG frame span must reach the profiler past an INFO log filter");
    assert!(
        stats
            .phases
            .iter()
            .any(|info| info.phase == FramePhase::Build),
        "the build phase inside it must be attributed; saw {:?}",
        stats.phases
    );
}
