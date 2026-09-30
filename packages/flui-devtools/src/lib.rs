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
//!   trace export and a bridge from the scheduler's `FrameSnapshot`s.
//! - **`inspector`** — [`inspector::InspectorCounters`], a counting
//!   `TreeObserver` over the ADR-0040 observation seam: mounts, moves,
//!   rebuilds per cause, unmounts.
//! - **`agent`** (off by default) — `agent::AgentServer`, a
//!   `DevAgentHook` that serves the application's semantics tree to an agent
//!   over a local endpoint: a named pipe or Unix socket with a launch token,
//!   in debug builds only (ADR-0095 §3). It reads and acts through each
//!   window's owner inbox, the way assistive technology does.
//!
//! # What this crate is not
//!
//! It is not an inspector UI and not a hot-reload tool: it opens no TCP
//! port, walks no tree itself, and watches no files (the source watcher
//! lives in the `flui` CLI). Earlier documentation of this crate promised a
//! widget inspector, a network monitor, a memory profiler and a remote-debug
//! protocol; none of them was ever implemented, and no feature here claims
//! them.
//!
//! # Profiling a running app
//!
//! ```no_run
//! use std::sync::Arc;
//!
//! use flui_devtools::{FrameTimingLayer, Profiler};
//! use flui_log::{InstallPolicy, LogBridgePolicy, LogConfig};
//! use tracing_subscriber::layer::SubscriberExt;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let profiler = Arc::new(Profiler::new());
//! // FLUI's own log subscriber, with the profiler layer beside it. The layer
//! // carries its own filter, so the log filter's level does not affect it.
//! let subscriber = LogConfig::default()
//!     .subscriber()?
//!     .with(FrameTimingLayer::new(Arc::clone(&profiler)));
//! flui_log::install_subscriber(subscriber, InstallPolicy::Auto, LogBridgePolicy::Auto)?;
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
//! - `timeline`: [`timeline`] (`flui_sdk::hooks::FrameSnapshot` for the
//!   scheduler bridge, `serde` for the exporters).
//! - `inspector`: [`inspector`] (`flui_sdk::foundation::observe` for the
//!   seam).
//! - `agent`: the `agent` module (`flui-protocol` for the wire schema, `interprocess`
//!   for the endpoint). Off by default.
//! - `full`: all of the above, as one name for feature-matrix runs.

// Every public item is documented; keep it that way.
#![deny(missing_docs)]
#![warn(missing_debug_implementations)]

#[cfg(feature = "agent")]
pub mod agent;
/// Feeds the profiler from the framework's own frame spans — the only seam
/// layering permits, since nothing in the framework may depend on this crate.
#[cfg(feature = "profiling")]
pub mod frame_timing_layer;
#[cfg(feature = "inspector")]
pub mod inspector;
#[cfg(feature = "profiling")]
pub mod profiler;
#[cfg(feature = "timeline")]
pub mod timeline;

#[cfg(feature = "profiling")]
pub use frame_timing_layer::FrameTimingLayer;
#[cfg(feature = "profiling")]
pub use profiler::{Profiler, ProfilerConfig};

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
    pub use crate::profiler::{FramePhase, FrameStats, Profiler, ProfilerConfig};
    #[cfg(feature = "timeline")]
    pub use crate::timeline::{Timeline, TimelineEvent};
}
