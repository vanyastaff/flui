//! Runtime developer tooling for FLUI: the half that runs *inside* the
//! application. Small, feature-gated modules, each an adapter over a seam the
//! framework already exposes, so nothing here reaches into a widget, element
//! or render tree:
//!
//! - **`profiling`** — [`Profiler`] with per-phase frame statistics, fed by
//!   [`FrameTimingLayer`], a `tracing` layer that subscribes to the `frame`
//!   span `UpdateScheduler::drive_frame` opens and the `build`/`layout`/
//!   `paint`/`compositing` spans the pipeline emits inside it.
//! - **`timeline`** — [`timeline::Timeline`], an event recorder with Chrome
//!   trace export and a view over the shared frame-span capture.
//! - **`inspector`** — [`gesture_inspector::GestureInspector`], which reports
//!   what a gesture arena decided per contest through `GestureObserver`, and
//!   [`gesture_overlay::GestureInspectorOverlay`], a debug overlay that draws
//!   those contests over the app, and [`inspector::InspectorCounters`], a counting
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
//! use tracing_subscriber::layer::SubscriberExt;
//!
//! let profiler = Arc::new(Profiler::new());
//! // The layer carries its own filter, so another layer's level does not
//! // affect it.
//! let subscriber = tracing_subscriber::registry()
//!     .with(FrameTimingLayer::new(Arc::clone(&profiler)));
//! tracing::subscriber::set_global_default(subscriber)
//!     .expect("no subscriber installed yet");
//! // ...run the app...
//! if let Some(frame) = profiler.frame_stats() {
//!     println!("last frame: {:.2} ms", frame.total_time_ms());
//! }
//! ```
//!
//! # Feature flags
//!
//! The first three are on by default; disable what you do not need.
//!
//! - `profiling`: [`Profiler`] and [`FrameTimingLayer`] (`tracing` +
//!   `tracing-subscriber`).
//! - `timeline`: [`timeline`] (`serde` for the exporters).
//! - `inspector`: [`inspector`] (`flui_sdk::foundation::observe` for the
//!   seam), [`gesture_overlay`] (`flui_sdk::widgets`) and [`gesture_inspector`] (`flui_sdk::interaction`'s arena
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

/// The one capture of frame spans every profiling tool reads.
#[cfg(feature = "profiling")]
pub mod capture;
/// Feeds the profiler from the framework's own frame spans — the only seam
/// layering permits, since nothing in the framework may depend on this crate.
#[cfg(feature = "profiling")]
pub mod frame_timing_layer;
#[cfg(feature = "inspector")]
pub mod gesture_inspector;
#[cfg(feature = "inspector")]
pub mod gesture_overlay;
#[cfg(feature = "inspector")]
pub mod inspector;
/// Which spans are frame phases.
#[cfg(feature = "profiling")]
pub mod phase;
#[cfg(feature = "profiling")]
pub mod profiler;
#[cfg(feature = "timeline")]
pub mod timeline;

#[cfg(feature = "profiling")]
pub use capture::{Capture, CaptureLayer, CapturedFrame, CapturedSpan, FrameOpening, Partition};
#[cfg(feature = "profiling")]
pub use frame_timing_layer::FrameTimingLayer;
#[cfg(feature = "profiling")]
pub use phase::Phase;
#[cfg(feature = "profiling")]
pub use profiler::{JankLine, Profiler, ProfilerConfig};

/// Prelude module for convenient imports
///
/// ```rust
/// use flui_devtools::prelude::*;
/// ```
pub mod prelude {
    #[cfg(feature = "profiling")]
    pub use crate::frame_timing_layer::FrameTimingLayer;
    #[cfg(feature = "inspector")]
    pub use crate::inspector::{InspectorCounters, InspectorSnapshot};
    #[cfg(feature = "profiling")]
    pub use crate::profiler::{FramePhase, FrameStats, JankLine, Profiler, ProfilerConfig};
    #[cfg(feature = "timeline")]
    pub use crate::timeline::{Timeline, TimelineEvent};
}
