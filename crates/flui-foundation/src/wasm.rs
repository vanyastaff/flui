//! WASM compatibility traits.
//!
//! This module provides traits that adapt behavior between native and WASM targets,
//! similar to wgpu's approach.
//!
//! On native platforms, `WasmNotSendSync` requires `Send + Sync`.
//! On WASM (single-threaded), it's an empty trait allowing non-thread-safe types.
//!
//! # Usage
//!
//! ```rust
//! use flui_foundation::WasmNotSendSync;
//!
//! // This type works on both native and WASM
//! struct MyType<T: WasmNotSendSync> {
//!     value: T,
//! }
//! ```

/// Trait for types that are `Send + Sync` on native but not necessarily on WASM.
///
/// On native platforms (non-WASM), this trait requires `Send + Sync`.
/// On WASM, where there's typically only one thread, this trait is empty,
/// allowing types that can't be `Send + Sync` (like JS handles) to be used.
///
/// # Example
///
/// ```rust
/// use flui_foundation::WasmNotSendSync;
///
/// // On native: requires Send + Sync
/// // On WASM: no requirements
/// fn process<T: WasmNotSendSync>(value: T) {
///     // ...
/// }
/// ```
#[cfg(not(target_arch = "wasm32"))]
pub trait WasmNotSendSync: Send + Sync {}

#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + Sync> WasmNotSendSync for T {}

/// Trait for types that are `Send + Sync` on native but not necessarily on WASM.
///
/// On WASM, this is an empty trait since WASM is single-threaded.
#[cfg(target_arch = "wasm32")]
pub trait WasmNotSendSync {}

#[cfg(target_arch = "wasm32")]
impl<T> WasmNotSendSync for T {}

/// Trait for types that are `Send` on native but not necessarily on WASM.
#[cfg(not(target_arch = "wasm32"))]
pub trait WasmNotSend: Send {}

#[cfg(not(target_arch = "wasm32"))]
impl<T: Send> WasmNotSend for T {}

/// Trait for types that are `Send` on native but not necessarily on WASM.
#[cfg(target_arch = "wasm32")]
pub trait WasmNotSend {}

#[cfg(target_arch = "wasm32")]
impl<T> WasmNotSend for T {}
