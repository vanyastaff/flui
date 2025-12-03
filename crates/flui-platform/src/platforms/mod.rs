//! Platform-specific embedder implementations
//!
//! Each platform module provides a thin wrapper around `EmbedderCore`,
//! adding only platform-specific behavior.

#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub mod desktop;

#[cfg(target_os = "android")]
pub mod android;

#[cfg(target_os = "ios")]
pub mod ios;

#[cfg(target_arch = "wasm32")]
pub mod web;
