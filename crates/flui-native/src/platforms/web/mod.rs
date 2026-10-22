//! Web/WASM platform implementation
//!
//! Implements the Platform trait for web browsers via WebAssembly using
//! wasm-bindgen and web-sys for browser API access.

// The backend's `Send`/`Sync` claims rest on the page having one thread: the
// JS handles and `RefCell`s it holds are never reached from another. A
// shared-memory build (`+atomics`, wasm threads) breaks that, so it is refused
// here rather than left to a data race.
#[cfg(target_feature = "atomics")]
compile_error!(
    "the web backend assumes one thread; build wasm32 without the `atomics` target feature"
);

pub(crate) mod events;
