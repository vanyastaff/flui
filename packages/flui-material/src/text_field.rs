//! [`TextField`] — the Material single-line text field: an
//! [`EditableText`] wrapped directly in an [`InputDecorator`], with live
//! focus/enabled/error plumbing and a tap target spanning the whole
//! decorated area.
//!
//! The field composes a raw [`EditableText`] and [`InputDecorator`] inline,
//! with no
//! [`flui_sdk::widgets::RawTextField`](flui_sdk::widgets::text::text_field::RawTextField)
//! in the middle — that type is this crate's theme-free sibling for a tree
//! with no `Theme` ancestor, named `RawTextField` (not `TextField`)
//! specifically so this Material type gets to be the one thing named
//! `TextField` throughout the facade; see that type's own module doc for
//! the naming rationale.
//!
//! # Live plumbing — what's wired and how
//!
//! - **Focus**: the state owns its effective [`FocusNode`] (or retains the
//!   caller-provided node), passes that exact node to [`EditableText`], and
//!   listens to the node directly. This matches the oracle's
//!   `_effectiveFocusNode.addListener(_handleFocusChanged)`
//!   (`text_field.dart:1273`) without ambient manager lookup, controller
//!   metadata, or an ID registry. The listener schedules a rebuild through a
//!   [`RebuildHandle`] acquired in `init_state`,
//!   never from inside `build` (ADR-0018).
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
//! - **Enabled**: resolved exactly like the oracle's `_isEnabled`
//!   (`widget.enabled ?? widget.decoration?.enabled ?? true`,
//!   `text_field.dart:1183`), narrowed to two links —
//!   [`TextField::enabled`] is `Option<bool>`; `Some` wins outright,
//!   `None` falls through to [`InputDecoration::enabled`] (which already
//!   defaults `true`, covering the oracle's trailing `?? true`). The
//!   resolved value is written into the effective decoration before it
//!   reaches [`InputDecorator`] (mirroring `_getEffectiveDecoration()`'s
//!   `.copyWith(enabled: _isEnabled)`, `text_field.dart:1206-1213`) and
//!   passed straight to [`EditableText::enabled`] — both sinks always agree
//!   because both are computed from the one resolved value, never read
//!   from two different fields independently.
//!
//! # Controller identity — a swap retargets BOTH halves, or neither
//!
//! `MaterialTextFieldState` (this widget) and `EditableTextState` (the
//! interior) each hold their own view of the controller, and a parent that
//! swaps in a different [`TextEditingController`] on an already-mounted
//! `TextField` retargets both in the same `did_update_view` pass.
//!
//! Both, deliberately: retargeting one alone split-brains the composite —
//! this widget's `is_empty` tracking the new controller while `EditableText`
//! keeps typing into the old one — which is worse than ignoring the swap.
//! That is why it WAS ignored at both layers, and why the fix had to land at
//! both. The interior retargets everything that reaches a controller by
//! writing one shared cell (`EditableTextState::controller`); this layer only
//! tracks `is_empty` for the decoration's floating label, so its half is the
//! listener plus its own clone. Mirrors the oracle's `didUpdateWidget`
//! (`text_field.dart:1303-1311`).
//!
//! # Caret color and text style
//!
//! Caret color: `colors.error` when [`InputDecoration::error_text`] is set,
//! `colors.primary` otherwise (an error always wins over any caret color
//! override). No `cursorColor`/`cursorErrorColor` override slot yet — named
//! deferral.
//!
//! Text style: `theme.text_theme.body_large`, unconditionally — the M3 input
//! style. The per-state resolution table and the `TextField.style` override
//! are both named deferrals — this substrate always renders `bodyLarge` verbatim.
//!
//! # Tap-to-focus over the whole decorated area
//!
//! A [`GestureDetector`] wraps the composed [`InputDecorator`] (not just the
//! inner [`EditableText`]), so the
//! *entire* decorated box (fill, underline, label/hint rows) is a valid tap
//! target, not just the text-content rect. `GestureDetector`'s default
//! [`flui_sdk::widgets::HitTestBehavior::DeferToChild`] is sufficient here
//! because `InputDecorator`'s own inner `MouseRegion` defaults to
//! [`flui_sdk::widgets::HitTestBehavior::Opaque`] and spans the full decorated
//! rect, so every point within it already resolves a hit for the outer
//! detector to defer to.
//!
//! # DEFERRED (v1)
//!
//! Everything [`EditableText`] itself defers applies here too (multi-line,
//! input formatters, overflow scrolling) — see its own module docs.
//! Additionally, narrowed at this layer:
//! - **Selection colors** — no collapsed-caret-only substrate has a
//!   selection to color yet (see `TextEditingController`'s own deferral
//!   list).
//! - **`cursorColor`/style overrides** — see the caret-color/text-style
//!   sections above.
//! - **Label/hint/helper/error as `Widget`** — `InputDecoration` is
//!   `String`-only V1 (see `input_decorator.rs`'s module docs).

use std::rc::Rc;
use std::sync::Arc;

use flui_sdk::foundation::ListenerId;
use flui_sdk::foundation::notifier::Listenable;
use flui_sdk::interaction::FocusNode;
use flui_sdk::view::RebuildHandle;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{EditableText, GestureDetector, SubmitCallback, TextEditingController};

use crate::input_decorator::{InputDecoration, InputDecorator};
use crate::theme::Theme;

// ============================================================================
// TextField
// ============================================================================

/// The Material single-line text field — [`EditableText`] decorated by
/// [`InputDecorator`], with live focus/enabled/error plumbing. See the
/// module docs for exactly what's wired and what's deferred.
#[derive(Clone)]
pub struct TextField {
    controller: TextEditingController,
    external_focus_node: Option<Rc<FocusNode>>,
    decoration: InputDecoration,
    enabled: Option<bool>,
    /// Forwarded to [`EditableText::obscure_text`] — a password field.
    obscure_text: bool,
    /// Forwarded to [`EditableText::on_submitted`] — see
    /// [`Self::on_submitted`].
    on_submitted: Option<SubmitCallback>,
    /// Forwarded to [`EditableText::on_changed`] — see [`Self::on_changed`].
    on_changed: Option<TextChanged>,
}

/// Callback for [`TextField::on_changed`], with the dispatch's [`EventCx`].
type TextChanged = Rc<dyn Fn(&mut EventCx<'_>, &str)>;

/// Store a text callback, adapted to report its outcome (ADR-0086).
fn text_callback<F, R>(callback: F) -> TextChanged
where
    F: Fn(&mut EventCx<'_>, &str) -> R + 'static,
    R: EventOutcome,
{
    Rc::new(move |cx: &mut EventCx<'_>, text: &str| callback(cx, text).report())
}

// Hand-written rather than derived: `on_submitted`'s `Rc<dyn Fn(&str)>` has
// no `Debug` impl. Mirrors `flui_sdk::widgets::EditableText`'s own manual impl,
// which exists for the identical reason.
impl std::fmt::Debug for TextField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextField")
            .field("controller", &self.controller)
            .field("external_focus_node", &self.external_focus_node)
            .field("decoration", &self.decoration)
            .field("enabled", &self.enabled)
            .field("obscure_text", &self.obscure_text)
            .field("on_submitted", &self.on_submitted.is_some())
            .field("on_changed", &self.on_changed.is_some())
            .finish()
    }
}

impl TextField {
    /// Show every character as a bullet — a password field (default
    /// `false`). Not only pixels: the substitution is upstream of the render
    /// object, so its diagnostics carry the mask too.
    ///
    /// Forwards to [`EditableText::obscure_text`], where the substitution
    /// happens at the point the controller's text becomes the render view's,
    /// so the real characters never reach the render object or anything below
    /// it.
    ///
    /// Deliberately narrow: there is no `obscuringCharacter` override, and no
    /// `!obscureText || maxLines == 1` invariant to check — this field is
    /// single-line by construction. The character override is available one
    /// layer down on `EditableText` until a caller needs it here.
    #[must_use]
    pub fn obscure_text(mut self, obscure: bool) -> Self {
        self.obscure_text = obscure;
        self
    }

    /// Create a `TextField` driven by `controller`, with no decoration
    /// (label/hint/helper/error all unset — see [`InputDecoration::default`])
    /// and no `enabled` override (falls through to
    /// [`InputDecoration::enabled`]'s own `true` default — see
    /// [`Self::enabled`]).
    #[must_use]
    pub fn new(controller: TextEditingController) -> Self {
        Self {
            controller,
            external_focus_node: None,
            decoration: InputDecoration::default(),
            enabled: None,
            obscure_text: false,
            on_submitted: None,
            on_changed: None,
        }
    }

    /// Use a caller-owned focus node instead of the node owned by this
    /// field's state.
    #[must_use]
    pub fn focus_node(mut self, focus_node: Rc<FocusNode>) -> Self {
        self.external_focus_node = Some(focus_node);
        self
    }

    /// Set the field's decoration — label, hint, helper/error text, and
    /// fill. `decoration.enabled` is respected as-is unless [`Self::enabled`]
    /// is *also* called — see the module docs' "Enabled" section for the
    /// resolution order.
    #[must_use]
    pub fn decoration(mut self, decoration: InputDecoration) -> Self {
        self.decoration = decoration;
        self
    }

    /// Override whether the field accepts focus and input, beating whatever
    /// [`InputDecoration::enabled`] the decoration carries — see the module
    /// docs' "Enabled" section for the full resolution chain. Absent this
    /// call, [`InputDecoration::enabled`] decides.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = Some(enabled);
        self
    }

    /// Call `callback` with the field's current text when Enter is pressed
    /// while it has focus. Forwards to [`EditableText::on_submitted`] — see
    /// that method's doc for exactly when it fires and why it is Enter
    /// rather than an IME action-button commit. The callback receives the
    /// dispatch's `&mut EventCx<'_>` first (ADR-0086).
    #[must_use]
    pub fn on_submitted<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, &str) -> R + 'static,
        R: EventOutcome,
    {
        self.on_submitted = Some(text_callback(callback));
        self
    }

    /// Call `callback` with the dispatch's `&mut EventCx<'_>` and the new
    /// text after each user edit. Forwards
    /// to [`EditableText::on_changed`]; a caller's own controller edits do
    /// not call it.
    #[must_use]
    pub fn on_changed<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, &str) -> R + 'static,
        R: EventOutcome,
    {
        self.on_changed = Some(text_callback(callback));
        self
    }
}

// ============================================================================
// MaterialTextFieldState
// ============================================================================

impl View for TextField {
    fn create_element(&self) -> flui_sdk::view::element::ElementKind {
        flui_sdk::view::element::ElementKind::stateful(self)
    }
}

/// Persistent state behind [`TextField`] — owns the effective focus node and
/// the live controller/focus listeners described in the module docs.
pub struct MaterialTextFieldState {
    controller: TextEditingController,
    focus_node: Rc<FocusNode>,
    rebuild: Option<RebuildHandle>,
    controller_listener_id: Option<ListenerId>,
    focus_listener_id: Option<ListenerId>,
}

impl std::fmt::Debug for MaterialTextFieldState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MaterialTextFieldState")
            .field("controller", &self.controller)
            .field("focus_node", &self.focus_node.id())
            .finish_non_exhaustive()
    }
}

impl StatefulView for TextField {
    type State = MaterialTextFieldState;

    fn create_state(&self) -> Self::State {
        MaterialTextFieldState {
            controller: self.controller.clone(),
            focus_node: self.external_focus_node.as_ref().map_or_else(
                || FocusNode::with_debug_label("MaterialTextField"),
                Rc::clone,
            ),
            rebuild: None,
            controller_listener_id: None,
            focus_listener_id: None,
        }
    }
}

impl MaterialTextFieldState {
    fn install_focus_listener(&mut self) {
        let rebuild = self
            .rebuild
            .as_ref()
            .expect("BUG: MaterialTextFieldState must retain its rebuild handle")
            .clone();
        self.focus_listener_id = Some(self.focus_node.add_listener(Rc::new(move || {
            rebuild.schedule(flui_sdk::view::RebuildReason::StateChange);
        })));
    }
}

impl ViewState<TextField> for MaterialTextFieldState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        // ADR-0018: `rebuild_handle()` is acquired here, fired later from
        // the listeners below — never called from `build`.
        let rebuild = ctx.rebuild_handle();
        self.rebuild = Some(rebuild.clone());

        // Rebuild on every edit — `is_empty` (fed to `InputDecorator`) is
        // recomputed fresh in `build`, so a text change must trigger one.
        let rebuild_on_edit = rebuild;
        self.controller_listener_id = Some(self.controller.add_listener(Arc::new(move || {
            rebuild_on_edit.schedule(flui_sdk::view::RebuildReason::StateChange);
        })));

        // The effective node is the single source of focus truth for the
        // decorated field and its EditableText child.
        self.install_focus_listener();
    }

    fn did_update_view(&mut self, old_view: &TextField, new_view: &TextField) {
        // The controller half, mirroring `EditableTextState`'s: drop the
        // listener from the old, adopt the replacement, add it to the new.
        // This layer only tracks `is_empty` for the decoration's floating
        // label, so retargeting is the listener plus the pinned clone —
        // the interior does the rest, and does it through a shared cell.
        if !self.controller.is_same_controller(&new_view.controller) {
            if let Some(id) = self.controller_listener_id.take() {
                self.controller.remove_listener(id);
            }
            self.controller = new_view.controller.clone();
            if let Some(rebuild) = self.rebuild.clone() {
                let rebuild_on_edit = rebuild.clone();
                self.controller_listener_id =
                    Some(self.controller.add_listener(Arc::new(move || {
                        rebuild_on_edit.schedule(flui_sdk::view::RebuildReason::StateChange);
                    })));
                // `is_empty` feeds the decoration's floating label and is
                // recomputed in `build`; the replacement has not changed since
                // it was handed over, so nothing else would ask for that pass.
                rebuild.schedule(flui_sdk::view::RebuildReason::StateChange);
            }
        }

        let focus_node_changed = match (
            old_view.external_focus_node.as_ref(),
            new_view.external_focus_node.as_ref(),
        ) {
            (Some(old), Some(new)) => !Rc::ptr_eq(old, new),
            (None, None) => false,
            (Some(_), None) | (None, Some(_)) => true,
        };
        if !focus_node_changed {
            return;
        }

        if let Some(id) = self.focus_listener_id.take() {
            self.focus_node.remove_listener(id);
        }
        self.focus_node = new_view.external_focus_node.as_ref().map_or_else(
            || FocusNode::with_debug_label("MaterialTextField"),
            Rc::clone,
        );
        self.install_focus_listener();
    }

    fn dispose(&mut self) {
        if let Some(id) = self.controller_listener_id.take() {
            self.controller.remove_listener(id);
        }
        if let Some(id) = self.focus_listener_id.take() {
            self.focus_node.remove_listener(id);
        }
        self.rebuild = None;
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

        // `self.controller`, which `did_update_view` keeps pointed at the
        // current one. Reading `view.controller` directly here would be
        // correct too, now that both halves retarget — but it would take the
        // listener registration out of step with the value this reads, and
        // one place deciding "which controller is mine" is what keeps the
        // two halves from disagreeing (see the module docs).
        let is_empty = self.controller.text().is_empty();
        let focused = self.focus_node.has_focus();

        // The oracle's null-coalescing chain (`widget.enabled ??
        // decoration?.enabled ?? true`, `text_field.dart:1183`), narrowed to
        // two links since `InputDecoration::enabled` already defaults
        // `true`: an explicit `TextField::enabled` call wins outright;
        // absent that, the decoration's own `enabled` (set directly by the
        // caller, or its `true` default) is respected as-is.
        let effective_enabled = view.enabled.unwrap_or(view.decoration.enabled);
        let mut decoration = view.decoration.clone();
        decoration.enabled = effective_enabled;

        let mut editable = EditableText::new(self.controller.clone(), Rc::clone(&self.focus_node))
            .enabled(effective_enabled)
            .caret_color(caret_color)
            .obscure_text(view.obscure_text);
        if let Some(text_style) = theme.text_theme.body_large {
            editable = editable.text_style(text_style);
        }
        if let Some(on_submitted) = view.on_submitted.clone() {
            editable = editable.on_submitted(move |cx, text| on_submitted(cx, text));
        }
        if let Some(on_changed) = view.on_changed.clone() {
            editable = editable.on_changed(move |cx, text| on_changed(cx, text));
        }

        let focus_node = Rc::clone(&self.focus_node);

        GestureDetector::new()
            .on_tap(move |_cx| {
                focus_node.request_focus();
            })
            .child(
                InputDecorator::new(decoration)
                    .focused(focused)
                    .is_empty(is_empty)
                    .child(editable),
            )
    }
}
