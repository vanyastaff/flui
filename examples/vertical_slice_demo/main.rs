//! Vertical-slice demo — mounts [`tree::DemoRoot`] (a counter, a scrollable
//! list, and an implicitly-animated box) through the real platform pipeline:
//! View → Element → RenderObject → layout/paint → `LayerTree` → `wgpu`.
//!
//! `tree.rs` is `#[path]`-included unchanged by `tests/vertical_slice_demo.rs`
//! at the workspace root, so the acceptance test exercises the exact tree
//! this binary runs.
//!
//! Run with: cargo run --example vertical_slice_demo
//!
//! Set `FLUI_FRAME_HISTOGRAM` (any value) to additionally drive a free-running
//! [`AnimationController`] and log wall-clock inter-tick histograms — see
//! [`frame_histogram`] for what this measures and why.
//!
//! No local `tracing_subscriber` init here, unlike some other examples:
//! `run_app` installs the process-global subscriber itself (`RUST_LOG`-aware,
//! `flui_app::app::runner::init_logging`) before its event loop starts, and
//! a second `set_global_default` call panics — the pattern every other
//! `run_app`-based example (`colored_box_app`, `animated_box_app`, …)
//! already follows.

#[path = "tree.rs"]
mod tree;

mod frame_histogram;

use flui_app::run_app;

fn main() {
    // `HistogramProbe::wrap` is a no-op wrapper (identical mounted behavior)
    // when `FLUI_FRAME_HISTOGRAM` is unset. When set, it mounts a
    // free-running controller registered with the ambient `VsyncScope` —
    // see `frame_histogram`'s module doc. Any of its own log lines emitted
    // before `run_app` installs the subscriber (above) are dropped by the
    // no-op default dispatcher — harmless, since the periodic histogram
    // windows it exists to report only start firing once the event loop
    // (and therefore the subscriber) is up.
    run_app(frame_histogram::HistogramProbe::wrap(tree::DemoApp));
}
