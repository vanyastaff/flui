//! [`RawTextField`] — an [`EditableText`] with a plain border decoration and
//! tap-to-focus behavior, for callers with no `Theme` ancestor.
//!
//! **Not a Flutter-parity port.** Flutter has no widgets-layer text field —
//! `material/text_field.dart`'s `TextField` is the *only* oracle, and its
//! parity claim belongs to
//! [`flui_material::TextField`](https://docs.rs/flui-material) (M3
//! decoration via `InputDecorator`, live focus/enabled/error plumbing, theme
//! colors). This type is this crate's own plain stand-in: a fixed 1px
//! gray-border box with no theming, no label/hint/helper/error slots, and no
//! state-table colors — for a widgets-only tree with no `Theme` above it to
//! decorate from. Prefer `flui_material::TextField` whenever a `Theme` is
//! available.
//!
//! # Name
//!
//! Named `RawTextField`, not `TextField` — `flui::prelude` (the facade)
//! needs `TextField` to mean exactly one thing, the Material one, whenever
//! the `material` feature is on, with no shadowing and no feature-dependent
//! meaning for a name a caller might already be using. Before this rename,
//! both this type and `flui_material::TextField` were named `TextField`,
//! and the facade's prelude picked between them by having the Material one
//! explicitly shadow this one via Rust's explicit-import-over-glob rule —
//! technically sound, but `TextField`'s meaning then depended on whether the
//! `material` feature happened to be on, which is a feature-additivity
//! violation (enabling a Cargo feature is supposed to only ADD symbols, not
//! change what an existing name resolves to). See `ARCHITECTURE.md`'s
//! `## Mapping decisions` entry for the record of both the original
//! shadowing decision and this rename that superseded it.

use std::rc::Rc;

use flui_geometry::{EdgeInsets, px};
use flui_interaction::FocusNode;
use flui_types::styling::{Border, BorderSide, BorderStyle, BoxDecoration};
use flui_types::{Color, Pixels};
use flui_view::prelude::*;

use crate::interaction::GestureDetector;
use crate::layout::Padding;
use crate::paint::DecoratedBox;
use crate::text::controller::TextEditingController;
use crate::text::editable_text::{EditableText, SubmitCallback, TextChanged};

// ============================================================================
// RawTextField
// ============================================================================

/// A plain decorated, tap-to-focus single-line text input field — wraps
/// [`EditableText`] with a fixed [`DecoratedBox`] border and a
/// [`GestureDetector`] that requests focus on tap. See the module docs: this
/// is a theme-free stand-in, not the Material `TextField` — prefer
/// `flui_material::TextField` whenever a `Theme` ancestor is available.
///
/// # DEFERRED (v1)
///
/// Everything deferred in [`EditableText`] applies here too:
/// - IME / composing region
/// - Text selection by drag + selection rendering
/// - Multi-line support
/// - Input formatters
/// - Scroll when text overflows the visible width
/// - Label / hint text / error text / `InputDecoration` in general
/// - Focus decoration changes (highlighted border on focus)
#[derive(Clone)]
pub struct RawTextField {
    /// Controller that owns the text buffer and caret position.
    controller: TextEditingController,
    /// Optional caller-owned focus node. When absent, the state owns one for
    /// the field's mounted lifetime.
    external_focus_node: Option<Rc<FocusNode>>,
    /// Height of the caret bar, forwarded to [`EditableText`].
    caret_height: f32,
    /// Color of the caret bar when focused, forwarded to [`EditableText`].
    caret_color: Color,
    /// Inner padding between the decoration border and the text.
    content_padding: EdgeInsets,
    /// Forwarded to [`EditableText::obscure_text`] — a password field.
    obscure_text: bool,
    /// Forwarded to [`EditableText::enabled`].
    enabled: bool,
    /// Forwarded to [`EditableText::on_changed`].
    on_changed: Option<TextChanged>,
    /// Forwarded to [`EditableText::on_submitted`] — see
    /// [`Self::on_submitted`].
    on_submitted: Option<SubmitCallback>,
}

// Hand-written rather than derived: `on_submitted`'s `Rc<dyn Fn(&str)>` has
// no `Debug` impl. Mirrors `EditableText`'s own manual impl, which exists
// for the identical reason.
impl std::fmt::Debug for RawTextField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawTextField")
            .field("controller", &self.controller)
            .field("external_focus_node", &self.external_focus_node)
            .field("caret_height", &self.caret_height)
            .field("caret_color", &self.caret_color)
            .field("content_padding", &self.content_padding)
            .field("obscure_text", &self.obscure_text)
            .field("enabled", &self.enabled)
            .field("on_changed", &self.on_changed.is_some())
            .field("on_submitted", &self.on_submitted.is_some())
            .finish()
    }
}

impl RawTextField {
    /// Create a `RawTextField` driven by `controller`.
    #[must_use]
    pub fn new(controller: TextEditingController) -> Self {
        Self {
            controller,
            external_focus_node: None,
            caret_height: 18.0,
            caret_color: Color::BLACK,
            content_padding: EdgeInsets::symmetric(px(8.0), px(12.0)),
            obscure_text: false,
            enabled: true,
            on_changed: None,
            on_submitted: None,
        }
    }

    /// Use a caller-owned focus node instead of the field state's internal
    /// node.
    #[must_use]
    pub fn focus_node(mut self, focus_node: Rc<FocusNode>) -> Self {
        self.external_focus_node = Some(focus_node);
        self
    }

    /// Override the caret bar height (default 18 logical pixels).
    #[must_use]
    pub fn caret_height(mut self, height: f32) -> Self {
        self.caret_height = height;
        self
    }

    /// Override the caret color (default black).
    #[must_use]
    pub fn caret_color(mut self, color: Color) -> Self {
        self.caret_color = color;
        self
    }

    /// Show every character as a bullet — a password field (default
    /// `false`). Forwards to [`EditableText::obscure_text`], where the
    /// substitution happens before the render object sees the text, so it
    /// covers diagnostics as well as pixels.
    #[must_use]
    pub fn obscure_text(mut self, obscure: bool) -> Self {
        self.obscure_text = obscure;
        self
    }

    /// Whether the field accepts focus and input (default `true`). Forwards
    /// to [`EditableText::enabled`].
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Call `callback` with the new text after each user edit. Forwards to
    /// [`EditableText::on_changed`].
    #[must_use]
    pub fn on_changed(mut self, callback: impl Fn(&str) + 'static) -> Self {
        self.on_changed = Some(Rc::new(callback));
        self
    }

    /// Override the content padding (default 8 vertical × 12 horizontal).
    #[must_use]
    pub fn content_padding(mut self, padding: EdgeInsets) -> Self {
        self.content_padding = padding;
        self
    }

    /// Call `callback` with the field's current text when Enter is pressed
    /// while it has focus. Forwards to [`EditableText::on_submitted`] — see
    /// that method's doc for exactly when it fires. Added for symmetry with
    /// `flui_material::TextField::on_submitted`, which forwards the same
    /// way.
    #[must_use]
    pub fn on_submitted(mut self, callback: impl Fn(&str) + 'static) -> Self {
        self.on_submitted = Some(Rc::new(callback));
        self
    }
}

impl View for RawTextField {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl StatefulView for RawTextField {
    type State = RawTextFieldState;

    fn create_state(&self) -> Self::State {
        RawTextFieldState {
            focus_node: self
                .external_focus_node
                .as_ref()
                .map_or_else(|| FocusNode::with_debug_label("RawTextField"), Rc::clone),
            using_external_node: self.external_focus_node.is_some(),
        }
    }
}

/// Persistent focus ownership for a plain [`RawTextField`].
pub struct RawTextFieldState {
    focus_node: Rc<FocusNode>,
    using_external_node: bool,
}

impl std::fmt::Debug for RawTextFieldState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawTextFieldState")
            .field("focus_node", &self.focus_node.id())
            .field("using_external_node", &self.using_external_node)
            .finish()
    }
}

impl ViewState<RawTextField> for RawTextFieldState {
    fn build(&self, view: &RawTextField, _ctx: &dyn BuildContext) -> impl IntoView {
        let mut editable = EditableText::new(view.controller.clone(), Rc::clone(&self.focus_node))
            .caret_height(view.caret_height)
            .caret_color(view.caret_color)
            .obscure_text(view.obscure_text)
            .enabled(view.enabled);
        if let Some(on_submitted) = view.on_submitted.clone() {
            editable = editable.on_submitted(move |text| on_submitted(text));
        }
        if let Some(on_changed) = view.on_changed.clone() {
            editable = editable.on_changed(move |text| on_changed(text));
        }

        let padded = Padding::new(view.content_padding).child(editable);
        let decorated = DecoratedBox::new(field_border_decoration()).child(padded);

        let focus_node = Rc::clone(&self.focus_node);
        GestureDetector::new()
            .on_tap(move || {
                focus_node.request_focus();
            })
            .child(decorated)
    }

    fn did_update_view(&mut self, old_view: &RawTextField, new_view: &RawTextField) {
        let external_changed = match (
            old_view.external_focus_node.as_ref(),
            new_view.external_focus_node.as_ref(),
        ) {
            (Some(old), Some(new)) => !Rc::ptr_eq(old, new),
            (None, None) => false,
            _ => true,
        };
        if !external_changed {
            return;
        }

        self.focus_node = new_view
            .external_focus_node
            .as_ref()
            .map_or_else(|| FocusNode::with_debug_label("RawTextField"), Rc::clone);
        self.using_external_node = new_view.external_focus_node.is_some();
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Simple 1-px dark-gray border over a white background for the field.
fn field_border_decoration() -> BoxDecoration<Pixels> {
    BoxDecoration::new()
        .set_color(Some(Color::WHITE))
        .set_border(Some(Border::all(BorderSide::new(
            Color::rgb(180, 180, 180),
            px(1.0),
            BorderStyle::Solid,
        ))))
}

// The mounted `RawTextField` tests live in
// `crates/flui-widgets/tests/text_field_widget.rs`.
#[cfg(test)]
mod tests {
    #![expect(clippy::float_cmp)] // unit tests assert exact set-then-read values, not computed floats

    use super::*;

    #[test]
    fn field_border_decoration_is_a_white_fill_with_a_gray_solid_border() {
        let decoration = field_border_decoration();
        assert_eq!(decoration.color, Some(Color::WHITE));

        let border = decoration.border.expect("border must be set");
        let top = border.top.expect("top side must be set");
        assert_eq!(top.color, Color::rgb(180, 180, 180));
        assert_eq!(top.width, px(1.0));
        assert_eq!(top.style, BorderStyle::Solid);
    }

    #[test]
    fn builder_methods_override_caret_height_caret_color_and_content_padding() {
        let controller = TextEditingController::new();
        let field = RawTextField::new(controller)
            .caret_height(24.0)
            .caret_color(Color::rgb(1, 2, 3))
            .content_padding(EdgeInsets::all(px(5.0)));

        assert_eq!(field.caret_height, 24.0);
        assert_eq!(field.caret_color, Color::rgb(1, 2, 3));
        assert_eq!(field.content_padding, EdgeInsets::all(px(5.0)));
    }

    #[test]
    fn new_defaults_to_documented_caret_height_color_and_padding() {
        let field = RawTextField::new(TextEditingController::new());
        assert_eq!(field.caret_height, 18.0);
        assert_eq!(field.caret_color, Color::BLACK);
        assert_eq!(
            field.content_padding,
            EdgeInsets::symmetric(px(8.0), px(12.0))
        );
        assert!(field.on_submitted.is_none());
    }
}
