//! macOS platform implementation using AppKit/Cocoa
//!
//! This module provides a native macOS implementation using AppKit (Cocoa) APIs.
//!
//! # Architecture
//!
//! - **NSApplication**: Main application and event loop
//! - **NSWindow**: Window management
//! - **NSScreen**: Display enumeration and info
//! - **NSRunLoop**: Event loop for foreground executor
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
//! platform.run(Box::new(|| {
//!     println!("macOS platform ready!");
//! }));
//! ```

#[macro_use]
extern crate objc;

mod display;
mod events;
mod liquid_glass;
mod platform;
mod view;
mod window;

pub use display::MacOSDisplay;
pub use events::convert_ns_event;
pub use liquid_glass::{LiquidGlassMaterial, LiquidGlassConfig, BlendingMode};
pub use platform::MacOSPlatform;
pub use window::MacOSWindow;
