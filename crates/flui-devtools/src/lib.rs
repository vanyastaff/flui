//! FLUI DevTools - Developer tools for FLUI framework
//!
//! What this crate actually contains today — three small, feature-gated
//! subsystems, all standalone (no view/element/render-tree access):
//!
//! # Features
//!
//! ## 🎯 Performance Profiler (feature: profiling)
//! - Frame timing and jank detection
//! - Build/layout/paint phase profiling, fed manually by the caller
//! - Performance timeline with markers
//!
//! ## ⏱️ Timeline View (feature: timeline)
//! - Event timeline visualization
//! - Frame boundaries
//! - Custom trace events
//!
//! ## 🔥 Hot Reload (feature: hot-reload)
//! - Watch file changes and report them to a callback
//! - That is all: rebuild triggering and state preservation live in
//!   `flui-hot-reload` (worker/host split), not here
//!
//! ## 🔎 Inspector counters (feature: inspector)
//! - A counting/logging `TreeObserver` (ADR-0040 seam) — tallies element
//!   mounts, moves, rebuilds (per cause), and unmounts of a running realm
//! - Depends only on `flui-foundation`; installed via
//!   `WidgetsBinding::install_tree_observer` by the embedder
//!
//! # What this crate is NOT (yet)
//!
//! It is not a full inspector: it still has no dependency on any flui tree
//! crate and cannot walk widgets, elements, render objects, or semantics.
//! What exists (feature `inspector`) is the *event* half of ADR-0040's
//! dependency-inverted seam — structural observations pushed by the core.
//! Pull-shaped inspection (state versions, dependency edges, memory) waits
//! on its own future seam. There is likewise no network monitor, no memory
//! profiler, and no remote-debug protocol — earlier versions of this
//! documentation advertised those as features; they were never implemented.
//!
//! # Usage
//!
//! ## Basic Profiling
//!
//! ```rust,ignore
//! use flui_devtools::profiler::{Profiler, FramePhase};
//!
//! // Create profiler
//! let mut profiler = Profiler::new();
//!
//! // Start frame
//! profiler.begin_frame();
//!
//! // Profile build phase
//! let _guard = profiler.profile_phase(FramePhase::Build);
//! // ... your build code ...
//! drop(_guard);
//!
//! // End frame and get metrics
//! profiler.end_frame();
//! let stats = profiler.frame_stats();
//! println!("Frame time: {:.2}ms", stats.total_time_ms());
//! ```
//!
//! ## Hot Reload
//!
//! ```rust,ignore
//! #[cfg(feature = "hot-reload")]
//! use flui_devtools::hot_reload::HotReloader;
//!
//! #[cfg(feature = "hot-reload")]
//! {
//!     let mut reloader = HotReloader::new("./src");
//!     reloader.on_change(|path| {
//!         println!("File changed: {:?}", path);
//!         // Trigger rebuild
//!     });
//!     reloader.watch();
//! }
//! ```
//!
//! # Feature Flags
//!
//! - `default`: no features enabled; opt in via `profiling`, `timeline`, or `hot-reload`
//! - `profiling`: Performance profiling tools (no external dependencies)
//! - `timeline`: Timeline view for events
//! - `hot-reload`: File watching (reports changes; nothing more)
//! - `inspector`: Counting/logging tree observer over the ADR-0040 seam
//! - `full`: all of the above
//!
//! No other feature exists. The `default = []` boundary is what keeps a
//! release build at zero devtools overhead: nothing here is compiled, no
//! port is opened, no background work runs.

// Ship bar (wave 4): every public item is documented; keep it that way.
#![deny(missing_docs)]
#![warn(missing_debug_implementations)]
mod common;
/// Feeds the profiler from the framework's own frame spans — the only seam
/// layering permits, since nothing in the framework may depend on this crate.
#[cfg(feature = "profiling")]
pub mod frame_timing_layer;
#[cfg(feature = "hot-reload")]
pub mod hot_reload;
#[cfg(feature = "inspector")]
pub mod inspector;
#[cfg(feature = "profiling")]
pub mod profiler;
#[cfg(feature = "timeline")]
pub mod timeline;

// Re-exports
pub use common::*;
#[cfg(feature = "profiling")]
pub use frame_timing_layer::FrameTimingLayer;
#[cfg(feature = "profiling")]
pub use profiler::Profiler;

/// DevTools version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Prelude module for convenient imports
///
/// ```rust
/// use flui_devtools::prelude::*;
/// ```
pub mod prelude {
    #[cfg(feature = "profiling")]
    pub use crate::frame_timing_layer::FrameTimingLayer;
    #[cfg(feature = "hot-reload")]
    pub use crate::hot_reload::HotReloader;
    #[cfg(feature = "inspector")]
    pub use crate::inspector::{InspectorCounters, InspectorSnapshot};
    #[cfg(feature = "profiling")]
    pub use crate::profiler::{FramePhase, FrameStats, Profiler};
    #[cfg(feature = "timeline")]
    pub use crate::timeline::{Timeline, TimelineEvent};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }
}
