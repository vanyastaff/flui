//! Platform text input (IME) capability
//!
//! Flutter's `services` package is deliberately dissolved in FLUI
//! (`docs/FOUNDATIONS.md`); its IME/text-input residue becomes a capability
//! trait here instead of a standalone crate. [`PlatformTextInput`] is reached
//! through [`PlatformWindow::text_input`](super::window::PlatformWindow::text_input)
//! — the same capability-discovery pattern as
//! [`PlatformWindow::display`](super::window::PlatformWindow::display) and
//! [`Platform::primary_display`](super::platform::Platform::primary_display):
//! a fallible accessor returning `Option<Arc<dyn _>>`, not a method bolted
//! directly onto `PlatformWindow` with a panicking/no-op default. A backend
//! that cannot support IME (a minimal future embedder; headless without its
//! `FakeTextInput`) returns `None` from the accessor instead of every
//! `PlatformWindow` implementor inheriting IME methods it cannot honor.
//! [`PlatformHaptics`](super::haptics::PlatformHaptics) follows the same
//! template (`PlatformWindow::haptics`, ADR-0031); `PlatformSystemChrome`
//! is deferred (ADR-0031) with no target date.

use flui_types::geometry::{Bounds, Pixels};

/// Platform capability for IME-driven text input on one window.
///
/// # Scope (V1 / PR1)
///
/// This trait only carries the platform *composition* controls
/// (enable/disable IME, place the candidate window). It does not model a
/// text buffer, cursor/selection state, or the suppression contract a
/// client applies to incoming [`flui_types::ImeEvent`]s — that is
/// `flui-interaction`'s `TextInputRegistry` client contract (a named PR2
/// deferral; see its module doc).
pub trait PlatformTextInput: Send + Sync {
    /// Enable or disable IME composition for this window's active input.
    ///
    /// Disabling mid-composition follows winit's own semantics: the
    /// in-progress composing text is dropped, not committed (a documented
    /// divergence from Flutter's `TextInputConnection.connectionClosed`,
    /// which keeps the uncommitted text — see the `PlatformTextInput` ADR).
    fn set_ime_allowed(&self, allowed: bool);

    /// Tell the platform IME where to draw its candidate/composition
    /// window, in logical window coordinates (origin + size, matching
    /// [`PlatformWindow::bounds`](super::window::PlatformWindow::bounds)'s
    /// convention).
    fn set_ime_cursor_area(&self, area: Bounds<Pixels>);

    /// Downcast support for tests that need to reach a concrete recording
    /// fake (e.g. the headless backend's `FakeTextInput`) behind the trait
    /// object `PlatformWindow::text_input` returns.
    fn as_any(&self) -> &dyn std::any::Any;
}
