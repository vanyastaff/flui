//! [`EditableText`] — single-line editable text backed by a
//! [`TextEditingController`].

use std::{rc::Rc, sync::Arc};

use flui_foundation::ListenerId;
use flui_foundation::notifier::Listenable;
use flui_interaction::events::{Key, KeyState, NamedKey};
use flui_interaction::routing::{FocusManager, FocusNode, KeyEventCallback};
use flui_objects::RenderEditable;
use flui_rendering::protocol::BoxProtocol;
use flui_types::{
    Color,
    typography::{TextDirection, TextSpan, TextStyle},
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
/// - **Swapping the controller on a live field** — `EditableTextState` pins
///   its own clone of `EditableText::controller` at `create_state` and reads
///   `self.controller` in `build`/`init_state`, never `view.controller`
///   again after mount. A parent rebuilding this widget with a *different*
///   `TextEditingController` value does not retarget the mounted field's
///   focus-node registration or key handler — both keep driving the
///   ORIGINAL controller. Full re-registration on controller swap (the
///   oracle's `didUpdateWidget`, `text_field.dart:1303-1311`, tag `3.44.0`)
///   is a named deferral; an enclosing decorated field
///   (`flui_material::TextField`) pins its own clone the same way for the
///   same reason — see that type's module docs' "Controller identity"
///   section.
#[derive(Clone, Debug, StatefulView)]
pub struct EditableText {
    /// Controller that owns the text buffer and caret.
    pub(super) controller: TextEditingController,
    /// Height of the rendered caret bar in logical pixels.
    pub(super) caret_height: f32,
    /// Color of the caret bar when the field is focused.
    pub(super) caret_color: Color,
    /// Whether this field accepts focus and input. `true` by default.
    ///
    /// **Named hoist, not a direct port**: the oracle has no
    /// `EditableText.enabled` property at this tag — `enabled` lives on
    /// `TextField` and flows down as `_isEnabled` into
    /// `_effectiveFocusNode.canRequestFocus`
    /// (`text_field.dart:1183,1282-1299`, tag `3.44.0`). FLUI's
    /// [`TextField`](super::text_field::TextField) has no decoration/enabled
    /// plumbing yet, so this substrate hoists the behavior onto
    /// `EditableText` itself, one layer lower than the oracle — see
    /// [`enabled`](Self::enabled)'s doc comment for exactly what it
    /// withholds.
    pub(super) enabled: bool,
    /// Style applied to the field's [`TextSpan`], flowing through
    /// [`TextSpan::with_style`]. `None` renders with the span's own default.
    pub(super) text_style: Option<TextStyle>,
}

impl EditableText {
    /// Create an `EditableText` driven by `controller`.
    #[must_use]
    pub fn new(controller: TextEditingController) -> Self {
        Self {
            controller,
            caret_height: 18.0,
            caret_color: Color::BLACK,
            enabled: true,
            text_style: None,
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

    /// Set whether the field accepts focus and keyboard input (default
    /// `true`) — see the [`enabled`](Self::enabled) field's doc comment for
    /// why this is a named hoist of `TextField.enabled`, not a direct
    /// `EditableText` parity port.
    ///
    /// A disabled field withholds focus acquisition — it stops publishing its
    /// [`FocusNode`] id on [`TextEditingController`], so an enclosing
    /// `TextField`'s tap-to-focus (which reads
    /// `controller.focus_node_id()`) finds nothing to focus, the same
    /// withdraw-on-unavailable mechanism `dispose` already uses for an
    /// unmounted field — and marks the node
    /// [`FocusNode::set_can_request_focus`]`(false)`, which keyboard-traversal
    /// (`focus_next`/`focus_previous`) already honors. **Unlike** Flutter's
    /// `FocusNode.canRequestFocus` setter, FLUI's `set_can_request_focus` is a
    /// bare atomic store with no side effects — so if the field is focused
    /// when it becomes disabled, `did_update_view` explicitly calls
    /// `FocusManager::unfocus` (a step the oracle's setter performs
    /// implicitly as part of assigning `canRequestFocus`). Its key handler
    /// also stops mutating the controller while disabled, so even a stray
    /// dispatch reaching an already-focused-then-disabled node is a no-op.
    ///
    /// Tap suppression is a decoration-level concern (an enclosing
    /// `TextField`'s `GestureDetector`), not this primitive's — out of scope
    /// here, see `TextField`'s own docs.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// Apply `style` to the field's rendered [`TextSpan`] via
    /// [`TextSpan::with_style`].
    #[must_use]
    pub fn text_style(mut self, style: TextStyle) -> Self {
        self.text_style = Some(style);
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
    /// Publishes the field's `RenderId` while mounted, so the node's rect
    /// provider can measure it for reading-order traversal.
    anchor: flui_objects::SubtreeAnchor,
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
    /// dispose removes exactly ours.
    focus_listener_id: Option<ListenerId>,
    /// The view's `enabled` at construction time, cached because
    /// `init_state` has no `view` parameter — see [`EditableText::enabled`].
    /// Read exactly once, by `init_state`'s initial-publish decision; there
    /// is no reader afterward, so `did_update_view` deliberately does NOT
    /// keep this field in sync post-mount (every later change is read
    /// straight from the view in `did_update_view`/`build` instead, which is
    /// the only place that still needs it).
    enabled: bool,
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
        let focus_node = FocusNode::with_debug_label("EditableText");
        focus_node.set_can_request_focus(self.enabled);
        EditableTextState {
            focus_node,
            anchor: flui_objects::SubtreeAnchor::new(),
            parent: None,
            controller: self.controller.clone(),
            controller_listener_id: None,
            rebuild_notifier: flui_foundation::notifier::ChangeNotifier::new(),
            focus_listener_id: None,
            enabled: self.enabled,
        }
    }
}

impl ViewState<EditableText> for EditableTextState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        // 1. Attach our focus node under the nearest enclosing `FocusScope` —
        //    a `ModalRoute`'s per-route scope when this field sits in a page —
        //    falling back to the root scope, and publish the
        //    node on the controller so the enclosing `TextField`'s tap can
        //    focus *this* field.
        let parent = crate::interaction::enclosing_focus_parent(ctx);
        parent.attach_node(&self.focus_node);
        // Publish the node only while enabled — see `EditableText::enabled`'s
        // doc comment on why withholding publication is the focus-acquisition
        // gate (mirrors `dispose`'s withdraw-on-unmount below).
        if self.enabled {
            self.controller
                .set_focus_node_id(Some(self.focus_node.id()));
        }
        self.parent = Some(parent);
        crate::interaction::install_rect_provider(&self.focus_node, &self.anchor, ctx);

        // 2. Register a key handler with the FocusManager.  Only fires when
        //    this node is the primary-focused node. Gated on
        //    `can_request_focus` (kept in sync with `enabled` in
        //    `did_update_view`) so a stray dispatch to an already-focused
        //    field that has since been disabled is a no-op.
        let key_handler = build_key_handler(self.controller.clone(), Arc::clone(&self.focus_node));
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
        self.focus_listener_id = Some(FocusManager::global().add_listener(Rc::new(
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

    fn did_update_view(&mut self, _old_view: &EditableText, new_view: &EditableText) {
        self.focus_node.set_can_request_focus(new_view.enabled);
        if new_view.enabled {
            self.controller
                .set_focus_node_id(Some(self.focus_node.id()));
        } else {
            // A field disabled while focused must not keep the caret and
            // keyboard input — mirrors Flutter's `TextField`/`EditableText`
            // unfocusing when `enabled` flips false mid-focus.
            //
            // Unfocus BEFORE withdrawing the published node id — load-
            // bearing order, not incidental. `FocusManager::unfocus` notifies
            // every registered listener with the (previous, current) pair;
            // an enclosing decorated field (`flui_material::TextField`)
            // compares that pair against `controller.focus_node_id()` to
            // detect ITS OWN focus-loss transition. Clearing the id first
            // would make that comparison vacuous by the time the
            // notification fires (the id is already gone), silently masking
            // the transition from any such listener.
            if self.focus_node.has_primary_focus() {
                FocusManager::global().unfocus();
            }
            self.controller.set_focus_node_id(None);
        }
    }

    fn build(&self, view: &EditableText, _ctx: &dyn BuildContext) -> impl IntoView {
        let controller = self.controller.clone();
        let focus_node = Arc::clone(&self.focus_node);
        let caret_height = view.caret_height;
        let caret_color = view.caret_color;
        let enabled = view.enabled;
        let text_style = view.text_style.clone();

        crate::navigator::AnchoredBox::new(
            self.anchor.clone(),
            AnimatedBuilder::new(Arc::new(self.rebuild_notifier.clone()), move || {
                build_field_view(
                    &controller,
                    &focus_node,
                    caret_height,
                    caret_color,
                    enabled,
                    text_style.clone(),
                )
            }),
        )
    }

    fn dispose(&mut self) {
        self.focus_node.clear_rect_provider();
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
/// Only `KeyState::Down` events (which cover key-repeat) are acted upon, and
/// only while `focus_node` still allows focus — kept in sync with
/// `EditableText::enabled` by `did_update_view` — so input is ignored on a
/// field disabled after it was focused. Returns `true` when the event is
/// consumed so propagation stops.
fn build_key_handler(
    controller: TextEditingController,
    focus_node: Arc<FocusNode>,
) -> KeyEventCallback {
    Rc::new(move |event| {
        if !focus_node.can_request_focus() {
            return false;
        }
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
    text_style: Option<TextStyle>,
}

impl EditableTextRenderView {
    fn build_render_object(&self) -> RenderEditable {
        let mut span = TextSpan::new(self.text.clone());
        if let Some(style) = self.text_style.clone() {
            span = span.with_style(style);
        }
        RenderEditable::new(span, TextDirection::Ltr)
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

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        self.build_render_object()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) {
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
    enabled: bool,
    text_style: Option<TextStyle>,
) -> BoxedView {
    EditableTextRenderView {
        text: controller.text(),
        caret_byte_offset: controller.caret_byte_offset(),
        // `enabled` is defensive here: `did_update_view` already unfocuses a
        // field that becomes disabled while focused, so `has_primary_focus`
        // should already be `false` by the time this runs.
        show_caret: enabled && focus_node.has_primary_focus(),
        caret_height,
        caret_color,
        text_style,
    }
    .boxed()
}

#[cfg(test)]
mod tests {
    use flui_interaction::routing::FocusManager;

    use super::*;
    use crate::text::controller::TextEditingController;

    /// A field constructed disabled never publishes its focus node, so an
    /// enclosing `TextField`'s tap-to-focus (which reads
    /// `controller.focus_node_id()`) finds nothing to focus — the
    /// withhold-acquisition contract [`EditableText::enabled`] documents.
    ///
    /// Red-check: drop the `if self.enabled` guard around
    /// `set_focus_node_id` in `init_state` — the node publishes
    /// unconditionally and this assertion fails.
    #[test]
    fn disabled_field_does_not_publish_its_focus_node() {
        let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
        FocusManager::global().unfocus();

        let controller = TextEditingController::new();
        let _harness =
            crate::test_harness::mount(EditableText::new(controller.clone()).enabled(false));

        assert_eq!(
            controller.focus_node_id(),
            None,
            "a disabled field must not publish a focus node to focus"
        );
    }

    /// An enabled field (the default) does publish, so the same field
    /// re-enabled is focusable again — the contrast case for the test above.
    #[test]
    fn enabled_field_publishes_its_focus_node() {
        let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
        FocusManager::global().unfocus();

        let controller = TextEditingController::new();
        let _harness = crate::test_harness::mount(EditableText::new(controller.clone()));

        assert!(
            controller.focus_node_id().is_some(),
            "an enabled field must publish a focus node"
        );
    }

    /// Disabling a focused field unfocuses it and withdraws its published
    /// node. Load-bearing: FLUI's `FocusNode::set_can_request_focus` is a
    /// bare atomic store with no side effects — unlike Flutter's
    /// `FocusNode.canRequestFocus` setter, which auto-unfocuses — so nothing
    /// but `did_update_view`'s explicit `FocusManager::unfocus` call (see
    /// [`EditableText::enabled`]'s doc comment) does this.
    ///
    /// Red-check: delete the `FocusManager::global().unfocus()` call in
    /// `did_update_view`'s disabled branch — the node stays primary-focused
    /// and the first assertion fails.
    #[test]
    fn disabling_a_focused_field_unfocuses_it_and_withdraws_the_node() {
        let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
        FocusManager::global().unfocus();

        let controller = TextEditingController::new();
        let mut harness = crate::test_harness::mount(EditableText::new(controller.clone()));

        let node_id = controller
            .focus_node_id()
            .expect("an enabled field publishes its node");
        FocusManager::global().request_focus(node_id);
        assert_eq!(FocusManager::global().primary_focus(), Some(node_id));

        harness.swap_root(EditableText::new(controller.clone()).enabled(false));

        assert_ne!(
            FocusManager::global().primary_focus(),
            Some(node_id),
            "disabling a focused field must unfocus it"
        );
        assert_eq!(
            controller.focus_node_id(),
            None,
            "disabling must withdraw the published focus node"
        );
    }

    /// The contrast case: re-enabling a disabled field republishes its
    /// node, so it becomes focusable again.
    #[test]
    fn re_enabling_a_disabled_field_republishes_its_focus_node() {
        let _guard = crate::test_harness::FOCUS_TEST_LOCK.lock();
        FocusManager::global().unfocus();

        let controller = TextEditingController::new();
        let mut harness =
            crate::test_harness::mount(EditableText::new(controller.clone()).enabled(false));
        assert_eq!(controller.focus_node_id(), None);

        harness.swap_root(EditableText::new(controller.clone()));

        assert!(
            controller.focus_node_id().is_some(),
            "re-enabling must republish the focus node"
        );
    }

    /// The key handler's `can_request_focus` guard, in isolation: invoked
    /// directly (bypassing `FocusManager::dispatch_key_event`'s own
    /// primary-focus routing), a disabled node's handler must still refuse
    /// to mutate the controller.
    ///
    /// Red-check: delete the `if !focus_node.can_request_focus() { return
    /// false; }` guard at the top of `build_key_handler`'s closure — the
    /// character is inserted and both assertions fail.
    #[test]
    fn disabled_key_handler_ignores_input_even_when_invoked_directly() {
        use flui_interaction::events::Code;
        use flui_interaction::testing::input::KeyEventBuilder;

        let controller = TextEditingController::new();
        let focus_node = FocusNode::with_debug_label("test");
        focus_node.set_can_request_focus(false);
        let handler = build_key_handler(controller.clone(), Arc::clone(&focus_node));

        let event = KeyEventBuilder::new(Code::KeyA)
            .with_key(Key::Character("a".to_string()))
            .with_state(KeyState::Down)
            .build();

        let consumed = handler(&event);

        assert!(
            !consumed,
            "a disabled node's key handler must not consume the event"
        );
        assert_eq!(
            controller.text(),
            "",
            "a disabled node's key handler must not mutate the controller"
        );
    }

    /// `EditableText::text_style` flows through to the rendered `TextSpan` —
    /// the render-view assembly this builder feeds.
    #[test]
    fn text_style_reaches_the_rendered_span() {
        let style = TextStyle::default().with_color(Color::rgb(10, 20, 30));
        let render_view = EditableTextRenderView {
            text: "hello".to_string(),
            caret_byte_offset: 0,
            show_caret: false,
            caret_height: 18.0,
            caret_color: Color::BLACK,
            text_style: Some(style.clone()),
        };

        let render_object = render_view.build_render_object();
        let rendered_style = render_object
            .painter()
            .text()
            .expect("a span was set")
            .style()
            .expect("text_style was set");
        assert_eq!(rendered_style.color, style.color);
    }

    /// Without `text_style`, the span carries no explicit style — no
    /// override was silently invented.
    #[test]
    fn no_text_style_leaves_the_span_unstyled() {
        let render_view = EditableTextRenderView {
            text: "hello".to_string(),
            caret_byte_offset: 0,
            show_caret: false,
            caret_height: 18.0,
            caret_color: Color::BLACK,
            text_style: None,
        };

        let render_object = render_view.build_render_object();
        assert!(
            render_object
                .painter()
                .text()
                .expect("a span was set")
                .style()
                .is_none()
        );
    }
}
