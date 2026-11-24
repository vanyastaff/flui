//! Runtime developer tooling for FLUI: the half that runs *inside* the
//! application. Small, feature-gated modules, each an adapter over a seam the
//! framework already exposes, so nothing here reaches into a widget, element
//! or render tree:
//!
//! - **`profiling`** — [`Profiler`] with per-phase frame statistics, fed by
//!   [`FrameTimingLayer`], a `tracing` layer that subscribes to the `frame`
//!   span `UpdateScheduler::drive_frame` opens and the `build`/`layout`/
//!   `paint`/`compositing` spans the pipeline emits inside it.
//! - **`timeline`** — [`Timeline`], an event recorder with Chrome
//!   trace export and a bridge from the scheduler's `FrameSnapshot`s.
//! - **`inspector`** — [`GestureInspector`], which reports
//!   what a gesture arena decided per contest through `GestureObserver`, and
//!   [`GestureInspectorOverlay`], a debug overlay that draws
//!   those contests over the app, and [`InspectorCounters`], a counting
//!   `TreeObserver` over the ADR-0040 observation seam: mounts, moves,
//!   rebuilds per cause, unmounts.
//!
//! # What this crate is not
//!
//! Its one on-screen tool is the gesture overlay; it has no widget-tree
//! inspector UI and is not a hot-reload tool or a driver agent (that is
//! `flui-app`'s, ADR-0239): it opens no endpoint, walks no
//! tree itself, and watches no files (the source watcher lives in the `flui`
//! CLI). It has no network monitor, memory profiler or remote-debug protocol.
//!
//! # Profiling a running app
//!
//! ```no_run
//! use std::sync::Arc;
//!
//! use flui_devtools::{FrameTimingLayer, Profiler};
//! use flui_log::config::LogConfig;
//! use flui_log::ownership::{InstallPolicy, LogBridgePolicy, install_subscriber};
//! use tracing_subscriber::layer::SubscriberExt;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let profiler = Arc::new(Profiler::new());
//! // FLUI's own log subscriber, with the profiler layer beside it. The layer
//! // carries its own filter, so the log filter's level does not affect it.
//! let subscriber = LogConfig::default()
//!     .subscriber()?
//!     .with(FrameTimingLayer::new(Arc::clone(&profiler)));
//! install_subscriber(subscriber, InstallPolicy::Auto, LogBridgePolicy::Auto)?;
//! // ...run the app; `flui-app` inherits an installed subscriber...
//! if let Some(frame) = profiler.frame_stats() {
//!     println!("last frame: {:.2} ms", frame.total_time_ms());
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Feature flags
//!
//! The first three are on by default; disable what you do not need.
//!
//! - `profiling`: [`Profiler`] and [`FrameTimingLayer`] (`tracing` +
//!   `tracing-subscriber`).
//! - `timeline`: [`Timeline`] and [`TimelineEvent`] (`flui_sdk::hooks::FrameSnapshot` for the
//!   scheduler bridge, `serde` for the exporters).
//! - `inspector`: [`InspectorCounters`] (`flui_sdk::foundation::observe` for the
//!   seam), [`GestureInspectorOverlay`] (`flui_sdk::widgets`) and [`GestureInspector`] (`flui_sdk::interaction`'s arena
//!   observation).
//! - `full`: all of the above, as one name for feature-matrix runs.

#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "test code catches a panic to assert on it (ADR-0303 §5)"
    )
)]
// Every public item is documented; keep it that way.
#![deny(missing_docs)]
#![warn(missing_debug_implementations)]

#[cfg(feature = "profiling")]
mod capture;
// Feeds the profiler from the framework's own frame spans: the only seam
// layering permits, since nothing in the framework may depend on this crate.
#[cfg(feature = "profiling")]
mod frame_timing_layer;
#[cfg(feature = "inspector")]
mod gesture_inspector;
#[cfg(feature = "inspector")]
mod gesture_overlay;
#[cfg(feature = "inspector")]
mod inspector;
#[cfg(feature = "profiling")]
mod phase;
#[cfg(feature = "profiling")]
mod profiler;
#[cfg(feature = "timeline")]
mod timeline;

#[cfg(feature = "profiling")]
pub use capture::{
    Capture, CaptureLayer, CapturedFrame, CapturedSpan, FrameOpening, FrameSpanFilter, Partition,
};
#[cfg(feature = "profiling")]
pub use frame_timing_layer::FrameTimingLayer;
#[cfg(feature = "inspector")]
pub use gesture_inspector::{
    CompositionStep, ContestReport, DEFAULT_CONTEST_CAPACITY, DeadlineReport, GestureInspector,
    InspectedComposition, InspectedMember, MemberReport, Verdict,
};
#[cfg(feature = "inspector")]
pub use gesture_overlay::{
    CONTEST_FADE, GestureInspectorOverlay, GestureInspectorOverlayState, GestureOverlayController,
};
#[cfg(feature = "inspector")]
pub use inspector::{InspectorCounters, InspectorSnapshot};
#[cfg(feature = "profiling")]
pub use phase::Phase;
#[cfg(feature = "profiling")]
pub use profiler::{
    FramePhase, FrameStats, JankLine, PhaseGuard, PhaseInfo, Profiler, ProfilerConfig,
};
#[cfg(feature = "timeline")]
pub use timeline::{EventCategory, EventGuard, Timeline, TimelineEvent};
