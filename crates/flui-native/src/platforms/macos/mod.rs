//! macOS platform implementation using AppKit/Cocoa
//!
//! This module provides a native macOS implementation using AppKit (Cocoa)
//! APIs.
//!
//! # Architecture
//!
//! - **NSApplication**: Main application and event loop
//! - **NSWindow**: Window management, the window feature's
//!   (`window/apple/macos.rs`), re-exported here
//! - **NSScreen**: Display enumeration and info (`display::apple::macos`)
//! - **NSRunLoop**: Owner-thread application event loop
//!   (`owner/apple/macos.rs`)
//! - **GCD/Tokio**: Background task execution
//!
//! # Features
//!
//! - ✅ Window creation and management
//! - ✅ Multi-display support with Retina/HiDPI
//! - ✅ Event loop integration
//! - ✅ raw-window-handle for wgpu/Metal
//! - ✅ Keyboard, mouse, scroll and hover events (`input::apple::macos`)
//! - ✅ `NSPasteboard` clipboard, routed through the owner lane
//!   (`clipboard/apple/macos.rs`)
//! - ✅ IME composition via an `NSTextInputClient` conformance
//!   (`crate::text::apple::macos`, [ADR-0069](../../../../../docs/adr/ADR-0069-a-keydown-produces-one-semantic-event.md)):
//!   `keyDown:` is a gate, so one press reaches the application exactly once —
//!   either as a composition/commit or as a key event, never both
//! - ✅ A display snapshot from `NSScreen.screens`, each display keyed by its
//!   UUID with its refresh period read from its current mode, resampled after
//!   `NSApplicationDidChangeScreenParametersNotification` and the window's own
//!   display
//! - ✅ A wake pump that actuates the registered wake deadline
//!   (`owner/apple/macos/wake_pump.rs`), since AppKit exposes no
//!   `ControlFlow::WaitUntil`
//!
//! No Core Text system is needed: text is shaped by Parley and discovered by
//! fontdb in flui-painting, end to end
//! ([ADR-0092](../../../../../docs/adr/ADR-0092-per-realm-text-over-parley.md)),
//! which is why the historical "Core Text (TODO)" item is gone rather than done.
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_native::MacOSPlatform;
//!
//! let platform = MacOSPlatform::new()?;
//! platform.run(Box::new(|_owner| {
//!     println!("macOS platform ready!");
//!     Ok(())
//! }))?;
//! ```

// The window lives in the window feature; its public names stay here.
pub use crate::window::apple::liquid_glass::{
    BlendingMode, LiquidGlassConfig, LiquidGlassMaterial,
};
pub use crate::window::apple::macos::{
    MacOSCollectionBehavior, MacOSWindow, MacOSWindowExt, MacOSWindowLevel, TilePosition,
    TilingConfiguration, TilingError, TilingLayout, TilingState,
};
