//! Core trait definitions for platform abstraction
//!
//! This module defines the contract between the framework and platform-specific
//! embedders. The traits are designed for maximum code reuse while allowing
//! platform-specific customization.

mod capabilities;
mod embedder;
mod lifecycle;
mod window;

pub use capabilities::{
    DesktopCapabilities, MobileCapabilities, PlatformCapabilities, WebCapabilities,
};
pub use embedder::PlatformEmbedder;
pub use lifecycle::{DefaultLifecycle, LifecycleEvent, LifecycleState, PlatformLifecycle};
pub use window::{PlatformWindow, WinitWindow};
