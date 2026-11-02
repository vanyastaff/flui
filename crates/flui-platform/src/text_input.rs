//! Platform text input (IME) capability
//!
//! FLUI has no standalone `services` layer (ADR-0030); IME
//! text input is a capability trait here instead of a standalone crate.
//! [`PlatformTextInput`] is reached
//! through [`PlatformWindow::text_input`](crate::PlatformWindow::text_input) — the same
//! capability-discovery pattern as [`PlatformWindow::haptics`](crate::PlatformWindow::haptics):
//! a fallible accessor
//! returning `Option<Arc<dyn _>>`, not a method bolted directly onto
//! `PlatformWindow` with a panicking/no-op default. A backend that cannot
//! support IME (a minimal future embedder; headless without its
//! `FakeTextInput`) returns `None` from the accessor instead of every
//! `PlatformWindow` implementor inheriting IME methods it cannot honor.
//! [`PlatformHaptics`](crate::PlatformHaptics) follows the same
//! template (`PlatformWindow::haptics`, ADR-0031); `PlatformSystemChrome`
//! is deferred (ADR-0031) with no target date.

use flui_foundation::geometry::Bounds;

/// Platform capability for IME-driven text input on one window.
///
/// # Scope
///
/// This trait only carries the platform *composition* controls
/// (enable/disable IME, place the candidate window). The document the input
/// method reads and edits is the field's [`TextStore`](crate::TextStore)
/// (ADR-0090), attached through `flui-interaction`'s presentation-owned
/// text-input owner, which projects incoming [`ImeEvent`](crate::ImeEvent)s onto
/// it. A backend cannot hold that store through this trait yet: the trait is
/// `Send + Sync` and the store is owner-thread (ADR-0082 §4).
pub trait PlatformTextInput: Send + Sync {
    /// Enable or disable IME composition for this window's active input.
    ///
    /// Disabling mid-composition drops the in-progress composing text rather
    /// than committing it — winit's macOS backend clears its marked text on
    /// `set_ime_allowed(false)` before queueing `Ime::Disabled`. Leaving the
    /// composed characters in the text and clearing only the composing range
    /// is deliberately not done. See the `PlatformTextInput` ADR (ADR-0069).
    fn set_ime_allowed(&self, allowed: bool);

    /// Tell the platform IME where to draw its candidate/composition
    /// window, in logical window coordinates (origin + size, matching
    /// [`WindowState::outer`](crate::WindowState::outer)'s convention).
    fn set_ime_cursor_area(&self, area: Bounds<f64>);

    /// Make the platform forget its in-progress composition: the caller
    /// commits the store's own copy of the composed text in place, so the
    /// input method must not commit it again.
    fn discard_composition(&self);

    /// The focused field's standing properties: whether it is protected (a
    /// password the keyboard must not learn from or show), whether it holds
    /// one line, and what it is for. Told when a field is attached and
    /// whenever its status changes, since a push platform reads no store.
    ///
    /// Required, with no default: a backend that ignored it would let a
    /// password reach input-method learning without a trace. One with
    /// nothing to apply it to says why in an empty body.
    fn set_field_status(&self, status: crate::text_store::TextStoreStatus);
}
