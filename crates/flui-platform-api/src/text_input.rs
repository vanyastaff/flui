//! Platform text input (IME) capability
//!
//! Flutter's `services` package is deliberately dissolved in FLUI
//! (`docs/FOUNDATIONS.md`); its IME/text-input residue becomes a capability
//! trait here instead of a standalone crate. [`PlatformTextInput`] is reached
//! through [`PlatformWindow::text_input`](crate::PlatformWindow::text_input) — the same
//! capability-discovery pattern as [`PlatformWindow::display`](crate::PlatformWindow::display)
//! and `flui_platform::Platform::primary_display`: a fallible accessor
//! returning `Option<Arc<dyn _>>`, not a method bolted directly onto
//! `PlatformWindow` with a panicking/no-op default. A backend that cannot
//! support IME (a minimal future embedder; headless without its
//! `FakeTextInput`) returns `None` from the accessor instead of every
//! `PlatformWindow` implementor inheriting IME methods it cannot honor.
//! [`PlatformHaptics`](crate::PlatformHaptics) follows the same
//! template (`PlatformWindow::haptics`, ADR-0031); `PlatformSystemChrome`
//! is deferred (ADR-0031) with no target date.

use flui_types::geometry::{Bounds, Pixels};

/// Platform capability for IME-driven text input on one window.
///
/// # Scope
///
/// This trait only carries the platform *composition* controls
/// (enable/disable IME, place the candidate window). It does not model a
/// text buffer, cursor/selection state, or the suppression contract a
/// client applies to incoming [`flui_types::ImeEvent`]s — that is
/// `flui-interaction`'s presentation-owned text-input client contract.
pub trait PlatformTextInput: Send + Sync {
    /// Enable or disable IME composition for this window's active input.
    ///
    /// Disabling mid-composition drops the in-progress composing text rather
    /// than committing it — winit's macOS backend clears its marked text on
    /// `set_ime_allowed(false)` before queueing `Ime::Disabled`, and a
    /// deliberate divergence from Flutter, which closes a connection leaving
    /// the composed characters in the controller's text and clears only the
    /// composing range. See the `PlatformTextInput` ADR (ADR-0069).
    fn set_ime_allowed(&self, allowed: bool);

    /// Tell the platform IME where to draw its candidate/composition
    /// window, in logical window coordinates (origin + size, matching
    /// [`PlatformWindow::bounds`](crate::PlatformWindow::bounds)'s convention).
    fn set_ime_cursor_area(&self, area: Bounds<Pixels>);
}
