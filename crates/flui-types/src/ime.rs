//! IME (Input Method Editor) composition vocabulary.
//!
//! [`ImeEvent`] is the framework-wide vocabulary a platform backend converts
//! its native IME notifications into. The shape follows winit 0.30's `Ime`
//! enum (`Enabled` / `Preedit` / `Commit` / `Disabled`) rather than the W3C
//! `compositionstart`/`compositionupdate`/`compositionend` web events or
//! Android's `InputConnection` callback set — winit is FLUI's lead desktop
//! backend (see `flui-platform`'s winit integration), and its four-variant
//! shape maps *imperfectly* onto both alternatives:
//!
//! - The W3C model splits "composition changed" from "commit" as two
//!   `compositionupdate`/`compositionend` events that both carry text, where
//!   winit collapses the change into [`ImeEvent::Preedit`] and the commit
//!   into a separate [`ImeEvent::Commit`] with no shared "composition id".
//! - Android's `InputConnection` is a pull model (the IME calls back into
//!   the app's `Editable` to read/replace spans) rather than winit's push
//!   model of discrete events.
//!
//! A future web or Android backend adapts its native model into this
//! vocabulary; this enum does not grow variants to accommodate them.
//!
//! See `docs/adr/` for the "Platform text input (IME) capability" ADR
//! (the platform capability trait, `PlatformTextInput`, lives in
//! `flui-platform`; this crate only defines the event vocabulary the
//! capability's window callback delivers).

/// A single IME composition/commit notification delivered by the platform.
///
/// # Preedit cursor offsets
///
/// [`ImeEvent::Preedit`]'s `cursor` field indexes *into the preedit string
/// itself* (`text`), as a byte offset `(start, end)` range — not into the
/// surrounding committed document. `cursor == None` means the platform wants
/// the composition caret hidden (winit's own semantics for this case); the
/// widget-side hidden-caret rendering state that honors that is a named PR2
/// deferral, not implemented by this vocabulary alone.
///
/// # Suppression contract (PR2 deferral, documented here for the client
/// authors that consume this vocabulary)
///
/// A text-input client must suppress `Key::Character` insertion **only**
/// while a composition is in progress (a non-empty preedit is active) —
/// winit itself already withholds `KeyboardInput` events during composition
/// and immediately after a commit, so a client that suppressed *all* typing
/// after [`ImeEvent::Enabled`] would silently kill plain (non-IME) keyboard
/// input for the rest of the session. [`ImeEvent::Disabled`] delivered
/// mid-composition means the client must strip the in-progress composing
/// slice from its buffer — winit's semantics, a documented divergence from
/// Flutter's `TextInputConnection.connectionClosed`, which instead *keeps*
/// the uncommitted text. Detach-on-dispose is part of the same client
/// contract (the bound-drop-cascade knot class this workspace has hit
/// before with other owner-thread callback registries).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ImeEvent {
    /// IME composition became available for the focused input.
    Enabled,
    /// The in-progress composition text changed.
    Preedit {
        /// The current composition text.
        text: String,
        /// Byte offset range within `text` for the composition cursor or
        /// selection. `None` hides the caret (see the type-level doc).
        cursor: Option<(usize, usize)>,
    },
    /// Composition finished; `String` is the final text to insert.
    Commit(String),
    /// IME composition ended; the input is no longer receiving composition
    /// events.
    Disabled,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preedit_cursor_none_and_some_are_distinct_and_index_the_preedit_string() {
        let hidden_caret = ImeEvent::Preedit {
            text: "ni".to_string(),
            cursor: None,
        };
        let visible_caret = ImeEvent::Preedit {
            text: "ni".to_string(),
            cursor: Some((0, 2)),
        };
        assert_ne!(hidden_caret, visible_caret);

        let ImeEvent::Preedit { cursor, .. } = visible_caret else {
            unreachable!("constructed as Preedit above")
        };
        assert_eq!(
            cursor,
            Some((0, 2)),
            "cursor offsets index the preedit string, not a surrounding document"
        );
    }

    #[test]
    fn commit_carries_the_final_text() {
        let event = ImeEvent::Commit("\u{4f60}\u{597d}".to_string());
        assert_eq!(event, ImeEvent::Commit("你好".to_string()));
    }

    #[test]
    fn enabled_and_disabled_are_unit_variants() {
        assert_eq!(ImeEvent::Enabled, ImeEvent::Enabled);
        assert_eq!(ImeEvent::Disabled, ImeEvent::Disabled);
        assert_ne!(ImeEvent::Enabled, ImeEvent::Disabled);
    }
}
