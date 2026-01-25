//! Core trait definitions for platform abstraction
//!
//! This module defines the contract between the framework and platform-specific
//! embedders. The traits are designed for maximum code reuse while allowing
//! platform-specific customization.

mod capabilities;
mod display;
mod embedder;
mod input;
mod lifecycle;
mod platform;
mod window;

pub use capabilities::{
    DesktopCapabilities, MobileCapabilities, PlatformCapabilities, WebCapabilities,
};
pub use display::{DisplayId, PlatformDisplay};
pub use embedder::PlatformEmbedder;
pub use input::{
    FileDropEvent, FileDropPhase, InputEvent, KeyCode, KeyDownEvent, KeyUpEvent, LogicalKey,
    Modifiers, ModifiersChangedEvent, MouseButton, NamedKey, PlatformInput, PointerEvent,
    PointerKind, PointerPhase, PointerTilt, ScrollDelta, ScrollPhase, ScrollWheelEvent, Velocity,
    VelocityTracker,
};
pub use lifecycle::{DefaultLifecycle, LifecycleEvent, LifecycleState, PlatformLifecycle};
pub use platform::{
    Clipboard, Platform, PlatformExecutor, PlatformTextSystem, WindowEvent, WindowId, WindowOptions,
};
pub use window::PlatformWindow;

#[cfg(feature = "winit-backend")]
pub use window::WinitWindow;
