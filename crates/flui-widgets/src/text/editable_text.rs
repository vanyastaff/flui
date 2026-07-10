//! [`EditableText`] — single-line editable text backed by a
//! [`TextEditingController`].

use std::sync::Arc;

use flui_foundation::ListenerId;
use flui_foundation::notifier::Listenable;
use flui_interaction::events::{Key, KeyState, NamedKey};
use flui_interaction::routing::{FocusManager, FocusNode, KeyEventCallback};
use flui_objects::RenderEditable;
use flui_rendering::protocol::BoxProtocol;
use flui_types::{
    Color,
    typography::{TextDirection, TextSpan},
};
use flui_view::prelude::*;
use flui_view::{BoxedView, RenderView, impl_render_view};

use crate::AnimatedBuilder;
use crate::text::controller::TextEditingController;

// ============================================================================
// EditableText
// ============================================================================

/// A single-line text field that accepts keyboard input when focused.
///
/// Flutter parity: `widgets/editable_text.dart` `EditableText` — the low-level
/// editable primitive.  [`TextField`](super::text_field::TextField) wraps this
/// with decoration and tap-to-focus.
///
/// # Key routing
///
/// `EditableText` registers a per-node key handler with the
/// [`FocusManager`] singleton in `init_state`.  Platform key events arrive via
/// `FocusManager::dispatch_key_event` (wired in `flui-app`), which routes them
/// to the focused node's handler.  Only `KeyState::Down` events (including
/// key-repeat) are processed; `KeyState::Up` events are ignored.
///
/// # DEFERRED (v1)
///
/// The following are absent in v1; do not use these features and expect them
/// to work:
/// - **IME / composing region** — CJK input, dead keys, and OS input methods
///   are not handled.  Printable characters arrive via `Key::Character` which
///   covers basic ASCII and composed Unicode characters already delivered as a
///   single string by the platform.
/// - **Text selection by drag** — only a collapsed caret is tracked; drag
///   selection, shift-click, and selection rendering are not implemented.
/// - **Clipboard** — copy / paste / cut (`Ctrl+C/V/X`) are not wired.
/// - **Multi-line** — newlines are inserted as literal characters but line
///   wrapping, multi-line layout, and vertical scrolling are not implemented.
/// - **`obscureText`** — password masking is not implemented.
/// - **Input formatters** — no validation or transformation pipeline.
/// - **Scroll when text overflows** — the rendered text clips without scrolling.
#[derive(Clone, Debug, StatefulView)]
pub struct EditableText {
    /// Controller that owns the text buffer and caret.
    pub(super) controller: TextEditingController,
    /// Height of the rendered caret bar in logical pixels.
    pub(super) caret_height: f32,
    /// Color of the caret bar when the field is focused.
    pub(super) caret_color: Color,
}

impl EditableText {
    /// Create an `EditableText` driven by `controller`.
    #[must_use]
    pub fn new(controller: TextEditingController) -> Self {
        Self {
            controller,
            caret_height: 18.0,
            caret_color: Color::BLACK,
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
}

// ============================================================================
// EditableTextState
// ============================================================================

/// Persistent state for [`EditableText`].
///
/// Owns the [`FocusNode`] for this field and wires it to the
/// [`FocusManager`] key-dispatch machinery on mount.
pub struct EditableTextState {
    /// Focus node representing this field in the global focus tree.
    focus_node: Arc<FocusNode>,
    /// The node this field's node hangs under — the nearest enclosing focus
    /// parent at mount, or the root scope's backing node. Detached from in
    /// `dispose`.
    parent: Option<Arc<FocusNode>>,
    /// Clone of the controller captured in `create_state`; used to register
    /// listeners in `init_state` without needing the `view` reference.
    controller: TextEditingController,
    /// ID for the listener we added to `controller` so we can remove it on
    /// dispose — avoids a `remove_all_listeners` that would disrupt other
    /// subscribers.
    controller_listener_id: Option<ListenerId>,
    /// The single notifier that drives the inner `AnimatedBuilder`.  Fires on
    /// text changes (forwarded from the controller listener) **and** on focus
    /// changes (forwarded from the FocusManager listener).
    rebuild_notifier: flui_foundation::notifier::ChangeNotifier,
    /// ID for the focus-change listener we added to the [`FocusManager`], so
    /// dispose removes exactly ours (ADR-0022 U1).
    focus_listener_id: Option<ListenerId>,
}

impl std::fmt::Debug for EditableTextState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EditableTextState")
            .field("focus_node_id", &self.focus_node.id().get())
            .finish_non_exhaustive()
    }
}

impl StatefulView for EditableText {
    type State = EditableTextState;

    fn create_state(&self) -> EditableTextState {
        EditableTextState {
            focus_node: FocusNode::with_debug_label("EditableText"),
            parent: None,
            controller: self.controller.clone(),
            controller_listener_id: None,
            rebuild_notifier: flui_foundation::notifier::ChangeNotifier::new(),
            focus_listener_id: None,
        }
    }
}

impl ViewState<EditableText> for EditableTextState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        // 1. Attach our focus node under the nearest enclosing `FocusScope` —
        //    a `ModalRoute`'s per-route scope when this field sits in a page
        //    (ADR-0022 U3/U4) — falling back to the root scope, and publish the
        //    node on the controller so the enclosing `TextField`'s tap can
        //    focus *this* field.
        let parent = crate::interaction::enclosing_focus_parent(ctx);
        parent.attach_node(&self.focus_node);
        self.controller
            .set_focus_node_id(Some(self.focus_node.id()));
        self.parent = Some(parent);

        // 2. Register a key handler with the FocusManager.  Only fires when
        //    this node is the primary-focused node.
        let key_handler = build_key_handler(self.controller.clone());
        FocusManager::global().register_key_handler(self.focus_node.id(), key_handler);

        // 3. Forward controller change events into the rebuild notifier so the
        //    inner AnimatedBuilder rebuilds on every keystroke.
        let rebuild_notifier_for_text = self.rebuild_notifier.clone();
        let controller_listener_id = self.controller.add_listener(Arc::new(move || {
            rebuild_notifier_for_text.notify_listeners();
        }));
        self.controller_listener_id = Some(controller_listener_id);

        // 4. Forward FocusManager focus-change events into the rebuild notifier
        //    so the caret appears / disappears immediately when this field
        //    gains or loses focus. Removed by id in `dispose`.
        let rebuild_notifier_for_focus = self.rebuild_notifier.clone();
        let node_id = self.focus_node.id();
        self.focus_listener_id = Some(FocusManager::global().add_listener(Arc::new(
            move |previous, current| {
                // Only rebuild when this node's focus state actually changed.
                let was_focused = previous == Some(node_id);
                let now_focused = current == Some(node_id);
                if was_focused != now_focused {
                    rebuild_notifier_for_focus.notify_listeners();
                }
            },
        )));
    }

    fn build(&self, view: &EditableText, _ctx: &dyn BuildContext) -> impl IntoView {
        let controller = self.controller.clone();
        let focus_node = Arc::clone(&self.focus_node);
        let caret_height = view.caret_height;
        let caret_color = view.caret_color;

        AnimatedBuilder::new(Arc::new(self.rebuild_notifier.clone()), move || {
            build_field_view(&controller, &focus_node, caret_height, caret_color)
        })
    }

    fn dispose(&mut self) {
        // Remove the focus-change listener we registered in init_state.
        if let Some(id) = self.focus_listener_id.take() {
            FocusManager::global().remove_listener(id);
        }

        // Unregister the key handler from the FocusManager, and withdraw the
        // node from the controller — an unmounted field must not be a tap
        // target.
        FocusManager::global().unregister_key_handler(self.focus_node.id());
        self.controller.set_focus_node_id(None);

        // Detach the focus node from wherever it hangs (also clears primary
        // focus if this node held it).
        if let Some(parent) = self.parent.take() {
            match parent.as_scope() {
                Some(scope) => scope.detach_node(self.focus_node.id()),
                None => parent.detach_node(self.focus_node.id()),
            }
        }

        // Remove the controller listener we registered in init_state.
        if let Some(id) = self.controller_listener_id.take() {
            self.controller.remove_listener(id);
        }

        // Dispose the rebuild notifier so any remaining AnimatedBuilder
        // subscribers can detect the widget is gone.
        self.rebuild_notifier.dispose();
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Build the key-event handler closure for `controller`.
///
/// Only `KeyState::Down` events (which cover key-repeat) are acted upon.
/// Returns `true` when the event is consumed so propagation stops.
fn build_key_handler(controller: TextEditingController) -> KeyEventCallback {
    Arc::new(move |event| {
        if event.state != KeyState::Down {
            return false;
        }
        match &event.key {
            Key::Character(character_string) => {
                controller.insert_str(character_string.as_str());
                true
            }
            Key::Named(NamedKey::Backspace) => {
                controller.backspace();
                true
            }
            Key::Named(NamedKey::Delete) => {
                controller.delete_forward();
                true
            }
            Key::Named(NamedKey::ArrowLeft) => {
                controller.move_caret_left();
                true
            }
            Key::Named(NamedKey::ArrowRight) => {
                controller.move_caret_right();
                true
            }
            Key::Named(NamedKey::Home) => {
                controller.move_caret_home();
                true
            }
            Key::Named(NamedKey::End) => {
                controller.move_caret_end();
                true
            }
            Key::Named(_) => false,
        }
    })
}

#[derive(Clone, Debug)]
struct EditableTextRenderView {
    text: String,
    caret_byte_offset: usize,
    show_caret: bool,
    caret_height: f32,
    caret_color: Color,
}

impl EditableTextRenderView {
    fn build_render_object(&self) -> RenderEditable {
        RenderEditable::new(TextSpan::new(self.text.clone()), TextDirection::Ltr)
            .with_caret_byte_offset(self.caret_byte_offset)
            .with_show_caret(self.show_caret)
            .with_caret_width(2.0)
            .with_caret_height(self.caret_height)
            .with_caret_color(self.caret_color)
    }
}

impl RenderView for EditableTextRenderView {
    type Protocol = BoxProtocol;
    type RenderObject = RenderEditable;

    fn create_render_object(&self) -> Self::RenderObject {
        self.build_render_object()
    }

    fn update_render_object(&self, render_object: &mut Self::RenderObject) {
        *render_object = self.build_render_object();
    }
}

impl_render_view!(EditableTextRenderView);

/// Assemble the visual render view for the text field interior.
fn build_field_view(
    controller: &TextEditingController,
    focus_node: &Arc<FocusNode>,
    caret_height: f32,
    caret_color: Color,
) -> BoxedView {
    EditableTextRenderView {
        text: controller.text(),
        caret_byte_offset: controller.caret_byte_offset(),
        show_caret: focus_node.has_primary_focus(),
        caret_height,
        caret_color,
    }
    .boxed()
}
