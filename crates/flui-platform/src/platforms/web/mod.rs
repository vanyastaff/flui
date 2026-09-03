//! Web/WASM platform implementation
//!
//! Implements the Platform trait for web browsers via WebAssembly using
//! wasm-bindgen and web-sys for browser API access.

// This module (and its submodules) is one of the workspace's sanctioned
// `unsafe` FFI islands — `wasm-bindgen`'s JS interop requires it. The
// workspace lint `unsafe_code = "warn"` is opted out here, at the module
// boundary, rather than for the whole crate (see `lib.rs`).
#![expect(unsafe_code)]

mod clipboard;
mod display;
mod events;
mod executor;
mod platform;
mod window;

pub use platform::WebPlatform;
