//! [`TextField`] — an [`EditableText`] with a plain border decoration and
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
use crate::text::editable_text::EditableText;

// ============================================================================
// TextField
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
/// - Clipboard (copy / paste / cut)
/// - Multi-line support
/// - `obscureText` (password masking)
/// - Input formatters
/// - Scroll when text overflows the visible width
/// - Label / hint text / error text / `InputDecoration` in general
/// - Focus decoration changes (highlighted border on focus)
#[derive(Clone, Debug)]
pub struct TextField {
    // PORT-CHECK-OK-SP3: deliberate theme-free stand-in for a widgets-only tree; the M3 field is flui_material::TextField — see this module's docs
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
}

impl TextField {
    /// Create a `TextField` driven by `controller`.
    #[must_use]
    pub fn new(controller: TextEditingController) -> Self {
        Self {
            controller,
            external_focus_node: None,
            caret_height: 18.0,
            caret_color: Color::BLACK,
            content_padding: EdgeInsets::symmetric(px(8.0), px(12.0)),
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

    /// Override the content padding (default 8 vertical × 12 horizontal).
    #[must_use]
    pub fn content_padding(mut self, padding: EdgeInsets) -> Self {
        self.content_padding = padding;
        self
    }
}

impl View for TextField {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl StatefulView for TextField {
    type State = TextFieldState;

    fn create_state(&self) -> Self::State {
        TextFieldState {
            focus_node: self
                .external_focus_node
                .as_ref()
                .map_or_else(|| FocusNode::with_debug_label("TextField"), Rc::clone),
            using_external_node: self.external_focus_node.is_some(),
        }
    }
}

/// Persistent focus ownership for a plain [`TextField`].
pub struct TextFieldState {
    focus_node: Rc<FocusNode>,
    using_external_node: bool,
}

impl std::fmt::Debug for TextFieldState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextFieldState")
            .field("focus_node", &self.focus_node.id())
            .field("using_external_node", &self.using_external_node)
            .finish()
    }
}

impl ViewState<TextField> for TextFieldState {
    fn build(&self, view: &TextField, _ctx: &dyn BuildContext) -> impl IntoView {
        let editable = EditableText::new(view.controller.clone(), Rc::clone(&self.focus_node))
            .caret_height(view.caret_height)
            .caret_color(view.caret_color);

        let padded = Padding::new(view.content_padding).child(editable);
        let decorated = DecoratedBox::new(field_border_decoration()).child(padded);

        let focus_node = Rc::clone(&self.focus_node);
        GestureDetector::new()
            .on_tap(move || {
                focus_node.request_focus();
            })
            .child(decorated)
    }

    fn did_update_view(&mut self, old_view: &TextField, new_view: &TextField) {
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
            .map_or_else(|| FocusNode::with_debug_label("TextField"), Rc::clone);
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

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // unit tests assert exact set-then-read values, not computed floats

    use super::*;

    /// Each field carries its own explicit node into `EditableText`, so
    /// requests never resolve through controller metadata or a first-node
    /// registry.
    #[test]
    fn explicit_focus_requests_target_each_fields_own_node() {
        use flui_view::ViewExt;

        let first = TextEditingController::new();
        let second = TextEditingController::new();
        let first_node = FocusNode::with_debug_label("first");
        let second_node = FocusNode::with_debug_label("second");
        let mut harness = crate::test_harness::mount(crate::Column::new(vec![
            TextField::new(first)
                .focus_node(Rc::clone(&first_node))
                .into_view()
                .boxed(),
            TextField::new(second)
                .focus_node(Rc::clone(&second_node))
                .into_view()
                .boxed(),
        ]));
        let manager = harness.focus_manager();

        second_node.request_focus();
        assert!(second_node.has_primary_focus());
        first_node.request_focus();
        assert!(first_node.has_primary_focus());

        // Unmount: both attachment tokens are detached.
        harness.swap_root(crate::Column::new(Vec::<flui_view::BoxedView>::new()));
        assert!(!first_node.is_attached());
        assert!(!second_node.is_attached());
        assert!(manager.primary_focus().is_none());
    }

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
        let field = TextField::new(controller)
            .caret_height(24.0)
            .caret_color(Color::rgb(1, 2, 3))
            .content_padding(EdgeInsets::all(px(5.0)));

        assert_eq!(field.caret_height, 24.0);
        assert_eq!(field.caret_color, Color::rgb(1, 2, 3));
        assert_eq!(field.content_padding, EdgeInsets::all(px(5.0)));
    }

    #[test]
    fn new_defaults_to_documented_caret_height_color_and_padding() {
        let field = TextField::new(TextEditingController::new());
        assert_eq!(field.caret_height, 18.0);
        assert_eq!(field.caret_color, Color::BLACK);
        assert_eq!(
            field.content_padding,
            EdgeInsets::symmetric(px(8.0), px(12.0))
        );
    }
}
