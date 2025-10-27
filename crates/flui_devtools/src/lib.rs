//! FLUI DevTools - Developer tools for FLUI framework
//!
//! This crate provides a comprehensive suite of developer tools for debugging,
//! profiling, and inspecting FLUI applications. Inspired by Flutter DevTools
//! and React DevTools, it offers:
//!
//! # Features
//!
//! ## 🎯 Performance Profiler (default)
//! - Frame timing and jank detection
//! - Build/layout/paint phase profiling
//! - CPU usage tracking
//! - Performance timeline with markers
//!
//! ## 🔍 Widget Inspector (default)
//! - Widget tree visualization
//! - Property inspection
//! - Layout debugging
//! - Select widget from screen
//!
//! ## ⏱️ Timeline View
//! - Event timeline visualization
//! - Frame boundaries
//! - Custom trace events
//!
//! ## 🔥 Hot Reload (feature: hot-reload)
//! - Watch file changes
//! - Trigger rebuilds automatically
//! - State preservation
//!
//! ## 🌐 Network Monitor (feature: network-monitor)
//! - HTTP request tracking
//! - Response inspection
//! - Performance metrics
//!
//! ## 💾 Memory Profiler (feature: memory-profiler)
//! - Heap allocation tracking
//! - Memory usage over time
//! - Leak detection
//!
//! ## 🔌 Remote Debug (feature: remote-debug)
//! - WebSocket-based debugging protocol
//! - Connect from browser DevTools
//! - Remote widget inspection
//!
//! # Usage
//!
//! ## Basic Profiling
//!
//! ```rust
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
//! ## Widget Inspector
//!
//! ```rust
//! use flui_devtools::inspector::Inspector;
//!
//! let inspector = Inspector::new();
//! inspector.attach_to_tree(element_tree);
//!
//! // Select widget
//! let widget_info = inspector.select_widget(element_id);
//! println!("Widget: {:?}", widget_info.widget_type());
//! println!("Size: {:?}", widget_info.size());
//! ```
//!
//! ## Hot Reload
//!
//! ```rust
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
//! - `default`: Enables `profiling` and `inspector`
//! - `profiling`: Performance profiling tools
//! - `inspector`: Widget tree inspection
//! - `timeline`: Timeline view for events
//! - `hot-reload`: File watching and hot reload
//! - `network-monitor`: HTTP request monitoring
//! - `memory-profiler`: Memory usage tracking
//! - `remote-debug`: WebSocket debugging server
//! - `tracing-support`: Integration with `tracing` crate
//! - `full`: All features enabled

#![warn(missing_docs)]
#![warn(missing_debug_implementations)]
mod common;
#[cfg(feature = "hot-reload")]
pub mod hot_reload;
#[cfg(feature = "inspector")]
pub mod inspector;
#[cfg(feature = "memory-profiler")]
pub mod memory;
#[cfg(feature = "network-monitor")]
pub mod network;
pub mod profiler;
#[cfg(feature = "remote-debug")]
pub mod remote;
#[cfg(feature = "timeline")]
pub mod timeline;

// Feature-gated modules








// Re-exports
pub use common::*;

#[cfg(feature = "profiling")]
pub use profiler::Profiler;

#[cfg(feature = "inspector")]
pub use inspector::Inspector;

/// DevTools version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Prelude module for convenient imports
///
/// ```rust
/// use flui_devtools::prelude::*;
/// ```
pub mod prelude {
    #[cfg(feature = "profiling")]
    pub use crate::profiler::{Profiler, FramePhase, FrameStats};

    #[cfg(feature = "inspector")]
    pub use crate::inspector::{Inspector, WidgetInfo};

    #[cfg(feature = "timeline")]
    pub use crate::timeline::{Timeline, TimelineEvent};

    #[cfg(feature = "hot-reload")]
    pub use crate::hot_reload::HotReloader;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }
}


