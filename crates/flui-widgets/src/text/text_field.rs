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

use flui_geometry::{EdgeInsets, px};
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
#[derive(Clone, Debug, StatelessView)]
pub struct TextField {
    // PORT-CHECK-OK-SP3: deliberate theme-free stand-in for a widgets-only tree; the M3 field is flui_material::TextField — see this module's docs
    /// Controller that owns the text buffer and caret position.
    controller: TextEditingController,
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
            caret_height: 18.0,
            caret_color: Color::BLACK,
            content_padding: EdgeInsets::symmetric(px(8.0), px(12.0)),
        }
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

impl StatelessView for TextField {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let editable = EditableText::new(self.controller.clone())
            .caret_height(self.caret_height)
            .caret_color(self.caret_color);

        let padded = Padding::new(self.content_padding).child(editable);
        let decorated = DecoratedBox::new(field_border_decoration()).child(padded);

        let controller = self.controller.clone();
        GestureDetector::new()
            .on_tap(move || {
                focus_field(&controller);
            })
            .child(decorated)
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

/// Focus the field driven by `controller` — the node its `EditableTextState`
/// published on mount. Precise for multi-field forms, where the
/// old root-scope walk could only ever find the first field. A no-op while the
/// field is unmounted, since an unmounted node cannot take focus.
fn focus_field(controller: &TextEditingController) {
    use flui_interaction::routing::FocusManager;

    if let Some(node_id) = controller.focus_node_id() {
        FocusManager::global().request_focus(node_id);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // unit tests assert exact set-then-read values, not computed floats

    use flui_interaction::routing::FocusManager;

    use super::*;

    /// The tap focuses **its own** field. Each mounted
    /// `EditableText` publishes its focus node on its controller; with two
    /// fields, focusing through the second controller must land on the second
    /// node — exactly what the old first-node-with-a-key-handler root-scope
    /// walk could not do. Unmounting withdraws the node, making the tap a
    /// no-op.
    ///
    /// Red-check: point `focus_field` at the old root-scope walk — the second
    /// tap lands on the *first* field and the disambiguation assertion fails.
    #[test]
    fn a_tap_focuses_the_fields_own_node_not_the_first_registered() {
        use flui_view::ViewExt;

        let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
        let manager = FocusManager::global();
        manager.unfocus();

        let first = TextEditingController::new();
        let second = TextEditingController::new();
        let mut harness = crate::test_harness::mount(crate::Column::new(vec![
            TextField::new(first.clone()).into_view().boxed(),
            TextField::new(second.clone()).into_view().boxed(),
        ]));

        let first_id = first.focus_node_id().expect("the first field published");
        let second_id = second.focus_node_id().expect("the second field published");
        assert_ne!(first_id, second_id);

        focus_field(&second);
        assert_eq!(
            manager.primary_focus(),
            Some(second_id),
            "the tap focuses its own field, not the first with a key handler"
        );
        focus_field(&first);
        assert_eq!(manager.primary_focus(), Some(first_id));

        // Unmount: both fields withdraw, and a late tap is a no-op.
        harness.swap_root(crate::Column::new(Vec::<flui_view::BoxedView>::new()));
        assert_eq!(first.focus_node_id(), None, "unmount withdraws the node");
        manager.unfocus();
        focus_field(&second);
        assert_eq!(manager.primary_focus(), None, "a late tap is a no-op");
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
