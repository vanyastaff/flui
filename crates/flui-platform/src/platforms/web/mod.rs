//! Web/WASM platform implementation
//!
//! Implements the Platform trait for web browsers via WebAssembly using
//! wasm-bindgen and web-sys for browser API access.

// This module (and its submodules) is one of the workspace's sanctioned
// `unsafe` FFI islands — `wasm-bindgen`'s JS interop requires it. The
// workspace lint `unsafe_code = "warn"` is opted out here, at the module
// boundary, rather than for the whole crate (see `lib.rs`).
#![expect(unsafe_code)]

// The backend's `Send`/`Sync` claims rest on the page having one thread: the
// JS handles and `RefCell`s it holds are never reached from another. A
// shared-memory build (`+atomics`, wasm threads) breaks that, so it is refused
// here rather than left to a data race.
#[cfg(target_feature = "atomics")]
compile_error!(
    "the web backend assumes one thread; build wasm32 without the `atomics` target feature"
);

mod clipboard;
mod display;
mod display_events;
mod drag;
mod events;
mod executor;
mod features;
mod haptics;
mod listener;
mod page_events;
mod platform;
mod preferences;
mod registration;
mod text_input;
mod window;

pub use platform::WebPlatform;
