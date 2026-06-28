//! [`TextField`] — an [`EditableText`] with border decoration and
//! tap-to-focus behavior.

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

/// A decorated, tap-to-focus single-line text input field.
///
/// Flutter parity: `material/text_field.dart` `TextField` — wraps
/// [`EditableText`] with a [`DecoratedBox`] border and a [`GestureDetector`]
/// that requests focus on tap.
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
        // The EditableText inside this field owns its FocusNode.  We need to
        // call `request_focus()` on it from the GestureDetector's on_tap, but
        // we do not have a handle to the FocusNode from outside the state.
        //
        // For v1 the pragmatic path: walk the root scope's children for the
        // first node that has a registered key handler (= this field's
        // EditableText node).  This heuristic is correct for single-field
        // forms.
        //
        // # DEFERRED (v1)
        // Multi-field forms require a stable focus-node reference stored in
        // the controller or a more precise hit-test-level focus-request
        // mechanism.

        let editable = EditableText::new(self.controller.clone())
            .caret_height(self.caret_height)
            .caret_color(self.caret_color);

        let padded = Padding::new(self.content_padding).child(editable);
        let decorated = DecoratedBox::new(field_border_decoration()).child(padded);

        let controller = self.controller.clone();
        GestureDetector::new()
            .on_tap(move || {
                focus_first_text_node_in_root_scope(&controller);
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

/// Ask the `FocusManager` to focus the node that was registered by the
/// `EditableTextState` for `_controller`.
///
/// `EditableTextState::init_state` attaches the node to the root scope and
/// registers a key handler keyed by the node's id.  We identify the node by
/// searching the root scope's children for the first one that has a registered
/// key handler — since `TextField` wraps exactly one `EditableText`, this is
/// unambiguous in the single-field case.
///
/// # DEFERRED (v1)
/// The `_controller` parameter is reserved for a future implementation that
/// stores the focus-node id directly on the controller, enabling multi-field
/// disambiguation without the scope-search heuristic.
fn focus_first_text_node_in_root_scope(_controller: &TextEditingController) {
    use flui_interaction::routing::FocusManager;

    let manager = FocusManager::global();
    let root_scope = manager.root_scope();

    // Walk the root scope's children for the first attached node that has a
    // registered key handler (= the EditableText's node).
    // `root_scope` is `&Arc<FocusScopeNode>`.  Its underlying `FocusNode`
    // carries the list of attached child FocusNodes.
    let target_id = root_scope
        .as_focus_node()
        .children()
        .into_iter()
        .find(|node| manager.has_key_handler(node.id()))
        .map(|node| node.id());

    if let Some(node_id) = target_id {
        manager.request_focus(node_id);
    }
}
