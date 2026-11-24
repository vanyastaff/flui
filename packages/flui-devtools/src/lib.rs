//! Runtime developer tooling for FLUI: the half that runs *inside* the
//! application. Small, feature-gated modules, each an adapter over a seam the
//! framework already exposes, so nothing here reaches into a widget, element
//! or render tree:
//!
//! - **`profiling`** — [`Capture`], the one recorder of the framework's frame
//!   spans (its [`layer`](Capture::layer) subscribes to the `frame` span
//!   `UpdateScheduler::drive_frame` opens and the [`Phase`] spans the pipeline
//!   emits inside it), and [`Profiler`], per-frame statistics read from a capture.
//! - **`timeline`** — [`Timeline`], an event recorder with Chrome
//!   trace export and a bridge from the scheduler's `FrameSnapshot`s; over a
//!   capture it shows the framework's frames and phases beside the application's events.
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
//! use flui_devtools::{Capture, Profiler, ProfilerConfig};
//! use flui_log::config::LogConfig;
//! use flui_log::ownership::{InstallPolicy, LogBridgePolicy, install_subscriber};
//! use tracing_subscriber::layer::SubscriberExt;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let capture = Capture::new();
//! // FLUI's own log subscriber, with the capture layer beside it. The layer
//! // carries its own filter, so the log filter's level does not affect it.
//! let subscriber = LogConfig::default().subscriber()?.with(capture.layer());
//! install_subscriber(subscriber, InstallPolicy::Auto, LogBridgePolicy::Auto)?;
//! let profiler = Profiler::over(&capture, ProfilerConfig::default());
//! // ...run the app; `flui-app` inherits an installed subscriber...
//! if let Some(frame) = profiler.frame_stats() {
//!     println!("{frame}");
//! }
//! if let Some(rate) = profiler.frame_rate() {
//!     println!("{rate:.1} frames per second");
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Feature flags
//!
//! The first three are on by default; disable what you do not need.
//!
//! - `profiling`: [`Capture`] and [`Profiler`] (`tracing` + `tracing-subscriber`).
//! - `timeline`: [`Timeline`], [`TimelineEvent`] and [`TimelineEventKind`]
//!   (`flui_sdk::hooks::FrameSnapshot` for the scheduler bridge, `serde` for the exporters);
//!   with `profiling`, [`Timeline::over`] reads a capture.
//! - `inspector`: [`InspectorCounters`] (`flui_sdk::foundation::observe` for the
//!   seam), [`GestureInspectorOverlay`] (`flui_sdk::widgets`) and [`GestureInspector`] (`flui_sdk::interaction`'s arena
//!   observation).
//! - `full`: all of the above, as one name for feature-matrix runs.
//!
//! [`Phase`] comes with `profiling` or `timeline`.

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
// A table keyed by ids the process mints hashes with FxHash, not SipHash.
#![warn(clippy::disallowed_types)]

// Reads the framework's own frame spans: the only seam layering permits, since nothing in the
// framework may depend on this crate.
#[cfg(feature = "profiling")]
mod capture;
#[cfg(feature = "inspector")]
mod gesture_inspector;
#[cfg(feature = "inspector")]
mod gesture_overlay;
#[cfg(feature = "inspector")]
mod inspector;
#[cfg(any(feature = "profiling", feature = "timeline"))]
mod phase;
#[cfg(feature = "profiling")]
mod profiler;
#[cfg(feature = "timeline")]
mod timeline;

#[cfg(feature = "profiling")]
pub use capture::{
    Capture, CaptureLayer, CaptureLimits, CaptureSubscription, CapturedFrame, CapturedSpan,
    FrameSpanFilter, Partition,
};
#[cfg(feature = "inspector")]
pub use gesture_inspector::{
    CompositionStep, ContestReport, DEFAULT_MAX_CONTESTS, DeadlineReport, GestureInspector,
    InspectedComposition, InspectedMember, MemberReport, Verdict,
};
#[cfg(feature = "inspector")]
pub use gesture_overlay::{
    CONTEST_FADE, GestureInspectorOverlay, GestureInspectorOverlayState, GestureOverlayController,
};
#[cfg(feature = "inspector")]
pub use inspector::{InspectorCounters, InspectorSnapshot};
#[cfg(any(feature = "profiling", feature = "timeline"))]
pub use phase::Phase;
#[cfg(feature = "profiling")]
pub use profiler::{FrameStats, JankLine, PhaseTiming, Profiler, ProfilerConfig};
#[cfg(feature = "timeline")]
pub use timeline::{EventGuard, Timeline, TimelineEvent, TimelineEventKind};
