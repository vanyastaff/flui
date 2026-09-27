//! Core trait definitions for platform abstraction
//!
//! This module defines the contract between the framework and platform-specific
//! embedders. The traits are designed for maximum code reuse while allowing
//! platform-specific customization.

mod capabilities;
mod embedder;
mod host_window;
// The owner-thread capability (ADR-0039): `OwnerPlatform`, `PlatformProxy`,
// `PendingWindow`, and their typed errors. `pub(crate)` (not private): the
// `pub(crate)` seams inside it — `OwnerHooks`, `ProxyTransport`,
// `DirectOwnerHooks`, `ClosedTransport` — are wired up by every backend
// module, not just this crate's own `traits` tree.
pub(crate) mod owner;
mod platform;
mod velocity;

// The contracts live in `flui-platform-api` (ADR-0082) and are re-exported
// here under their old names, so every existing path keeps resolving.
pub use flui_platform_api::{
    Clipboard, ClipboardItem, CursorError, DispatchEventResult, DisplayId, DragDropEvent, Key,
    KeyboardEvent, Modifiers, PlatformDisplay, PlatformHaptics, PlatformInput, PlatformTextInput,
    PlatformWindow, PointerButton, PointerButtons, PointerEvent, PointerId, PointerType,
    PointerUpdate, ScrollDelta, WindowAppearance, WindowBackgroundAppearance, WindowBounds,
    WindowEvent, WindowExecutionState, WindowId, WindowMode, WindowOptions, WindowReveal,
    WindowShowError, delta_offset_from_coords, device_to_logical, logical_to_device,
    offset_from_coords,
};

// The accessibility capability lives with the semantics tree it publishes
// (ADR-0082 §2); re-exported here at its old path for the backends and the
// composition root.
pub use capabilities::{
    DesktopCapabilities, MobileCapabilities, PlatformCapabilities, WebCapabilities,
};
pub use embedder::PlatformEmbedder;
pub use flui_semantics::platform::{
    AccessibilityActionListener, AccessibilityActivationListener, PlatformAccessibility,
};
pub use host_window::HostWindow;
// Re-export keyboard-types for convenience
pub use keyboard_types::NamedKey;
pub use owner::{
    OpenWindowError, OwnerPlatform, PendingWindow, PlatformProxy, ProxySendError, SharedPlatform,
    WaitError, WakeRegistrationError, WindowOpen,
};
pub use platform::{PathPromptOptions, Platform, PlatformExecutor, PlatformReadyCallback};
pub use velocity::{BasicVelocityTracker, SystemTimestamp, TimestampProvider};
