//! Core trait definitions for platform abstraction
//!
//! This module defines the contract between the framework and platform-specific
//! embedders. The traits are designed for maximum code reuse while allowing
//! platform-specific customization.
//!
//! Input prediction and velocity tracking belong to the interaction layer.
//! The unused platform tracker and embedder abstraction are retired (ADR-0082 §5).
//! `trybuild_ui::ui_tests` checks each absent name separately, so one retired
//! export cannot return unnoticed while another import still fails.
//!
//! ```compile_fail,E0432
//! use flui_native::traits::{
//!     BasicVelocityTracker, PlatformEmbedder, SystemTimestamp, TimestampProvider,
//! };
//! ```

mod host_window;
// The owner-thread capability (ADR-0039): `OwnerPlatform`, `PlatformProxy`,
// `PendingWindow`, and their typed errors. `pub(crate)` (not private): the
// `pub(crate)` seams inside it — `OwnerHooks`, `ProxyTransport`,
// `DirectOwnerHooks` — are wired up by the direct backends
// module, not just this crate's own `traits` tree.
pub(crate) mod owner;
mod platform;

// The contracts live in `flui-platform` (ADR-0082) and the accessibility
// capability in `flui_semantics::platform`; each item has that one path
// (ADR-0151 §9), and this module re-exports none of them.
pub use host_window::HostWindow;
pub(crate) use host_window::OwnerThreadToken;
pub use owner::{
    OpenWindowError, OwnerPlatform, PendingWindow, PlatformProxy, ProxySendError, SharedPlatform,
    WaitError, WakeRegistrationError, WindowOpen,
};
pub use platform::{PathPromptOptions, Platform, PlatformExecutor, PlatformReadyCallback};
