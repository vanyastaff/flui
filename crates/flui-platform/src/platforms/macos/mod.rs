//! macOS platform implementation using AppKit/Cocoa
//!
//! This module provides a native macOS implementation using AppKit (Cocoa)
//! APIs.
//!
//! # Architecture
//!
//! - **NSApplication**: Main application and event loop
//! - **NSWindow**: Window management
//! - **NSScreen**: Display enumeration and info
//! - **NSRunLoop**: Owner-thread application event loop
//! - **GCD/Tokio**: Background task execution
//!
//! # Features
//!
//! - ✅ Window creation and management
//! - ✅ Multi-display support with Retina/HiDPI
//! - ✅ Event loop integration
//! - ✅ raw-window-handle for wgpu/Metal
//! - 🚧 Keyboard and mouse events (TODO)
//! - 🚧 NSPasteboard clipboard (TODO)
//! - 🚧 Core Text system (TODO)
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_platform::MacOSPlatform;
//!
//! let platform = MacOSPlatform::new()?;
//! platform.run(Box::new(|_platform| {
//!     println!("macOS platform ready!");
//! }));
//! ```

// cocoa 0.26 deprecates its entire API surface in favor of the objc2 family;
// this backend deliberately stays on the single cocoa/objc stack until a
// dedicated objc2 migration replaces it wholesale.
#![allow(deprecated)]

mod clipboard;
mod display;
mod events;
mod liquid_glass;
mod platform;
mod view;
mod window;
mod window_ext;
mod window_manager;
mod window_tiling;

pub use clipboard::MacOSClipboard;
pub use display::MacOSDisplay;
pub use events::convert_ns_event;
pub use liquid_glass::{BlendingMode, LiquidGlassConfig, LiquidGlassMaterial};
pub use platform::MacOSPlatform;
pub use window::MacOSWindow;
pub use window_ext::{MacOSCollectionBehavior, MacOSWindowExt, MacOSWindowLevel};
pub use window_manager::{
    GroupId, SharedWindowManager, WindowId, WindowInfo, WindowLevel, WindowManager, WindowOptions,
};
pub use window_tiling::{TilePosition, TilingConfiguration, TilingLayout, TilingState};
