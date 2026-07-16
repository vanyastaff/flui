//! [`TextField`] — the Material single-line text field: an
//! [`EditableText`] wrapped directly in an [`InputDecorator`], with live
//! focus/enabled/error plumbing and a tap target spanning the whole
//! decorated area.
//!
//! Flutter parity: `material/text_field.dart` `TextField` (oracle tag
//! `3.44.0`). The oracle has no widgets-layer `TextField` to extend —
//! `_TextFieldState.build` composes a raw `widgets.EditableText` and
//! `InputDecorator` inline (`text_field.dart:1684-1782`), which is exactly
//! this substrate's own shape: no
//! [`flui_widgets::TextField`](flui_widgets::text::text_field::TextField) in
//! the middle.
//!
//! # Live plumbing — what's wired and how
//!
//! - **Focus**: the oracle rebuilds on `_effectiveFocusNode`'s own
//!   `addListener(_handleFocusChanged)` (`text_field.dart:1273`). This
//!   substrate has no ambient `FocusNode` field to listen on directly —
//!   instead it registers with the process-wide
//!   [`FocusManager::add_listener`](flui_interaction::routing::FocusManager::add_listener)
//!   in `init_state` and compares against
//!   [`TextEditingController::focus_node_id`](flui_widgets::TextEditingController::focus_node_id)
//!   — the node [`EditableText`] itself
//!   published on mount — exactly the seam
//!   `EditableTextState`'s own internal focus listener uses to drive its
//!   caret visibility. A match/mismatch schedules a rebuild via
//!   [`BuildContext::rebuild_handle`], never from inside `build`
//!   (ADR-0018).
//! - **Hover**: the oracle owns `_isHovering` at the `TextField` level via
//!   its own outer `MouseRegion` and threads it into
//!   `InputDecorator.isHovering` (`text_field.dart:1463-1470,1773,1797-1800`).
//!   This substrate does **not** duplicate that — `InputDecorator` already
//!   self-tracks hover through its own internal `MouseRegion` wrapping the
//!   entire decorated area (see `input_decorator.rs`'s module docs), and
//!   that `MouseRegion` sits *inside* the tree this `TextField` composes.
//!   Adding a second, outer `MouseRegion` here would double-track the same
//!   pointer with two independent state machines for no behavioral gain —
//!   named divergence: `TextField` delegates hover entirely to the
//!   decorator it wraps.
//! - **Error**: [`InputDecoration::error_text`] presence — already the
//!   decorator's own state input — drives both the error row/underline (in
//!   `InputDecorator`) and this widget's caret color (see below). There is
//!   no separate "has error" flag on `TextField` itself, matching the
//!   oracle's `_hasError` being derived from the decoration, not a widget
//!   field of its own (`text_field.dart:1196-1201`, narrowed here: the
//!   oracle's `maxLength`-driven intrinsic error has no FLUI counterpart —
//!   `maxLength` is not ported).
//! - **Enabled**: [`TextField::enabled`] is the *single* source of truth,
//!   written into the effective [`InputDecoration::enabled`] before it
//!   reaches [`InputDecorator`] (mirroring `_getEffectiveDecoration()`'s
//!   `.copyWith(enabled: _isEnabled)`, `text_field.dart:1206-1213`) and
//!   passed straight to [`EditableText::enabled`] — both sinks always agree
//!   because both read the same field. Named divergence: the oracle
//!   resolves `_isEnabled` from a three-way null-coalescing chain
//!   (`widget.enabled ?? widget.decoration?.enabled ?? true`,
//!   `text_field.dart:1183`) since `TextField.enabled` is optional there;
//!   this substrate has no optional-override slot for a bare `bool` field,
//!   so `TextField::enabled` always wins outright rather than falling back
//!   to whatever `enabled` the caller set directly on the
//!   [`InputDecoration`] passed to [`TextField::decoration`].
//!
//! # Caret color and text style
//!
//! Caret color: `colors.error` when [`InputDecoration::error_text`] is set,
//! `colors.primary` otherwise — Flutter parity: the oracle's
//! `cursorColor = _hasError ? _errorColor : (widget.cursorColor ??
//! selectionStyle.cursorColor ?? theme.colorScheme.primary)`
//! (`text_field.dart:1637-1641`, the desktop/Android branch; every platform
//! branch shares the same `_hasError ? _errorColor : ...` shape). No
//! `cursorColor`/`cursorErrorColor` override slot yet — named deferral.
//!
//! Text style: `theme.text_theme.body_large`, unconditionally — Flutter
//! parity: `_m3InputStyle` (`text_field.dart:1893`) is `Theme.of(context)
//! .textTheme.bodyLarge!`, the M3 branch of `_getInputStyleForState`'s base
//! style (`text_field.dart:1547-1549`). The oracle's per-state resolution
//! table (`_m3StateInputStyle`) and the `TextField.style` override are both
//! named deferrals — this substrate always renders `bodyLarge` verbatim.
//!
//! # Tap-to-focus over the whole decorated area
//!
//! A [`GestureDetector`] wraps the composed [`InputDecorator`] (not just the
//! inner [`EditableText`]) — Flutter parity: the oracle's outer
//! `MouseRegion` → `TextFieldTapRegion` → `Semantics(onTap: ...
//! _requestKeyboard())` composition (`text_field.dart:1797,1811-1820`) makes
//! the *entire* decorated box (fill, underline, label/hint rows) a valid tap
//! target, not just the text-content rect. `GestureDetector`'s default
//! [`flui_widgets::HitTestBehavior::DeferToChild`] is sufficient here
//! because `InputDecorator`'s own inner `MouseRegion` defaults to
//! [`flui_widgets::HitTestBehavior::Opaque`] and spans the full decorated
//! rect, so every point within it already resolves a hit for the outer
//! detector to defer to.
//!
//! # DEFERRED (v1)
//!
//! Everything [`EditableText`] itself defers applies here too (IME, drag
//! selection, clipboard, multi-line, `obscureText`, input formatters,
//! overflow scrolling) — see its own module docs. Additionally, narrowed at
//! this layer:
//! - **`on_changed`** — [`EditableText`] has no change callback yet (only a
//!   [`Listenable`] seam); a caller observes edits via the
//!   [`TextEditingController`] itself.
//! - **`obscure_text`** — [`EditableText`] has no password-masking mode to
//!   forward.
//! - **Selection colors** — no collapsed-caret-only substrate has a
//!   selection to color yet (see `TextEditingController`'s own deferral
//!   list).
//! - **`cursorColor`/style overrides** — see the caret-color/text-style
//!   sections above.
//! - **Label/hint/helper/error as `Widget`** — `InputDecoration` is
//!   `String`-only V1 (see `input_decorator.rs`'s module docs).

use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::ListenerId;
use flui_foundation::notifier::Listenable;
use flui_interaction::routing::FocusManager;
use flui_view::prelude::*;
use flui_widgets::{EditableText, GestureDetector, TextEditingController};

use crate::input_decorator::{InputDecoration, InputDecorator};
use crate::theme::Theme;

// ============================================================================
// TextField
// ============================================================================

/// The Material single-line text field — [`EditableText`] decorated by
/// [`InputDecorator`], with live focus/enabled/error plumbing. See the
/// module docs for exactly what's wired and what's deferred.
#[derive(Clone, Debug, StatefulView)]
pub struct TextField {
    controller: TextEditingController,
    decoration: InputDecoration,
    enabled: bool,
}

impl TextField {
    /// Create a `TextField` driven by `controller`, with no decoration
    /// (label/hint/helper/error all unset — see [`InputDecoration::default`])
    /// and enabled.
    #[must_use]
    pub fn new(controller: TextEditingController) -> Self {
        Self {
            controller,
            decoration: InputDecoration::default(),
            enabled: true,
        }
    }

    /// Set the field's decoration — label, hint, helper/error text, and
    /// fill. `decoration.enabled` is overridden by [`Self::enabled`] before
    /// it reaches [`InputDecorator`] — see the module docs' "Enabled"
    /// section for why a directly-set `decoration.enabled` never diverges
    /// from `enabled` here.
    #[must_use]
    pub fn decoration(mut self, decoration: InputDecoration) -> Self {
        self.decoration = decoration;
        self
    }

    /// Set whether the field accepts focus and input (default `true`) —
    /// flows into both [`InputDecoration::enabled`] and
    /// [`EditableText::enabled`], see the module docs' "Enabled" section.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

// ============================================================================
// TextFieldState
// ============================================================================

/// Persistent state behind [`TextField`] — owns the live controller/focus
/// listeners described in the module docs.
pub struct TextFieldState {
    controller: TextEditingController,
    controller_listener_id: Option<ListenerId>,
    focus_listener_id: Option<ListenerId>,
}

impl std::fmt::Debug for TextFieldState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextFieldState")
            .field("controller", &self.controller)
            .finish_non_exhaustive()
    }
}

impl StatefulView for TextField {
    type State = TextFieldState;

    fn create_state(&self) -> Self::State {
        TextFieldState {
            controller: self.controller.clone(),
            controller_listener_id: None,
            focus_listener_id: None,
        }
    }
}

impl ViewState<TextField> for TextFieldState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        // ADR-0018: `rebuild_handle()` is acquired here, fired later from
        // the listeners below — never called from `build`.
        let rebuild = ctx.rebuild_handle();

        // Rebuild on every edit — `is_empty` (fed to `InputDecorator`) is
        // recomputed fresh in `build`, so a text change must trigger one.
        let rebuild_on_edit = rebuild.clone();
        self.controller_listener_id = Some(self.controller.add_listener(Arc::new(move || {
            rebuild_on_edit.schedule();
        })));

        // Rebuild exactly when *this field's own* published node transitions
        // into or out of primary focus — mirrors `EditableTextState`'s own
        // `FocusManager` listener (step 4 of its `init_state`).
        let controller_for_focus = self.controller.clone();
        self.focus_listener_id = Some(FocusManager::global().add_listener(Rc::new(
            move |previous, current| {
                let Some(node_id) = controller_for_focus.focus_node_id() else {
                    return;
                };
                let was_focused = previous == Some(node_id);
                let now_focused = current == Some(node_id);
                if was_focused != now_focused {
                    rebuild.schedule();
                }
            },
        )));
    }

    fn dispose(&mut self) {
        if let Some(id) = self.controller_listener_id.take() {
            self.controller.remove_listener(id);
        }
        if let Some(id) = self.focus_listener_id.take() {
            FocusManager::global().remove_listener(id);
        }
    }

    fn build(&self, view: &TextField, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = Theme::of(ctx);
        let colors = theme.color_scheme;

        let has_error = view.decoration.error_text.is_some();
        let caret_color = if has_error {
            colors.error
        } else {
            colors.primary
        };

        let is_empty = view.controller.text().is_empty();
        let focused = view
            .controller
            .focus_node_id()
            .is_some_and(|node_id| FocusManager::global().has_focus(node_id));

        // The single `enabled` source of truth — see the module docs'
        // "Enabled" section for why this overrides a directly-set
        // `decoration.enabled` rather than falling back to it.
        let mut decoration = view.decoration.clone();
        decoration.enabled = view.enabled;

        let mut editable = EditableText::new(view.controller.clone())
            .enabled(view.enabled)
            .caret_color(caret_color);
        if let Some(text_style) = theme.text_theme.body_large.clone() {
            editable = editable.text_style(text_style);
        }

        let controller_for_tap = view.controller.clone();

        GestureDetector::new()
            .on_tap(move || focus_field(&controller_for_tap))
            .child(
                InputDecorator::new(decoration)
                    .focused(focused)
                    .is_empty(is_empty)
                    .child(editable),
            )
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Focus the field driven by `controller` — the node its `EditableTextState`
/// published on mount (see
/// [`TextEditingController::focus_node_id`](flui_widgets::TextEditingController::focus_node_id)).
/// A no-op while the field is unmounted or disabled (a disabled field
/// withholds its published node — see `EditableText::enabled`'s doc
/// comment).
fn focus_field(controller: &TextEditingController) {
    if let Some(node_id) = controller.focus_node_id() {
        FocusManager::global().request_focus(node_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_decorates_with_default_decoration_and_is_enabled() {
        let field = TextField::new(TextEditingController::new());
        assert_eq!(field.decoration, InputDecoration::default());
        assert!(field.enabled);
    }

    #[test]
    fn builder_methods_override_decoration_and_enabled() {
        let decoration = InputDecoration {
            label_text: Some("Email".to_string()),
            ..Default::default()
        };
        let field = TextField::new(TextEditingController::new())
            .decoration(decoration.clone())
            .enabled(false);

        assert_eq!(field.decoration, decoration);
        assert!(!field.enabled);
    }

    #[test]
    fn focus_field_is_a_no_op_when_the_controller_has_no_published_node() {
        // No `EditableText` has mounted for this controller, so it has
        // never published a focus node — `focus_field` must not panic or
        // touch `FocusManager` in a way that would focus something.
        let controller = TextEditingController::new();
        assert_eq!(controller.focus_node_id(), None);
        focus_field(&controller); // Must not panic.
    }
}
