//! Web/WASM platform implementation
//!
//! Implements the Platform trait for web browsers via WebAssembly using
//! wasm-bindgen and web-sys for browser API access.

mod clipboard;
mod display;
mod events;
mod executor;
mod platform;
mod window;

pub use platform::WebPlatform;
