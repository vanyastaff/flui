//! Pins the mechanism `flui_engine::wgpu::renderer`'s private `RawHandles`
//! newtype relies on (ADR-0045 decision 1): a raw platform handle placed
//! directly on a struct — not behind a `Send`-asserting wrapper — is
//! `!Send` by default, so `Send` auto-derivation fails.
//!
//! `Renderer`'s fields are private, so this cannot literally add a field to
//! `Renderer` from an external fixture; it pins the general mechanism that
//! makes an un-wrapped `!Send` field a compile error instead of silently
//! riding a re-widened blanket `unsafe impl Send for Renderer`. See
//! `Renderer: Send` itself pinned unconditionally (not just here) via
//! `static_assertions::assert_impl_all!` in `src/wgpu/renderer.rs`.

fn assert_send<T: Send>() {}

struct WithUnwrappedHandle {
    handle: raw_window_handle::RawWindowHandle,
}

fn main() {
    assert_send::<WithUnwrappedHandle>();
}
