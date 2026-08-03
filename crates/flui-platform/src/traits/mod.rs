//! Core trait definitions for platform abstraction
//!
//! This module defines the contract between the framework and platform-specific
//! embedders. The traits are designed for maximum code reuse while allowing
//! platform-specific customization.

mod capabilities;
mod display;
mod embedder;
mod haptics;
mod input;
// The owner-thread capability (ADR-0039): `OwnerPlatform`, `PlatformProxy`,
// `PendingWindow`, and their typed errors. `pub(crate)` (not private): the
// `pub(crate)` seams inside it — `OwnerHooks`, `ProxyTransport`,
// `DirectOwnerHooks`, `ClosedTransport` — are wired up by every backend
// module, not just this crate's own `traits` tree.
pub(crate) mod owner;
mod platform;
mod text_input;
mod window;

pub use capabilities::{
    DesktopCapabilities, MobileCapabilities, PlatformCapabilities, WebCapabilities,
};
pub use display::{DisplayId, PlatformDisplay};
pub use embedder::PlatformEmbedder;
pub use haptics::PlatformHaptics;
pub use input::{
    // Platform utilities
    BasicVelocityTracker,
    // Event dispatch result
    DispatchEventResult,
    // System drag-and-drop (ADR-0038)
    DragDropEvent,
    // W3C event types (re-exported from ui-events)
    Key,
    KeyboardEvent,
    Modifiers,
    PlatformInput,
    PointerButton,
    PointerButtons,
    PointerEvent,
    PointerId,
    PointerType,
    PointerUpdate,
    ScrollDelta,
    SystemTimestamp,
    TimestampProvider,
    // Conversion helpers
    delta_offset_from_coords,
    device_to_logical,
    logical_to_device,
    offset_from_coords,
};
// Re-export keyboard-types for convenience
pub use keyboard_types::NamedKey;
pub use owner::{
    OpenWindowError, OwnerPlatform, PendingWindow, PlatformProxy, ProxySendError, WaitError,
    WindowOpen,
};
pub use platform::{
    Clipboard, ClipboardItem, PathPromptOptions, Platform, PlatformExecutor, PlatformReadyCallback,
    WindowEvent, WindowId, WindowMode, WindowOptions,
};
pub use text_input::PlatformTextInput;
#[cfg(feature = "winit-backend")]
pub use window::WinitWindow;
pub use window::{
    CursorError, PlatformWindow, WindowAppearance, WindowBackgroundAppearance, WindowBounds,
};
