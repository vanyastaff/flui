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
//! - ✅ Keyboard, mouse, scroll and hover events (`events.rs`/`view.rs`)
//! - ✅ `NSPasteboard` clipboard, routed through the owner lane (`clipboard.rs`)
//! - ✅ IME composition via an `NSTextInputClient` conformance
//!   (`text_input.rs`, [ADR-0066](../../../../../docs/adr/ADR-0066-a-keydown-produces-one-semantic-event.md)):
//!   `keyDown:` is a gate, so one press reaches the application exactly once —
//!   either as a composition/commit or as a key event, never both
//! - ✅ `refresh_period()` from the display's current mode
//! - ✅ A wake pump that actuates the registered wake deadline (`wake_pump.rs`),
//!   since AppKit exposes no `ControlFlow::WaitUntil`
//!
//! No Core Text system is needed: text shaping is cosmic-text end to end
//! ([ADR-0059](../../../../../docs/adr/ADR-0059-flui-stays-on-cosmic-text.md)),
//! which is why the historical "Core Text (TODO)" item is gone rather than done.
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_platform::MacOSPlatform;
//!
//! let platform = MacOSPlatform::new()?;
//! platform.run(Box::new(|_owner| {
//!     println!("macOS platform ready!");
//!     Ok(())
//! }))?;
//! ```

// cocoa 0.26 deprecates its entire API surface in favor of the objc2 family;
// this backend deliberately stays on the single cocoa/objc stack until a
// dedicated objc2 migration replaces it wholesale.
#![expect(deprecated)]
// This module (and its submodules) is one of the workspace's sanctioned
// `unsafe` FFI islands — direct AppKit/Cocoa objc calls have no safe
// wrapper. The workspace lint `unsafe_code = "warn"` is opted out here, at
// the module boundary, rather than for the whole crate (see `lib.rs`).
#![expect(unsafe_code)]

#[cfg(feature = "a11y")]
mod accessibility;
mod clipboard;
mod display;
mod display_pass;
mod events;
mod liquid_glass;
mod owner_lane;
mod platform;
mod text_input;
mod view;
mod wake_pump;
mod window;
mod window_ext;
mod window_manager;
mod window_tiling;

#[cfg(feature = "a11y")]
pub use accessibility::MacosAccessibility;
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
pub use window_tiling::{
    TilePosition, TilingConfiguration, TilingError, TilingLayout, TilingState,
};
